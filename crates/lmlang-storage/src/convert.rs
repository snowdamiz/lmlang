//! Decompose/recompose conversions between ProgramGraph and flat storage rows.
//!
//! [`decompose`] breaks a ProgramGraph into a [`DecomposedProgram`] containing
//! flat vectors of all components. [`recompose`] rebuilds a ProgramGraph from
//! a DecomposedProgram, handling StableGraph index gaps correctly.

use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::Directed;

use lmlang_core::edge::{FlowEdge, SemanticEdge};
use lmlang_core::function::FunctionDef;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, ModuleId, NodeId};
use lmlang_core::module::{ModuleDef, ModuleTree};
use lmlang_core::node::{ComputeNode, ModuleNode, SemanticMetadata, SemanticNode};
use lmlang_core::type_id::{TypeId, TypeRegistry};
use lmlang_core::types::LmType;

use crate::error::StorageError;

/// All components of a ProgramGraph broken into flat vectors for storage.
#[derive(Debug, Clone)]
pub struct DecomposedProgram {
    /// Compute nodes: (NodeId, ComputeNode)
    pub compute_nodes: Vec<(NodeId, ComputeNode)>,
    /// Flow edges: (edge_index, source_NodeId, target_NodeId, FlowEdge)
    pub flow_edges: Vec<(u32, NodeId, NodeId, FlowEdge)>,
    /// All types from the TypeRegistry: (TypeId, LmType)
    pub types: Vec<(TypeId, LmType)>,
    /// Type name mappings from the TypeRegistry
    pub type_names: HashMap<String, TypeId>,
    /// Next type ID counter
    pub type_next_id: u32,
    /// Functions: (FunctionId, FunctionDef)
    pub functions: Vec<(FunctionId, FunctionDef)>,
    /// Module definitions: (ModuleId, ModuleDef)
    pub modules: Vec<(ModuleId, ModuleDef)>,
    /// The full ModuleTree (for reconstruction of parent-child, functions, type_defs)
    pub module_tree: ModuleTree,
    /// Semantic nodes: (u32 index, SemanticNode)
    pub semantic_nodes: Vec<(u32, SemanticNode)>,
    /// Semantic edges: (u32 index, source_u32, target_u32, SemanticEdge)
    pub semantic_edges: Vec<(u32, u32, u32, SemanticEdge)>,
    /// Module-to-semantic-node index mapping
    pub module_semantic_indices: HashMap<ModuleId, NodeIndex<u32>>,
    /// Function-to-semantic-node index mapping
    pub function_semantic_indices: HashMap<FunctionId, NodeIndex<u32>>,
    /// Next function ID counter
    pub next_function_id: u32,
}

/// Decomposes a ProgramGraph into flat vectors suitable for storage.
pub fn decompose(graph: &ProgramGraph) -> DecomposedProgram {
    // Extract compute nodes
    let compute_nodes: Vec<(NodeId, ComputeNode)> = graph
        .compute()
        .node_indices()
        .map(|idx| {
            let node = graph.compute().node_weight(idx).unwrap().clone();
            (NodeId::from(idx), node)
        })
        .collect();

    // Extract flow edges
    let flow_edges: Vec<(u32, NodeId, NodeId, FlowEdge)> = graph
        .compute()
        .edge_references()
        .map(|edge_ref| {
            let idx = edge_ref.id().index() as u32;
            let source = NodeId::from(edge_ref.source());
            let target = NodeId::from(edge_ref.target());
            let weight = edge_ref.weight().clone();
            (idx, source, target, weight)
        })
        .collect();

    // Extract all types
    let types: Vec<(TypeId, LmType)> = graph
        .types
        .iter()
        .map(|(id, ty)| (id, ty.clone()))
        .collect();
    let type_names = graph.types.names().clone();
    let type_next_id = graph.types.next_id();

    // Extract functions
    let functions: Vec<(FunctionId, FunctionDef)> = graph
        .functions()
        .iter()
        .map(|(&id, def)| (id, def.clone()))
        .collect();

    // Extract modules
    let modules: Vec<(ModuleId, ModuleDef)> = graph
        .modules
        .all_modules()
        .map(|(&id, def)| (id, def.clone()))
        .collect();

    // Clone the entire ModuleTree for reconstruction
    let module_tree = graph.modules.clone();

    // Extract semantic nodes
    let semantic_nodes: Vec<(u32, SemanticNode)> = graph
        .semantic()
        .node_indices()
        .map(|idx| {
            let node = graph.semantic().node_weight(idx).unwrap().clone();
            (idx.index() as u32, node)
        })
        .collect();

    // Extract semantic edges
    let semantic_edges: Vec<(u32, u32, u32, SemanticEdge)> = graph
        .semantic()
        .edge_references()
        .map(|edge_ref| {
            let idx = edge_ref.id().index() as u32;
            let source = edge_ref.source().index() as u32;
            let target = edge_ref.target().index() as u32;
            let weight = *edge_ref.weight();
            (idx, source, target, weight)
        })
        .collect();

    DecomposedProgram {
        compute_nodes,
        flow_edges,
        types,
        type_names,
        type_next_id,
        functions,
        modules,
        module_tree,
        semantic_nodes,
        semantic_edges,
        module_semantic_indices: graph.module_semantic_indices().clone(),
        function_semantic_indices: graph.function_semantic_indices().clone(),
        next_function_id: graph.next_function_id(),
    }
}

