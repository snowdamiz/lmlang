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

use std::collections::HashMap;

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableGraph;
use petgraph::Directed;
use serde::{Deserialize, Serialize};

use crate::edge::{FlowEdge, SemanticEdge};
use crate::error::CoreError;
use crate::function::{Capture, FunctionDef};
use crate::id::{EdgeId, FunctionId, ModuleId, NodeId};
use crate::module::ModuleTree;
use crate::node::{ComputeNode, FunctionSignature, FunctionSummary, SemanticNode};
use crate::ops::{ComputeNodeOp, ComputeOp, StructuredOp};
use crate::type_id::{TypeId, TypeRegistry};
use crate::types::Visibility;

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
        let root_semantic_idx = semantic.add_node(SemanticNode::Module(root_module_def));

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
        let sem_idx = self.semantic.add_node(SemanticNode::Module(module_def));
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

        let mut func_def = FunctionDef::new(
            func_id,
            name.clone(),
            module,
            params.clone(),
            return_type,
        );
        func_def.visibility = visibility;

        self.functions.insert(func_id, func_def);
        self.modules.add_function(module, func_id)?;

        // Add semantic node.
        let summary = FunctionSummary {
            name,
            function_id: func_id,
            module,
            visibility,
            signature: FunctionSignature {
                params,
                return_type,
            },
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
            name,
            function_id: func_id,
            module,
            visibility: Visibility::Private,
            signature: FunctionSignature {
                params,
                return_type,
            },
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
    pub fn add_core_op(
        &mut self,
        op: ComputeOp,
        owner: FunctionId,
    ) -> Result<NodeId, CoreError> {
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
            .add_function(
                "foo".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        // Now: root module + foo function = 2 semantic nodes.
        assert_eq!(graph.semantic_node_count(), 2);
    }

    #[test]
    fn adding_compute_nodes_does_not_create_semantic_nodes() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let f = graph
            .add_function(
                "f".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
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

        let result = graph.add_core_op(
            ComputeOp::Parameter { index: 0 },
            FunctionId(999),
        );
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

        graph
            .add_data_edge(param_a, sum, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(param_b, sum, 0, 1, i32_id)
            .unwrap();
        graph
            .add_data_edge(sum, add_ret, 0, 0, i32_id)
            .unwrap();

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
        let adder_ret = graph
            .add_core_op(ComputeOp::Return, adder_fn_id)
            .unwrap();

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
        let ma_ret = graph
            .add_core_op(ComputeOp::Return, make_adder_id)
            .unwrap();

        graph
            .add_data_edge(ma_param_offset, make_closure, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(make_closure, ma_ret, 0, 0, fn_i32_to_i32)
            .unwrap();

        graph.get_function_mut(make_adder_id).unwrap().entry_node =
            Some(ma_param_offset);

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
}
