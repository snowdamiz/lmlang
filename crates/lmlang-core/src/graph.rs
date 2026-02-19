//! ProgramGraph: the dual-graph container tying together computational and
//! semantic layers.
//!
//! [`ProgramGraph`] is the single entry point for constructing and querying
//! programs. It enforces dual-graph consistency (DUAL-03) by automatically
//! creating semantic nodes when functions are added, and provides builder
//! methods for all graph mutations.
//!
//! # Dual-Graph Architecture (DUAL-02 + DUAL-03)
//!
//! The program is represented as two separate `StableGraph` instances:
//! - **Computational graph** (`StableGraph<ComputeNode, FlowEdge>`): The
//!   executable layer. Contains op nodes with typed data flow and control
//!   flow edges. This is what gets compiled to LLVM IR.
//! - **Semantic graph** (`StableGraph<SemanticNode, SemanticEdge>`): A
//!   lightweight structural skeleton tracking modules, functions, and type
//!   definitions with containment relationships.
//!
//! Both graphs are private. All mutations go through `ProgramGraph` methods
//! to maintain dual-graph consistency. Read-only accessors are provided for
//! traversals and queries.

use std::collections::{BTreeSet, HashMap, HashSet};

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableGraph;
use petgraph::visit::EdgeRef;
use petgraph::{Directed, Direction};
use serde::{Deserialize, Serialize};

use crate::edge::{FlowEdge, SemanticEdge};
use crate::error::CoreError;
use crate::function::{Capture, FunctionDef};
use crate::id::{EdgeId, FunctionId, ModuleId, NodeId};
use crate::module::ModuleTree;
use crate::node::{
    ComputeNode, DocNode, EmbeddingPayload, FunctionSignature, FunctionSummary, ModuleNode,
    SemanticMetadata, SemanticNode, SemanticSummaryPayload, SpecNode, TestNode,
};
use crate::ops::{ComputeNodeOp, ComputeOp, StructuredOp};
use crate::type_id::{TypeId, TypeRegistry};
use crate::types::Visibility;

/// The semantic layer where a propagation event originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropagationLayer {
    Semantic,
    Compute,
}

/// Event payload for semantic-origin changes that must propagate downward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticEvent {
    FunctionCreated {
        function_id: FunctionId,
    },
    FunctionSignatureChanged {
        function_id: FunctionId,
    },
    ContractAdded {
        function_id: FunctionId,
        contract_name: String,
    },
}

/// Event payload for compute-origin changes that must propagate upward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeEvent {
    NodeInserted {
        function_id: FunctionId,
        node_id: NodeId,
        op_kind: String,
    },
    NodeModified {
        function_id: FunctionId,
        node_id: NodeId,
        op_kind: String,
    },
    NodeRemoved {
        function_id: FunctionId,
        node_id: NodeId,
    },
    ControlFlowChanged {
        function_id: FunctionId,
    },
}

/// Bidirectional propagation event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropagationEventKind {
    Semantic(SemanticEvent),
    Compute(ComputeEvent),
}

/// Queue event envelope with deterministic sequencing and causal lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropagationEvent {
    pub id: u64,
    pub sequence: u64,
    pub origin: PropagationLayer,
    pub kind: PropagationEventKind,
    #[serde(default)]
    pub lineage: Vec<u64>,
}

impl PropagationEvent {
    fn priority(&self) -> u8 {
        match self.kind {
            PropagationEventKind::Semantic(_) => 0,
            PropagationEventKind::Compute(_) => 1,
        }
    }

    fn fingerprint(&self) -> String {
        match &self.kind {
            PropagationEventKind::Semantic(SemanticEvent::FunctionCreated { function_id }) => {
                format!("semantic:create_fn:{}", function_id.0)
            }
            PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                function_id,
            }) => format!("semantic:sig_fn:{}", function_id.0),
            PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                function_id,
                contract_name,
            }) => format!("semantic:contract:{}:{}", function_id.0, contract_name),
            PropagationEventKind::Compute(ComputeEvent::NodeInserted {
                function_id,
                node_id,
                op_kind,
            }) => format!("compute:insert:{}:{}:{}", function_id.0, node_id.0, op_kind),
            PropagationEventKind::Compute(ComputeEvent::NodeModified {
                function_id,
                node_id,
                op_kind,
            }) => format!("compute:modify:{}:{}:{}", function_id.0, node_id.0, op_kind),
            PropagationEventKind::Compute(ComputeEvent::NodeRemoved {
                function_id,
                node_id,
            }) => format!("compute:remove:{}:{}", function_id.0, node_id.0),
            PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id }) => {
                format!("compute:control:{}", function_id.0)
            }
        }
    }
}

/// Conflict priority class for deterministic dual-layer resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictPriorityClass {
    SemanticAuthoritative,
    ComputeAuthoritative,
    Mergeable,
    DiagnosticRequired,
}

/// Structured diagnostic for unresolved propagation conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropagationConflictDiagnostic {
    pub event_id: u64,
    pub conflicting_event_id: u64,
    pub precedence: ConflictPriorityClass,
    pub reason: String,
    pub node_refs: Vec<u32>,
    pub remediation: String,
}

/// Deterministic flush report returned by [`ProgramGraph::flush_propagation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PropagationFlushReport {
    pub processed_events: usize,
    pub applied_events: usize,
    pub skipped_events: usize,
    pub generated_events: usize,
    pub remaining_queue: usize,
    pub refreshed_semantic_nodes: Vec<u32>,
    pub refreshed_summary_nodes: Vec<u32>,
    pub diagnostics: Vec<PropagationConflictDiagnostic>,
}

/// The dual-graph program container.
///
/// Contains both the computational graph (executable ops + data/control flow)
/// and the semantic graph (structural skeleton of modules, functions, types).
/// All mutations go through `ProgramGraph` methods to maintain consistency
/// between the two layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramGraph {
    /// Layer 2: Executable Computational Graph
    compute: StableGraph<ComputeNode, FlowEdge, Directed, u32>,
    /// Layer 1: Semantic skeleton
    semantic: StableGraph<SemanticNode, SemanticEdge, Directed, u32>,
    /// Type registry for nominal type identity
    pub types: TypeRegistry,
    /// Module hierarchy
    pub modules: ModuleTree,
    /// Function definitions indexed by FunctionId
    functions: HashMap<FunctionId, FunctionDef>,
    /// Mapping from ModuleId to its semantic graph NodeIndex
    module_semantic_nodes: HashMap<ModuleId, NodeIndex<u32>>,
    /// Mapping from FunctionId to its semantic graph NodeIndex
    function_semantic_nodes: HashMap<FunctionId, NodeIndex<u32>>,
    /// Next function ID counter
    next_function_id: u32,
    /// Pending propagation events (explicit-flush model).
    #[serde(default)]
    propagation_queue: Vec<PropagationEvent>,
    /// Next event id for queue insertion.
    #[serde(default = "default_next_propagation_event_id")]
    next_propagation_event_id: u64,
    /// Next deterministic sequence number for queue ordering.
    #[serde(default = "default_next_propagation_sequence")]
    next_propagation_sequence: u64,
    /// Maximum times a flush may process events before halting as loop-safe guard.
    #[serde(default = "default_propagation_replay_limit")]
    propagation_replay_limit: usize,
}

