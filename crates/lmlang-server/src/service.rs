//! ProgramService: the single coordinator between HTTP handlers and the
//! graph/storage/checker/interpreter crates.
//!
//! All business logic flows through [`ProgramService`]. Handlers will be thin
//! wrappers that delegate to these methods.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;

use lmlang_check::interpreter::{ExecutionState, Interpreter, InterpreterConfig, Value};
use lmlang_check::typecheck;
use lmlang_core::edge::{FlowEdge, SemanticEdge};
use lmlang_core::graph::{
    ComputeEvent, ProgramGraph, PropagationEventKind, PropagationLayer, SemanticEvent,
};
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::node::{ComputeNode, SemanticNode};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp};
use lmlang_core::type_id::TypeId;
use lmlang_storage::traits::GraphStore;
use lmlang_storage::types::ProgramId;
use lmlang_storage::SqliteStore;

use crate::error::ApiError;
use crate::schema::diagnostics::DiagnosticError;
use crate::schema::diagnostics::PropagationConflictDiagnosticView;
use crate::schema::history::{
    CreateCheckpointResponse, DiffResponse, ListCheckpointsResponse, ListHistoryResponse,
    RedoResponse, RestoreCheckpointResponse, UndoResponse,
};
use crate::schema::mutations::{CreatedEntity, Mutation, ProposeEditRequest, ProposeEditResponse};
use crate::schema::observability::{
    ObservabilityEdgeView, ObservabilityGraphRequest, ObservabilityGraphResponse,
    ObservabilityGroupView, ObservabilityLayer, ObservabilityNodeView, ObservabilityPreset,
    ObservabilityQueryRequest, ObservabilityQueryResponse, ObservabilityQueryResultView,
    QueryContractEntryView, QueryContractsTabView, QueryInterpretationView,
    QueryRelationshipItemView, QueryRelationshipsTabView, QuerySummaryTabView,
    SuggestedPromptChipView,
};
use crate::schema::programs::ProgramSummaryView;
use crate::schema::queries::{
    DetailLevel, EdgeView, FunctionView, GetFunctionResponse, NeighborhoodResponse, NodeView,
    ProgramOverviewResponse, SearchRequest, SearchResponse, SemanticEdgeView, SemanticNodeView,
    SemanticOwnershipView, SemanticProvenanceView, SemanticQueryResponse,
};
use crate::schema::simulate::{SimulateRequest, SimulateResponse, TraceEntryView};
use crate::schema::verify::{
    FlushPropagationResponse, PropagationEventSeed, VerifyResponse, VerifyScope,
};
use crate::undo::{CheckpointManager, EditCommand, EditLog};

/// The central service coordinating graph mutations, queries, verification,
/// simulation, and undo/redo operations.
///
/// Holds the in-memory graph, a connection for edit log / checkpoint queries,
/// and a storage backend for persistence.
pub struct ProgramService {
    /// The current in-memory program graph.
    graph: ProgramGraph,
    /// SQLite storage backend for persistence.
    store: SqliteStore,
    /// The active program's ID.
    program_id: ProgramId,
    /// Shared connection for edit_log/checkpoint queries.
    conn: Connection,
    /// Incremental compilation state (lazily initialized on first compile).
    incremental_state: Option<lmlang_codegen::incremental::IncrementalState>,
}

impl ProgramService {
    /// Creates a new ProgramService, opening a SQLite database at `db_path`.
    ///
    /// Creates or loads a default program named "default".
    pub fn new(db_path: &str) -> Result<Self, ApiError> {
        let conn = lmlang_storage::schema::open_database(db_path)
            .map_err(|e| ApiError::InternalError(format!("failed to open database: {}", e)))?;
        let mut store = SqliteStore::new(db_path)
            .map_err(|e| ApiError::InternalError(format!("failed to open store: {}", e)))?;

        // Try to load the first program, or create a default one.
        let programs = store
            .list_programs()
            .map_err(|e| ApiError::InternalError(format!("failed to list programs: {}", e)))?;

        let (program_id, graph) = if let Some(first) = programs.first() {
            let graph = store
                .load_program(first.id)
                .map_err(|e| ApiError::InternalError(format!("failed to load program: {}", e)))?;
            (first.id, graph)
        } else {
            let id = store
                .create_program("default")
                .map_err(|e| ApiError::InternalError(format!("failed to create program: {}", e)))?;
            let graph = ProgramGraph::new("main");
            store
                .save_program(id, &graph)
                .map_err(|e| ApiError::InternalError(format!("failed to save program: {}", e)))?;
            (id, graph)
        };

        Ok(ProgramService {
            graph,
            store,
            program_id,
            conn,
            incremental_state: None,
        })
    }

