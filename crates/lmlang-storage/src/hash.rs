//! Deterministic content hashing for graph nodes using blake3.
//!
//! Provides Merkle-tree style composition from leaf nodes through edges
//! to per-function root hashes. Hashes are derived state, never stored
//! in the database.
//!
//! # Levels
//!
//! - **Level 1**: Node content hash (op + owner)
//! - **Level 2**: Node hash with edges (Merkle composition)
//! - **Level 3**: Per-function root hash (all nodes in function)
//!
//! # Determinism
//!
//! All hashing is deterministic: same content always produces the same hash.
//! This is ensured by:
//! - Using `serde_json::to_vec` for canonical serialization
//! - Sorting edges by deterministic keys before hashing
//! - Sorting nodes by `NodeId` for function root hash composition
//! - Never iterating `HashMap` directly for hash-affecting operations

use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::node::ComputeNode;

/// Computes a blake3 hash of a compute node's content (op + owner).
///
/// Deterministic: same node content always produces the same hash.
/// Different: changing any field (op variant, owner) produces a different hash.
/// Uses `serde_json::to_vec` for canonical serialization of the op, since
/// `ComputeNodeOp` uses no HashMap (only Vec/IndexMap), guaranteeing
/// deterministic JSON output.
pub fn hash_node_content(node: &ComputeNode) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();
    // Serialize the op -- deterministic because ComputeNodeOp uses no HashMap
    let op_bytes = serde_json::to_vec(&node.op)
        .expect("ComputeNodeOp serialization should never fail");
    hasher.update(&op_bytes);
    // Hash the owner function ID
    hasher.update(&node.owner.0.to_le_bytes());
    hasher.finalize()
}

/// Returns a deterministic sort key for a flow edge.
///
/// Data edges sort by (0, target_port, target_node_id).
/// Control edges sort by (1, branch_index or u16::MAX, target_node_id).
/// The leading discriminant separates Data from Control in sort order.
fn edge_sort_key(target_id: NodeId, edge: &FlowEdge) -> (u8, u16, u32) {
    match edge {
        FlowEdge::Data { target_port, .. } => (0, *target_port, target_id.0),
        FlowEdge::Control { branch_index } => {
            (1, branch_index.unwrap_or(u16::MAX), target_id.0)
        }
    }
}

/// Computes a composite hash of a node including its outgoing edges and
/// target node hashes (Merkle-tree composition).
///
/// `outgoing` is a slice of `(target_node_id, edge, target_node_hash)` tuples.
/// Edges are sorted by deterministic keys before hashing to ensure the same
/// set of edges always produces the same composite hash regardless of
/// iteration order.
pub fn hash_node_with_edges(
    node: &ComputeNode,
    outgoing: &[(NodeId, FlowEdge, blake3::Hash)],
) -> blake3::Hash {
    let mut hasher = blake3::Hasher::new();

    // Start with the node's content hash
    let content_hash = hash_node_content(node);
    hasher.update(content_hash.as_bytes());

    // Sort edges by deterministic key
    let mut sorted: Vec<&(NodeId, FlowEdge, blake3::Hash)> = outgoing.iter().collect();
    sorted.sort_by_key(|(target_id, edge, _)| edge_sort_key(*target_id, edge));

    // Hash each edge: serialized edge data + target node's hash
    for (_, edge, target_hash) in sorted {
        let edge_bytes = serde_json::to_vec(edge)
            .expect("FlowEdge serialization should never fail");
        hasher.update(&edge_bytes);
        hasher.update(target_hash.as_bytes());
    }

    hasher.finalize()
}

