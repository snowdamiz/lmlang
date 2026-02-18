//! In-memory implementation of [`GraphStore`].
//!
//! [`InMemoryStore`] is a first-class backend for tests, ephemeral agent
//! sessions, and anywhere persistence isn't needed. It stores all data in
//! HashMaps with identical semantics to the SQLite backend (coming in Plan 02).

use std::collections::HashMap;

use petgraph::graph::NodeIndex;

use lmlang_core::edge::{FlowEdge, SemanticEdge};
use lmlang_core::function::FunctionDef;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, ModuleId, NodeId};
use lmlang_core::module::ModuleDef;
use lmlang_core::node::{ComputeNode, SemanticNode};
use lmlang_core::type_id::TypeId;
use lmlang_core::types::LmType;

use crate::convert::{decompose, recompose, DecomposedProgram};
use crate::error::StorageError;
use crate::traits::GraphStore;
use crate::types::{ProgramId, ProgramSummary};

/// Data stored for a single program in the in-memory backend.
#[derive(Debug, Clone)]
struct StoredProgram {
    name: String,
    /// Compute nodes indexed by NodeId
    nodes: HashMap<NodeId, ComputeNode>,
    /// Flow edges indexed by EdgeId, with source and target NodeIds
    edges: HashMap<EdgeId, (NodeId, NodeId, FlowEdge)>,
    /// Types indexed by TypeId
    types: HashMap<TypeId, LmType>,
    /// Type name mappings
    type_names: HashMap<String, TypeId>,
    /// Next type ID counter
    type_next_id: u32,
    /// Functions indexed by FunctionId
    functions: HashMap<FunctionId, FunctionDef>,
    /// Modules indexed by ModuleId
    modules: HashMap<ModuleId, ModuleDef>,
    /// The full ModuleTree for reconstruction
    module_tree: lmlang_core::module::ModuleTree,
    /// Semantic nodes indexed by u32
    semantic_nodes: HashMap<u32, SemanticNode>,
    /// Semantic edges indexed by u32, with source and target indices
    semantic_edges: HashMap<u32, (u32, u32, SemanticEdge)>,
    /// Module-to-semantic-node index mapping
    module_semantic_indices: HashMap<ModuleId, NodeIndex<u32>>,
    /// Function-to-semantic-node index mapping
    function_semantic_indices: HashMap<FunctionId, NodeIndex<u32>>,
    /// Next function ID counter
    next_function_id: u32,
}