/// Recomposes a ProgramGraph from a DecomposedProgram.
///
/// Handles StableGraph index gaps: if stored nodes have non-contiguous indices
/// (e.g., NodeId(0), NodeId(2), NodeId(5)), dummy nodes are inserted for the
/// gaps and then removed after edges are added, preserving the index mapping.
pub fn recompose(decomposed: DecomposedProgram) -> Result<ProgramGraph, StorageError> {
    // Rebuild compute graph
    let compute = rebuild_compute_graph(&decomposed.compute_nodes, &decomposed.flow_edges)?;

    // Rebuild semantic graph
    let semantic = rebuild_semantic_graph(&decomposed.semantic_nodes, &decomposed.semantic_edges)?;

    // Rebuild TypeRegistry
    let types_vec: Vec<LmType> = {
        let mut sorted = decomposed.types.clone();
        sorted.sort_by_key(|(id, _)| id.0);
        sorted.into_iter().map(|(_, ty)| ty).collect()
    };
    let type_registry =
        TypeRegistry::from_parts(types_vec, decomposed.type_names, decomposed.type_next_id);

    // Rebuild functions map
    let functions: HashMap<FunctionId, FunctionDef> = decomposed.functions.into_iter().collect();

    // Use the stored ModuleTree directly
    let modules = decomposed.module_tree;

    Ok(ProgramGraph::from_parts(
        compute,
        semantic,
        type_registry,
        modules,
        functions,
        decomposed.module_semantic_indices,
        decomposed.function_semantic_indices,
        decomposed.next_function_id,
    ))
}