fn default_next_propagation_event_id() -> u64 {
    1
}

fn default_next_propagation_sequence() -> u64 {
    1
}

fn default_propagation_replay_limit() -> usize {
    1024
}

impl ProgramGraph {
    /// Creates a new ProgramGraph with a root module, empty graphs, a
    /// TypeRegistry with built-in types, and an empty functions map.
    pub fn new(root_module_name: &str) -> Self {
        let modules = ModuleTree::new(root_module_name);
        let root_id = modules.root_id();

        let mut semantic = StableGraph::<SemanticNode, SemanticEdge, Directed, u32>::new();

        // Create a semantic node for the root module.
        let root_module_def = modules.get_module(root_id).unwrap().clone();
        let root_semantic_idx = semantic.add_node(SemanticNode::Module(ModuleNode {
            module: root_module_def.clone(),
            metadata: SemanticMetadata::with_module(
                "module",
                root_id,
                &root_module_def.name,
                &format!("module {} declaration", root_module_def.name),
            ),
        }));

        let mut module_semantic_nodes = HashMap::new();
        module_semantic_nodes.insert(root_id, root_semantic_idx);

        ProgramGraph {
            compute: StableGraph::new(),
            semantic,
            types: TypeRegistry::new(),
            modules,
            functions: HashMap::new(),
            module_semantic_nodes,
            function_semantic_nodes: HashMap::new(),
            next_function_id: 0,
            propagation_queue: Vec::new(),
            next_propagation_event_id: default_next_propagation_event_id(),
            next_propagation_sequence: default_next_propagation_sequence(),
            propagation_replay_limit: default_propagation_replay_limit(),
        }
    }