impl StoredProgram {
    fn new(name: &str) -> Self {
        StoredProgram {
            name: name.to_string(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            types: HashMap::new(),
            type_names: HashMap::new(),
            type_next_id: 0,
            functions: HashMap::new(),
            modules: HashMap::new(),
            module_tree: lmlang_core::module::ModuleTree::new("__empty__"),
            semantic_nodes: HashMap::new(),
            semantic_edges: HashMap::new(),
            module_semantic_indices: HashMap::new(),
            function_semantic_indices: HashMap::new(),
            next_function_id: 0,
        }
    }

    /// Populates from a DecomposedProgram.
    fn store_decomposed(&mut self, decomposed: &DecomposedProgram) {
        self.nodes.clear();
        for (id, node) in &decomposed.compute_nodes {
            self.nodes.insert(*id, node.clone());
        }

        self.edges.clear();
        for (idx, source, target, edge) in &decomposed.flow_edges {
            self.edges
                .insert(EdgeId(*idx), (*source, *target, edge.clone()));
        }

        self.types.clear();
        for (id, ty) in &decomposed.types {
            self.types.insert(*id, ty.clone());
        }
        self.type_names = decomposed.type_names.clone();
        self.type_next_id = decomposed.type_next_id;

        self.functions.clear();
        for (id, func) in &decomposed.functions {
            self.functions.insert(*id, func.clone());
        }

        self.modules.clear();
        for (id, module) in &decomposed.modules {
            self.modules.insert(*id, module.clone());
        }
        self.module_tree = decomposed.module_tree.clone();

        self.semantic_nodes.clear();
        for (idx, node) in &decomposed.semantic_nodes {
            self.semantic_nodes.insert(*idx, node.clone());
        }

        self.semantic_edges.clear();
        for (idx, source, target, edge) in &decomposed.semantic_edges {
            self.semantic_edges.insert(*idx, (*source, *target, *edge));
        }

        self.module_semantic_indices = decomposed.module_semantic_indices.clone();
        self.function_semantic_indices = decomposed.function_semantic_indices.clone();
        self.next_function_id = decomposed.next_function_id;
    }

    /// Converts stored data back into a DecomposedProgram.
    fn to_decomposed(&self) -> DecomposedProgram {
        DecomposedProgram {
            compute_nodes: self.nodes.iter().map(|(&id, n)| (id, n.clone())).collect(),
            flow_edges: self
                .edges
                .iter()
                .map(|(eid, (src, tgt, e))| (eid.0, *src, *tgt, e.clone()))
                .collect(),
            types: self.types.iter().map(|(&id, ty)| (id, ty.clone())).collect(),
            type_names: self.type_names.clone(),
            type_next_id: self.type_next_id,
            functions: self
                .functions
                .iter()
                .map(|(&id, f)| (id, f.clone()))
                .collect(),
            modules: self
                .modules
                .iter()
                .map(|(&id, m)| (id, m.clone()))
                .collect(),
            module_tree: self.module_tree.clone(),
            semantic_nodes: self
                .semantic_nodes
                .iter()
                .map(|(&idx, n)| (idx, n.clone()))
                .collect(),
            semantic_edges: self
                .semantic_edges
                .iter()
                .map(|(&idx, (s, t, e))| (idx, *s, *t, *e))
                .collect(),
            module_semantic_indices: self.module_semantic_indices.clone(),
            function_semantic_indices: self.function_semantic_indices.clone(),
            next_function_id: self.next_function_id,
        }
    }
}

/// In-memory implementation of [`GraphStore`].
///
/// Production-quality backend for tests, ephemeral sessions, and anywhere
/// persistence isn't needed. All data lives in HashMaps.
#[derive(Debug)]
pub struct InMemoryStore {
    programs: HashMap<ProgramId, StoredProgram>,
    next_program_id: i64,
}

impl InMemoryStore {
    /// Creates a new empty in-memory store.
    pub fn new() -> Self {
        InMemoryStore {
            programs: HashMap::new(),
            next_program_id: 1,
        }
    }

    /// Returns a reference to the stored program, or error if not found.
    fn get_stored(&self, id: ProgramId) -> Result<&StoredProgram, StorageError> {
        self.programs
            .get(&id)
            .ok_or(StorageError::ProgramNotFound(id.0))
    }

    /// Returns a mutable reference to the stored program, or error if not found.
    fn get_stored_mut(&mut self, id: ProgramId) -> Result<&mut StoredProgram, StorageError> {
        self.programs
            .get_mut(&id)
            .ok_or(StorageError::ProgramNotFound(id.0))
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphStore for InMemoryStore {
    // -------------------------------------------------------------------
    // Program-level operations
    // -------------------------------------------------------------------

    fn create_program(&mut self, name: &str) -> Result<ProgramId, StorageError> {
        let id = ProgramId(self.next_program_id);
        self.next_program_id += 1;
        self.programs.insert(id, StoredProgram::new(name));
        Ok(id)
    }

    fn load_program(&self, id: ProgramId) -> Result<ProgramGraph, StorageError> {
        let stored = self.get_stored(id)?;
        let decomposed = stored.to_decomposed();
        recompose(decomposed)
    }

    fn delete_program(&mut self, id: ProgramId) -> Result<(), StorageError> {
        self.programs
            .remove(&id)
            .ok_or(StorageError::ProgramNotFound(id.0))?;
        Ok(())
    }

    fn list_programs(&self) -> Result<Vec<ProgramSummary>, StorageError> {
        let mut summaries: Vec<ProgramSummary> = self
            .programs
            .iter()
            .map(|(&id, stored)| ProgramSummary {
                id,
                name: stored.name.clone(),
            })
            .collect();
        summaries.sort_by_key(|s| s.id.0);
        Ok(summaries)
    }

    // -------------------------------------------------------------------
    // High-level convenience methods
    // -------------------------------------------------------------------

    fn save_program(&mut self, id: ProgramId, graph: &ProgramGraph) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(id)?;
        let decomposed = decompose(graph);
        stored.store_decomposed(&decomposed);
        Ok(())
    }

    fn save_function(
        &mut self,
        id: ProgramId,
        func_id: FunctionId,
        graph: &ProgramGraph,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(id)?;

        // Delete existing nodes and edges for this function
        let old_node_ids: Vec<NodeId> = stored
            .nodes
            .iter()
            .filter(|(_, node)| node.owner == func_id)
            .map(|(&id, _)| id)
            .collect();

        // Delete edges connected to old nodes
        let old_edge_ids: Vec<EdgeId> = stored
            .edges
            .iter()
            .filter(|(_, (src, tgt, _))| old_node_ids.contains(src) || old_node_ids.contains(tgt))
            .map(|(&id, _)| id)
            .collect();

        for eid in old_edge_ids {
            stored.edges.remove(&eid);
        }
        for nid in &old_node_ids {
            stored.nodes.remove(nid);
        }

        // Insert fresh nodes from graph
        for idx in graph.compute().node_indices() {
            let node = graph.compute().node_weight(idx).unwrap();
            if node.owner == func_id {
                stored.nodes.insert(NodeId::from(idx), node.clone());
            }
        }

        // Insert edges connected to these new nodes
        use petgraph::visit::{EdgeRef, IntoEdgeReferences};
        for edge_ref in graph.compute().edge_references() {
            let src = NodeId::from(edge_ref.source());
            let tgt = NodeId::from(edge_ref.target());
            let src_node = graph.compute().node_weight(edge_ref.source());
            let tgt_node = graph.compute().node_weight(edge_ref.target());
            if src_node.map_or(false, |n| n.owner == func_id)
                || tgt_node.map_or(false, |n| n.owner == func_id)
            {
                let eid = EdgeId(edge_ref.id().index() as u32);
                stored
                    .edges
                    .insert(eid, (src, tgt, edge_ref.weight().clone()));
            }
        }

        // Update function definition
        if let Some(func_def) = graph.get_function(func_id) {
            stored.functions.insert(func_id, func_def.clone());
        }

        Ok(())
    }

    // -------------------------------------------------------------------
    // Node CRUD
    // -------------------------------------------------------------------

    fn insert_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored.nodes.insert(node_id, node.clone());
        Ok(())
    }

