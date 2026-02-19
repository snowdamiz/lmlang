//! ProgramService: the single coordinator between HTTP handlers and the
//! graph/storage/checker/interpreter crates.
//!
//! All business logic flows through [`ProgramService`]. Handlers will be thin
//! wrappers that delegate to these methods.

use std::collections::{HashSet, VecDeque};

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::node::ComputeNode;
use lmlang_core::type_id::TypeId;
use lmlang_check::interpreter::{
    ExecutionState, Interpreter, InterpreterConfig, Value,
};
use lmlang_check::typecheck;
use lmlang_storage::types::ProgramId;
use lmlang_storage::traits::GraphStore;
use lmlang_storage::SqliteStore;

use crate::error::ApiError;
use crate::schema::diagnostics::DiagnosticError;
use crate::schema::history::{
    CreateCheckpointResponse, DiffResponse,
    ListCheckpointsResponse, ListHistoryResponse, RedoResponse, RestoreCheckpointResponse,
    UndoResponse,
};
use crate::schema::mutations::{CreatedEntity, Mutation, ProposeEditRequest, ProposeEditResponse};
use crate::schema::programs::ProgramSummaryView;
use crate::schema::queries::{
    DetailLevel, EdgeView, FunctionView, GetFunctionResponse, NeighborhoodResponse, NodeView,
    ProgramOverviewResponse, SearchRequest, SearchResponse,
};
use crate::schema::simulate::{SimulateRequest, SimulateResponse, TraceEntryView};
use crate::schema::verify::{VerifyResponse, VerifyScope};
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
                let (from, to) = endpoints.ok_or_else(|| {
                    ApiError::NotFound(format!("edge {} not found", edge_id.0))
                })?;
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
    fn apply_edit_command(
        graph: &mut ProgramGraph,
        cmd: &EditCommand,
    ) -> Result<(), ApiError> {
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
    pub fn get_node(
        &self,
        node_id: NodeId,
        detail: DetailLevel,
    ) -> Result<NodeView, ApiError> {
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
            for edge_ref in self.graph.compute().edges_directed(node_idx, Direction::Outgoing) {
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
            for edge_ref in self.graph.compute().edges_directed(idx, Direction::Outgoing) {
                let neighbor = NodeId::from(edge_ref.target());
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, depth + 1));
                    actual_hops = actual_hops.max(depth + 1);
                }
            }
            for edge_ref in self.graph.compute().edges_directed(idx, Direction::Incoming) {
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
            for edge_ref in self.graph.compute().edges_directed(idx, Direction::Outgoing) {
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
                    .chain(self.graph.compute().edges_directed(node_idx, Direction::Outgoing))
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

    // -----------------------------------------------------------------------
    // Detail level rendering helpers
    // -----------------------------------------------------------------------

    /// Renders a node view at the specified detail level.
    fn render_node(
        &self,
        node_id: NodeId,
        node: &ComputeNode,
        detail: DetailLevel,
    ) -> NodeView {
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
    pub fn simulate(
        &self,
        request: SimulateRequest,
    ) -> Result<SimulateResponse, ApiError> {
        // Verify function exists
        let func_def = self
            .graph
            .get_function(request.function_id)
            .ok_or_else(|| {
                ApiError::NotFound(format!(
                    "function {} not found",
                    request.function_id.0
                ))
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
                                output: e.output.as_ref().and_then(|v| serde_json::to_value(v).ok()),
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

        let result = lmlang_codegen::compile(&self.graph, &options)
            .map_err(|e| match e {
                lmlang_codegen::error::CodegenError::TypeCheckFailed(errors) => {
                    let diags: Vec<crate::schema::diagnostics::DiagnosticError> =
                        errors.into_iter().map(crate::schema::diagnostics::DiagnosticError::from).collect();
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