/// Rebuilds a compute StableGraph from sorted node/edge vectors, handling index gaps.
fn rebuild_compute_graph(
    nodes: &[(NodeId, ComputeNode)],
    edges: &[(u32, NodeId, NodeId, FlowEdge)],
) -> Result<StableGraph<ComputeNode, FlowEdge, Directed, u32>, StorageError> {
    let mut graph = StableGraph::<ComputeNode, FlowEdge, Directed, u32>::new();

    if nodes.is_empty() {
        return Ok(graph);
    }

    // Sort nodes by NodeId to add them in order
    let mut sorted_nodes: Vec<&(NodeId, ComputeNode)> = nodes.iter().collect();
    sorted_nodes.sort_by_key(|(id, _)| id.0);

    // Find the max node index to know the range
    let max_idx = sorted_nodes.last().unwrap().0 .0;

    // Build a map from index to node for O(1) lookup
    let node_map: std::collections::HashMap<u32, &ComputeNode> =
        sorted_nodes.iter().map(|(id, node)| (id.0, node)).collect();

    // Add nodes in order 0..=max_idx, using dummies for gaps
    let mut gap_indices = Vec::new();
    for i in 0..=max_idx {
        if let Some(node) = node_map.get(&i) {
            graph.add_node((*node).clone());
        } else {
            // Add a dummy node for the gap
            let dummy = ComputeNode::new(
                lmlang_core::ops::ComputeNodeOp::Core(lmlang_core::ops::ComputeOp::Alloc),
                FunctionId(0),
            );
            graph.add_node(dummy);
            gap_indices.push(i);
        }
    }

    // Add edges (they reference NodeId values which are now valid indices)
    // Sort edges by index for deterministic insertion
    let mut sorted_edges: Vec<&(u32, NodeId, NodeId, FlowEdge)> = edges.iter().collect();
    sorted_edges.sort_by_key(|(idx, _, _, _)| *idx);

    // Edges in StableGraph may also have gaps. We need to add edges in index order.
    // Find max edge index
    if !sorted_edges.is_empty() {
        let max_edge_idx = sorted_edges
            .iter()
            .map(|(idx, _, _, _)| *idx)
            .max()
            .unwrap();

        let edge_set: std::collections::HashSet<u32> =
            sorted_edges.iter().map(|(idx, _, _, _)| *idx).collect();

        let mut edge_gap_indices = Vec::new();
        let mut edge_iter = sorted_edges.iter().peekable();

        for i in 0..=max_edge_idx {
            if edge_set.contains(&i) {
                let (_, source, target, weight) = edge_iter.next().unwrap();
                let src_idx = NodeIndex::<u32>::new(source.0 as usize);
                let tgt_idx = NodeIndex::<u32>::new(target.0 as usize);
                graph.add_edge(src_idx, tgt_idx, weight.clone());
            } else {
                // Add a dummy edge for the gap (between first two valid nodes)
                // We'll remove it after
                if let Some(first_node) = sorted_nodes.first() {
                    let idx = NodeIndex::<u32>::new(first_node.0 .0 as usize);
                    graph.add_edge(idx, idx, FlowEdge::Control { branch_index: None });
                    edge_gap_indices.push(i);
                }
            }
        }

        // Remove dummy edges (in reverse order to preserve indices)
        for &gap_idx in edge_gap_indices.iter().rev() {
            let edge_idx = petgraph::graph::EdgeIndex::<u32>::new(gap_idx as usize);
            graph.remove_edge(edge_idx);
        }
    }

    // Remove dummy nodes (in reverse order to preserve indices)
    for &gap_idx in gap_indices.iter().rev() {
        let node_idx = NodeIndex::<u32>::new(gap_idx as usize);
        graph.remove_node(node_idx);
    }

    Ok(graph)
}