    fn get_node(
        &self,
        program: ProgramId,
        node_id: NodeId,
    ) -> Result<ComputeNode, StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .nodes
            .get(&node_id)
            .cloned()
            .ok_or(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            })
    }

    fn update_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
        node: &ComputeNode,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        if !stored.nodes.contains_key(&node_id) {
            return Err(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            });
        }
        stored.nodes.insert(node_id, node.clone());
        Ok(())
    }

    fn delete_node(
        &mut self,
        program: ProgramId,
        node_id: NodeId,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored
            .nodes
            .remove(&node_id)
            .ok_or(StorageError::NodeNotFound {
                program: program.0,
                node: node_id.0,
            })?;
        Ok(())
    }

    // -------------------------------------------------------------------
    // Edge CRUD
    // -------------------------------------------------------------------

    fn insert_edge(
        &mut self,
        program: ProgramId,
        edge_id: EdgeId,
        source: NodeId,
        target: NodeId,
        edge: &FlowEdge,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored
            .edges
            .insert(edge_id, (source, target, edge.clone()));
        Ok(())
    }

    fn get_edge(
        &self,
        program: ProgramId,
        edge_id: EdgeId,
    ) -> Result<(NodeId, NodeId, FlowEdge), StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .edges
            .get(&edge_id)
            .cloned()
            .ok_or(StorageError::EdgeNotFound {
                program: program.0,
                edge: edge_id.0,
            })
    }

    fn delete_edge(
        &mut self,
        program: ProgramId,
        edge_id: EdgeId,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored
            .edges
            .remove(&edge_id)
            .ok_or(StorageError::EdgeNotFound {
                program: program.0,
                edge: edge_id.0,
            })?;
        Ok(())
    }

    // -------------------------------------------------------------------
    // Type CRUD
    // -------------------------------------------------------------------

    fn insert_type(
        &mut self,
        program: ProgramId,
        type_id: TypeId,
        ty: &LmType,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored.types.insert(type_id, ty.clone());
        Ok(())
    }

    fn get_type(
        &self,
        program: ProgramId,
        type_id: TypeId,
    ) -> Result<LmType, StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .types
            .get(&type_id)
            .cloned()
            .ok_or(StorageError::TypeNotFound {
                program: program.0,
                type_id: type_id.0,
            })
    }

    // -------------------------------------------------------------------
    // Function CRUD
    // -------------------------------------------------------------------

    fn insert_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored.functions.insert(func_id, func.clone());
        Ok(())
    }

    fn get_function(
        &self,
        program: ProgramId,
        func_id: FunctionId,
    ) -> Result<FunctionDef, StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .functions
            .get(&func_id)
            .cloned()
            .ok_or(StorageError::FunctionNotFound {
                program: program.0,
                function: func_id.0,
            })
    }

    fn update_function(
        &mut self,
        program: ProgramId,
        func_id: FunctionId,
        func: &FunctionDef,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        if !stored.functions.contains_key(&func_id) {
            return Err(StorageError::FunctionNotFound {
                program: program.0,
                function: func_id.0,
            });
        }
        stored.functions.insert(func_id, func.clone());
        Ok(())
    }

    // -------------------------------------------------------------------
    // Module CRUD
    // -------------------------------------------------------------------

    fn insert_module(
        &mut self,
        program: ProgramId,
        module_id: ModuleId,
        module: &ModuleDef,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored.modules.insert(module_id, module.clone());
        Ok(())
    }

    fn get_module(
        &self,
        program: ProgramId,
        module_id: ModuleId,
    ) -> Result<ModuleDef, StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .modules
            .get(&module_id)
            .cloned()
            .ok_or(StorageError::ModuleNotFound {
                program: program.0,
                module: module_id.0,
            })
    }

    // -------------------------------------------------------------------
    // Semantic CRUD
    // -------------------------------------------------------------------

    fn insert_semantic_node(
        &mut self,
        program: ProgramId,
        index: u32,
        node: &SemanticNode,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored.semantic_nodes.insert(index, node.clone());
        Ok(())
    }

    fn get_semantic_node(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<SemanticNode, StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .semantic_nodes
            .get(&index)
            .cloned()
            .ok_or(StorageError::IntegrityError {
                reason: format!("semantic node {} not found in program {}", index, program.0),
            })
    }

    fn insert_semantic_edge(
        &mut self,
        program: ProgramId,
        index: u32,
        source: u32,
        target: u32,
        edge: &SemanticEdge,
    ) -> Result<(), StorageError> {
        let stored = self.get_stored_mut(program)?;
        stored
            .semantic_edges
            .insert(index, (source, target, *edge));
        Ok(())
    }

    fn get_semantic_edge(
        &self,
        program: ProgramId,
        index: u32,
    ) -> Result<(u32, u32, SemanticEdge), StorageError> {
        let stored = self.get_stored(program)?;
        stored
            .semantic_edges
            .get(&index)
            .cloned()
            .ok_or(StorageError::IntegrityError {
                reason: format!("semantic edge {} not found in program {}", index, program.0),
            })
    }

    // -------------------------------------------------------------------
    // Query methods
    // -------------------------------------------------------------------

    fn find_nodes_by_owner(
        &self,
        program: ProgramId,
        owner: FunctionId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError> {
        let stored = self.get_stored(program)?;
        Ok(stored
            .nodes
            .iter()
            .filter(|(_, node)| node.owner == owner)
            .map(|(&id, node)| (id, node.clone()))
            .collect())
    }

    fn find_edges_from(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError> {
        let stored = self.get_stored(program)?;
        Ok(stored
            .edges
            .iter()
            .filter(|(_, (src, _, _))| *src == node)
            .map(|(&eid, (_, tgt, edge))| (eid, *tgt, edge.clone()))
            .collect())
    }

    fn find_edges_to(
        &self,
        program: ProgramId,
        node: NodeId,
    ) -> Result<Vec<(EdgeId, NodeId, FlowEdge)>, StorageError> {
        let stored = self.get_stored(program)?;
        Ok(stored
            .edges
            .iter()
            .filter(|(_, (_, tgt, _))| *tgt == node)
            .map(|(&eid, (src, _, edge))| (eid, *src, edge.clone()))
            .collect())
    }

    fn find_functions_in_module(
        &self,
        program: ProgramId,
        module: ModuleId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError> {
        let stored = self.get_stored(program)?;
        Ok(stored
            .functions
            .iter()
            .filter(|(_, func)| func.module == module)
            .map(|(&id, func)| (id, func.clone()))
            .collect())
    }

    fn find_nodes_by_type(
        &self,
        program: ProgramId,
        type_id: TypeId,
    ) -> Result<Vec<(NodeId, ComputeNode)>, StorageError> {
        let stored = self.get_stored(program)?;
        // Find nodes whose edges carry the given type
        // First collect node IDs that have edges with this type
        let mut node_ids_with_type = std::collections::HashSet::new();
        for (_, (src, tgt, edge)) in &stored.edges {
            if edge.value_type() == Some(type_id) {
                node_ids_with_type.insert(*src);
                node_ids_with_type.insert(*tgt);
            }
        }
        Ok(stored
            .nodes
            .iter()
            .filter(|(id, _)| node_ids_with_type.contains(id))
            .map(|(&id, node)| (id, node.clone()))
            .collect())
    }

    fn list_functions(
        &self,
        program: ProgramId,
    ) -> Result<Vec<(FunctionId, FunctionDef)>, StorageError> {
        let stored = self.get_stored(program)?;
        Ok(stored
            .functions
            .iter()
            .map(|(&id, func)| (id, func.clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::function::{Capture, CaptureMode};
    use lmlang_core::ops::{ArithOp, ComputeOp};
    use lmlang_core::types::Visibility;

    /// Builds the multi-function closure program from Phase 1 integration test.
    fn build_full_program() -> ProgramGraph {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // Function "add"
        let add_fn_id = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), i32_id), ("b".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn_id)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn_id)
            .unwrap();
        let add_ret = graph
            .add_core_op(ComputeOp::Return, add_fn_id)
            .unwrap();

        graph
            .add_data_edge(param_a, sum, 0, 0, i32_id)
            .unwrap();
        graph
            .add_data_edge(param_b, sum, 0, 1, i32_id)
            .unwrap();
        graph
            .add_data_edge(sum, add_ret, 0, 0, i32_id)
            .unwrap();

        graph.get_function_mut(add_fn_id).unwrap().entry_node = Some(param_a);

        // Register function type
        let fn_i32_to_i32 = graph.types.register(lmlang_core::types::LmType::Function {
            params: vec![i32_id],
            return_type: i32_id,
        });

        // Function "make_adder"
        let make_adder_id = graph
            .add_function(
                "make_adder".into(),
                root,
                vec![("offset".into(), i32_id)],
                fn_i32_to_i32,
                Visibility::Public,
            )
            .unwrap();

        // Closure "adder"
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
                    mode: CaptureMode::ByValue,
                }],
            )
            .unwrap();

        // Closure body
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

        // make_adder body
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

        graph.get_function_mut(make_adder_id).unwrap().entry_node = Some(ma_param_offset);

        graph
    }

    #[test]
    fn test_create_and_load_program() {
        let mut store = InMemoryStore::new();
        let graph = build_full_program();

        let id = store.create_program("test_program").unwrap();
        store.save_program(id, &graph).unwrap();

        let loaded = store.load_program(id).unwrap();
        assert_eq!(loaded.node_count(), graph.node_count());
        assert_eq!(loaded.edge_count(), graph.edge_count());
        assert_eq!(loaded.function_count(), graph.function_count());
        assert_eq!(loaded.semantic_node_count(), graph.semantic_node_count());
        assert_eq!(loaded.semantic_edge_count(), graph.semantic_edge_count());
    }

    #[test]
    fn test_list_programs() {
        let mut store = InMemoryStore::new();
        store.create_program("alpha").unwrap();
        store.create_program("beta").unwrap();
        store.create_program("gamma").unwrap();

        let list = store.list_programs().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[1].name, "beta");
        assert_eq!(list[2].name, "gamma");
    }

    #[test]
    fn test_delete_program() {
        let mut store = InMemoryStore::new();
        let id = store.create_program("to_delete").unwrap();

        let graph = ProgramGraph::new("main");
        store.save_program(id, &graph).unwrap();

        // Delete
        store.delete_program(id).unwrap();

        // Load should fail
        let result = store.load_program(id);
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::ProgramNotFound(pid) => assert_eq!(pid, id.0),
            other => panic!("expected ProgramNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_crud_nodes() {
        let mut store = InMemoryStore::new();
        let id = store.create_program("crud_test").unwrap();

        let node = ComputeNode::core(ComputeOp::Parameter { index: 0 }, FunctionId(0));
        let node_id = NodeId(0);

        // Insert
        store.insert_node(id, node_id, &node).unwrap();

        // Get
        let retrieved = store.get_node(id, node_id).unwrap();
        assert_eq!(retrieved.owner, FunctionId(0));

        // Update
        let updated_node = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        store.update_node(id, node_id, &updated_node).unwrap();
        let retrieved2 = store.get_node(id, node_id).unwrap();
        assert!(matches!(
            retrieved2.op,
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Return)
        ));

        // Delete
        store.delete_node(id, node_id).unwrap();
        let result = store.get_node(id, node_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_crud_edges() {
        let mut store = InMemoryStore::new();
        let id = store.create_program("edge_test").unwrap();

        let edge = FlowEdge::Data {
            source_port: 0,
            target_port: 0,
            value_type: TypeId::I32,
        };
        let edge_id = EdgeId(0);

        // Insert
        store
            .insert_edge(id, edge_id, NodeId(0), NodeId(1), &edge)
            .unwrap();

        // Get
        let (src, tgt, retrieved) = store.get_edge(id, edge_id).unwrap();
        assert_eq!(src, NodeId(0));
        assert_eq!(tgt, NodeId(1));
        assert!(retrieved.is_data());

        // Delete
        store.delete_edge(id, edge_id).unwrap();
        assert!(store.get_edge(id, edge_id).is_err());
    }

    #[test]
    fn test_query_nodes_by_owner() {
        let mut store = InMemoryStore::new();
        let id = store.create_program("query_test").unwrap();

        // Add nodes with different owners
        let node_f0 = ComputeNode::core(ComputeOp::Parameter { index: 0 }, FunctionId(0));
        let node_f0b = ComputeNode::core(ComputeOp::Return, FunctionId(0));
        let node_f1 = ComputeNode::core(ComputeOp::Parameter { index: 0 }, FunctionId(1));

        store.insert_node(id, NodeId(0), &node_f0).unwrap();
        store.insert_node(id, NodeId(1), &node_f0b).unwrap();
        store.insert_node(id, NodeId(2), &node_f1).unwrap();

        // Query for FunctionId(0)
        let f0_nodes = store.find_nodes_by_owner(id, FunctionId(0)).unwrap();
        assert_eq!(f0_nodes.len(), 2);

        // Query for FunctionId(1)
        let f1_nodes = store.find_nodes_by_owner(id, FunctionId(1)).unwrap();
        assert_eq!(f1_nodes.len(), 1);

        // Query for nonexistent function
        let f99_nodes = store.find_nodes_by_owner(id, FunctionId(99)).unwrap();
        assert_eq!(f99_nodes.len(), 0);
    }

    #[test]
    fn test_save_load_roundtrip_full_program() {
        let mut store = InMemoryStore::new();
        let graph = build_full_program();

        let id = store.create_program("full_program").unwrap();
        store.save_program(id, &graph).unwrap();

        let loaded = store.load_program(id).unwrap();

        // Verify counts
        assert_eq!(loaded.node_count(), 11); // add: 4, adder: 4, make_adder: 3
        assert_eq!(loaded.edge_count(), 8); // add: 3, adder: 3, make_adder: 2
        assert_eq!(loaded.function_count(), 3); // add, make_adder, adder

        // Verify semantic graph
        assert_eq!(loaded.semantic_node_count(), 4); // root + 3 functions
        assert_eq!(loaded.semantic_edge_count(), 3); // Contains edges

        // Verify closure data
        let adder_def = loaded.get_function(FunctionId(2)).unwrap();
        assert!(adder_def.is_closure);
        assert_eq!(adder_def.captures.len(), 1);
        assert_eq!(adder_def.captures[0].name, "offset");
        assert_eq!(adder_def.parent_function, Some(FunctionId(1)));

        // Verify entry nodes
        let add_def = loaded.get_function(FunctionId(0)).unwrap();
        assert!(add_def.entry_node.is_some());
        assert_eq!(add_def.name, "add");
        assert_eq!(add_def.params.len(), 2);

        let make_adder_def = loaded.get_function(FunctionId(1)).unwrap();
        assert!(make_adder_def.entry_node.is_some());

        // Verify function_nodes returns correct count for "add"
        let add_nodes = loaded.function_nodes(FunctionId(0));
        assert_eq!(add_nodes.len(), 4);

        // Verify the entry node has the right op (Parameter { index: 0 })
        if let Some(entry_id) = add_def.entry_node {
            let entry_node = loaded.get_compute_node(entry_id).unwrap();
            assert!(matches!(
                entry_node.op,
                lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 })
            ));
        }
    }
}