    /// Creates a new ProgramService using a temporary database (for testing).
    ///
    /// Both the service's `conn` and `store` share the same temp file so that
    /// edit_log/checkpoint foreign keys referencing programs(id) are satisfied.
    pub fn in_memory() -> Result<Self, ApiError> {
        // Use a unique temp file so both conn and store share the same database.
        let tmp_path = std::env::temp_dir()
            .join(format!("lmlang_test_{}.db", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string();

        let conn = lmlang_storage::schema::open_database(&tmp_path)
            .map_err(|e| ApiError::InternalError(format!("failed to open test db: {}", e)))?;
        let mut store = SqliteStore::new(&tmp_path)
            .map_err(|e| ApiError::InternalError(format!("failed to open test store: {}", e)))?;

        let id = store
            .create_program("default")
            .map_err(|e| ApiError::InternalError(format!("failed to create program: {}", e)))?;
        let graph = ProgramGraph::new("main");
        store
            .save_program(id, &graph)
            .map_err(|e| ApiError::InternalError(format!("failed to save program: {}", e)))?;

        Ok(ProgramService {
            graph,
            store,
            program_id: id,
            conn,
            incremental_state: None,
        })
    }

    /// Returns a reference to the current graph.
    pub fn graph(&self) -> &ProgramGraph {
        &self.graph
    }

    /// Returns the active program ID.
    pub fn program_id(&self) -> ProgramId {
        self.program_id
    }

    // -----------------------------------------------------------------------
    // Program management (TOOL-01 prerequisite)
    // -----------------------------------------------------------------------

    /// Creates a new program.
    pub fn create_program(&mut self, name: &str) -> Result<ProgramId, ApiError> {
        let id = self.store.create_program(name)?;
        let graph = ProgramGraph::new("main");
        self.store.save_program(id, &graph)?;
        Ok(id)
    }

    /// Loads a program as the active graph.
    pub fn load_program(&mut self, id: ProgramId) -> Result<(), ApiError> {
        let graph = self.store.load_program(id)?;
        self.graph = graph;
        self.program_id = id;
        Ok(())
    }

    /// Lists all programs.
    pub fn list_programs(&self) -> Result<Vec<ProgramSummaryView>, ApiError> {
        let programs = self.store.list_programs()?;
        Ok(programs
            .into_iter()
            .map(|p| ProgramSummaryView {
                id: p.id,
                name: p.name,
            })
            .collect())
    }

    /// Deletes a program.
    pub fn delete_program(&mut self, id: ProgramId) -> Result<(), ApiError> {
        if id == self.program_id {
            return Err(ApiError::BadRequest(
                "cannot delete the active program".to_string(),
            ));
        }
        self.store.delete_program(id)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Mutation methods (TOOL-01)
    // -----------------------------------------------------------------------

    /// Proposes one or more graph mutations.
    ///
    /// Supports three modes:
    /// - **dry_run=true**: clone graph, apply mutations, validate, return results, discard.
    /// - **dry_run=false, single mutation**: apply to real graph, validate, revert on failure.
    /// - **dry_run=false, batch (>1 mutation)**: clone-and-swap atomically.
    pub fn propose_edit(
        &mut self,
        request: ProposeEditRequest,
    ) -> Result<ProposeEditResponse, ApiError> {
        if request.mutations.is_empty() {
            return Ok(ProposeEditResponse {
                valid: true,
                created: Vec::new(),
                errors: Vec::new(),
                warnings: Vec::new(),
                committed: false,
            });
        }

        if request.dry_run {
            // Dry run: clone, apply, validate, discard
            let mut clone = self.graph.clone();
            let mut created = Vec::new();
            let mut all_commands = Vec::new();

            for mutation in &request.mutations {
                match Self::apply_mutation(&mut clone, mutation) {
                    Ok((entity, cmd)) => {
                        if let Some(e) = entity {
                            created.push(e);
                        }
                        all_commands.push(cmd);
                    }
                    Err(e) => {
                        return Ok(ProposeEditResponse {
                            valid: false,
                            created: Vec::new(),
                            errors: vec![DiagnosticError {
                                code: "MUTATION_FAILED".to_string(),
                                message: e.to_string(),
                                details: None,
                            }],
                            warnings: Vec::new(),
                            committed: false,
                        });
                    }
                }
            }

            // Run full validation on the clone
            let type_errors = typecheck::validate_graph(&clone);
            let errors: Vec<DiagnosticError> =
                type_errors.into_iter().map(DiagnosticError::from).collect();

            Ok(ProposeEditResponse {
                valid: errors.is_empty(),
                created,
                errors,
                warnings: Vec::new(),
                committed: false,
            })
        } else if request.mutations.len() == 1 {
            // Single mutation: apply to real graph
            let mutation = &request.mutations[0];
            match Self::apply_mutation(&mut self.graph, mutation) {
                Ok((entity, cmd)) => {
                    // Validate affected area
                    let type_errors = typecheck::validate_graph(&self.graph);
                    let errors: Vec<DiagnosticError> =
                        type_errors.into_iter().map(DiagnosticError::from).collect();

                    if !errors.is_empty() {
                        // Revert via inverse command
                        let inverse = cmd.inverse();
                        let _ = Self::apply_edit_command(&mut self.graph, &inverse);
                        return Ok(ProposeEditResponse {
                            valid: false,
                            created: Vec::new(),
                            errors,
                            warnings: Vec::new(),
                            committed: false,
                        });
                    }

                    // Clear redo stack and record
                    EditLog::clear_redo_stack(&self.conn, self.program_id)?;
                    let description = describe_mutation(mutation);
                    EditLog::record(&self.conn, self.program_id, &cmd, Some(&description))?;

                    // Persist to store
                    self.store.save_program(self.program_id, &self.graph)?;
                    self.enqueue_propagation_for_mutations(&request.mutations);

                    let mut created = Vec::new();
                    if let Some(e) = entity {
                        created.push(e);
                    }

                    Ok(ProposeEditResponse {
                        valid: true,
                        created,
                        errors: Vec::new(),
                        warnings: Vec::new(),
                        committed: true,
                    })
                }
                Err(e) => Ok(ProposeEditResponse {
                    valid: false,
                    created: Vec::new(),
                    errors: vec![DiagnosticError {
                        code: "MUTATION_FAILED".to_string(),
                        message: e.to_string(),
                        details: None,
                    }],
                    warnings: Vec::new(),
                    committed: false,
                }),
            }
        } else {
            // Batch: clone-and-swap (all-or-nothing)
            let mut clone = self.graph.clone();
            let mut created = Vec::new();
            let mut all_commands = Vec::new();

            for mutation in &request.mutations {
                match Self::apply_mutation(&mut clone, mutation) {
                    Ok((entity, cmd)) => {
                        if let Some(e) = entity {
                            created.push(e);
                        }
                        all_commands.push(cmd);
                    }
                    Err(e) => {
                        return Ok(ProposeEditResponse {
                            valid: false,
                            created: Vec::new(),
                            errors: vec![DiagnosticError {
                                code: "MUTATION_FAILED".to_string(),
                                message: e.to_string(),
                                details: None,
                            }],
                            warnings: Vec::new(),
                            committed: false,
                        });
                    }
                }
            }

            // Validate the clone
            let type_errors = typecheck::validate_graph(&clone);
            let errors: Vec<DiagnosticError> =
                type_errors.into_iter().map(DiagnosticError::from).collect();

            if !errors.is_empty() {
                return Ok(ProposeEditResponse {
                    valid: false,
                    created: Vec::new(),
                    errors,
                    warnings: Vec::new(),
                    committed: false,
                });
            }

            // All passed: swap clone into real graph
            self.graph = clone;

            // Record as a batch command
            let batch_cmd = EditCommand::Batch {
                commands: all_commands,
                description: format!("batch of {} mutations", request.mutations.len()),
            };

            EditLog::clear_redo_stack(&self.conn, self.program_id)?;
            EditLog::record(
                &self.conn,
                self.program_id,
                &batch_cmd,
                Some(&format!("batch of {} mutations", request.mutations.len())),
            )?;

            // Persist to store
            self.store.save_program(self.program_id, &self.graph)?;
            self.enqueue_propagation_for_mutations(&request.mutations);

            Ok(ProposeEditResponse {
                valid: true,
                created,
                errors: Vec::new(),
                warnings: Vec::new(),
                committed: true,
            })
        }
    }

    /// Applies a single Mutation to the given graph, returning the created entity
    /// and the EditCommand for undo recording.
    fn apply_mutation(
        graph: &mut ProgramGraph,
        mutation: &Mutation,
    ) -> Result<(Option<CreatedEntity>, EditCommand), ApiError> {
        match mutation {
            Mutation::InsertNode { op, owner } => {
                let node_id = graph.add_compute_node(op.clone(), *owner)?;
                let cmd = EditCommand::InsertNode {
                    node_id,
                    op: op.clone(),
                    owner: *owner,
                };
                Ok((Some(CreatedEntity::Node { id: node_id }), cmd))
            }
            Mutation::RemoveNode { node_id } => {
                let removed = graph.remove_compute_node(*node_id)?;
                let cmd = EditCommand::RemoveNode {
                    node_id: *node_id,
                    removed_node: removed,
                };
                Ok((None, cmd))
            }
            Mutation::ModifyNode { node_id, new_op } => {
                let node = graph
                    .get_compute_node(*node_id)
                    .ok_or_else(|| ApiError::NotFound(format!("node {} not found", node_id.0)))?;
                let owner = node.owner;
                let old_op = graph.modify_compute_node_op(*node_id, new_op.clone())?;
                let cmd = EditCommand::ModifyNode {
                    node_id: *node_id,
                    old_op,
                    new_op: new_op.clone(),
                    owner,
                };
                Ok((None, cmd))
            }
            Mutation::AddEdge {
                from,
                to,
                source_port,
                target_port,
                value_type,
            } => {
                let edge_id =
                    graph.add_data_edge(*from, *to, *source_port, *target_port, *value_type)?;
                let cmd = EditCommand::InsertDataEdge {
                    edge_id,
                    from: *from,
                    to: *to,
                    source_port: *source_port,
                    target_port: *target_port,
                    value_type: *value_type,
                };
                Ok((Some(CreatedEntity::Edge { id: edge_id }), cmd))
            }
            Mutation::AddControlEdge {
                from,
                to,
                branch_index,
            } => {
                let edge_id = graph.add_control_edge(*from, *to, *branch_index)?;
                let cmd = EditCommand::InsertControlEdge {
                    edge_id,
                    from: *from,
                    to: *to,
                    branch_index: *branch_index,
                };
                Ok((Some(CreatedEntity::Edge { id: edge_id }), cmd))
            }
            Mutation::RemoveEdge { edge_id } => {
                // We need from/to for the EditCommand
                let edge_idx = EdgeIndex::<u32>::new(edge_id.0 as usize);
                let endpoints = graph.compute().edge_endpoints(edge_idx);
                let (from, to) = endpoints
                    .ok_or_else(|| ApiError::NotFound(format!("edge {} not found", edge_id.0)))?;
                let removed = graph.remove_edge(*edge_id)?;
                let cmd = EditCommand::RemoveEdge {
                    edge_id: *edge_id,
                    from: NodeId::from(from),
                    to: NodeId::from(to),
                    removed_edge: removed,
                };
                Ok((None, cmd))
            }
            Mutation::AddFunction {
                name,
                module,
                params,
                return_type,
                visibility,
            } => {
                let func_id = graph.add_function(
                    name.clone(),
                    *module,
                    params.clone(),
                    *return_type,
                    *visibility,
                )?;
                let cmd = EditCommand::AddFunction {
                    func_id,
                    name: name.clone(),
                    module: *module,
                    params: params.clone(),
                    return_type: *return_type,
                    visibility: *visibility,
                };
                Ok((Some(CreatedEntity::Function { id: func_id }), cmd))
            }
            Mutation::AddModule {
                name,
                parent,
                visibility,
            } => {
                let root = graph.modules.root_id();
                let actual_parent = parent.unwrap_or(root);
                let module_id = graph.add_module(name.clone(), actual_parent, *visibility)?;
                let cmd = EditCommand::AddModule {
                    module_id,
                    name: name.clone(),
                    parent: actual_parent,
                    visibility: *visibility,
                };
                Ok((Some(CreatedEntity::Module { id: module_id }), cmd))
            }
        }
    }

    /// Applies an EditCommand to the graph (used for undo/redo replay).
    fn apply_edit_command(graph: &mut ProgramGraph, cmd: &EditCommand) -> Result<(), ApiError> {
        match cmd {
            EditCommand::InsertNode { op, owner, .. } => {
                graph.add_compute_node(op.clone(), *owner)?;
                Ok(())
            }
            EditCommand::RemoveNode { node_id, .. } => {
                graph.remove_compute_node(*node_id)?;
                Ok(())
            }
            EditCommand::ModifyNode {
                node_id, new_op, ..
            } => {
                graph.modify_compute_node_op(*node_id, new_op.clone())?;
                Ok(())
            }
            EditCommand::InsertDataEdge {
                from,
                to,
                source_port,
                target_port,
                value_type,
                ..
            } => {
                graph.add_data_edge(*from, *to, *source_port, *target_port, *value_type)?;
                Ok(())
            }
            EditCommand::InsertControlEdge {
                from,
                to,
                branch_index,
                ..
            } => {
                graph.add_control_edge(*from, *to, *branch_index)?;
                Ok(())
            }
            EditCommand::RemoveEdge { edge_id, .. } => {
                graph.remove_edge(*edge_id)?;
                Ok(())
            }
            EditCommand::AddFunction {
                name,
                module,
                params,
                return_type,
                visibility,
                ..
            } => {
                graph.add_function(
                    name.clone(),
                    *module,
                    params.clone(),
                    *return_type,
                    *visibility,
                )?;
                Ok(())
            }
            EditCommand::AddModule {
                name,
                parent,
                visibility,
                ..
            } => {
                graph.add_module(name.clone(), *parent, *visibility)?;
                Ok(())
            }
            EditCommand::Batch { commands, .. } => {
                for sub_cmd in commands {
                    Self::apply_edit_command(graph, sub_cmd)?;
                }
                Ok(())
            }
        }
    }

    fn enqueue_propagation_for_mutations(&mut self, mutations: &[Mutation]) {
        for mutation in mutations {
            match mutation {
                Mutation::AddFunction { .. } => {
                    // Structural semantic edit: map to semantic -> compute propagation.
                    if let Some(function_id) =
                        self.graph.functions().keys().max_by_key(|f| f.0).copied()
                    {
                        self.graph.enqueue_propagation(
                            PropagationLayer::Semantic,
                            PropagationEventKind::Semantic(SemanticEvent::FunctionCreated {
                                function_id,
                            }),
                        );
                    }
                }
                Mutation::InsertNode { op, owner } => {
                    // Local compute edit: enqueue upward propagation to semantic layer.
                    if let Some(node_id) = self
                        .graph
                        .function_nodes(*owner)
                        .into_iter()
                        .max_by_key(|n| n.0)
                    {
                        self.graph.enqueue_propagation(
                            PropagationLayer::Compute,
                            PropagationEventKind::Compute(ComputeEvent::NodeInserted {
                                function_id: *owner,
                                node_id,
                                op_kind: format!("{:?}", op),
                            }),
                        );
                        if let Some(contract_name) = contract_name_from_op(op) {
                            self.graph.enqueue_propagation(
                                PropagationLayer::Semantic,
                                PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                                    function_id: *owner,
                                    contract_name,
                                }),
                            );
                        }
                    }
                }
                Mutation::ModifyNode { node_id, new_op } => {
                    if let Some(function_id) = self.owner_for_node(*node_id) {
                        self.graph.enqueue_propagation(
                            PropagationLayer::Compute,
                            PropagationEventKind::Compute(ComputeEvent::NodeModified {
                                function_id,
                                node_id: *node_id,
                                op_kind: format!("{:?}", new_op),
                            }),
                        );
                    }
                }
                Mutation::RemoveNode { node_id } => {
                    if let Some(function_id) = self.owner_for_node(*node_id) {
                        self.graph.enqueue_propagation(
                            PropagationLayer::Compute,
                            PropagationEventKind::Compute(ComputeEvent::NodeRemoved {
                                function_id,
                                node_id: *node_id,
                            }),
                        );
                    }
                }
                Mutation::AddEdge { from, .. } | Mutation::AddControlEdge { from, .. } => {
                    if let Some(function_id) = self.owner_for_node(*from) {
                        self.graph.enqueue_propagation(
                            PropagationLayer::Compute,
                            PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged {
                                function_id,
                            }),
                        );
                    }
                }
                Mutation::RemoveEdge { .. } | Mutation::AddModule { .. } => {}
            }
        }
    }