/// Computes the root hash for a function within a program graph.
///
/// Includes all nodes owned by the function, composed in sorted NodeId order.
/// Uses a two-pass approach:
/// 1. Compute content hashes for all nodes in the function
/// 2. Compute composite hashes (content + edges) for each node
///
/// For cycles (control flow back-edges), if a target node is outside this
/// function, its content hash alone is used as the target hash.
///
/// The final function hash combines all composite node hashes in sorted
/// NodeId order.
pub fn hash_function(graph: &ProgramGraph, func_id: FunctionId) -> blake3::Hash {
    // Get all nodes owned by this function, sorted by NodeId for determinism
    let mut func_nodes = graph.function_nodes(func_id);
    func_nodes.sort_by_key(|n| n.0);

    // Build a set of node IDs in this function for membership checks
    let func_node_set: std::collections::HashSet<NodeId> =
        func_nodes.iter().copied().collect();

    // Pass 1: compute content hashes for all nodes
    let mut content_hashes: HashMap<NodeId, blake3::Hash> = HashMap::new();
    for &node_id in &func_nodes {
        let node = graph.get_compute_node(node_id).unwrap();
        content_hashes.insert(node_id, hash_node_content(node));
    }

    // Pass 2: compute composite hashes (with edges) for each node
    let mut composite_hashes: HashMap<NodeId, blake3::Hash> = HashMap::new();
    for &node_id in &func_nodes {
        let node = graph.get_compute_node(node_id).unwrap();
        let node_idx: NodeIndex<u32> = node_id.into();

        // Collect outgoing edges from this node
        let mut outgoing: Vec<(NodeId, FlowEdge, blake3::Hash)> = Vec::new();
        for edge_ref in graph.compute().edges_directed(node_idx, Direction::Outgoing) {
            let target_id = NodeId::from(edge_ref.target());
            let edge = edge_ref.weight().clone();

            // Use the target's content hash. For nodes within the function,
            // this is from our content_hashes map. For cross-function edges,
            // compute on the fly (these are rare).
            let target_hash = if func_node_set.contains(&target_id) {
                content_hashes[&target_id]
            } else {
                // Cross-function edge: compute target's content hash
                let target_node = graph.get_compute_node(target_id).unwrap();
                hash_node_content(target_node)
            };

            outgoing.push((target_id, edge, target_hash));
        }

        let composite = hash_node_with_edges(node, &outgoing);
        composite_hashes.insert(node_id, composite);
    }

    // Combine all composite hashes into a single function root hash
    let mut final_hasher = blake3::Hasher::new();
    for &node_id in &func_nodes {
        final_hasher.update(&node_id.0.to_le_bytes());
        final_hasher.update(composite_hashes[&node_id].as_bytes());
    }
    final_hasher.finalize()
}