    /// Constructs a `ProgramGraph` from all its component parts.
    ///
    /// This enables the storage layer to reconstruct a ProgramGraph from
    /// loaded data without going through the builder methods (which enforce
    /// invariants that are already satisfied by stored data).
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        compute: StableGraph<ComputeNode, FlowEdge, Directed, u32>,
        semantic: StableGraph<SemanticNode, SemanticEdge, Directed, u32>,
        types: TypeRegistry,
        modules: ModuleTree,
        functions: HashMap<FunctionId, FunctionDef>,
        module_semantic_nodes: HashMap<ModuleId, NodeIndex<u32>>,
        function_semantic_nodes: HashMap<FunctionId, NodeIndex<u32>>,
        next_function_id: u32,
    ) -> Self {
        ProgramGraph {
            compute,
            semantic,
            types,
            modules,
            functions,
            module_semantic_nodes,
            function_semantic_nodes,
            next_function_id,
            propagation_queue: Vec::new(),
            next_propagation_event_id: default_next_propagation_event_id(),
            next_propagation_sequence: default_next_propagation_sequence(),
            propagation_replay_limit: default_propagation_replay_limit(),
        }
    }

    // -----------------------------------------------------------------------
    // Read-only accessors
    // -----------------------------------------------------------------------

    /// Returns a read-only reference to the computational graph.
    pub fn compute(&self) -> &StableGraph<ComputeNode, FlowEdge, Directed, u32> {
        &self.compute
    }

    /// Returns a read-only reference to the semantic graph.
    pub fn semantic(&self) -> &StableGraph<SemanticNode, SemanticEdge, Directed, u32> {
        &self.semantic
    }

    /// Returns a read-only reference to the functions map.
    pub fn functions(&self) -> &HashMap<FunctionId, FunctionDef> {
        &self.functions
    }

    /// Returns the module-to-semantic-node-index mapping.
    pub fn module_semantic_indices(&self) -> &HashMap<ModuleId, NodeIndex<u32>> {
        &self.module_semantic_nodes
    }

    /// Returns the function-to-semantic-node-index mapping.
    pub fn function_semantic_indices(&self) -> &HashMap<FunctionId, NodeIndex<u32>> {
        &self.function_semantic_nodes
    }

    /// Returns the next function ID counter value.
    pub fn next_function_id(&self) -> u32 {
        self.next_function_id
    }

    /// Returns pending propagation events in queue order.
    pub fn pending_propagation_events(&self) -> &[PropagationEvent] {
        &self.propagation_queue
    }

    // -----------------------------------------------------------------------
    // Module methods
    // -----------------------------------------------------------------------

    /// Adds a child module under `parent`.
    ///
    /// Delegates to [`ModuleTree`], then adds a `SemanticNode::Module` to the
    /// semantic graph with a `Contains` edge from the parent module's semantic
    /// node.
    pub fn add_module(
        &mut self,
        name: String,
        parent: ModuleId,
        visibility: Visibility,
    ) -> Result<ModuleId, CoreError> {
        let module_id = self.modules.add_module(name, parent, visibility)?;

        // Add semantic node for the new module.
        let module_def = self.modules.get_module(module_id).unwrap().clone();
        let sem_idx = self.semantic.add_node(SemanticNode::Module(ModuleNode {
            module: module_def.clone(),
            metadata: SemanticMetadata::with_module(
                "module",
                module_id,
                &module_def.name,
                &format!("module {} declaration", module_def.name),
            ),
        }));
        self.module_semantic_nodes.insert(module_id, sem_idx);

        // Add Contains edge from parent module's semantic node.
        if let Some(&parent_sem_idx) = self.module_semantic_nodes.get(&parent) {
            self.semantic
                .add_edge(parent_sem_idx, sem_idx, SemanticEdge::Contains);
        }

        Ok(module_id)
    }

    // -----------------------------------------------------------------------
    // Function methods (auto-sync: creates semantic node automatically)
    // -----------------------------------------------------------------------

    /// Adds a function to a module.
    ///
    /// Creates a `FunctionDef`, stores it, adds a `SemanticNode::Function` to
    /// the semantic graph with a `Contains` edge from the module's semantic
    /// node, and registers the function in the `ModuleTree`.
    pub fn add_function(
        &mut self,
        name: String,
        module: ModuleId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
        visibility: Visibility,
    ) -> Result<FunctionId, CoreError> {
        // Verify module exists.
        if self.modules.get_module(module).is_none() {
            return Err(CoreError::ModuleNotFound { id: module });
        }

        let func_id = FunctionId(self.next_function_id);
        self.next_function_id += 1;

        let mut func_def =
            FunctionDef::new(func_id, name.clone(), module, params.clone(), return_type);
        func_def.visibility = visibility;

        self.functions.insert(func_id, func_def);
        self.modules.add_function(module, func_id)?;

        // Add semantic node.
        let summary = FunctionSummary {
            name: name.clone(),
            function_id: func_id,
            module,
            visibility,
            signature: FunctionSignature {
                params,
                return_type,
            },
            metadata: SemanticMetadata::with_function(
                "function",
                module,
                func_id,
                &name,
                &format!("fn {} signature declaration", name),
            ),
        };
        let sem_idx = self.semantic.add_node(SemanticNode::Function(summary));
        self.function_semantic_nodes.insert(func_id, sem_idx);

        // Contains edge from module.
        if let Some(&mod_sem_idx) = self.module_semantic_nodes.get(&module) {
            self.semantic
                .add_edge(mod_sem_idx, sem_idx, SemanticEdge::Contains);
        }

        #[cfg(debug_assertions)]
        self.assert_consistency();

        Ok(func_id)
    }

    /// Adds a closure function with captures and a parent function.
    ///
    /// Like [`add_function`](Self::add_function) but creates a closure
    /// `FunctionDef` with `is_closure=true`, `parent_function` set, and
    /// captures filled.
    pub fn add_closure(
        &mut self,
        name: String,
        module: ModuleId,
        parent: FunctionId,
        params: Vec<(String, TypeId)>,
        return_type: TypeId,
        captures: Vec<Capture>,
    ) -> Result<FunctionId, CoreError> {
        // Verify module exists.
        if self.modules.get_module(module).is_none() {
            return Err(CoreError::ModuleNotFound { id: module });
        }
        // Verify parent function exists.
        if !self.functions.contains_key(&parent) {
            return Err(CoreError::FunctionNotFound { id: parent });
        }

        let func_id = FunctionId(self.next_function_id);
        self.next_function_id += 1;

        let func_def = FunctionDef::closure(
            func_id,
            name.clone(),
            module,
            parent,
            params.clone(),
            return_type,
            captures,
        );

        self.functions.insert(func_id, func_def);
        self.modules.add_function(module, func_id)?;

        // Add semantic node.
        let summary = FunctionSummary {
            name: name.clone(),
            function_id: func_id,
            module,
            visibility: Visibility::Private,
            signature: FunctionSignature {
                params,
                return_type,
            },
            metadata: SemanticMetadata::with_function(
                "function",
                module,
                func_id,
                &name,
                &format!("closure {} signature declaration", name),
            ),
        };
        let sem_idx = self.semantic.add_node(SemanticNode::Function(summary));
        self.function_semantic_nodes.insert(func_id, sem_idx);

        // Contains edge from module.
        if let Some(&mod_sem_idx) = self.module_semantic_nodes.get(&module) {
            self.semantic
                .add_edge(mod_sem_idx, sem_idx, SemanticEdge::Contains);
        }

        #[cfg(debug_assertions)]
        self.assert_consistency();

        Ok(func_id)
    }

    /// Looks up a function definition by ID.
    pub fn get_function(&self, id: FunctionId) -> Option<&FunctionDef> {
        self.functions.get(&id)
    }

    /// Looks up a function definition by ID (mutable, e.g. for setting
    /// `entry_node` after the body is built).
    pub fn get_function_mut(&mut self, id: FunctionId) -> Option<&mut FunctionDef> {
        self.functions.get_mut(&id)
    }

    // -----------------------------------------------------------------------
    // Compute node methods
    // -----------------------------------------------------------------------

    /// Adds a compute node to the computational graph.
    ///
    /// Returns the new [`NodeId`]. Errors if the owner function does not exist.
    /// Does NOT add a semantic node (individual ops don't need semantic entries).
    pub fn add_compute_node(
        &mut self,
        op: ComputeNodeOp,
        owner: FunctionId,
    ) -> Result<NodeId, CoreError> {
        if !self.functions.contains_key(&owner) {
            return Err(CoreError::FunctionNotFound { id: owner });
        }
        let node = ComputeNode::new(op, owner);
        let idx = self.compute.add_node(node);
        Ok(NodeId::from(idx))
    }

    /// Convenience: adds a core (Tier 1) op node.
    pub fn add_core_op(&mut self, op: ComputeOp, owner: FunctionId) -> Result<NodeId, CoreError> {
        self.add_compute_node(ComputeNodeOp::Core(op), owner)
    }

    /// Convenience: adds a structured (Tier 2) op node.
    pub fn add_structured_op(
        &mut self,
        op: StructuredOp,
        owner: FunctionId,
    ) -> Result<NodeId, CoreError> {
        self.add_compute_node(ComputeNodeOp::Structured(op), owner)
    }

    /// Removes a compute node and all its connected edges.
    ///
    /// Returns the removed `ComputeNode`. Errors if the node is not found.
    pub fn remove_compute_node(&mut self, id: NodeId) -> Result<ComputeNode, CoreError> {
        let idx: NodeIndex<u32> = id.into();
        match self.compute.remove_node(idx) {
            Some(node) => Ok(node),
            None => Err(CoreError::NodeNotFound { id }),
        }
    }

    /// Looks up a compute node by ID.
    pub fn get_compute_node(&self, id: NodeId) -> Option<&ComputeNode> {
        let idx: NodeIndex<u32> = id.into();
        self.compute.node_weight(idx)
    }

    /// Modifies a compute node's operation in place, returning the old op.
    ///
    /// This is used by the service layer for ModifyNode mutations and undo.
    pub fn modify_compute_node_op(
        &mut self,
        id: NodeId,
        new_op: ComputeNodeOp,
    ) -> Result<ComputeNodeOp, CoreError> {
        let idx: NodeIndex<u32> = id.into();
        let node = self
            .compute
            .node_weight_mut(idx)
            .ok_or(CoreError::NodeNotFound { id })?;
        let old = std::mem::replace(&mut node.op, new_op);
        Ok(old)
    }

    // -----------------------------------------------------------------------
    // Edge methods
    // -----------------------------------------------------------------------

    /// Adds a data flow edge between two compute nodes.
    ///
    /// Both nodes must exist. Returns the new [`EdgeId`].
    pub fn add_data_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        source_port: u16,
        target_port: u16,
        value_type: TypeId,
    ) -> Result<EdgeId, CoreError> {
        let from_idx: NodeIndex<u32> = from.into();
        let to_idx: NodeIndex<u32> = to.into();

        // Validate both nodes exist.
        if self.compute.node_weight(from_idx).is_none() {
            return Err(CoreError::NodeNotFound { id: from });
        }
        if self.compute.node_weight(to_idx).is_none() {
            return Err(CoreError::NodeNotFound { id: to });
        }

        let edge = FlowEdge::Data {
            source_port,
            target_port,
            value_type,
        };
        let idx = self.compute.add_edge(from_idx, to_idx, edge);
        Ok(EdgeId(idx.index() as u32))
    }

    /// Adds a control flow edge between two compute nodes.
    ///
    /// Both nodes must exist. Returns the new [`EdgeId`].
    pub fn add_control_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        branch_index: Option<u16>,
    ) -> Result<EdgeId, CoreError> {
        let from_idx: NodeIndex<u32> = from.into();
        let to_idx: NodeIndex<u32> = to.into();

        if self.compute.node_weight(from_idx).is_none() {
            return Err(CoreError::NodeNotFound { id: from });
        }
        if self.compute.node_weight(to_idx).is_none() {
            return Err(CoreError::NodeNotFound { id: to });
        }

        let edge = FlowEdge::Control { branch_index };
        let idx = self.compute.add_edge(from_idx, to_idx, edge);
        Ok(EdgeId(idx.index() as u32))
    }

    /// Removes an edge from the computational graph.
    ///
    /// Returns the removed `FlowEdge`. Errors if the edge is not found.
    pub fn remove_edge(&mut self, id: EdgeId) -> Result<FlowEdge, CoreError> {
        let idx = EdgeIndex::<u32>::new(id.0 as usize);
        match self.compute.remove_edge(idx) {
            Some(edge) => Ok(edge),
            None => Err(CoreError::InvalidEdge {
                reason: format!("edge not found: EdgeId({})", id.0),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Query methods
    // -----------------------------------------------------------------------

    /// Returns all compute node IDs owned by a function.
    pub fn function_nodes(&self, id: FunctionId) -> Vec<NodeId> {
        self.compute
            .node_indices()
            .filter(|&idx| {
                self.compute
                    .node_weight(idx)
                    .map_or(false, |n| n.owner == id)
            })
            .map(NodeId::from)
            .collect()
    }

    /// Returns the number of nodes in the computational graph.
    pub fn node_count(&self) -> usize {
        self.compute.node_count()
    }

    /// Returns the number of edges in the computational graph.
    pub fn edge_count(&self) -> usize {
        self.compute.edge_count()
    }

    /// Returns the number of registered functions.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    // -----------------------------------------------------------------------
    // Semantic query methods
    // -----------------------------------------------------------------------

    /// Returns the number of nodes in the semantic graph.
    pub fn semantic_node_count(&self) -> usize {
        self.semantic.node_count()
    }

    /// Returns the number of edges in the semantic graph.
    pub fn semantic_edge_count(&self) -> usize {
        self.semantic.edge_count()
    }

    /// Creates a semantic spec node under a module.
    pub fn add_spec_node(
        &mut self,
        module: ModuleId,
        spec_id: String,
        title: String,
    ) -> Result<u32, CoreError> {
        if self.modules.get_module(module).is_none() {
            return Err(CoreError::ModuleNotFound { id: module });
        }

        let node = SemanticNode::Spec(SpecNode {
            spec_id: spec_id.clone(),
            title: title.clone(),
            metadata: SemanticMetadata::with_module(
                "spec",
                module,
                &spec_id,
                &format!("spec {}", title),
            ),
        });
        let idx = self.semantic.add_node(node);
        if let Some(&module_idx) = self.module_semantic_nodes.get(&module) {
            self.semantic
                .add_edge(module_idx, idx, SemanticEdge::Contains);
        }
        Ok(idx.index() as u32)
    }

    /// Creates a semantic test node under a module.
    pub fn add_test_node(
        &mut self,
        module: ModuleId,
        test_id: String,
        title: String,
        target_function: Option<FunctionId>,
    ) -> Result<u32, CoreError> {
        if self.modules.get_module(module).is_none() {
            return Err(CoreError::ModuleNotFound { id: module });
        }

        let mut metadata =
            SemanticMetadata::with_module("test", module, &test_id, &format!("test {}", title));
        metadata.ownership.function = target_function;

        let idx = self.semantic.add_node(SemanticNode::Test(TestNode {
            test_id,
            title,
            target_function,
            metadata,
        }));
        if let Some(&module_idx) = self.module_semantic_nodes.get(&module) {
            self.semantic
                .add_edge(module_idx, idx, SemanticEdge::Contains);
        }
        Ok(idx.index() as u32)
    }

    /// Creates a semantic documentation node under a module.
    pub fn add_doc_node(
        &mut self,
        module: ModuleId,
        doc_id: String,
        title: String,
    ) -> Result<u32, CoreError> {
        if self.modules.get_module(module).is_none() {
            return Err(CoreError::ModuleNotFound { id: module });
        }

        let idx = self.semantic.add_node(SemanticNode::Doc(DocNode {
            doc_id: doc_id.clone(),
            title: title.clone(),
            metadata: SemanticMetadata::with_module(
                "doc",
                module,
                &doc_id,
                &format!("doc {}", title),
            ),
        }));
        if let Some(&module_idx) = self.module_semantic_nodes.get(&module) {
            self.semantic
                .add_edge(module_idx, idx, SemanticEdge::Contains);
        }
        Ok(idx.index() as u32)
    }

    /// Adds a semantic edge between two semantic nodes by index.
    pub fn add_semantic_edge(
        &mut self,
        from_semantic_idx: u32,
        to_semantic_idx: u32,
        edge: SemanticEdge,
    ) -> Result<u32, CoreError> {
        let from = NodeIndex::<u32>::new(from_semantic_idx as usize);
        let to = NodeIndex::<u32>::new(to_semantic_idx as usize);
        if self.semantic.node_weight(from).is_none() || self.semantic.node_weight(to).is_none() {
            return Err(CoreError::InvalidEdge {
                reason: format!(
                    "invalid semantic edge {} -> {}",
                    from_semantic_idx, to_semantic_idx
                ),
            });
        }
        let edge_idx = self.semantic.add_edge(from, to, edge);
        Ok(edge_idx.index() as u32)
    }

    /// Replaces deterministic summary payload for a semantic node.
    pub fn update_semantic_summary(
        &mut self,
        semantic_idx: u32,
        kind: &str,
        identifier: &str,
        summary_body: &str,
    ) -> Result<(), CoreError> {
        let idx = NodeIndex::<u32>::new(semantic_idx as usize);
        let node =
            self.semantic
                .node_weight_mut(idx)
                .ok_or_else(|| CoreError::GraphInconsistency {
                    reason: format!("semantic node {} not found", semantic_idx),
                })?;
        node.metadata_mut().summary =
            SemanticSummaryPayload::deterministic(kind, identifier, summary_body);
        Ok(())
    }

    /// Replaces node/subgraph embeddings for a semantic node.
    pub fn update_semantic_embeddings(
        &mut self,
        semantic_idx: u32,
        node_embedding: Option<Vec<f32>>,
        subgraph_summary_embedding: Option<Vec<f32>>,
    ) -> Result<(), CoreError> {
        let idx = NodeIndex::<u32>::new(semantic_idx as usize);
        let node =
            self.semantic
                .node_weight_mut(idx)
                .ok_or_else(|| CoreError::GraphInconsistency {
                    reason: format!("semantic node {} not found", semantic_idx),
                })?;
        node.metadata_mut().embeddings = EmbeddingPayload {
            node_embedding,
            subgraph_summary_embedding,
        };
        Ok(())
    }

    /// Enqueues a propagation event and returns the assigned event id.
    pub fn enqueue_propagation(
        &mut self,
        origin: PropagationLayer,
        kind: PropagationEventKind,
    ) -> u64 {
        let event_id = self.next_propagation_event_id;
        let sequence = self.next_propagation_sequence;
        self.next_propagation_event_id += 1;
        self.next_propagation_sequence += 1;

        self.propagation_queue.push(PropagationEvent {
            id: event_id,
            sequence,
            origin,
            kind,
            lineage: Vec::new(),
        });
        event_id
    }

    /// Clears all pending propagation events.
    pub fn clear_propagation_queue(&mut self) {
        self.propagation_queue.clear();
    }

    /// Flushes the propagation queue with deterministic ordering and loop guards.
    pub fn flush_propagation(&mut self) -> Result<PropagationFlushReport, CoreError> {
        if self.propagation_queue.is_empty() {
            return Ok(PropagationFlushReport::default());
        }

        let mut report = PropagationFlushReport::default();
        let mut pending = std::mem::take(&mut self.propagation_queue);
        let mut seen_fingerprints = HashSet::new();
        let mut target_last_event: HashMap<String, PropagationEvent> = HashMap::new();
        let mut replay_count = 0usize;

        while !pending.is_empty() {
            if replay_count >= self.propagation_replay_limit {
                return Err(CoreError::PropagationLoopDetected {
                    reason: format!(
                        "flush exceeded replay limit ({})",
                        self.propagation_replay_limit
                    ),
                });
            }

            let (next_idx, _) = pending
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| (e.priority(), e.sequence, e.id))
                .expect("non-empty queue has min");
            let event = pending.remove(next_idx);
            replay_count += 1;
            report.processed_events += 1;

            if event.lineage.contains(&event.id) {
                report.skipped_events += 1;
                continue;
            }

            let fingerprint = event.fingerprint();
            if !seen_fingerprints.insert(fingerprint) {
                report.skipped_events += 1;
                continue;
            }

            let target = self.propagation_target_key(&event);
            if let Some(previous) = target_last_event.get(&target) {
                let class = Self::classify_conflict(previous, &event);
                match class {
                    ConflictPriorityClass::Mergeable => {}
                    ConflictPriorityClass::SemanticAuthoritative => {
                        if matches!(event.origin, PropagationLayer::Compute) {
                            report.skipped_events += 1;
                            continue;
                        }
                    }
                    ConflictPriorityClass::ComputeAuthoritative => {
                        if matches!(event.origin, PropagationLayer::Semantic) {
                            report.skipped_events += 1;
                            continue;
                        }
                    }
                    ConflictPriorityClass::DiagnosticRequired => {
                        report.diagnostics.push(PropagationConflictDiagnostic {
                            event_id: event.id,
                            conflicting_event_id: previous.id,
                            precedence: class,
                            reason: "conflict requires explicit human/agent remediation"
                                .to_string(),
                            node_refs: self.conflict_node_refs(previous, &event),
                            remediation:
                                "flush conflicting edits separately after semantic correction"
                                    .to_string(),
                        });
                        report.skipped_events += 1;
                        continue;
                    }
                }
            }

            self.apply_propagation_event(&event, &mut report)?;
            report.applied_events += 1;
            target_last_event.insert(target, event);
        }

        report.remaining_queue = 0;
        report.refreshed_semantic_nodes = dedupe_u32(report.refreshed_semantic_nodes);
        report.refreshed_summary_nodes = dedupe_u32(report.refreshed_summary_nodes);
        self.propagation_queue.clear();
        Ok(report)
    }

    fn propagation_target_key(&self, event: &PropagationEvent) -> String {
        match event.kind {
            PropagationEventKind::Semantic(SemanticEvent::FunctionCreated { function_id })
            | PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                function_id,
            })
            | PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                function_id, ..
            })
            | PropagationEventKind::Compute(ComputeEvent::NodeInserted { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::NodeModified { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::NodeRemoved { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id }) => {
                format!("function:{}", function_id.0)
            }
        }
    }

    fn classify_conflict(
        previous: &PropagationEvent,
        current: &PropagationEvent,
    ) -> ConflictPriorityClass {
        match (&previous.kind, &current.kind) {
            (
                PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged { .. }),
                PropagationEventKind::Compute(ComputeEvent::NodeModified { .. }),
            )
            | (
                PropagationEventKind::Compute(ComputeEvent::NodeModified { .. }),
                PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged { .. }),
            ) => ConflictPriorityClass::DiagnosticRequired,
            (
                PropagationEventKind::Semantic(SemanticEvent::ContractAdded { .. }),
                PropagationEventKind::Compute(_),
            ) => ConflictPriorityClass::SemanticAuthoritative,
            (
                PropagationEventKind::Compute(ComputeEvent::NodeRemoved { .. }),
                PropagationEventKind::Semantic(_),
            ) => ConflictPriorityClass::ComputeAuthoritative,
            _ => ConflictPriorityClass::Mergeable,
        }
    }

    fn conflict_node_refs(
        &self,
        previous: &PropagationEvent,
        current: &PropagationEvent,
    ) -> Vec<u32> {
        let mut refs = Vec::new();

        for event in [previous, current] {
            match event.kind {
                PropagationEventKind::Compute(ComputeEvent::NodeInserted { node_id, .. })
                | PropagationEventKind::Compute(ComputeEvent::NodeModified { node_id, .. })
                | PropagationEventKind::Compute(ComputeEvent::NodeRemoved { node_id, .. }) => {
                    refs.push(node_id.0);
                }
                PropagationEventKind::Semantic(SemanticEvent::FunctionCreated { function_id })
                | PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                    function_id,
                })
                | PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                    function_id,
                    ..
                })
                | PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id }) => {
                    refs.push(function_id.0)
                }
            }
        }

        dedupe_u32(refs)
    }

    fn apply_propagation_event(
        &mut self,
        event: &PropagationEvent,
        report: &mut PropagationFlushReport,
    ) -> Result<(), CoreError> {
        match &event.kind {
            PropagationEventKind::Semantic(SemanticEvent::FunctionCreated { function_id })
            | PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                function_id,
            })
            | PropagationEventKind::Compute(ComputeEvent::NodeInserted { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::NodeModified { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::NodeRemoved { function_id, .. })
            | PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id }) => {
                self.refresh_function_semantics(*function_id, report)?
            }
            PropagationEventKind::Semantic(SemanticEvent::ContractAdded {
                function_id,
                contract_name,
            }) => {
                self.attach_contract_spec(*function_id, contract_name, report)?;
                self.refresh_function_semantics(*function_id, report)?;
            }
        }
        Ok(())
    }

    fn refresh_function_semantics(
        &mut self,
        function_id: FunctionId,
        report: &mut PropagationFlushReport,
    ) -> Result<(), CoreError> {
        let func_def = self
            .functions
            .get(&function_id)
            .ok_or(CoreError::FunctionNotFound { id: function_id })?
            .clone();
        let sem_idx = *self
            .function_semantic_nodes
            .get(&function_id)
            .ok_or_else(|| CoreError::GraphInconsistency {
                reason: format!("missing semantic node for function {}", function_id.0),
            })?;

        let node_ids = self.function_nodes(function_id);
        let node_count = node_ids.len() as u32;

        // Rebuild call relationships deterministically from compute ops.
        let mut called_functions = BTreeSet::new();
        for node_id in &node_ids {
            if let Some(node) = self.get_compute_node(*node_id) {
                if let ComputeNodeOp::Core(ComputeOp::Call { target }) = &node.op {
                    called_functions.insert(target.0);
                }
            }
        }

        let remove_calls: Vec<_> = self
            .semantic
            .edges_directed(sem_idx, Direction::Outgoing)
            .filter(|e| matches!(e.weight(), SemanticEdge::Calls))
            .map(|e| e.id())
            .collect();
        for edge_id in remove_calls {
            self.semantic.remove_edge(edge_id);
        }

        for callee in called_functions {
            let callee = FunctionId(callee);
            if let Some(&callee_sem_idx) = self.function_semantic_nodes.get(&callee) {
                self.semantic
                    .add_edge(sem_idx, callee_sem_idx, SemanticEdge::Calls);
            }
        }

        if let Some(SemanticNode::Function(summary)) = self.semantic.node_weight_mut(sem_idx) {
            summary.signature.params = func_def.params.clone();
            summary.signature.return_type = func_def.return_type;
            summary.visibility = func_def.visibility;

            let summary_text = format!(
                "fn {} has {} compute nodes and return type {}",
                summary.name, node_count, summary.signature.return_type.0
            );
            summary.metadata.summary =
                SemanticSummaryPayload::deterministic("function", &summary.name, &summary_text);
            summary.metadata.complexity = Some(node_count);
            summary.metadata.provenance.version += 1;
            summary.metadata.provenance.updated_at_ms += 1;
            summary.metadata.embeddings = EmbeddingPayload {
                node_embedding: Some(deterministic_embedding(&summary.metadata.summary.body, 8)),
                subgraph_summary_embedding: Some(deterministic_embedding(
                    &summary.metadata.summary.checksum,
                    8,
                )),
            };
        }

        report.refreshed_semantic_nodes.push(sem_idx.index() as u32);
        self.refresh_module_semantics(func_def.module, report)?;
        Ok(())
    }

    fn refresh_module_semantics(
        &mut self,
        module_id: ModuleId,
        report: &mut PropagationFlushReport,
    ) -> Result<(), CoreError> {
        let module_sem_idx = *self
            .module_semantic_nodes
            .get(&module_id)
            .ok_or(CoreError::ModuleNotFound { id: module_id })?;
        let module_name = self
            .modules
            .get_module(module_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let function_count = self
            .functions
            .values()
            .filter(|f| f.module == module_id)
            .count();

        if let Some(SemanticNode::Module(module_node)) =
            self.semantic.node_weight_mut(module_sem_idx)
        {
            let summary_text = format!(
                "module {} contains {} function summaries",
                module_name, function_count
            );
            module_node.metadata.summary =
                SemanticSummaryPayload::deterministic("module", &module_name, &summary_text);
            module_node.metadata.provenance.version += 1;
            module_node.metadata.provenance.updated_at_ms += 1;
            module_node.metadata.embeddings = EmbeddingPayload {
                node_embedding: Some(deterministic_embedding(
                    &module_node.metadata.summary.body,
                    8,
                )),
                subgraph_summary_embedding: Some(deterministic_embedding(
                    &module_node.metadata.summary.checksum,
                    8,
                )),
            };
        }

        report
            .refreshed_summary_nodes
            .push(module_sem_idx.index() as u32);
        Ok(())
    }

    fn attach_contract_spec(
        &mut self,
        function_id: FunctionId,
        contract_name: &str,
        report: &mut PropagationFlushReport,
    ) -> Result<(), CoreError> {
        let func = self
            .functions
            .get(&function_id)
            .ok_or(CoreError::FunctionNotFound { id: function_id })?
            .clone();
        let function_sem_idx =
            *self
                .function_semantic_nodes
                .get(&function_id)
                .ok_or_else(|| CoreError::GraphInconsistency {
                    reason: format!("missing semantic node for function {}", function_id.0),
                })?;
        let module_sem_idx = *self
            .module_semantic_nodes
            .get(&func.module)
            .ok_or(CoreError::ModuleNotFound { id: func.module })?;

        let spec_idx = self.semantic.add_node(SemanticNode::Spec(SpecNode {
            spec_id: format!("contract-{}-{}", function_id.0, contract_name),
            title: format!("contract {}", contract_name),
            metadata: SemanticMetadata::with_function(
                "spec",
                func.module,
                function_id,
                contract_name,
                &format!("contract {} for function {}", contract_name, func.name),
            ),
        }));

        self.semantic
            .add_edge(module_sem_idx, spec_idx, SemanticEdge::Contains);
        self.semantic
            .add_edge(function_sem_idx, spec_idx, SemanticEdge::Implements);
        report
            .refreshed_semantic_nodes
            .push(spec_idx.index() as u32);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Debug consistency assertion
    // -----------------------------------------------------------------------

    /// Verifies that every FunctionId in the functions map has a
    /// corresponding `SemanticNode::Function` in the semantic graph.
    ///
    /// Only called in debug builds (via `cfg(debug_assertions)`).
    #[cfg(debug_assertions)]
    fn assert_consistency(&self) {
        for func_id in self.functions.keys() {
            assert!(
                self.function_semantic_nodes.contains_key(func_id),
                "Function {:?} has no semantic node mapping",
                func_id
            );
            let sem_idx = self.function_semantic_nodes[func_id];
            assert!(
                self.semantic.node_weight(sem_idx).is_some(),
                "Function {:?} semantic node index {:?} has no node in semantic graph",
                func_id,
                sem_idx
            );
            let node = &self.semantic[sem_idx];
            match node {
                SemanticNode::Function(summary) => {
                    assert_eq!(
                        summary.function_id, *func_id,
                        "Semantic node function_id mismatch"
                    );
                }
                _ => panic!(
                    "Function {:?} semantic node is not SemanticNode::Function",
                    func_id
                ),
            }
        }
    }
}

fn dedupe_u32(values: Vec<u32>) -> Vec<u32> {
    let set: BTreeSet<u32> = values.into_iter().collect();
    set.into_iter().collect()
}

fn deterministic_embedding(seed: &str, dims: usize) -> Vec<f32> {
    if dims == 0 {
        return Vec::new();
    }

    let mut bytes = seed.as_bytes().to_vec();
    if bytes.is_empty() {
        bytes.push(0);
    }

    let mut out = Vec::with_capacity(dims);
    for i in 0..dims {
        let a = bytes[i % bytes.len()] as f32 / 255.0;
        let b = bytes[(i * 7 + 3) % bytes.len()] as f32 / 255.0;
        out.push((a + b) / 2.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::ArithOp;

    #[test]
    fn basic_program_graph_construction() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        // Add a function.
        let add_fn = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // Add compute nodes.
        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn)
            .unwrap();

        // Add data edges.
        graph
            .add_data_edge(param_a, sum, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(param_b, sum, 0, 1, TypeId::I32)
            .unwrap();

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
        assert_eq!(graph.function_count(), 1);
    }

    #[test]
    fn adding_function_creates_semantic_node() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        // Root module = 1 semantic node.
        assert_eq!(graph.semantic_node_count(), 1);

        graph
            .add_function("foo".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        // Now: root module + foo function = 2 semantic nodes.
        assert_eq!(graph.semantic_node_count(), 2);
    }

    #[test]
    fn adding_compute_nodes_does_not_create_semantic_nodes() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let f = graph
            .add_function("f".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        let sem_count_before = graph.semantic_node_count();

        graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f)
            .unwrap();
        graph.add_core_op(ComputeOp::Return, f).unwrap();

        assert_eq!(graph.semantic_node_count(), sem_count_before);
    }

    #[test]
    fn add_compute_node_nonexistent_owner_errors() {
        let mut graph = ProgramGraph::new("main");

        let result = graph.add_core_op(ComputeOp::Parameter { index: 0 }, FunctionId(999));
        assert!(result.is_err());
        match result {
            Err(CoreError::FunctionNotFound { id }) => assert_eq!(id, FunctionId(999)),
            _ => panic!("expected FunctionNotFound error"),
        }
    }

    #[test]
    fn remove_compute_node_removes_node_and_edges() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let f = graph
            .add_function("f".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        let a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f)
            .unwrap();
        let b = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, f)
            .unwrap();
        let c = graph.add_core_op(ComputeOp::Return, f).unwrap();

        graph.add_data_edge(a, b, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(b, c, 0, 0, TypeId::I32).unwrap();

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);

        // Remove middle node -- should remove its connected edges too.
        let removed = graph.remove_compute_node(b).unwrap();
        assert!(matches!(
            removed.op,
            ComputeNodeOp::Core(ComputeOp::BinaryArith { op: ArithOp::Add })
        ));

        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn function_nodes_returns_correct_nodes() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let f1 = graph
            .add_function("f1".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();
        let f2 = graph
            .add_function("f2".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        let n1 = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f1)
            .unwrap();
        let n2 = graph.add_core_op(ComputeOp::Return, f1).unwrap();
        let _n3 = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f2)
            .unwrap();

        let f1_nodes = graph.function_nodes(f1);
        assert_eq!(f1_nodes.len(), 2);
        assert!(f1_nodes.contains(&n1));
        assert!(f1_nodes.contains(&n2));
    }

    #[test]
    fn add_module_creates_semantic_node_and_edge() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        // Root module = 1 semantic node, 0 edges.
        assert_eq!(graph.semantic_node_count(), 1);
        assert_eq!(graph.semantic_edge_count(), 0);

        let _child = graph
            .add_module("child".into(), root, Visibility::Public)
            .unwrap();

        // Root + child = 2 semantic nodes, 1 Contains edge.
        assert_eq!(graph.semantic_node_count(), 2);
        assert_eq!(graph.semantic_edge_count(), 1);
    }

    #[test]
    fn closure_creates_semantic_node() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let parent_fn = graph
            .add_function(
                "parent".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        let sem_before = graph.semantic_node_count();

        let _closure = graph
            .add_closure(
                "lambda".into(),
                root,
                parent_fn,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                vec![Capture {
                    name: "y".into(),
                    captured_type: TypeId::I32,
                    mode: crate::function::CaptureMode::ByValue,
                }],
            )
            .unwrap();

        assert_eq!(graph.semantic_node_count(), sem_before + 1);
    }

    #[test]
    fn read_only_accessors() {
        let graph = ProgramGraph::new("main");

        // Can access compute and semantic graphs read-only.
        assert_eq!(graph.compute().node_count(), 0);
        assert_eq!(graph.semantic().node_count(), 1); // root module
    }

    /// Comprehensive integration test constructing a multi-function program
    /// with a closure. Proves the entire Phase 1 data model works end-to-end.
    ///
    /// Program represented:
    /// ```text
    /// module main {
    ///     fn add(a: i32, b: i32) -> i32 {
    ///         return a + b;
    ///     }
    ///
    ///     fn make_adder(offset: i32) -> fn(i32) -> i32 {
    ///         let adder = |x: i32| -> i32 { add(x, offset) };
    ///         return adder;
    ///     }
    /// }
    /// ```
    #[test]
    fn test_multi_function_program_with_closure() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // ---------------------------------------------------------------
        // 1. Add function "add" to root module
        // ---------------------------------------------------------------
        let add_fn_id = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), i32_id), ("b".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        // Build body of "add":
        //   param_a -> sum (port 0->0)
        //   param_b -> sum (port 0->1)
        //   sum -> ret (port 0->0)
        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn_id)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn_id)
            .unwrap();
        let add_ret = graph.add_core_op(ComputeOp::Return, add_fn_id).unwrap();

        graph.add_data_edge(param_a, sum, 0, 0, i32_id).unwrap();
        graph.add_data_edge(param_b, sum, 0, 1, i32_id).unwrap();
        graph.add_data_edge(sum, add_ret, 0, 0, i32_id).unwrap();

        // Set entry node on "add".
        graph.get_function_mut(add_fn_id).unwrap().entry_node = Some(param_a);

        // ---------------------------------------------------------------
        // 2. Register function type for make_adder's return type
        // ---------------------------------------------------------------
        let fn_i32_to_i32 = graph.types.register(crate::types::LmType::Function {
            params: vec![i32_id],
            return_type: i32_id,
        });

        // ---------------------------------------------------------------
        // 3. Add function "make_adder"
        // ---------------------------------------------------------------
        let make_adder_id = graph
            .add_function(
                "make_adder".into(),
                root,
                vec![("offset".into(), i32_id)],
                fn_i32_to_i32,
                Visibility::Public,
            )
            .unwrap();

        // ---------------------------------------------------------------
        // 4. Add closure "adder" with parent make_adder
        // ---------------------------------------------------------------
        let adder_fn_id = graph
            .add_closure(
                "adder".into(),
                root,
                make_adder_id,
                vec![("x".into(), i32_id)],
                i32_id,
                vec![Capture {
                    name: "offset".into(),
                    captured_type: i32_id,
                    mode: crate::function::CaptureMode::ByValue,
                }],
            )
            .unwrap();

        // ---------------------------------------------------------------
        // 5. Build closure "adder" body:
        //   param_x -> call_add (port 0->0)
        //   cap_offset -> call_add (port 0->1)
        //   call_add -> adder_ret (port 0->0)
        // ---------------------------------------------------------------
        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, adder_fn_id)
            .unwrap();
        let cap_offset = graph
            .add_core_op(ComputeOp::CaptureAccess { index: 0 }, adder_fn_id)
            .unwrap();
        let call_add = graph
            .add_core_op(ComputeOp::Call { target: add_fn_id }, adder_fn_id)
            .unwrap();
        let adder_ret = graph.add_core_op(ComputeOp::Return, adder_fn_id).unwrap();

        graph
            .add_data_edge(param_x, call_add, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(cap_offset, call_add, 0, 1, i32_id)
            .unwrap();
        graph
            .add_data_edge(call_add, adder_ret, 0, 0, i32_id)
            .unwrap();

        graph.get_function_mut(adder_fn_id).unwrap().entry_node = Some(param_x);

        // ---------------------------------------------------------------
        // 6. Build make_adder body:
        //   param_offset -> make_closure (port 0->0, i32 -- captured value)
        //   make_closure -> ma_ret (port 0->0, fn type)
        // ---------------------------------------------------------------
        let ma_param_offset = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, make_adder_id)
            .unwrap();
        let make_closure = graph
            .add_core_op(
                ComputeOp::MakeClosure {
                    function: adder_fn_id,
                },
                make_adder_id,
            )
            .unwrap();
        let ma_ret = graph.add_core_op(ComputeOp::Return, make_adder_id).unwrap();

        graph
            .add_data_edge(ma_param_offset, make_closure, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(make_closure, ma_ret, 0, 0, fn_i32_to_i32)
            .unwrap();

        graph.get_function_mut(make_adder_id).unwrap().entry_node = Some(ma_param_offset);

        // ===============================================================
        // ASSERTIONS
        // ===============================================================

        // Function count: add, make_adder, adder (closure) = 3
        assert_eq!(graph.function_count(), 3);

        // Node count: add has 4, adder has 4, make_adder has 3 = 11
        assert_eq!(graph.node_count(), 11);

        // Edge count: add has 3, adder has 3, make_adder has 2 = 8
        assert_eq!(graph.edge_count(), 8);

        // function_nodes for "add" returns exactly 4
        let add_nodes = graph.function_nodes(add_fn_id);
        assert_eq!(add_nodes.len(), 4);

        // Closure verification
        let adder_def = graph.get_function(adder_fn_id).unwrap();
        assert!(adder_def.is_closure);
        assert_eq!(adder_def.captures.len(), 1);
        assert_eq!(adder_def.captures[0].name, "offset");
        assert_eq!(adder_def.parent_function, Some(make_adder_id));

        // Semantic graph checks:
        // root module + add + make_adder + adder = 4 semantic nodes
        assert_eq!(graph.semantic_node_count(), 4);

        // Contains edges: root->add, root->make_adder, root->adder = 3
        assert_eq!(graph.semantic_edge_count(), 3);

        // Verify semantic edges are Contains type from root module
        let sem = graph.semantic();
        for edge_idx in sem.edge_indices() {
            let edge_weight = &sem[edge_idx];
            assert_eq!(
                *edge_weight,
                SemanticEdge::Contains,
                "All semantic edges should be Contains"
            );
        }

        // ---------------------------------------------------------------
        // Serde round-trip
        // ---------------------------------------------------------------
        let json = serde_json::to_string(&graph).unwrap();
        let deserialized: ProgramGraph = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.node_count(), graph.node_count());
        assert_eq!(deserialized.edge_count(), graph.edge_count());
        assert_eq!(deserialized.function_count(), graph.function_count());
        assert_eq!(
            deserialized.semantic_node_count(),
            graph.semantic_node_count()
        );
        assert_eq!(
            deserialized.semantic_edge_count(),
            graph.semantic_edge_count()
        );

        // Verify function data survives round-trip
        let rt_adder = deserialized.get_function(adder_fn_id).unwrap();
        assert!(rt_adder.is_closure);
        assert_eq!(rt_adder.captures.len(), 1);
        assert_eq!(rt_adder.parent_function, Some(make_adder_id));

        let rt_add = deserialized.get_function(add_fn_id).unwrap();
        assert_eq!(rt_add.name, "add");
        assert_eq!(rt_add.params.len(), 2);
        assert_eq!(rt_add.entry_node, Some(param_a));
    }

    #[test]
    fn semantic_helpers_create_and_update_rich_nodes() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let f = graph
            .add_function(
                "work".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let spec_idx = graph
            .add_spec_node(root, "SPEC-8".into(), "must be deterministic".into())
            .unwrap();
        let test_idx = graph
            .add_test_node(
                root,
                "TEST-8".into(),
                "deterministic replay".into(),
                Some(f),
            )
            .unwrap();
        let doc_idx = graph
            .add_doc_node(root, "DOC-8".into(), "phase 8 notes".into())
            .unwrap();

        graph
            .add_semantic_edge(spec_idx, test_idx, SemanticEdge::Validates)
            .unwrap();
        graph
            .add_semantic_edge(doc_idx, spec_idx, SemanticEdge::Documents)
            .unwrap();
        graph
            .update_semantic_summary(spec_idx, "spec", "SPEC-8", "all updates are deterministic")
            .unwrap();
        graph
            .update_semantic_embeddings(
                spec_idx,
                Some(vec![0.1, 0.2, 0.3]),
                Some(vec![0.4, 0.5, 0.6]),
            )
            .unwrap();

        let spec_node = graph
            .semantic()
            .node_weight(NodeIndex::new(spec_idx as usize))
            .unwrap();
        assert_eq!(spec_node.kind(), "spec");
        assert_eq!(spec_node.metadata().embeddings.node_dim(), Some(3));
        assert_eq!(spec_node.metadata().embeddings.summary_dim(), Some(3));
        assert!(!spec_node.metadata().summary.checksum.is_empty());
    }

    #[test]
    fn propagation_flush_is_deterministic_and_idempotent() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let f = graph
            .add_function(
                "flush_me".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let node_id = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f)
            .unwrap();

        graph.enqueue_propagation(
            PropagationLayer::Compute,
            PropagationEventKind::Compute(ComputeEvent::NodeInserted {
                function_id: f,
                node_id,
                op_kind: "Parameter".to_string(),
            }),
        );
        graph.enqueue_propagation(
            PropagationLayer::Compute,
            PropagationEventKind::Compute(ComputeEvent::ControlFlowChanged { function_id: f }),
        );

        let first = graph.flush_propagation().unwrap();
        assert_eq!(first.remaining_queue, 0);
        assert!(first.applied_events >= 1);

        // Idempotent: flushing with unchanged queue is a no-op.
        let second = graph.flush_propagation().unwrap();
        assert_eq!(second.processed_events, 0);
        assert_eq!(second.applied_events, 0);
        assert_eq!(second.diagnostics.len(), 0);
    }

    #[test]
    fn propagation_conflict_emits_diagnostic_required() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let f = graph
            .add_function(
                "conflict".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let n = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, f)
            .unwrap();

        graph.enqueue_propagation(
            PropagationLayer::Compute,
            PropagationEventKind::Compute(ComputeEvent::NodeModified {
                function_id: f,
                node_id: n,
                op_kind: "Parameter".to_string(),
            }),
        );
        graph.enqueue_propagation(
            PropagationLayer::Semantic,
            PropagationEventKind::Semantic(SemanticEvent::FunctionSignatureChanged {
                function_id: f,
            }),
        );

        let report = graph.flush_propagation().unwrap();
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.precedence == ConflictPriorityClass::DiagnosticRequired));
    }
}