    fn owner_for_node(&self, node_id: NodeId) -> Option<FunctionId> {
        self.graph.get_compute_node(node_id).map(|n| n.owner)
    }

    /// Enqueues explicit propagation seeds from API input.
    pub fn enqueue_seed_events(&mut self, events: &[PropagationEventSeed]) -> Result<(), ApiError> {
        for event in events {
            let function_id = FunctionId(event.function_id);
            let kind = match event.kind.as_str() {
                "semantic.function_created" => {
                    PropagationEventKind::Semantic(SemanticEvent::FunctionCreated { function_id })
                }
                "semantic.function_signature_changed" => {
                    PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                        function_id,
                    })
                }
                "semantic.contract_added" => {
                    PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                        function_id,
                        contract_name: event
                            .contract_name
                            .clone()
                            .unwrap_or_else(|| "contract".to_string()),
                    })
                }
                "compute.node_inserted" => {
                    let node_id = NodeId(event.node_id.ok_or_else(|| {
                        ApiError::BadRequest(
                            "node_id required for compute.node_inserted".to_string(),
                        )
                    })?);
                    PropagationEventKind::Compute(ComputeEvent::NodeInserted {
                        function_id,
                        node_id,
                        op_kind: event
                            .op_kind
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                    })
                }
                "compute.node_modified" => {
                    let node_id = NodeId(event.node_id.ok_or_else(|| {
                        ApiError::BadRequest(
                            "node_id required for compute.node_modified".to_string(),
                        )
                    })?);
                    PropagationEventKind::Compute(ComputeEvent::NodeModified {
                        function_id,
                        node_id,
                        op_kind: event
                            .op_kind
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                    })
                }
                "compute.node_removed" => {
                    let node_id = NodeId(event.node_id.ok_or_else(|| {
                        ApiError::BadRequest(
                            "node_id required for compute.node_removed".to_string(),
                        )
                    })?);
                    PropagationEventKind::Compute(ComputeEvent::NodeRemoved {
                        function_id,
                        node_id,
                    })
                }
                "compute.control_flow_changed" => {
                    PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id })
                }
                other => {
                    return Err(ApiError::BadRequest(format!(
                        "unknown propagation event kind '{}'",
                        other
                    )));
                }
            };

            let origin = if event.kind.starts_with("semantic.") {
                PropagationLayer::Semantic
            } else {
                PropagationLayer::Compute
            };
            self.graph.enqueue_propagation(origin, kind);
        }

        Ok(())
    }

    /// Flushes queued propagation events with deterministic conflict handling.
    pub fn flush_propagation(
        &mut self,
    ) -> Result<lmlang_core::graph::PropagationFlushReport, ApiError> {
        let report = self.graph.flush_propagation()?;
        self.store.save_program(self.program_id, &self.graph)?;
        Ok(report)
    }

    /// Flushes propagation queue and projects into API schema.
    pub fn flush_propagation_response(&mut self) -> Result<FlushPropagationResponse, ApiError> {
        let report = self.flush_propagation()?;
        let diagnostics: Vec<PropagationConflictDiagnosticView> = report
            .diagnostics
            .into_iter()
            .map(PropagationConflictDiagnosticView::from)
            .collect();

        let unresolved: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.precedence == "diagnostic-required")
            .cloned()
            .collect();
        if !unresolved.is_empty() {
            return Err(ApiError::ConflictWithDetails {
                message: "unresolved dual-layer propagation conflicts".to_string(),
                details: serde_json::to_value(unresolved).unwrap_or(serde_json::Value::Null),
            });
        }

        Ok(FlushPropagationResponse {
            processed_events: report.processed_events,
            applied_events: report.applied_events,
            skipped_events: report.skipped_events,
            generated_events: report.generated_events,
            remaining_queue: report.remaining_queue,
            refreshed_semantic_nodes: report.refreshed_semantic_nodes,
            refreshed_summary_nodes: report.refreshed_summary_nodes,
            diagnostics,
        })
    }

    // -----------------------------------------------------------------------
    // Validation method (TOOL-03)
    // -----------------------------------------------------------------------

    /// Runs type verification on the graph.
    ///
    /// - `VerifyScope::Local`: validates data edges touching the specified affected nodes.
    /// - `VerifyScope::Full`: validates the entire graph.
    pub fn verify(
        &self,
        scope: VerifyScope,
        affected_nodes: Option<Vec<NodeId>>,
    ) -> Result<VerifyResponse, ApiError> {
        match scope {
            VerifyScope::Local => {
                let nodes = affected_nodes.unwrap_or_default();
                let mut errors = Vec::new();

                for &node_id in &nodes {
                    let node_idx: NodeIndex<u32> = node_id.into();
                    // Check all incoming data edges to this node
                    for edge_ref in self
                        .graph
                        .compute()
                        .edges_directed(node_idx, Direction::Incoming)
                    {
                        if let FlowEdge::Data {
                            source_port,
                            target_port,
                            value_type,
                        } = edge_ref.weight()
                        {
                            let from = NodeId::from(edge_ref.source());
                            if let Err(type_errors) = typecheck::validate_data_edge(
                                &self.graph,
                                from,
                                node_id,
                                *source_port,
                                *target_port,
                                *value_type,
                            ) {
                                for te in type_errors {
                                    errors.push(DiagnosticError::from(te));
                                }
                            }
                        }
                    }
                }

                Ok(VerifyResponse {
                    valid: errors.is_empty(),
                    errors,
                    warnings: Vec::new(),
                })
            }
            VerifyScope::Full => {
                let type_errors = typecheck::validate_graph(&self.graph);
                let errors: Vec<DiagnosticError> =
                    type_errors.into_iter().map(DiagnosticError::from).collect();

                Ok(VerifyResponse {
                    valid: errors.is_empty(),
                    errors,
                    warnings: Vec::new(),
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // Query methods (TOOL-02)
    // -----------------------------------------------------------------------

    /// Gets a single node by ID.
    pub fn get_node(&self, node_id: NodeId, detail: DetailLevel) -> Result<NodeView, ApiError> {
        let node = self
            .graph
            .get_compute_node(node_id)
            .ok_or_else(|| ApiError::NotFound(format!("node {} not found", node_id.0)))?;
        Ok(self.render_node(node_id, node, detail))
    }

    /// Gets a function and all its contents (nodes + edges).
    pub fn get_function_context(
        &self,
        func_id: FunctionId,
        detail: DetailLevel,
    ) -> Result<GetFunctionResponse, ApiError> {
        let func_def = self
            .graph
            .get_function(func_id)
            .ok_or_else(|| ApiError::NotFound(format!("function {} not found", func_id.0)))?;

        let func_node_ids = self.graph.function_nodes(func_id);
        let func_node_set: HashSet<NodeId> = func_node_ids.iter().copied().collect();

        let mut nodes = Vec::new();
        for &nid in &func_node_ids {
            if let Some(node) = self.graph.get_compute_node(nid) {
                nodes.push(self.render_node(nid, node, detail));
            }
        }

        let mut edges = Vec::new();
        for &nid in &func_node_ids {
            let node_idx: NodeIndex<u32> = nid.into();
            for edge_ref in self
                .graph
                .compute()
                .edges_directed(node_idx, Direction::Outgoing)
            {
                let target = NodeId::from(edge_ref.target());
                if func_node_set.contains(&target) {
                    edges.push(self.render_edge_ref(edge_ref));
                }
            }
        }

        let func_view = FunctionView {
            id: func_id,
            name: func_def.name.clone(),
            module: func_def.module,
            params: func_def.params.clone(),
            return_type: func_def.return_type,
            visibility: func_def.visibility,
            is_closure: func_def.is_closure,
            node_count: func_node_ids.len(),
        };

        Ok(GetFunctionResponse {
            function: func_view,
            nodes,
            edges,
        })
    }

    /// Gets the N-hop neighborhood around a node (BFS, capped at 3 hops).
    pub fn get_neighborhood(
        &self,
        node_id: NodeId,
        max_hops: u32,
        detail: DetailLevel,
    ) -> Result<NeighborhoodResponse, ApiError> {
        // Verify node exists
        self.graph
            .get_compute_node(node_id)
            .ok_or_else(|| ApiError::NotFound(format!("node {} not found", node_id.0)))?;

        let max_hops = max_hops.min(3); // Cap at 3
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut queue: VecDeque<(NodeId, u32)> = VecDeque::new();
        let mut actual_hops = 0u32;

        visited.insert(node_id);
        queue.push_back((node_id, 0));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }

            let idx: NodeIndex<u32> = current.into();

            // Visit neighbors in both directions
            for edge_ref in self
                .graph
                .compute()
                .edges_directed(idx, Direction::Outgoing)
            {
                let neighbor = NodeId::from(edge_ref.target());
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                    actual_hops = actual_hops.max(depth + 1);
                }
            }
            for edge_ref in self
                .graph
                .compute()
                .edges_directed(idx, Direction::Incoming)
            {
                let neighbor = NodeId::from(edge_ref.source());
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                    actual_hops = actual_hops.max(depth + 1);
                }
            }
        }

        // Build node views
        let mut nodes = Vec::new();
        for &nid in &visited {
            if let Some(node) = self.graph.get_compute_node(nid) {
                nodes.push(self.render_node(nid, node, detail));
            }
        }

        // Build edge views (only edges between visited nodes)
        let mut edges = Vec::new();
        for &nid in &visited {
            let idx: NodeIndex<u32> = nid.into();
            for edge_ref in self
                .graph
                .compute()
                .edges_directed(idx, Direction::Outgoing)
            {
                let target = NodeId::from(edge_ref.target());
                if visited.contains(&target) {
                    edges.push(self.render_edge_ref(edge_ref));
                }
            }
        }

        Ok(NeighborhoodResponse {
            center: node_id,
            nodes,
            edges,
            hops_used: actual_hops,
        })
    }

    /// Searches nodes by filter criteria.
    pub fn search_nodes(&self, filter: SearchRequest) -> Result<SearchResponse, ApiError> {
        let mut nodes = Vec::new();

        for node_idx in self.graph.compute().node_indices() {
            let node = match self.graph.compute().node_weight(node_idx) {
                Some(n) => n,
                None => continue,
            };
            let node_id = NodeId::from(node_idx);

            // Apply filters
            if let Some(ref filter_type) = filter.filter_type {
                let op_name = format!("{:?}", node.op);
                if !op_name.contains(filter_type.as_str()) {
                    continue;
                }
            }

            if let Some(owner_fn) = filter.owner_function {
                if node.owner != owner_fn {
                    continue;
                }
            }

            if let Some(vt) = filter.value_type {
                // Check if any connected edge has this value type
                let has_type = self
                    .graph
                    .compute()
                    .edges_directed(node_idx, Direction::Incoming)
                    .chain(
                        self.graph
                            .compute()
                            .edges_directed(node_idx, Direction::Outgoing),
                    )
                    .any(|edge_ref| match edge_ref.weight() {
                        FlowEdge::Data { value_type, .. } => *value_type == vt,
                        _ => false,
                    });

                if !has_type {
                    continue;
                }
            }

            nodes.push(self.render_node(node_id, node, filter.detail));
        }

        let total_count = nodes.len();
        Ok(SearchResponse { nodes, total_count })
    }

    /// Retrieves semantic graph entities and relationships for retrieval/navigation.
    pub fn semantic_query(
        &self,
        include_embeddings: bool,
    ) -> Result<SemanticQueryResponse, ApiError> {
        let mut nodes = Vec::new();
        for idx in self.graph.semantic().node_indices() {
            let node = match self.graph.semantic().node_weight(idx) {
                Some(n) => n,
                None => continue,
            };
            let metadata = node.metadata();
            let embeddings = &metadata.embeddings;

            nodes.push(SemanticNodeView {
                id: idx.index() as u32,
                kind: node.kind().to_string(),
                label: node.label(),
                ownership: SemanticOwnershipView {
                    module: metadata.ownership.module,
                    function: metadata.ownership.function,
                    domain: metadata.ownership.domain.clone(),
                },
                provenance: SemanticProvenanceView {
                    source: metadata.provenance.source.clone(),
                    version: metadata.provenance.version,
                    created_at_ms: metadata.provenance.created_at_ms,
                    updated_at_ms: metadata.provenance.updated_at_ms,
                },
                summary_title: metadata.summary.title.clone(),
                summary_body: metadata.summary.body.clone(),
                summary_checksum: metadata.summary.checksum.clone(),
                token_count: metadata.summary.token_count,
                complexity: metadata.complexity,
                has_node_embedding: embeddings.node_embedding.is_some(),
                node_embedding_dim: embeddings.node_dim(),
                has_subgraph_summary_embedding: embeddings.subgraph_summary_embedding.is_some(),
                subgraph_summary_embedding_dim: embeddings.summary_dim(),
                node_embedding: if include_embeddings {
                    embeddings.node_embedding.clone()
                } else {
                    None
                },
                subgraph_summary_embedding: if include_embeddings {
                    embeddings.subgraph_summary_embedding.clone()
                } else {
                    None
                },
            });
        }

        let mut edges = Vec::new();
        for edge_idx in self.graph.semantic().edge_indices() {
            let Some((from, to)) = self.graph.semantic().edge_endpoints(edge_idx) else {
                continue;
            };
            let Some(weight) = self.graph.semantic().edge_weight(edge_idx) else {
                continue;
            };
            edges.push(SemanticEdgeView {
                id: edge_idx.index() as u32,
                from: from.index() as u32,
                to: to.index() as u32,
                relationship: *weight,
            });
        }

        Ok(SemanticQueryResponse {
            node_count: nodes.len(),
            edge_count: edges.len(),
            nodes,
            edges,
        })
    }

    /// Projects the dual-layer graph into a UI-oriented observability payload.
    pub fn observability_graph(
        &self,
        request: ObservabilityGraphRequest,
    ) -> Result<ObservabilityGraphResponse, ApiError> {
        let mut function_name_by_id = HashMap::new();
        let mut groups = Vec::new();
        for function_id in self.graph.sorted_function_ids() {
            let Some(function) = self.graph.get_function(function_id) else {
                continue;
            };
            function_name_by_id.insert(function_id, function.name.clone());
            let compute_node_ids: Vec<String> = self
                .graph
                .function_nodes_sorted(function_id)
                .into_iter()
                .map(observability_compute_node_id)
                .collect();
            groups.push(ObservabilityGroupView {
                id: observability_group_id(function_id),
                function_id,
                function_name: function.name.clone(),
                module_id: function.module,
                semantic_anchor_id: self
                    .graph
                    .semantic_node_id_for_function(function_id)
                    .map(observability_semantic_node_id),
                compute_node_ids,
            });
        }
        groups.sort_by(|a, b| a.function_id.0.cmp(&b.function_id.0));

        let mut nodes = Vec::new();
        let mut semantic_indices: Vec<_> = self.graph.semantic().node_indices().collect();
        semantic_indices.sort_by_key(|idx| idx.index());
        if preset_includes_layer(request.preset, ObservabilityLayer::Semantic) {
            for idx in semantic_indices {
                let Some(node) = self.graph.semantic().node_weight(idx) else {
                    continue;
                };
                let semantic_node_id = idx.index() as u32;
                let function_id = node.function_id();
                nodes.push(ObservabilityNodeView {
                    id: observability_semantic_node_id(semantic_node_id),
                    layer: ObservabilityLayer::Semantic,
                    kind: node.kind().to_string(),
                    label: node.label(),
                    short_label: abbreviate_label(&node.label(), 20),
                    group_id: function_id.map(observability_group_id),
                    function_id,
                    function_name: function_id
                        .and_then(|fid| function_name_by_id.get(&fid).cloned()),
                    module_id: node.module_id(),
                    compute_node_id: None,
                    semantic_node_id: Some(semantic_node_id),
                    summary: Some(node.metadata().summary.body.clone()),
                });
            }
        }

        let mut compute_indices: Vec<_> = self.graph.compute().node_indices().collect();
        compute_indices.sort_by_key(|idx| idx.index());
        if preset_includes_layer(request.preset, ObservabilityLayer::Compute) {
            for idx in compute_indices {
                let Some(node) = self.graph.compute().node_weight(idx) else {
                    continue;
                };
                let node_id = NodeId::from(idx);
                nodes.push(ObservabilityNodeView {
                    id: observability_compute_node_id(node_id),
                    layer: ObservabilityLayer::Compute,
                    kind: op_kind_name(&node.op),
                    label: format!("{} #{}", op_kind_name(&node.op), node_id.0),
                    short_label: op_kind_name(&node.op),
                    group_id: Some(observability_group_id(node.owner)),
                    function_id: Some(node.owner),
                    function_name: function_name_by_id.get(&node.owner).cloned(),
                    module_id: self.graph.get_function(node.owner).map(|f| f.module),
                    compute_node_id: Some(node_id),
                    semantic_node_id: None,
                    summary: None,
                });
            }
        }

        nodes.sort_by(|a, b| {
            layer_rank(a.layer)
                .cmp(&layer_rank(b.layer))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut edges = Vec::new();
        let mut compute_edge_indices: Vec<_> = self.graph.compute().edge_indices().collect();
        compute_edge_indices.sort_by_key(|idx| idx.index());
        for edge_idx in compute_edge_indices {
            let Some((from_idx, to_idx)) = self.graph.compute().edge_endpoints(edge_idx) else {
                continue;
            };
            let Some(weight) = self.graph.compute().edge_weight(edge_idx) else {
                continue;
            };
            edges.push(match weight {
                FlowEdge::Data {
                    source_port,
                    target_port,
                    value_type,
                } => ObservabilityEdgeView {
                    id: format!("compute-edge:{}", edge_idx.index()),
                    from: observability_compute_node_id(NodeId::from(from_idx)),
                    to: observability_compute_node_id(NodeId::from(to_idx)),
                    from_layer: ObservabilityLayer::Compute,
                    to_layer: ObservabilityLayer::Compute,
                    edge_kind: "data".to_string(),
                    cross_layer: false,
                    value_type: Some(*value_type),
                    source_port: Some(*source_port),
                    target_port: Some(*target_port),
                    branch_index: None,
                    relationship: None,
                },
                FlowEdge::Control { branch_index } => ObservabilityEdgeView {
                    id: format!("compute-edge:{}", edge_idx.index()),
                    from: observability_compute_node_id(NodeId::from(from_idx)),
                    to: observability_compute_node_id(NodeId::from(to_idx)),
                    from_layer: ObservabilityLayer::Compute,
                    to_layer: ObservabilityLayer::Compute,
                    edge_kind: "control".to_string(),
                    cross_layer: false,
                    value_type: None,
                    source_port: None,
                    target_port: None,
                    branch_index: *branch_index,
                    relationship: None,
                },
            });
        }

        let mut semantic_edge_indices: Vec<_> = self.graph.semantic().edge_indices().collect();
        semantic_edge_indices.sort_by_key(|idx| idx.index());
        for edge_idx in semantic_edge_indices {
            let Some((from_idx, to_idx)) = self.graph.semantic().edge_endpoints(edge_idx) else {
                continue;
            };
            let Some(weight) = self.graph.semantic().edge_weight(edge_idx) else {
                continue;
            };
            edges.push(ObservabilityEdgeView {
                id: format!("semantic-edge:{}", edge_idx.index()),
                from: observability_semantic_node_id(from_idx.index() as u32),
                to: observability_semantic_node_id(to_idx.index() as u32),
                from_layer: ObservabilityLayer::Semantic,
                to_layer: ObservabilityLayer::Semantic,
                edge_kind: semantic_edge_kind(*weight),
                cross_layer: false,
                value_type: None,
                source_port: None,
                target_port: None,
                branch_index: None,
                relationship: Some(*weight),
            });
        }

        if request.include_cross_layer {
            for group in &groups {
                let Some(semantic_anchor_id) = &group.semantic_anchor_id else {
                    continue;
                };
                for compute_id in &group.compute_node_ids {
                    edges.push(ObservabilityEdgeView {
                        id: format!("cross-edge:{}:{}", group.function_id.0, compute_id),
                        from: semantic_anchor_id.clone(),
                        to: compute_id.clone(),
                        from_layer: ObservabilityLayer::Semantic,
                        to_layer: ObservabilityLayer::Compute,
                        edge_kind: "function_boundary".to_string(),
                        cross_layer: true,
                        value_type: None,
                        source_port: None,
                        target_port: None,
                        branch_index: None,
                        relationship: None,
                    });
                }
            }
        }

        let node_ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
        edges.retain(|edge| {
            node_ids.contains(&edge.from)
                && node_ids.contains(&edge.to)
                && preset_includes_edge(request.preset, edge)
        });
        edges.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(ObservabilityGraphResponse {
            preset: request.preset,
            node_count: nodes.len(),
            edge_count: edges.len(),
            nodes,
            edges,
            groups,
        })
    }

    /// Executes a natural-language observability query with ranking,
    /// disambiguation, and low-confidence fallback.
    pub fn observability_query(
        &self,
        request: ObservabilityQueryRequest,
    ) -> Result<ObservabilityQueryResponse, ApiError> {
        let query = request.query.trim();
        if query.is_empty() {
            return Err(ApiError::BadRequest(
                "query must be non-empty for observability search".to_string(),
            ));
        }

        let max_results = request.max_results.clamp(1, 10);
        let query_terms = normalized_terms(query);

        #[derive(Clone)]
        struct Candidate {
            semantic_node_id: u32,
            label: String,
            score: f32,
            lexical: f32,
            reason: String,
        }

        let mut candidates = Vec::new();
        let mut semantic_indices: Vec<_> = self.graph.semantic().node_indices().collect();
        semantic_indices.sort_by_key(|idx| idx.index());
        for idx in semantic_indices {
            let Some(node) = self.graph.semantic().node_weight(idx) else {
                continue;
            };
            let metadata = node.metadata();
            let semantic_node_id = idx.index() as u32;

            let corpus = format!(
                "{} {} {}",
                node.label(),
                metadata.summary.title,
                metadata.summary.body
            );
            let lexical = lexical_overlap_score(&query_terms, &normalized_terms(&corpus));
            let relationship_degree = self
                .graph
                .semantic()
                .edges_directed(idx, Direction::Incoming)
                .count()
                + self
                    .graph
                    .semantic()
                    .edges_directed(idx, Direction::Outgoing)
                    .count();
            let relationship_score = (relationship_degree as f32 / 6.0).min(1.0);

            let embedding_score = embedding_similarity(
                query,
                metadata
                    .embeddings
                    .node_embedding
                    .as_ref()
                    .or(metadata.embeddings.subgraph_summary_embedding.as_ref()),
            )
            .unwrap_or(0.0);

            let score = 0.55 * embedding_score + 0.35 * lexical + 0.10 * relationship_score;
            let reason = if embedding_score >= lexical {
                "embedding".to_string()
            } else {
                "text-match".to_string()
            };

            candidates.push(Candidate {
                semantic_node_id,
                label: node.label(),
                score,
                lexical,
                reason,
            });
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.semantic_node_id.cmp(&b.semantic_node_id))
        });

        let selected_candidate = match &request.selected_candidate_id {
            Some(candidate_id) => {
                let maybe_id = candidate_id
                    .strip_prefix("semantic:")
                    .and_then(|id| id.parse::<u32>().ok());
                let Some(semantic_node_id) = maybe_id else {
                    return Err(ApiError::BadRequest(format!(
                        "invalid selected_candidate_id '{}'",
                        candidate_id
                    )));
                };
                candidates
                    .iter()
                    .find(|c| c.semantic_node_id == semantic_node_id)
                    .cloned()
            }
            None => candidates.first().cloned(),
        };

        let confidence = selected_candidate.as_ref().map(|c| c.score).unwrap_or(0.0);
        let top_gap_small =
            candidates.len() > 1 && (candidates[0].score - candidates[1].score).abs() <= 0.12;
        let lexical_tie = candidates.len() > 1
            && candidates[0].lexical >= 0.8
            && candidates[1].lexical >= 0.8
            && query_terms.len() <= 2;
        let ambiguous = request.selected_candidate_id.is_none()
            && candidates.len() > 1
            && (top_gap_small || lexical_tie)
            && candidates[0].score >= 0.20;
        let low_confidence = confidence < 0.28;

        let mut selected_ids = Vec::new();
        if let Some(primary) = &selected_candidate {
            selected_ids.push(primary.semantic_node_id);
        }

        if low_confidence {
            if let Some(primary) = &selected_candidate {
                let primary_idx = NodeIndex::<u32>::new(primary.semantic_node_id as usize);
                let mut neighbor_ids = Vec::new();
                for edge_ref in self
                    .graph
                    .semantic()
                    .edges_directed(primary_idx, Direction::Outgoing)
                {
                    neighbor_ids.push(edge_ref.target().index() as u32);
                }
                for edge_ref in self
                    .graph
                    .semantic()
                    .edges_directed(primary_idx, Direction::Incoming)
                {
                    neighbor_ids.push(edge_ref.source().index() as u32);
                }
                neighbor_ids.sort_unstable();
                neighbor_ids.dedup();
                for id in neighbor_ids {
                    if selected_ids.len() >= max_results {
                        break;
                    }
                    if !selected_ids.contains(&id) {
                        selected_ids.push(id);
                    }
                }
            }
            if selected_ids.is_empty() {
                selected_ids.extend(
                    candidates
                        .iter()
                        .take(max_results)
                        .map(|candidate| candidate.semantic_node_id),
                );
            }
        } else {
            selected_ids.extend(
                candidates
                    .iter()
                    .take(max_results)
                    .map(|candidate| candidate.semantic_node_id),
            );
            selected_ids.sort_unstable();
            selected_ids.dedup();
        }

        let candidate_score_by_id: HashMap<u32, f32> = candidates
            .iter()
            .map(|candidate| (candidate.semantic_node_id, candidate.score))
            .collect();

        let mut results = Vec::new();
        for (rank, semantic_node_id) in selected_ids.iter().copied().enumerate() {
            let idx = NodeIndex::<u32>::new(semantic_node_id as usize);
            let Some(node) = self.graph.semantic().node_weight(idx) else {
                continue;
            };

            let relationships = self.query_relationship_tab(semantic_node_id);
            let contracts = self.query_contracts_tab(node);

            results.push(ObservabilityQueryResultView {
                rank: rank + 1,
                node_id: observability_semantic_node_id(semantic_node_id),
                label: node.label(),
                layer: ObservabilityLayer::Semantic,
                score: *candidate_score_by_id.get(&semantic_node_id).unwrap_or(&0.0),
                related_node_ids: relationships.mini_graph_node_ids.clone(),
                summary: QuerySummaryTabView {
                    title: node.metadata().summary.title.clone(),
                    body: node.metadata().summary.body.clone(),
                    module_id: node.module_id(),
                    function_id: node.function_id(),
                    function_name: node
                        .function_id()
                        .and_then(|fid| self.graph.get_function(fid).map(|f| f.name.clone())),
                    complexity: node.metadata().complexity,
                },
                relationships,
                contracts,
            });
        }

        let interpretations = candidates
            .iter()
            .take(3)
            .map(|candidate| QueryInterpretationView {
                candidate_id: observability_semantic_node_id(candidate.semantic_node_id),
                node_id: observability_semantic_node_id(candidate.semantic_node_id),
                label: candidate.label.clone(),
                score: candidate.score,
                reason: candidate.reason.clone(),
            })
            .collect();

        Ok(ObservabilityQueryResponse {
            query: query.to_string(),
            suggested_prompts: default_prompt_chips(),
            ambiguous,
            low_confidence,
            confidence,
            ambiguity_prompt: if ambiguous {
                Some(
                    "This query matches multiple semantic entities. Choose an interpretation."
                        .to_string(),
                )
            } else {
                None
            },
            interpretations,
            fallback_reason: if low_confidence {
                Some("No strong semantic match; showing nearest related nodes.".to_string())
            } else {
                None
            },
            selected_graph_node_id: results.first().map(|result| result.node_id.clone()),
            results,
        })
    }

    /// Returns a high-level program overview.
    pub fn program_overview(&self) -> Result<ProgramOverviewResponse, ApiError> {
        let modules: Vec<ModuleId> = self
            .graph
            .module_semantic_indices()
            .keys()
            .copied()
            .collect();

        let functions: Vec<FunctionId> = self.graph.functions().keys().copied().collect();

        Ok(ProgramOverviewResponse {
            program_id: self.program_id,
            name: "default".to_string(), // TODO: store program name on service
            modules,
            functions,
            node_count: self.graph.node_count(),
            edge_count: self.graph.edge_count(),
        })
    }

    fn query_relationship_tab(&self, semantic_node_id: u32) -> QueryRelationshipsTabView {
        let idx = NodeIndex::<u32>::new(semantic_node_id as usize);
        let mut mini_graph_node_ids = vec![observability_semantic_node_id(semantic_node_id)];
        let mut mini_graph_edge_ids = Vec::new();
        let mut items = Vec::new();

        for edge_ref in self
            .graph
            .semantic()
            .edges_directed(idx, Direction::Outgoing)
        {
            let neighbor_id = edge_ref.target().index() as u32;
            if let Some(neighbor) = self.graph.semantic().node_weight(edge_ref.target()) {
                mini_graph_node_ids.push(observability_semantic_node_id(neighbor_id));
                mini_graph_edge_ids.push(format!("semantic-edge:{}", edge_ref.id().index()));
                items.push(QueryRelationshipItemView {
                    direction: "outgoing".to_string(),
                    edge_kind: semantic_edge_kind(*edge_ref.weight()),
                    node_id: observability_semantic_node_id(neighbor_id),
                    label: neighbor.label(),
                });
            }
        }

        for edge_ref in self
            .graph
            .semantic()
            .edges_directed(idx, Direction::Incoming)
        {
            let neighbor_id = edge_ref.source().index() as u32;
            if let Some(neighbor) = self.graph.semantic().node_weight(edge_ref.source()) {
                mini_graph_node_ids.push(observability_semantic_node_id(neighbor_id));
                mini_graph_edge_ids.push(format!("semantic-edge:{}", edge_ref.id().index()));
                items.push(QueryRelationshipItemView {
                    direction: "incoming".to_string(),
                    edge_kind: semantic_edge_kind(*edge_ref.weight()),
                    node_id: observability_semantic_node_id(neighbor_id),
                    label: neighbor.label(),
                });
            }
        }

        if let Some(node) = self.graph.semantic().node_weight(idx) {
            if let Some(function_id) = node.function_id() {
                let compute_nodes = self.graph.function_nodes_sorted(function_id);
                for node_id in compute_nodes {
                    if let Some(compute_node) = self.graph.get_compute_node(node_id) {
                        mini_graph_node_ids.push(observability_compute_node_id(node_id));
                        mini_graph_edge_ids.push(format!(
                            "cross-edge:{}:{}",
                            function_id.0,
                            observability_compute_node_id(node_id)
                        ));
                        items.push(QueryRelationshipItemView {
                            direction: "boundary".to_string(),
                            edge_kind: "function_boundary".to_string(),
                            node_id: observability_compute_node_id(node_id),
                            label: op_kind_name(&compute_node.op),
                        });
                    }
                }
            }
        }

        mini_graph_node_ids.sort();
        mini_graph_node_ids.dedup();
        mini_graph_edge_ids.sort();
        mini_graph_edge_ids.dedup();
        items.sort_by(|a, b| a.node_id.cmp(&b.node_id));

        QueryRelationshipsTabView {
            mini_graph_node_ids,
            mini_graph_edge_ids,
            items,
        }
    }

    fn query_contracts_tab(&self, semantic_node: &SemanticNode) -> QueryContractsTabView {
        let mut entries = Vec::new();
        if let Some(function_id) = semantic_node.function_id() {
            for node_id in self.graph.function_nodes_sorted(function_id) {
                let Some(node) = self.graph.get_compute_node(node_id) else {
                    continue;
                };
                let contract_kind = contract_name_from_op(&node.op);
                let message = contract_message_from_op(&node.op);
                if let (Some(contract_kind), Some(message)) = (contract_kind, message) {
                    entries.push(QueryContractEntryView {
                        node_id: observability_compute_node_id(node_id),
                        contract_kind,
                        message,
                    });
                }
            }
        }

        entries.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        QueryContractsTabView {
            has_contracts: !entries.is_empty(),
            entries,
        }
    }

    // -----------------------------------------------------------------------
    // Detail level rendering helpers
    // -----------------------------------------------------------------------

    /// Renders a node view at the specified detail level.
    fn render_node(&self, node_id: NodeId, node: &ComputeNode, detail: DetailLevel) -> NodeView {
        let node_idx: NodeIndex<u32> = node_id.into();

        match detail {
            DetailLevel::Summary => NodeView {
                id: node_id,
                op: node.op.clone(),
                owner: node.owner,
                op_data: None,
                incoming_edges: None,
                outgoing_edges: None,
            },
            DetailLevel::Standard => {
                let incoming: Vec<EdgeId> = self
                    .graph
                    .compute()
                    .edges_directed(node_idx, Direction::Incoming)
                    .map(|e| EdgeId(e.id().index() as u32))
                    .collect();

                NodeView {
                    id: node_id,
                    op: node.op.clone(),
                    owner: node.owner,
                    op_data: None,
                    incoming_edges: Some(incoming),
                    outgoing_edges: None,
                }
            }
            DetailLevel::Full => {
                let incoming: Vec<EdgeId> = self
                    .graph
                    .compute()
                    .edges_directed(node_idx, Direction::Incoming)
                    .map(|e| EdgeId(e.id().index() as u32))
                    .collect();
                let outgoing: Vec<EdgeId> = self
                    .graph
                    .compute()
                    .edges_directed(node_idx, Direction::Outgoing)
                    .map(|e| EdgeId(e.id().index() as u32))
                    .collect();
                let op_data = serde_json::to_value(&node.op).ok();

                NodeView {
                    id: node_id,
                    op: node.op.clone(),
                    owner: node.owner,
                    op_data,
                    incoming_edges: Some(incoming),
                    outgoing_edges: Some(outgoing),
                }
            }
        }
    }

    /// Renders a petgraph edge reference into an EdgeView.
    fn render_edge_ref(
        &self,
        edge_ref: petgraph::stable_graph::EdgeReference<'_, FlowEdge, u32>,
    ) -> EdgeView {
        let edge_idx = edge_ref.id();
        let from = NodeId::from(edge_ref.source());
        let to = NodeId::from(edge_ref.target());
        match edge_ref.weight() {
            FlowEdge::Data {
                source_port,
                target_port,
                value_type,
            } => EdgeView {
                id: EdgeId(edge_idx.index() as u32),
                from,
                to,
                kind: "data".to_string(),
                value_type: Some(*value_type),
                source_port: Some(*source_port),
                target_port: Some(*target_port),
                branch_index: None,
            },
            FlowEdge::Control { branch_index } => EdgeView {
                id: EdgeId(edge_idx.index() as u32),
                from,
                to,
                kind: "control".to_string(),
                value_type: None,
                source_port: None,
                target_port: None,
                branch_index: *branch_index,
            },
        }
    }

    // -----------------------------------------------------------------------
    // Simulate method (TOOL-04)
    // -----------------------------------------------------------------------

    /// Runs the interpreter on a function with provided inputs.
    pub fn simulate(&self, request: SimulateRequest) -> Result<SimulateResponse, ApiError> {
        // Verify function exists
        let func_def = self
            .graph
            .get_function(request.function_id)
            .ok_or_else(|| {
                ApiError::NotFound(format!("function {} not found", request.function_id.0))
            })?;

        // Convert JSON inputs to interpreter Values
        let mut inputs = Vec::new();
        for (i, json_val) in request.inputs.iter().enumerate() {
            let value = json_to_value(json_val, func_def.params.get(i).map(|(_, t)| *t));
            inputs.push(value);
        }

        let trace_enabled = request.trace_enabled.unwrap_or(false);
        let config = InterpreterConfig {
            trace_enabled,
            max_recursion_depth: 256,
        };

        let mut interp = Interpreter::new(&self.graph, config);
        interp.start(request.function_id, inputs);
        interp.run();

        match interp.state() {
            ExecutionState::Completed { result } => {
                let result_json = serde_json::to_value(result).ok();
                let trace = if trace_enabled {
                    interp.trace().map(|entries| {
                        entries
                            .iter()
                            .map(|e| TraceEntryView {
                                node_id: e.node_id,
                                op: e.op_description.clone(),
                                inputs: e
                                    .inputs
                                    .iter()
                                    .map(|(port, val)| {
                                        (*port, serde_json::to_value(val).unwrap_or_default())
                                    })
                                    .collect(),
                                output: e
                                    .output
                                    .as_ref()
                                    .and_then(|v| serde_json::to_value(v).ok()),
                            })
                            .collect()
                    })
                } else {
                    None
                };

                let io_log: Vec<serde_json::Value> = interp
                    .io_log()
                    .iter()
                    .filter_map(|v| serde_json::to_value(v).ok())
                    .collect();

                Ok(SimulateResponse {
                    success: true,
                    result: result_json,
                    trace,
                    error: None,
                    io_log,
                })
            }
            ExecutionState::Error { error, .. } => {
                let io_log: Vec<serde_json::Value> = interp
                    .io_log()
                    .iter()
                    .filter_map(|v| serde_json::to_value(v).ok())
                    .collect();

                Ok(SimulateResponse {
                    success: false,
                    result: None,
                    trace: None,
                    error: Some(DiagnosticError {
                        code: "RUNTIME_ERROR".to_string(),
                        message: format!("{}", error),
                        details: None,
                    }),
                    io_log,
                })
            }
            _ => Err(ApiError::InternalError(
                "interpreter in unexpected state".to_string(),
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Property testing method (CNTR-05)
    // -----------------------------------------------------------------------

    /// Runs property-based tests on a function's contracts.
    ///
    /// Converts JSON seed inputs to interpreter Values, builds a PropertyTestConfig,
    /// runs the harness, and converts results to the API response format.
    pub fn property_test(
        &self,
        request: crate::schema::contracts::PropertyTestRequest,
    ) -> Result<crate::schema::contracts::PropertyTestResponse, ApiError> {
        use crate::schema::contracts::{
            ContractViolationView, PropertyTestFailureView, PropertyTestResponse, TraceEntryView,
        };
        use lmlang_check::contracts::property::{run_property_tests, PropertyTestConfig};

        let func_id = FunctionId(request.function_id);

        // Verify function exists and get param types for seed conversion
        let func_def = self.graph.get_function(func_id).ok_or_else(|| {
            ApiError::NotFound(format!("function {} not found", request.function_id))
        })?;

        let param_types: Vec<Option<TypeId>> =
            func_def.params.iter().map(|(_, t)| Some(*t)).collect();

        // Convert JSON seeds to interpreter Values
        let mut seeds = Vec::new();
        for seed_json in &request.seeds {
            let mut seed_values = Vec::new();
            for (i, json_val) in seed_json.iter().enumerate() {
                let type_hint = param_types.get(i).copied().flatten();
                let value = json_to_value(json_val, type_hint);
                seed_values.push(value);
            }
            seeds.push(seed_values);
        }

        // Generate random seed if not provided
        let random_seed = request.random_seed.unwrap_or_else(|| {
            use std::time::SystemTime;
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(42)
        });

        let config = PropertyTestConfig {
            seeds,
            iterations: request.iterations,
            random_seed,
        };

        let result = run_property_tests(&self.graph, func_id, config)
            .map_err(|e| ApiError::InternalError(format!("property test failed: {}", e)))?;

        // Convert to API response
        let failures: Vec<PropertyTestFailureView> = result
            .failures
            .iter()
            .map(|f| {
                let violation = &f.violation;
                let violation_view = ContractViolationView {
                    kind: match violation.kind {
                        lmlang_check::contracts::ContractKind::Precondition => {
                            "precondition".to_string()
                        }
                        lmlang_check::contracts::ContractKind::Postcondition => {
                            "postcondition".to_string()
                        }
                        lmlang_check::contracts::ContractKind::Invariant => "invariant".to_string(),
                    },
                    contract_node: violation.contract_node.0,
                    function_id: violation.function_id.0,
                    message: violation.message.clone(),
                    inputs: violation
                        .inputs
                        .iter()
                        .filter_map(|v| serde_json::to_value(v).ok())
                        .collect(),
                    actual_return: violation
                        .actual_return
                        .as_ref()
                        .and_then(|v| serde_json::to_value(v).ok()),
                    counterexample: violation
                        .counterexample
                        .iter()
                        .filter_map(|(nid, v)| serde_json::to_value(v).ok().map(|jv| (nid.0, jv)))
                        .collect(),
                };

                let trace = if request.trace_failures {
                    Some(
                        f.trace
                            .iter()
                            .map(|t| TraceEntryView {
                                node_id: t.node_id,
                                op: t.op_description.clone(),
                                inputs: t
                                    .inputs
                                    .iter()
                                    .map(|(port, val)| {
                                        (*port, serde_json::to_value(val).unwrap_or_default())
                                    })
                                    .collect(),
                                output: t
                                    .output
                                    .as_ref()
                                    .and_then(|v| serde_json::to_value(v).ok()),
                            })
                            .collect(),
                    )
                } else {
                    None
                };

                PropertyTestFailureView {
                    inputs: f
                        .inputs
                        .iter()
                        .filter_map(|v| serde_json::to_value(v).ok())
                        .collect(),
                    violation: violation_view,
                    trace,
                }
            })
            .collect();

        let failed = failures.len() as u32;

        Ok(PropertyTestResponse {
            total_run: result.total_run,
            passed: result.passed,
            failed,
            random_seed: result.random_seed,
            failures,
        })
    }

    // -----------------------------------------------------------------------
    // Compile method (EXEC-03/04)
    // -----------------------------------------------------------------------

    /// Compiles the current program graph to a native executable.
    ///
    /// Parses the opt_level string from the HTTP request, builds CompileOptions,
    /// and delegates to `lmlang_codegen::compile()`. TypeCheckFailed maps to 422,
    /// other errors to 500.
    pub fn compile(
        &self,
        request: &crate::schema::compile::CompileRequest,
    ) -> Result<crate::schema::compile::CompileResponse, ApiError> {
        let opt_level = parse_opt_level(&request.opt_level)?;

        let options = lmlang_codegen::CompileOptions {
            output_dir: request
                .output_dir
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("./build")),
            opt_level,
            target_triple: request.target_triple.clone(),
            debug_symbols: request.debug_symbols,
            entry_function: request.entry_function.clone(),
        };

        let result = lmlang_codegen::compile(&self.graph, &options).map_err(|e| match e {
            lmlang_codegen::error::CodegenError::TypeCheckFailed(errors) => {
                let diags: Vec<crate::schema::diagnostics::DiagnosticError> = errors
                    .into_iter()
                    .map(crate::schema::diagnostics::DiagnosticError::from)
                    .collect();
                ApiError::ValidationFailed(diags)
            }
            other => ApiError::InternalError(other.to_string()),
        })?;

        Ok(crate::schema::compile::CompileResponse {
            binary_path: result.binary_path.to_string_lossy().to_string(),
            target_triple: result.target_triple,
            binary_size: result.binary_size,
            compilation_time_ms: result.compilation_time_ms,
        })
    }

    // -----------------------------------------------------------------------
    // Dirty status query (STORE-05)
    // -----------------------------------------------------------------------

    /// Returns the dirty status of all functions relative to the last compilation.
    ///
    /// If no incremental state exists (no prior compilation), all functions are
    /// reported as dirty. Otherwise, computes current hashes, builds the call
    /// graph, and runs dirty analysis to categorize functions.
    pub fn dirty_status(&self) -> Result<crate::schema::compile::DirtyStatusResponse, ApiError> {
        use crate::schema::compile::{CachedFunctionView, DirtyFunctionView, DirtyStatusResponse};
        use lmlang_codegen::incremental::build_call_graph;
        use lmlang_storage::hash::hash_all_functions_for_compilation;

        let functions = self.graph.functions();

        match &self.incremental_state {
            None => {
                // No prior compilation: everything is dirty
                let dirty_functions: Vec<DirtyFunctionView> = functions
                    .iter()
                    .map(|(&fid, fdef)| DirtyFunctionView {
                        function_id: fid.0,
                        function_name: fdef.name.clone(),
                        reason: "no_prior_compilation".to_string(),
                    })
                    .collect();

                Ok(DirtyStatusResponse {
                    dirty_functions,
                    dirty_dependents: Vec::new(),
                    cached_functions: Vec::new(),
                    needs_recompilation: true,
                })
            }
            Some(state) => {
                // Compute current hashes and call graph
                let blake_hashes = hash_all_functions_for_compilation(&self.graph);
                let current_hashes: std::collections::HashMap<FunctionId, [u8; 32]> = blake_hashes
                    .iter()
                    .map(|(&fid, h)| (fid, *h.as_bytes()))
                    .collect();

                let call_graph = build_call_graph(&self.graph);
                let plan = state.compute_dirty(&current_hashes, &call_graph);

                // Map to response views with function names
                let dirty_functions: Vec<DirtyFunctionView> = plan
                    .dirty
                    .iter()
                    .filter_map(|&fid| {
                        functions.get(&fid).map(|fdef| DirtyFunctionView {
                            function_id: fid.0,
                            function_name: fdef.name.clone(),
                            reason: "changed".to_string(),
                        })
                    })
                    .collect();

                let dirty_dependents: Vec<DirtyFunctionView> = plan
                    .dirty_dependents
                    .iter()
                    .filter_map(|&fid| {
                        functions.get(&fid).map(|fdef| DirtyFunctionView {
                            function_id: fid.0,
                            function_name: fdef.name.clone(),
                            reason: "dependent".to_string(),
                        })
                    })
                    .collect();

                let cached_functions: Vec<CachedFunctionView> = plan
                    .cached
                    .iter()
                    .filter_map(|&fid| {
                        functions.get(&fid).map(|fdef| CachedFunctionView {
                            function_id: fid.0,
                            function_name: fdef.name.clone(),
                        })
                    })
                    .collect();

                Ok(DirtyStatusResponse {
                    dirty_functions,
                    dirty_dependents,
                    cached_functions,
                    needs_recompilation: plan.needs_recompilation,
                })
            }
        }
    }

    /// Compiles using incremental compilation and updates internal state.
    ///
    /// On first call, initializes the IncrementalState. On subsequent calls,
    /// reuses the stored state for dirty detection.
    pub fn compile_incremental(
        &mut self,
        request: &crate::schema::compile::CompileRequest,
    ) -> Result<crate::schema::compile::CompileResponse, ApiError> {
        let opt_level = parse_opt_level(&request.opt_level)?;

        let cache_dir = request
            .output_dir
            .as_ref()
            .map(|d| std::path::PathBuf::from(d).join(".lmlang_cache"))
            .unwrap_or_else(|| std::path::PathBuf::from("./build/.lmlang_cache"));

        let options = lmlang_codegen::CompileOptions {
            output_dir: request
                .output_dir
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("./build")),
            opt_level,
            target_triple: request.target_triple.clone(),
            debug_symbols: request.debug_symbols,
            entry_function: request.entry_function.clone(),
        };

        // Initialize or reuse incremental state
        let state = self
            .incremental_state
            .get_or_insert_with(|| lmlang_codegen::incremental::IncrementalState::new(cache_dir));

        let (result, _plan) = lmlang_codegen::compile_incremental(&self.graph, &options, state)
            .map_err(|e| match e {
                lmlang_codegen::error::CodegenError::TypeCheckFailed(errors) => {
                    let diags: Vec<crate::schema::diagnostics::DiagnosticError> = errors
                        .into_iter()
                        .map(crate::schema::diagnostics::DiagnosticError::from)
                        .collect();
                    ApiError::ValidationFailed(diags)
                }
                other => ApiError::InternalError(other.to_string()),
            })?;

        Ok(crate::schema::compile::CompileResponse {
            binary_path: result.binary_path.to_string_lossy().to_string(),
            target_triple: result.target_triple,
            binary_size: result.binary_size,
            compilation_time_ms: result.compilation_time_ms,
        })
    }

    // -----------------------------------------------------------------------
    // Undo methods (STORE-03)
    // -----------------------------------------------------------------------

    /// Undoes the last committed mutation.
    pub fn undo(&mut self) -> Result<UndoResponse, ApiError> {
        match EditLog::undo(&self.conn, self.program_id)? {
            None => Ok(UndoResponse {
                success: false,
                restored_edit: None,
            }),
            Some((inverse_cmd, entry)) => {
                Self::apply_edit_command(&mut self.graph, &inverse_cmd)?;
                self.store.save_program(self.program_id, &self.graph)?;
                Ok(UndoResponse {
                    success: true,
                    restored_edit: Some(entry),
                })
            }
        }
    }

    /// Redoes the last undone mutation.
    pub fn redo(&mut self) -> Result<RedoResponse, ApiError> {
        match EditLog::redo(&self.conn, self.program_id)? {
            None => Ok(RedoResponse {
                success: false,
                reapplied_edit: None,
            }),
            Some((cmd, entry)) => {
                Self::apply_edit_command(&mut self.graph, &cmd)?;
                self.store.save_program(self.program_id, &self.graph)?;
                Ok(RedoResponse {
                    success: true,
                    reapplied_edit: Some(entry),
                })
            }
        }
    }

    /// Creates a named checkpoint of the current graph state.
    pub fn create_checkpoint(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<CreateCheckpointResponse, ApiError> {
        let timestamp =
            CheckpointManager::create(&self.conn, self.program_id, name, description, &self.graph)?;
        Ok(CreateCheckpointResponse {
            name: name.to_string(),
            timestamp,
        })
    }

    /// Restores a named checkpoint, replacing the current graph.
    pub fn restore_checkpoint(
        &mut self,
        name: &str,
    ) -> Result<RestoreCheckpointResponse, ApiError> {
        let graph = CheckpointManager::restore(&self.conn, self.program_id, name)?;
        self.graph = graph;
        self.store.save_program(self.program_id, &self.graph)?;
        Ok(RestoreCheckpointResponse {
            success: true,
            name: name.to_string(),
        })
    }

    /// Lists all checkpoints for the active program.
    pub fn list_checkpoints(&self) -> Result<ListCheckpointsResponse, ApiError> {
        let checkpoints = CheckpointManager::list(&self.conn, self.program_id)?;
        Ok(ListCheckpointsResponse { checkpoints })
    }

    /// Lists all edit history entries.
    pub fn list_history(&self) -> Result<ListHistoryResponse, ApiError> {
        let entries = EditLog::list(&self.conn, self.program_id)?;
        let total = entries.len();
        Ok(ListHistoryResponse { entries, total })
    }

    /// Diffs between two checkpoints or current state.
    pub fn diff_versions(
        &self,
        from_checkpoint: Option<&str>,
        to_checkpoint: Option<&str>,
    ) -> Result<DiffResponse, ApiError> {
        let from_graph = match from_checkpoint {
            Some(name) => CheckpointManager::restore(&self.conn, self.program_id, name)?,
            None => ProgramGraph::new("empty"), // Compare from empty
        };
        let to_graph = match to_checkpoint {
            Some(name) => CheckpointManager::restore(&self.conn, self.program_id, name)?,
            None => self.graph.clone(), // Compare to current
        };

        // Collect node sets
        let from_nodes: HashSet<NodeId> = from_graph
            .compute()
            .node_indices()
            .map(NodeId::from)
            .collect();
        let to_nodes: HashSet<NodeId> = to_graph
            .compute()
            .node_indices()
            .map(NodeId::from)
            .collect();

        let added_nodes: Vec<NodeId> = to_nodes.difference(&from_nodes).copied().collect();
        let removed_nodes: Vec<NodeId> = from_nodes.difference(&to_nodes).copied().collect();

        // Modified: nodes that exist in both but have different ops
        let mut modified_nodes = Vec::new();
        for &nid in from_nodes.intersection(&to_nodes) {
            let from_node = from_graph.get_compute_node(nid);
            let to_node = to_graph.get_compute_node(nid);
            if let (Some(f), Some(t)) = (from_node, to_node) {
                // Compare ops via serialization
                let f_json = serde_json::to_string(&f.op).unwrap_or_default();
                let t_json = serde_json::to_string(&t.op).unwrap_or_default();
                if f_json != t_json {
                    modified_nodes.push(nid);
                }
            }
        }

        // Collect edge sets
        let from_edges: HashSet<EdgeId> = from_graph
            .compute()
            .edge_indices()
            .map(|idx| EdgeId(idx.index() as u32))
            .collect();
        let to_edges: HashSet<EdgeId> = to_graph
            .compute()
            .edge_indices()
            .map(|idx| EdgeId(idx.index() as u32))
            .collect();

        let added_edges: Vec<EdgeId> = to_edges.difference(&from_edges).copied().collect();
        let removed_edges: Vec<EdgeId> = from_edges.difference(&to_edges).copied().collect();

        Ok(DiffResponse {
            added_nodes,
            removed_nodes,
            modified_nodes,
            added_edges,
            removed_edges,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn observability_compute_node_id(node_id: NodeId) -> String {
    format!("compute:{}", node_id.0)
}

fn observability_semantic_node_id(node_id: u32) -> String {
    format!("semantic:{}", node_id)
}

fn observability_group_id(function_id: FunctionId) -> String {
    format!("function:{}", function_id.0)
}

fn layer_rank(layer: ObservabilityLayer) -> u8 {
    match layer {
        ObservabilityLayer::Semantic => 0,
        ObservabilityLayer::Compute => 1,
    }
}

fn preset_includes_layer(preset: ObservabilityPreset, layer: ObservabilityLayer) -> bool {
    match preset {
        ObservabilityPreset::All | ObservabilityPreset::Interop => true,
        ObservabilityPreset::SemanticOnly => layer == ObservabilityLayer::Semantic,
        ObservabilityPreset::ComputeOnly => layer == ObservabilityLayer::Compute,
    }
}

fn preset_includes_edge(preset: ObservabilityPreset, edge: &ObservabilityEdgeView) -> bool {
    match preset {
        ObservabilityPreset::All => true,
        ObservabilityPreset::SemanticOnly => {
            !edge.cross_layer
                && edge.from_layer == ObservabilityLayer::Semantic
                && edge.to_layer == ObservabilityLayer::Semantic
        }
        ObservabilityPreset::ComputeOnly => {
            !edge.cross_layer
                && edge.from_layer == ObservabilityLayer::Compute
                && edge.to_layer == ObservabilityLayer::Compute
        }
        ObservabilityPreset::Interop => edge.cross_layer,
    }
}

fn semantic_edge_kind(edge: SemanticEdge) -> String {
    format!("{:?}", edge).to_ascii_lowercase()
}

fn abbreviate_label(label: &str, max_len: usize) -> String {
    if label.chars().count() <= max_len {
        label.to_string()
    } else {
        let truncated: String = label.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

fn op_kind_name(op: &ComputeNodeOp) -> String {
    match op {
        ComputeNodeOp::Core(inner) => match inner {
            ComputeOp::Const { .. } => "Const".to_string(),
            ComputeOp::BinaryArith { .. } => "BinaryArith".to_string(),
            ComputeOp::UnaryArith { .. } => "UnaryArith".to_string(),
            ComputeOp::Compare { .. } => "Compare".to_string(),
            ComputeOp::BinaryLogic { .. } => "BinaryLogic".to_string(),
            ComputeOp::Not => "Not".to_string(),
            ComputeOp::Shift { .. } => "Shift".to_string(),
            ComputeOp::IfElse => "IfElse".to_string(),
            ComputeOp::Loop => "Loop".to_string(),
            ComputeOp::Match => "Match".to_string(),
            ComputeOp::Branch => "Branch".to_string(),
            ComputeOp::Jump => "Jump".to_string(),
            ComputeOp::Phi => "Phi".to_string(),
            ComputeOp::Alloc => "Alloc".to_string(),
            ComputeOp::Load => "Load".to_string(),
            ComputeOp::Store => "Store".to_string(),
            ComputeOp::GetElementPtr => "GetElementPtr".to_string(),
            ComputeOp::Call { .. } => "Call".to_string(),
            ComputeOp::IndirectCall => "IndirectCall".to_string(),
            ComputeOp::Return => "Return".to_string(),
            ComputeOp::Parameter { .. } => "Parameter".to_string(),
            ComputeOp::Print => "Print".to_string(),
            ComputeOp::ReadLine => "ReadLine".to_string(),
            ComputeOp::FileOpen => "FileOpen".to_string(),
            ComputeOp::FileRead => "FileRead".to_string(),
            ComputeOp::FileWrite => "FileWrite".to_string(),
            ComputeOp::FileClose => "FileClose".to_string(),
            ComputeOp::MakeClosure { .. } => "MakeClosure".to_string(),
            ComputeOp::CaptureAccess { .. } => "CaptureAccess".to_string(),
            ComputeOp::Precondition { .. } => "Precondition".to_string(),
            ComputeOp::Postcondition { .. } => "Postcondition".to_string(),
            ComputeOp::Invariant { .. } => "Invariant".to_string(),
        },
        ComputeNodeOp::Structured(inner) => format!("{:?}", inner),
    }
}

fn normalized_terms(input: &str) -> Vec<String> {
    input
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn lexical_overlap_score(query_terms: &[String], corpus_terms: &[String]) -> f32 {
    if query_terms.is_empty() || corpus_terms.is_empty() {
        return 0.0;
    }
    let corpus: HashSet<&str> = corpus_terms.iter().map(String::as_str).collect();
    let overlap = query_terms
        .iter()
        .filter(|term| corpus.contains(term.as_str()))
        .count();
    overlap as f32 / query_terms.len() as f32
}

fn deterministic_embedding(seed: &str, dims: usize) -> Vec<f32> {
    if dims == 0 {
        return Vec::new();
    }
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in seed.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    let mut vec = Vec::with_capacity(dims);
    for idx in 0..dims {
        let mixed = hash.wrapping_add((idx as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let scaled = (mixed % 10_000) as f32 / 10_000.0;
        vec.push((scaled * 2.0) - 1.0);
    }
    vec
}

fn vector_cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (lhs, rhs) in a.iter().zip(b.iter()) {
        dot += lhs * rhs;
        norm_a += lhs * lhs;
        norm_b += rhs * rhs;
    }
    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        return None;
    }
    Some((dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0))
}

fn embedding_similarity(query: &str, embedding: Option<&Vec<f32>>) -> Option<f32> {
    let embedding = embedding?;
    let q = deterministic_embedding(query, embedding.len());
    vector_cosine_similarity(&q, embedding)
}

fn default_prompt_chips() -> Vec<SuggestedPromptChipView> {
    vec![
        SuggestedPromptChipView {
            id: "chip-overview".to_string(),
            label: "Program Overview".to_string(),
            query: "show the high level structure".to_string(),
        },
        SuggestedPromptChipView {
            id: "chip-contracts".to_string(),
            label: "Contract Checks".to_string(),
            query: "which functions enforce contracts".to_string(),
        },
        SuggestedPromptChipView {
            id: "chip-data-flow".to_string(),
            label: "Data Flow".to_string(),
            query: "trace how values flow through main".to_string(),
        },
        SuggestedPromptChipView {
            id: "chip-interpreter".to_string(),
            label: "Execution Path".to_string(),
            query: "what does the interpreter execute first".to_string(),
        },
    ]
}

/// Converts a serde_json::Value to an interpreter Value.
///
/// Uses the function parameter type hint to disambiguate numeric types.
fn json_to_value(json: &serde_json::Value, type_hint: Option<TypeId>) -> Value {
    match json {
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            match type_hint {
                Some(TypeId::I8) => Value::I8(n.as_i64().unwrap_or(0) as i8),
                Some(TypeId::I16) => Value::I16(n.as_i64().unwrap_or(0) as i16),
                Some(TypeId::I32) => Value::I32(n.as_i64().unwrap_or(0) as i32),
                Some(TypeId::I64) => Value::I64(n.as_i64().unwrap_or(0)),
                Some(TypeId::F32) => Value::F32(n.as_f64().unwrap_or(0.0) as f32),
                Some(TypeId::F64) => Value::F64(n.as_f64().unwrap_or(0.0)),
                _ => {
                    // Best effort: if it has a decimal point use F64, else I32
                    if n.is_f64() && n.as_i64().is_none() {
                        Value::F64(n.as_f64().unwrap_or(0.0))
                    } else {
                        Value::I32(n.as_i64().unwrap_or(0) as i32)
                    }
                }
            }
        }
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::String(s) => {
            // Try to parse as a number if type hint suggests numeric
            match type_hint {
                Some(TypeId::I32) => Value::I32(s.parse().unwrap_or(0)),
                Some(TypeId::I64) => Value::I64(s.parse().unwrap_or(0)),
                Some(TypeId::F64) => Value::F64(s.parse().unwrap_or(0.0)),
                _ => Value::Unit, // Strings don't have a direct Value equivalent
            }
        }
        serde_json::Value::Array(arr) => {
            let values: Vec<Value> = arr.iter().map(|v| json_to_value(v, None)).collect();
            Value::Array(values)
        }
        serde_json::Value::Object(_) => Value::Unit, // Objects not directly supported
    }
}

/// Parse an optimization level string to `lmlang_codegen::OptLevel`.
fn parse_opt_level(s: &str) -> Result<lmlang_codegen::OptLevel, ApiError> {
    match s {
        "O0" | "o0" => Ok(lmlang_codegen::OptLevel::O0),
        "O1" | "o1" => Ok(lmlang_codegen::OptLevel::O1),
        "O2" | "o2" => Ok(lmlang_codegen::OptLevel::O2),
        "O3" | "o3" => Ok(lmlang_codegen::OptLevel::O3),
        _ => Err(ApiError::BadRequest(format!(
            "invalid optimization level '{}', expected O0/O1/O2/O3",
            s
        ))),
    }
}

/// Generates a human-readable description for a mutation.
fn describe_mutation(mutation: &Mutation) -> String {
    match mutation {
        Mutation::InsertNode { op, owner } => {
            format!("insert {:?} node in function {}", op, owner.0)
        }
        Mutation::RemoveNode { node_id } => {
            format!("remove node {}", node_id.0)
        }
        Mutation::ModifyNode { node_id, new_op } => {
            format!("modify node {} to {:?}", node_id.0, new_op)
        }
        Mutation::AddEdge { from, to, .. } => {
            format!("add data edge {} -> {}", from.0, to.0)
        }
        Mutation::AddControlEdge { from, to, .. } => {
            format!("add control edge {} -> {}", from.0, to.0)
        }
        Mutation::RemoveEdge { edge_id } => {
            format!("remove edge {}", edge_id.0)
        }
        Mutation::AddFunction { name, .. } => {
            format!("add function '{}'", name)
        }
        Mutation::AddModule { name, .. } => {
            format!("add module '{}'", name)
        }
    }
}

fn contract_name_from_op(op: &ComputeNodeOp) -> Option<String> {
    match op {
        ComputeNodeOp::Core(ComputeOp::Precondition { .. }) => Some("precondition".to_string()),
        ComputeNodeOp::Core(ComputeOp::Postcondition { .. }) => Some("postcondition".to_string()),
        ComputeNodeOp::Core(ComputeOp::Invariant { .. }) => Some("invariant".to_string()),
        _ => None,
    }
}

fn contract_message_from_op(op: &ComputeNodeOp) -> Option<String> {
    match op {
        ComputeNodeOp::Core(ComputeOp::Precondition { message })
        | ComputeNodeOp::Core(ComputeOp::Postcondition { message })
        | ComputeNodeOp::Core(ComputeOp::Invariant { message, .. }) => Some(message.clone()),
        _ => None,
    }
}