/// Computes root hashes for all functions in a program graph.
///
/// Returns a map from FunctionId to the function's root hash.
/// Iterates functions in sorted FunctionId order (never iterates HashMap
/// directly for hash-affecting operations).
pub fn hash_all_functions(graph: &ProgramGraph) -> HashMap<FunctionId, blake3::Hash> {
    let mut func_ids: Vec<FunctionId> = graph.functions().keys().copied().collect();
    func_ids.sort_by_key(|f| f.0);

    let mut result = HashMap::new();
    for func_id in func_ids {
        result.insert(func_id, hash_function(graph, func_id));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::id::FunctionId;
    use lmlang_core::node::ComputeNode;
    use lmlang_core::ops::{ArithOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    // -----------------------------------------------------------------------
    // Level 1: Node content hash tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_node_content_hash_deterministic() {
        let node = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(0),
        );
        let hash1 = hash_node_content(&node);
        let hash2 = hash_node_content(&node);
        assert_eq!(hash1, hash2, "Same node must produce same hash");
    }

    #[test]
    fn test_node_content_hash_changes_on_op_change() {
        let node_add = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(0),
        );
        let node_sub = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Sub },
            FunctionId(0),
        );
        let hash_add = hash_node_content(&node_add);
        let hash_sub = hash_node_content(&node_sub);
        assert_ne!(
            hash_add, hash_sub,
            "Different ops must produce different hashes"
        );
    }

    #[test]
    fn test_node_content_hash_changes_on_owner_change() {
        let node_fn0 = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(0),
        );
        let node_fn1 = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(1),
        );
        let hash_fn0 = hash_node_content(&node_fn0);
        let hash_fn1 = hash_node_content(&node_fn1);
        assert_ne!(
            hash_fn0, hash_fn1,
            "Different owners must produce different hashes"
        );
    }

    // -----------------------------------------------------------------------
    // Level 2: Node hash with edges (Merkle composition) tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_node_with_edges_changes_on_edge_add() {
        let node = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(0),
        );
        let hash_no_edges = hash_node_with_edges(&node, &[]);

        let target_hash = hash_node_content(&ComputeNode::core(
            ComputeOp::Parameter { index: 0 },
            FunctionId(0),
        ));
        let edge = FlowEdge::Data {
            source_port: 0,
            target_port: 0,
            value_type: TypeId::I32,
        };
        let hash_with_edge = hash_node_with_edges(
            &node,
            &[(NodeId(1), edge, target_hash)],
        );

        assert_ne!(
            hash_no_edges, hash_with_edge,
            "Adding an edge must change the hash"
        );
    }

    #[test]
    fn test_node_with_edges_changes_on_target_hash_change() {
        let node = ComputeNode::core(
            ComputeOp::BinaryArith { op: ArithOp::Add },
            FunctionId(0),
        );
        let edge = FlowEdge::Data {
            source_port: 0,
            target_port: 0,
            value_type: TypeId::I32,
        };

        let target_hash_a = blake3::hash(b"target_a");
        let target_hash_b = blake3::hash(b"target_b");

        let hash_a = hash_node_with_edges(
            &node,
            &[(NodeId(1), edge.clone(), target_hash_a)],
        );
        let hash_b = hash_node_with_edges(
            &node,
            &[(NodeId(1), edge, target_hash_b)],
        );

        assert_ne!(
            hash_a, hash_b,
            "Different target hashes must produce different composite hashes (Merkle propagation)"
        );
    }

    // -----------------------------------------------------------------------
    // Level 3: Per-function root hash tests
    // -----------------------------------------------------------------------

    /// Helper: build a minimal program graph with two functions.
    fn build_two_function_graph() -> (ProgramGraph, FunctionId, FunctionId) {
        let mut graph = ProgramGraph::new("main");
        let root = graph.modules.root_id();

        let fn_a = graph
            .add_function(
                "fn_a".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let fn_b = graph
            .add_function(
                "fn_b".into(),
                root,
                vec![("y".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // fn_a body: param -> return
        let pa = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_a)
            .unwrap();
        let ret_a = graph.add_core_op(ComputeOp::Return, fn_a).unwrap();
        graph
            .add_data_edge(pa, ret_a, 0, 0, TypeId::I32)
            .unwrap();

        // fn_b body: param -> return
        let pb = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, fn_b)
            .unwrap();
        let ret_b = graph.add_core_op(ComputeOp::Return, fn_b).unwrap();
        graph
            .add_data_edge(pb, ret_b, 0, 0, TypeId::I32)
            .unwrap();

        (graph, fn_a, fn_b)
    }

    #[test]
    fn test_function_hash_deterministic() {
        let (graph, fn_a, _fn_b) = build_two_function_graph();
        let hash1 = hash_function(&graph, fn_a);
        let hash2 = hash_function(&graph, fn_a);
        assert_eq!(hash1, hash2, "Same function must produce same hash");
    }

    #[test]
    fn test_function_hash_changes_on_node_mutation() {
        let (mut graph, fn_a, _fn_b) = build_two_function_graph();
        let hash_before = hash_function(&graph, fn_a);

        // Add a new node to fn_a
        let _extra = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, fn_a)
            .unwrap();

        let hash_after = hash_function(&graph, fn_a);
        assert_ne!(
            hash_before, hash_after,
            "Adding a node to a function must change its hash"
        );
    }

    #[test]
    fn test_function_hash_independent_across_functions() {
        let (mut graph, fn_a, fn_b) = build_two_function_graph();
        let hash_a_before = hash_function(&graph, fn_a);

        // Mutate fn_b only: add a node
        let _extra = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, fn_b)
            .unwrap();

        let hash_a_after = hash_function(&graph, fn_a);
        assert_eq!(
            hash_a_before, hash_a_after,
            "Modifying function B must NOT change function A's hash"
        );
    }

    #[test]
    fn test_function_hash_changes_on_edge_add() {
        let (mut graph, fn_a, _fn_b) = build_two_function_graph();
        let hash_before = hash_function(&graph, fn_a);

        // Add an extra node and edge within fn_a
        let extra = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, fn_a)
            .unwrap();
        // Find an existing node to connect from
        let fn_a_nodes = graph.function_nodes(fn_a);
        let first_node = fn_a_nodes[0];
        graph
            .add_data_edge(first_node, extra, 0, 0, TypeId::I32)
            .unwrap();

        let hash_after = hash_function(&graph, fn_a);
        assert_ne!(
            hash_before, hash_after,
            "Adding an edge within a function must change its hash"
        );
    }
}