/// Rebuilds a semantic StableGraph from sorted node/edge vectors, handling index gaps.
fn rebuild_semantic_graph(
    nodes: &[(u32, SemanticNode)],
    edges: &[(u32, u32, u32, SemanticEdge)],
) -> Result<StableGraph<SemanticNode, SemanticEdge, Directed, u32>, StorageError> {
    let mut graph = StableGraph::<SemanticNode, SemanticEdge, Directed, u32>::new();

    if nodes.is_empty() {
        return Ok(graph);
    }

    // Sort nodes by index
    let mut sorted_nodes: Vec<&(u32, SemanticNode)> = nodes.iter().collect();
    sorted_nodes.sort_by_key(|(idx, _)| *idx);

    let max_idx = sorted_nodes.last().unwrap().0;

    // Build a map from index to node for O(1) lookup
    let node_map: std::collections::HashMap<u32, &SemanticNode> = sorted_nodes
        .iter()
        .map(|(idx, node)| (*idx, node))
        .collect();

    // Add nodes in order, with dummies for gaps
    let mut gap_indices = Vec::new();
    for i in 0..=max_idx {
        if let Some(node) = node_map.get(&i) {
            graph.add_node((*node).clone());
        } else {
            // Dummy semantic node
            let dummy = SemanticNode::Module(ModuleNode {
                module: lmlang_core::module::ModuleDef {
                    id: ModuleId(u32::MAX),
                    name: "__dummy__".to_string(),
                    parent: None,
                    visibility: lmlang_core::types::Visibility::Private,
                },
                metadata: SemanticMetadata::default(),
            });
            graph.add_node(dummy);
            gap_indices.push(i);
        }
    }

    // Add edges
    let mut sorted_edges: Vec<&(u32, u32, u32, SemanticEdge)> = edges.iter().collect();
    sorted_edges.sort_by_key(|(idx, _, _, _)| *idx);

    if !sorted_edges.is_empty() {
        let max_edge_idx = sorted_edges
            .iter()
            .map(|(idx, _, _, _)| *idx)
            .max()
            .unwrap();
        let edge_set: std::collections::HashSet<u32> =
            sorted_edges.iter().map(|(idx, _, _, _)| *idx).collect();

        let mut edge_gap_indices = Vec::new();
        let mut edge_iter = sorted_edges.iter().peekable();

        for i in 0..=max_edge_idx {
            if edge_set.contains(&i) {
                let (_, source, target, weight) = edge_iter.next().unwrap();
                let src_idx = NodeIndex::<u32>::new(*source as usize);
                let tgt_idx = NodeIndex::<u32>::new(*target as usize);
                graph.add_edge(src_idx, tgt_idx, *weight);
            } else {
                if let Some(first_node) = sorted_nodes.first() {
                    let idx = NodeIndex::<u32>::new(first_node.0 as usize);
                    graph.add_edge(idx, idx, SemanticEdge::Contains);
                    edge_gap_indices.push(i);
                }
            }
        }

        for &gap_idx in edge_gap_indices.iter().rev() {
            let edge_idx = petgraph::graph::EdgeIndex::<u32>::new(gap_idx as usize);
            graph.remove_edge(edge_idx);
        }
    }

    // Remove dummy nodes
    for &gap_idx in gap_indices.iter().rev() {
        let node_idx = NodeIndex::<u32>::new(gap_idx as usize);
        graph.remove_node(node_idx);
    }

    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::function::{Capture, CaptureMode};
    use lmlang_core::ops::{ArithOp, ComputeOp};
    use lmlang_core::types::Visibility;

    /// Builds a multi-function ProgramGraph for testing.
    fn build_test_program() -> ProgramGraph {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // Add function "add"
        let add_fn = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), i32_id), ("b".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, add_fn)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, add_fn)
            .unwrap();
        let sum = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_fn)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, add_fn).unwrap();

        graph.add_data_edge(param_a, sum, 0, 0, i32_id).unwrap();
        graph.add_data_edge(param_b, sum, 0, 1, i32_id).unwrap();
        graph.add_data_edge(sum, ret, 0, 0, i32_id).unwrap();

        graph.get_function_mut(add_fn).unwrap().entry_node = Some(param_a);

        // Add function "negate"
        let neg_fn = graph
            .add_function(
                "negate".into(),
                root,
                vec![("x".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, neg_fn)
            .unwrap();
        let neg = graph
            .add_core_op(
                ComputeOp::UnaryArith {
                    op: lmlang_core::ops::UnaryArithOp::Neg,
                },
                neg_fn,
            )
            .unwrap();
        let neg_ret = graph.add_core_op(ComputeOp::Return, neg_fn).unwrap();

        graph.add_data_edge(param_x, neg, 0, 0, i32_id).unwrap();
        graph.add_data_edge(neg, neg_ret, 0, 0, i32_id).unwrap();

        graph.get_function_mut(neg_fn).unwrap().entry_node = Some(param_x);

        graph
    }

    #[test]
    fn test_decompose_recompose_roundtrip() {
        let graph = build_test_program();

        let original_nodes = graph.node_count();
        let original_edges = graph.edge_count();
        let original_functions = graph.function_count();
        let original_semantic_nodes = graph.semantic_node_count();
        let original_semantic_edges = graph.semantic_edge_count();

        let decomposed = decompose(&graph);
        let recomposed = recompose(decomposed).unwrap();

        assert_eq!(recomposed.node_count(), original_nodes);
        assert_eq!(recomposed.edge_count(), original_edges);
        assert_eq!(recomposed.function_count(), original_functions);
        assert_eq!(recomposed.semantic_node_count(), original_semantic_nodes);
        assert_eq!(recomposed.semantic_edge_count(), original_semantic_edges);
    }

    #[test]
    fn test_decompose_recompose_with_closure() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let i32_id = TypeId::I32;

        // Parent function
        let parent_fn = graph
            .add_function(
                "make_adder".into(),
                root,
                vec![("offset".into(), i32_id)],
                i32_id,
                Visibility::Public,
            )
            .unwrap();

        // Closure
        let closure_fn = graph
            .add_closure(
                "adder".into(),
                root,
                parent_fn,
                vec![("x".into(), i32_id)],
                i32_id,
                vec![Capture {
                    name: "offset".into(),
                    captured_type: i32_id,
                    mode: CaptureMode::ByValue,
                }],
            )
            .unwrap();

        // Add nodes to closure body
        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, closure_fn)
            .unwrap();
        let cap = graph
            .add_core_op(ComputeOp::CaptureAccess { index: 0 }, closure_fn)
            .unwrap();
        let add = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, closure_fn)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, closure_fn).unwrap();

        graph.add_data_edge(param_x, add, 0, 0, i32_id).unwrap();
        graph.add_data_edge(cap, add, 0, 1, i32_id).unwrap();
        graph.add_data_edge(add, ret, 0, 0, i32_id).unwrap();

        graph.get_function_mut(closure_fn).unwrap().entry_node = Some(param_x);

        // Roundtrip
        let decomposed = decompose(&graph);
        let recomposed = recompose(decomposed).unwrap();

        // Verify closure data survives
        let rc = recomposed.get_function(closure_fn).unwrap();
        assert!(rc.is_closure);
        assert_eq!(rc.captures.len(), 1);
        assert_eq!(rc.captures[0].name, "offset");
        assert_eq!(rc.captures[0].mode, CaptureMode::ByValue);
        assert_eq!(rc.parent_function, Some(parent_fn));

        assert_eq!(recomposed.node_count(), graph.node_count());
        assert_eq!(recomposed.edge_count(), graph.edge_count());
        assert_eq!(recomposed.function_count(), graph.function_count());
    }

    #[test]
    fn test_decompose_recompose_preserves_node_ids() {
        let graph = build_test_program();

        // Get the first function's entry node before roundtrip
        let add_fn_id = FunctionId(0);
        let original_entry = graph.get_function(add_fn_id).unwrap().entry_node;

        let decomposed = decompose(&graph);
        let recomposed = recompose(decomposed).unwrap();

        // Entry node should be the same
        let rt_entry = recomposed.get_function(add_fn_id).unwrap().entry_node;
        assert_eq!(rt_entry, original_entry);

        // Verify the node at that ID has the correct op
        if let Some(entry_id) = rt_entry {
            let node = recomposed.get_compute_node(entry_id).unwrap();
            assert!(matches!(
                node.op,
                lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Parameter { index: 0 })
            ));
        }

        // Verify function_nodes returns same set
        let original_nodes = graph.function_nodes(add_fn_id);
        let rt_nodes = recomposed.function_nodes(add_fn_id);
        assert_eq!(original_nodes.len(), rt_nodes.len());
        for id in &original_nodes {
            assert!(rt_nodes.contains(id));
        }
    }

    #[test]
    fn test_decompose_recompose_preserves_semantic_embeddings() {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();
        let _f = graph
            .add_function(
                "semantic_fn".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let spec_idx = graph
            .add_spec_node(root, "SPEC-EMB".into(), "embedding retention".into())
            .unwrap();
        graph
            .update_semantic_summary(
                spec_idx,
                "spec",
                "SPEC-EMB",
                "semantic embedding payload survives roundtrip",
            )
            .unwrap();
        graph
            .update_semantic_embeddings(
                spec_idx,
                Some(vec![0.11, 0.22, 0.33]),
                Some(vec![0.44, 0.55]),
            )
            .unwrap();

        let before_node = graph
            .semantic()
            .node_weight(NodeIndex::new(spec_idx as usize))
            .unwrap()
            .clone();

        let decomposed = decompose(&graph);
        let recomposed = recompose(decomposed).unwrap();

        let after_node = recomposed
            .semantic()
            .node_weight(NodeIndex::new(spec_idx as usize))
            .unwrap();

        assert_eq!(
            before_node.metadata().summary.checksum,
            after_node.metadata().summary.checksum
        );
        assert_eq!(
            before_node.metadata().embeddings.node_embedding,
            after_node.metadata().embeddings.node_embedding
        );
        assert_eq!(
            before_node.metadata().embeddings.subgraph_summary_embedding,
            after_node.metadata().embeddings.subgraph_summary_embedding
        );
    }
}
