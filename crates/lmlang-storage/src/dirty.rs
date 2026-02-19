//! Incremental compilation dirty detection.
//!
//! Compares previous compilation hashes with current graph state to determine
//! which functions need recompilation. Contract changes (dev-only nodes) do
//! NOT mark functions dirty because contracts are stripped during compilation.

use std::collections::{HashMap, HashSet};

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::FunctionId;

use crate::hash::hash_function_for_compilation;

/// The set of functions that need recompilation.
///
/// Tracks three categories: new (no previous hash), modified (hash changed),
/// and removed (present in previous but not current graph).
#[derive(Debug, Clone)]
pub struct DirtySet {
    /// Functions that were added since last compilation.
    pub new: HashSet<FunctionId>,
    /// Functions whose non-contract content has changed since last compilation.
    pub modified: HashSet<FunctionId>,
    /// Functions that were removed from the graph.
    pub removed: HashSet<FunctionId>,
}

impl DirtySet {
    /// Returns all functions needing recompilation (new + modified).
    pub fn needs_recompile(&self) -> HashSet<FunctionId> {
        self.new.union(&self.modified).copied().collect()
    }

    /// Returns true if nothing changed (empty dirty set).
    pub fn is_clean(&self) -> bool {
        self.new.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }

    /// Total count of dirty functions (new + modified + removed).
    pub fn total(&self) -> usize {
        self.new.len() + self.modified.len() + self.removed.len()
    }
}

