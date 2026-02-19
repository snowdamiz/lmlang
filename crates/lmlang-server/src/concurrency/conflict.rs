//! Conflict detection via per-function blake3 hash comparison.
//!
//! When an agent submits mutations, it provides expected function hashes
//! from its last read. If the current hash differs, a conflict is reported
//! with a structured diff showing what changed.

use std::collections::HashMap;

use serde::Serialize;

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{EdgeId, FunctionId, NodeId};
use lmlang_storage::hash::hash_function;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Detail of a single function conflict.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictDetail {
    /// Which function has a conflict.
    pub function_id: FunctionId,
    /// The hash the agent expected (from its last read).
    pub expected_hash: String,
    /// The current hash in the graph.
    pub current_hash: String,
    /// Structural diff showing current function state.
    pub changes: FunctionDiff,
}

/// Structural description of a function's current content.
///
/// Since we lack a "before" snapshot, this shows the current state. Combined
/// with the hash mismatch, agents can understand what changed and re-plan.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDiff {
    /// Nodes currently in the function (treated as "added" relative to agent's view).
    pub added_nodes: Vec<NodeId>,
    /// Nodes removed since agent's view (empty without snapshot).
    pub removed_nodes: Vec<NodeId>,
    /// Nodes modified since agent's view (empty without snapshot).
    pub modified_nodes: Vec<NodeId>,
    /// Edges currently in the function.
    pub added_edges: Vec<EdgeId>,
    /// Edges removed since agent's view (empty without snapshot).
    pub removed_edges: Vec<EdgeId>,
}

/// Checks function hashes against expected values.
///
/// For each entry in `expected_hashes`, computes the current hash via
/// `lmlang_storage::hash::hash_function()` and compares hex strings.
/// Returns `Ok(())` if all match, or `Err(conflicts)` with details for
/// each mismatch.
pub fn check_hashes(
    graph: &ProgramGraph,
    expected_hashes: &HashMap<FunctionId, String>,
) -> Result<(), Vec<ConflictDetail>> {
    let mut conflicts = Vec::new();

    for (&func_id, expected_hex) in expected_hashes {
        let current_hash = hash_function(graph, func_id);
        let current_hex = current_hash.to_hex().to_string();

        if current_hex != *expected_hex {
            let diff = build_function_diff(graph, func_id);
            conflicts.push(ConflictDetail {
                function_id: func_id,
                expected_hash: expected_hex.clone(),
                current_hash: current_hex,
                changes: diff,
            });
        }
    }

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(conflicts)
    }
}

/// Builds a structural description of a function's current nodes and edges.
///
/// Since we don't store a "before" snapshot, the diff shows all current
/// nodes as `added_nodes` and all current intra-function edges as
/// `added_edges`. The `removed_*` and `modified_*` fields are empty.
/// The hash mismatch in [`ConflictDetail`] tells the agent something
/// changed; this diff shows the current state for re-planning.
pub fn build_function_diff(graph: &ProgramGraph, func_id: FunctionId) -> FunctionDiff {
    let func_nodes = graph.function_nodes(func_id);
    let func_node_set: std::collections::HashSet<NodeId> = func_nodes.iter().copied().collect();

    // Collect intra-function edges
    let mut edges = Vec::new();
    for &node_id in &func_nodes {
        let node_idx: NodeIndex<u32> = node_id.into();
        for edge_ref in graph
            .compute()
            .edges_directed(node_idx, Direction::Outgoing)
        {
            let target = NodeId::from(edge_ref.target());
            if func_node_set.contains(&target) {
                edges.push(EdgeId(edge_ref.id().index() as u32));
            }
        }
    }

    FunctionDiff {
        added_nodes: func_nodes,
        removed_nodes: Vec::new(),
        modified_nodes: Vec::new(),
        added_edges: edges,
        removed_edges: Vec::new(),
    }
}