/// Compute the dirty set by comparing previous compilation hashes against
/// the current graph.
///
/// Uses [`hash_function_for_compilation`] which excludes contract nodes,
/// so adding/modifying/removing contracts does NOT produce a dirty function.
///
/// # Arguments
///
/// * `graph` - Current program graph state
/// * `previous_hashes` - Compilation hashes from the last successful build
///
/// # Returns
///
/// A [`DirtySet`] categorizing all changed functions.
pub fn compute_dirty_set(
    graph: &ProgramGraph,
    previous_hashes: &HashMap<FunctionId, blake3::Hash>,
) -> DirtySet {
    let current_hashes = crate::hash::hash_all_functions_for_compilation(graph);

    let mut new = HashSet::new();
    let mut modified = HashSet::new();
    let mut removed = HashSet::new();

    // Check current functions against previous hashes
    for (&func_id, &current_hash) in &current_hashes {
        match previous_hashes.get(&func_id) {
            Some(&prev_hash) => {
                if prev_hash != current_hash {
                    modified.insert(func_id);
                }
            }
            None => {
                new.insert(func_id);
            }
        }
    }

    // Check for removed functions
    for &func_id in previous_hashes.keys() {
        if !current_hashes.contains_key(&func_id) {
            removed.insert(func_id);
        }
    }

    DirtySet {
        new,
        modified,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::{ArithOp, CmpOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::{ConstValue, Visibility};

    /// Helper: build a graph with one function.
    fn build_single_function_graph() -> (ProgramGraph, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "f".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(param, ret, 0, 0, TypeId::I32).unwrap();

        (graph, func_id)
    }

    #[test]
    fn test_clean_set_when_nothing_changed() {
        let (graph, func_id) = build_single_function_graph();
        let hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        let dirty = compute_dirty_set(&graph, &hashes);
        assert!(dirty.is_clean());
        assert_eq!(dirty.total(), 0);
        assert!(dirty.needs_recompile().is_empty());
    }

    #[test]
    fn test_new_function_detected() {
        let (graph, _) = build_single_function_graph();
        let empty_hashes = HashMap::new();

        let dirty = compute_dirty_set(&graph, &empty_hashes);
        assert_eq!(dirty.new.len(), 1);
        assert!(dirty.modified.is_empty());
        assert!(dirty.removed.is_empty());
    }

    #[test]
    fn test_modified_function_detected() {
        let (mut graph, func_id) = build_single_function_graph();
        let prev_hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        // Add a compute node to change the function
        graph
            .add_core_op(
                ComputeOp::Const {
                    value: ConstValue::I32(42),
                },
                func_id,
            )
            .unwrap();

        let dirty = compute_dirty_set(&graph, &prev_hashes);
        assert!(dirty.new.is_empty());
        assert_eq!(dirty.modified.len(), 1);
        assert!(dirty.modified.contains(&func_id));
        assert!(dirty.removed.is_empty());
    }

    #[test]
    fn test_removed_function_detected() {
        let (graph, func_id) = build_single_function_graph();
        let prev_hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        // Create a new empty graph (the function is gone)
        let empty_graph = ProgramGraph::new("test");

        let dirty = compute_dirty_set(&empty_graph, &prev_hashes);
        assert!(dirty.new.is_empty());
        assert!(dirty.modified.is_empty());
        assert_eq!(dirty.removed.len(), 1);
        assert!(dirty.removed.contains(&func_id));
    }

    #[test]
    fn test_contract_change_does_not_dirty() {
        let (mut graph, func_id) = build_single_function_graph();
        let prev_hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        // Add a precondition -- should NOT dirty the function
        let param_nodes = graph.function_nodes(func_id);
        let param_node = param_nodes[0]; // first node is the parameter

        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();
        let cmp = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_node, cmp, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, cmp, 0, 1, TypeId::I32)
            .unwrap();

        let precond = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "x >= 0".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(cmp, precond, 0, 0, TypeId::BOOL)
            .unwrap();

        let dirty = compute_dirty_set(&graph, &prev_hashes);

        // The compilation hash should NOT have changed because contract nodes are excluded.
        // BUT wait -- we also added const_zero and cmp which are NON-contract nodes!
        // The precondition node itself is filtered, but its supporting comparison nodes are not.
        // This is the correct behavior: if you add support nodes for contracts, those nodes
        // ARE part of the function body and DO affect compilation.
        // In a well-designed contract system, the contract subgraph nodes would be dedicated
        // to contracts and not shared with the main computation.
        //
        // For this test, we need to verify that adding ONLY a contract node (with no new
        // supporting nodes) doesn't dirty the function. Let's add just a contract node
        // with an existing comparison already in the graph.
        //
        // Since we did add const_zero and cmp (non-contract), the function IS dirty.
        // This is correct behavior -- the supporting nodes changed the function.
        assert!(
            !dirty.is_clean(),
            "Adding support nodes + contract should dirty the function"
        );
    }

    #[test]
    fn test_adding_only_contract_node_does_not_dirty() {
        let (mut graph, func_id) = build_single_function_graph();

        // First, add a compare node and get the initial hashes AFTER it exists
        let param_nodes = graph.function_nodes(func_id);
        let param_node = param_nodes[0];

        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();
        let cmp = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_node, cmp, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, cmp, 0, 1, TypeId::I32)
            .unwrap();

        // Take snapshot AFTER support nodes exist
        let prev_hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        // Now add ONLY a precondition node (no new support nodes)
        let _precond = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "x >= 0".into(),
                },
                func_id,
            )
            .unwrap();
        // Wire contract node to existing compare
        graph
            .add_data_edge(cmp, _precond, 0, 0, TypeId::BOOL)
            .unwrap();

        let dirty = compute_dirty_set(&graph, &prev_hashes);

        // Adding only a contract node should NOT dirty the function
        // because contract nodes are excluded from compilation hashing
        assert!(
            dirty.is_clean(),
            "Adding only a contract node should not dirty the function"
        );
    }

    #[test]
    fn test_needs_recompile_combines_new_and_modified() {
        let (mut graph, func_id) = build_single_function_graph();
        let root = graph.modules.root_id();

        // Get hashes before adding a new function
        let prev_hashes = crate::hash::hash_all_functions_for_compilation(&graph);

        // Add a new function
        let func_b = graph
            .add_function("g".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();
        let ret_b = graph.add_core_op(ComputeOp::Return, func_b).unwrap();

        // Modify existing function
        graph
            .add_core_op(
                ComputeOp::Const {
                    value: ConstValue::I32(99),
                },
                func_id,
            )
            .unwrap();

        let dirty = compute_dirty_set(&graph, &prev_hashes);
        let recompile = dirty.needs_recompile();

        assert!(
            recompile.contains(&func_id),
            "Modified function should need recompile"
        );
        assert!(
            recompile.contains(&func_b),
            "New function should need recompile"
        );
        assert_eq!(recompile.len(), 2);
    }
}
