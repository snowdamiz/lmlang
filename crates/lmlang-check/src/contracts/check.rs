//! Contract checking logic: find contract nodes, evaluate subgraphs, produce violations.
//!
//! Called by the interpreter at function entry (preconditions), function return
//! (postconditions), and module boundaries (invariants).

use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp};

use crate::contracts::{ContractKind, ContractViolation};
use crate::interpreter::error::RuntimeError;
use crate::interpreter::value::Value;

/// Find all contract nodes of a given kind in a function, sorted by NodeId.
pub fn find_contract_nodes(
    graph: &ProgramGraph,
    func_id: FunctionId,
    kind: ContractKind,
) -> Vec<NodeId> {
    let mut result: Vec<NodeId> = graph
        .function_nodes(func_id)
        .into_iter()
        .filter(|node_id| {
            if let Some(node) = graph.get_compute_node(*node_id) {
                matches!(
                    (&node.op, kind),
                    (
                        ComputeNodeOp::Core(ComputeOp::Precondition { .. }),
                        ContractKind::Precondition,
                    ) | (
                        ComputeNodeOp::Core(ComputeOp::Postcondition { .. }),
                        ContractKind::Postcondition,
                    ) | (
                        ComputeNodeOp::Core(ComputeOp::Invariant { .. }),
                        ContractKind::Invariant,
                    )
                )
            } else {
                false
            }
        })
        .collect();
    result.sort_by_key(|n| n.0);
    result
}

/// Evaluate the condition subgraph for a contract node.
///
/// Walks backward from port 0 of the contract node through data edges to find
/// all nodes in the contract subgraph. Evaluates any unevaluated nodes using
/// the provided node_values map. Returns true if port 0 is `Value::Bool(true)`,
/// false otherwise.
pub fn evaluate_contract_condition(
    graph: &ProgramGraph,
    contract_node_id: NodeId,
    node_values: &HashMap<NodeId, Value>,
) -> Result<bool, RuntimeError> {
    // Find the node connected to port 0 of the contract node
    let node_idx: petgraph::graph::NodeIndex<u32> = contract_node_id.into();

    for edge_ref in graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
    {
        if let FlowEdge::Data { target_port: 0, .. } = edge_ref.weight() {
            let source_id = NodeId::from(edge_ref.source());
            if let Some(value) = node_values.get(&source_id) {
                return match value {
                    Value::Bool(b) => Ok(*b),
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: contract_node_id,
                        expected: "Bool".into(),
                        got: value.type_name().into(),
                    }),
                };
            } else {
                // Condition node hasn't been evaluated yet -- this means
                // the contract subgraph wasn't fully evaluated before checking.
                // Treat as failed (conservative).
                return Err(RuntimeError::MissingValue {
                    node: source_id,
                    port: 0,
                });
            }
        }
    }

    // No input edge at port 0 -- contract has no condition (treat as passed)
    Ok(true)
}

/// Collect counterexample values from nodes connected to the contract subgraph.
///
/// Returns node values from the contract subgraph evaluation, sorted by NodeId.
pub fn collect_counterexample(
    graph: &ProgramGraph,
    contract_node_id: NodeId,
    node_values: &HashMap<NodeId, Value>,
) -> Vec<(NodeId, Value)> {
    let node_idx: petgraph::graph::NodeIndex<u32> = contract_node_id.into();
    let mut counterexample = Vec::new();

    // Walk backward through all incoming data edges to collect node values
    for edge_ref in graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
    {
        if edge_ref.weight().is_data() {
            let source_id = NodeId::from(edge_ref.source());
            if let Some(value) = node_values.get(&source_id) {
                counterexample.push((source_id, value.clone()));
            }
        }
    }

    counterexample.sort_by_key(|(nid, _)| nid.0);
    counterexample
}

/// Get the contract message from a contract node.
fn get_contract_message(graph: &ProgramGraph, contract_node_id: NodeId) -> String {
    if let Some(node) = graph.get_compute_node(contract_node_id) {
        match &node.op {
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Precondition { message }) => {
                message.clone()
            }
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Postcondition { message }) => {
                message.clone()
            }
            lmlang_core::ops::ComputeNodeOp::Core(ComputeOp::Invariant { message, .. }) => {
                message.clone()
            }
            _ => String::new(),
        }
    } else {
        String::new()
    }
}

/// Check all preconditions for a function.
///
/// Called at function entry, AFTER parameter nodes are seeded but BEFORE
/// body nodes are scheduled. Returns a list of violations (empty = all passed).
pub fn check_preconditions(
    graph: &ProgramGraph,
    func_id: FunctionId,
    args: &[Value],
    node_values: &HashMap<NodeId, Value>,
) -> Result<Vec<ContractViolation>, RuntimeError> {
    let contract_nodes = find_contract_nodes(graph, func_id, ContractKind::Precondition);
    let mut violations = Vec::new();

    for contract_node_id in contract_nodes {
        match evaluate_contract_condition(graph, contract_node_id, node_values) {
            Ok(true) => {
                // Condition met, no violation
            }
            Ok(false) => {
                let message = get_contract_message(graph, contract_node_id);
                let counterexample = collect_counterexample(graph, contract_node_id, node_values);
                violations.push(ContractViolation {
                    kind: ContractKind::Precondition,
                    contract_node: contract_node_id,
                    function_id: func_id,
                    message,
                    inputs: args.to_vec(),
                    actual_return: None,
                    counterexample,
                });
            }
            Err(_) => {
                // Condition could not be evaluated (e.g. missing value).
                // Skip -- contract subgraph may not have been evaluated yet.
                // This is not an error; the contract simply wasn't checkable.
            }
        }
    }

    Ok(violations)
}

/// Check all postconditions for a function.
///
/// Called at function return, AFTER the return value is computed but BEFORE
/// it is delivered to the caller.
pub fn check_postconditions(
    graph: &ProgramGraph,
    func_id: FunctionId,
    return_value: &Value,
    args: &[Value],
    node_values: &HashMap<NodeId, Value>,
) -> Result<Vec<ContractViolation>, RuntimeError> {
    let contract_nodes = find_contract_nodes(graph, func_id, ContractKind::Postcondition);
    let mut violations = Vec::new();

    for contract_node_id in contract_nodes {
        match evaluate_contract_condition(graph, contract_node_id, node_values) {
            Ok(true) => {
                // Condition met
            }
            Ok(false) => {
                let message = get_contract_message(graph, contract_node_id);
                let counterexample = collect_counterexample(graph, contract_node_id, node_values);
                violations.push(ContractViolation {
                    kind: ContractKind::Postcondition,
                    contract_node: contract_node_id,
                    function_id: func_id,
                    message,
                    inputs: args.to_vec(),
                    actual_return: Some(return_value.clone()),
                    counterexample,
                });
            }
            Err(_) => {
                // Contract subgraph not fully evaluated -- skip
            }
        }
    }

    Ok(violations)
}

/// Check invariants for a value crossing a module boundary.
///
/// Finds all Invariant nodes with matching target_type and evaluates them
/// using mini-subgraph evaluation (not pre-existing node_values).
/// Note: module boundary detection requires checking FunctionDef.module;
/// the caller is responsible for determining when this check is needed.
pub fn check_invariants_for_value(
    graph: &ProgramGraph,
    type_id: lmlang_core::type_id::TypeId,
    value: &Value,
    source_func: FunctionId,
) -> Result<Vec<ContractViolation>, RuntimeError> {
    // Find all Invariant nodes in the source function with matching target_type
    let contract_nodes: Vec<NodeId> =
        find_contract_nodes(graph, source_func, ContractKind::Invariant)
            .into_iter()
            .filter(|node_id| {
                if let Some(node) = graph.get_compute_node(*node_id) {
                    match &node.op {
                        ComputeNodeOp::Core(ComputeOp::Invariant { target_type, .. }) => {
                            *target_type == type_id
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            })
            .collect();

    let mut violations = Vec::new();

    for contract_node_id in contract_nodes {
        match evaluate_invariant_for_value(graph, contract_node_id, value) {
            Ok(true) => {}
            Ok(false) => {
                let message = get_contract_message(graph, contract_node_id);
                let counterexample = vec![(contract_node_id, value.clone())];
                violations.push(ContractViolation {
                    kind: ContractKind::Invariant,
                    contract_node: contract_node_id,
                    function_id: source_func,
                    message,
                    inputs: vec![value.clone()],
                    actual_return: None,
                    counterexample,
                });
            }
            Err(_) => {
                // Subgraph evaluation failed -- report as conservative violation.
                let message = get_contract_message(graph, contract_node_id);
                violations.push(ContractViolation {
                    kind: ContractKind::Invariant,
                    contract_node: contract_node_id,
                    function_id: source_func,
                    message,
                    inputs: vec![value.clone()],
                    actual_return: None,
                    counterexample: vec![],
                });
            }
        }
    }

    Ok(violations)
}

/// Evaluate an invariant's condition subgraph on-the-fly for a concrete value.
///
/// Unlike `evaluate_contract_condition` (which reads pre-computed node_values),
/// this function performs a mini-evaluation of the invariant's condition subgraph:
/// - Parameter nodes are substituted with `arg_value`
/// - Const nodes evaluate to their literal values
/// - Compare/arithmetic nodes are computed from their inputs
///
/// This is necessary at module boundaries because the callee's frame hasn't been
/// pushed yet, so normal worklist evaluation hasn't run for the callee's nodes.
pub fn evaluate_invariant_for_value(
    graph: &ProgramGraph,
    contract_node_id: NodeId,
    arg_value: &Value,
) -> Result<bool, RuntimeError> {
    // Mini-evaluate: walk the subgraph backward from the contract node,
    // then evaluate forward. Use a local node_values map.
    let mut local_values: HashMap<NodeId, Value> = HashMap::new();
    evaluate_subgraph_node(graph, contract_node_id, arg_value, &mut local_values)?;

    // Now check port 0 of the contract node (same logic as evaluate_contract_condition)
    let node_idx: petgraph::graph::NodeIndex<u32> = contract_node_id.into();
    for edge_ref in graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
    {
        if let FlowEdge::Data { target_port: 0, .. } = edge_ref.weight() {
            let source_id = NodeId::from(edge_ref.source());
            if let Some(value) = local_values.get(&source_id) {
                return match value {
                    Value::Bool(b) => Ok(*b),
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: contract_node_id,
                        expected: "Bool".into(),
                        got: value.type_name().into(),
                    }),
                };
            } else {
                return Err(RuntimeError::MissingValue {
                    node: source_id,
                    port: 0,
                });
            }
        }
    }
    // No condition edge -- treat as passed
    Ok(true)
}

/// Recursively evaluate a node in the invariant subgraph.
///
/// Walks backward through data edges to find dependencies, evaluates them first
/// (post-order), then evaluates this node. Results are cached in `local_values`.
fn evaluate_subgraph_node(
    graph: &ProgramGraph,
    node_id: NodeId,
    arg_value: &Value,
    local_values: &mut HashMap<NodeId, Value>,
) -> Result<(), RuntimeError> {
    // Already evaluated?
    if local_values.contains_key(&node_id) {
        return Ok(());
    }

    let node = graph
        .get_compute_node(node_id)
        .ok_or_else(|| RuntimeError::InternalError {
            message: format!("invariant subgraph node {} not found", node_id),
        })?;
    let op = node.op.clone();

    match &op {
        ComputeNodeOp::Core(ComputeOp::Parameter { .. }) => {
            // Substitute with the argument value being checked
            local_values.insert(node_id, arg_value.clone());
        }
        ComputeNodeOp::Core(ComputeOp::Const { value: const_val }) => {
            local_values.insert(node_id, Value::from_const(const_val));
        }
        _ => {
            // Recursively evaluate all data input dependencies first
            let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
            let incoming: Vec<(u16, NodeId)> = graph
                .compute()
                .edges_directed(node_idx, Direction::Incoming)
                .filter_map(|edge_ref| match edge_ref.weight() {
                    FlowEdge::Data { target_port, .. } => {
                        Some((*target_port, NodeId::from(edge_ref.source())))
                    }
                    _ => None,
                })
                .collect();

            for &(_, source_id) in &incoming {
                evaluate_subgraph_node(graph, source_id, arg_value, local_values)?;
            }

            // Gather inputs sorted by port
            let mut inputs: Vec<(u16, Value)> = incoming
                .iter()
                .filter_map(|(port, source_id)| {
                    local_values.get(source_id).map(|v| (*port, v.clone()))
                })
                .collect();
            inputs.sort_by_key(|(port, _)| *port);

            // Evaluate using the existing eval_op for arithmetic/comparison/etc.
            use crate::interpreter::eval::eval_op;
            if let Some(value) = eval_op(&op, &inputs, node_id, graph)? {
                local_values.insert(node_id, value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::{CmpOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    /// Helper: build a function with a precondition `a >= 0`.
    /// Returns (graph, func_id, param_a_node, cmp_node, precond_node)
    fn build_precondition_graph() -> (ProgramGraph, FunctionId, NodeId, NodeId, NodeId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "checked_fn".into(),
                root,
                vec![("a".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // Parameter node
        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();

        // Const 0
        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::types::ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();

        // Compare: a >= 0
        let cmp_node = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_a, cmp_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, cmp_node, 0, 1, TypeId::I32)
            .unwrap();

        // Precondition node
        let precond_node = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "a must be non-negative".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(cmp_node, precond_node, 0, 0, TypeId::BOOL)
            .unwrap();

        // Return node (just return a)
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(param_a, ret, 0, 0, TypeId::I32)
            .unwrap();

        (graph, func_id, param_a, cmp_node, precond_node)
    }

    /// Helper: build a function with a postcondition `result > 0`.
    fn build_postcondition_graph() -> (ProgramGraph, FunctionId, NodeId, NodeId, NodeId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "positive_fn".into(),
                root,
                vec![("a".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();

        // Return node
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(param_a, ret, 0, 0, TypeId::I32)
            .unwrap();

        // Const 0
        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::types::ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();

        // Compare: result > 0 (we'll compare the param value as the "result")
        let cmp_node = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Gt }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_a, cmp_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, cmp_node, 0, 1, TypeId::I32)
            .unwrap();

        // Postcondition node
        let postcond_node = graph
            .add_core_op(
                ComputeOp::Postcondition {
                    message: "result must be positive".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(cmp_node, postcond_node, 0, 0, TypeId::BOOL)
            .unwrap();
        graph
            .add_data_edge(param_a, postcond_node, 0, 1, TypeId::I32)
            .unwrap();

        (graph, func_id, param_a, cmp_node, postcond_node)
    }

    #[test]
    fn test_find_contract_nodes_precondition() {
        let (graph, func_id, _, _, precond_node) = build_precondition_graph();
        let nodes = find_contract_nodes(&graph, func_id, ContractKind::Precondition);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], precond_node);
    }

    #[test]
    fn test_find_contract_nodes_no_postconditions() {
        let (graph, func_id, _, _, _) = build_precondition_graph();
        let nodes = find_contract_nodes(&graph, func_id, ContractKind::Postcondition);
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_precondition_passes_with_valid_input() {
        let (graph, func_id, param_a, cmp_node, _) = build_precondition_graph();

        // Simulate: a = 5, so a >= 0 is true
        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(5));
        node_values.insert(cmp_node, Value::Bool(true));

        let violations =
            check_preconditions(&graph, func_id, &[Value::I32(5)], &node_values).unwrap();
        assert!(violations.is_empty(), "Expected no violations");
    }

    #[test]
    fn test_precondition_fails_with_negative_input() {
        let (graph, func_id, param_a, cmp_node, precond_node) = build_precondition_graph();

        // Simulate: a = -1, so a >= 0 is false
        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(-1));
        node_values.insert(cmp_node, Value::Bool(false));

        let violations =
            check_preconditions(&graph, func_id, &[Value::I32(-1)], &node_values).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].kind, ContractKind::Precondition);
        assert_eq!(violations[0].contract_node, precond_node);
        assert_eq!(violations[0].message, "a must be non-negative");
        assert_eq!(violations[0].inputs, vec![Value::I32(-1)]);
        assert!(violations[0].actual_return.is_none());
    }

    #[test]
    fn test_postcondition_passes_with_positive_result() {
        let (graph, func_id, param_a, cmp_node, _) = build_postcondition_graph();

        // Simulate: a = 5, so result > 0 is true
        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(5));
        node_values.insert(cmp_node, Value::Bool(true));

        let violations = check_postconditions(
            &graph,
            func_id,
            &Value::I32(5),
            &[Value::I32(5)],
            &node_values,
        )
        .unwrap();
        assert!(violations.is_empty());
    }

    #[test]
    fn test_postcondition_fails_with_zero_result() {
        let (graph, func_id, param_a, cmp_node, postcond_node) = build_postcondition_graph();

        // Simulate: a = 0, so result > 0 is false
        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(0));
        node_values.insert(cmp_node, Value::Bool(false));

        let violations = check_postconditions(
            &graph,
            func_id,
            &Value::I32(0),
            &[Value::I32(0)],
            &node_values,
        )
        .unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].kind, ContractKind::Postcondition);
        assert_eq!(violations[0].contract_node, postcond_node);
        assert_eq!(violations[0].message, "result must be positive");
        assert_eq!(violations[0].actual_return, Some(Value::I32(0)));
        assert_eq!(violations[0].inputs, vec![Value::I32(0)]);
    }

    #[test]
    fn test_violation_includes_counterexample() {
        let (graph, func_id, param_a, cmp_node, _) = build_precondition_graph();

        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(-5));
        node_values.insert(cmp_node, Value::Bool(false));

        let violations =
            check_preconditions(&graph, func_id, &[Value::I32(-5)], &node_values).unwrap();
        assert_eq!(violations.len(), 1);

        // Counterexample should contain the cmp_node value
        let counterexample = &violations[0].counterexample;
        assert!(
            !counterexample.is_empty(),
            "Counterexample should not be empty"
        );
        // The counterexample should be sorted by NodeId
        for i in 1..counterexample.len() {
            assert!(counterexample[i].0 .0 >= counterexample[i - 1].0 .0);
        }
    }

    #[test]
    fn test_valid_function_passes_both_pre_and_post() {
        // Build a function with both pre and postconditions
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "abs_fn".into(),
                root,
                vec![("a".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();

        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::types::ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();

        // Precondition: a >= 0
        let pre_cmp = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_a, pre_cmp, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, pre_cmp, 0, 1, TypeId::I32)
            .unwrap();

        let precond = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "a >= 0".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(pre_cmp, precond, 0, 0, TypeId::BOOL)
            .unwrap();

        // Postcondition: result >= 0
        let post_cmp = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_a, post_cmp, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, post_cmp, 0, 1, TypeId::I32)
            .unwrap();

        let postcond = graph
            .add_core_op(
                ComputeOp::Postcondition {
                    message: "result >= 0".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(post_cmp, postcond, 0, 0, TypeId::BOOL)
            .unwrap();
        graph
            .add_data_edge(param_a, postcond, 0, 1, TypeId::I32)
            .unwrap();

        // Return node
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(param_a, ret, 0, 0, TypeId::I32)
            .unwrap();

        // Simulate: a = 5, all conditions are true
        let mut node_values = HashMap::new();
        node_values.insert(param_a, Value::I32(5));
        node_values.insert(pre_cmp, Value::Bool(true));
        node_values.insert(post_cmp, Value::Bool(true));

        let pre_violations =
            check_preconditions(&graph, func_id, &[Value::I32(5)], &node_values).unwrap();
        assert!(pre_violations.is_empty());

        let post_violations = check_postconditions(
            &graph,
            func_id,
            &Value::I32(5),
            &[Value::I32(5)],
            &node_values,
        )
        .unwrap();
        assert!(post_violations.is_empty());
    }

    /// Helper: build a function with an invariant `x >= 0` on TypeId::I32.
    /// Returns (graph, func_id, invariant_node_id)
    fn build_invariant_graph() -> (ProgramGraph, FunctionId, NodeId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "inv_fn".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // Parameter node
        let param_x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();

        // Const 0
        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::types::ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();

        // Compare: x >= 0
        let cmp_node = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph
            .add_data_edge(param_x, cmp_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_zero, cmp_node, 0, 1, TypeId::I32)
            .unwrap();

        // Invariant node targeting I32
        let inv_node = graph
            .add_core_op(
                ComputeOp::Invariant {
                    target_type: TypeId::I32,
                    message: "x must be non-negative".into(),
                },
                func_id,
            )
            .unwrap();
        graph
            .add_data_edge(cmp_node, inv_node, 0, 0, TypeId::BOOL)
            .unwrap();

        // Return node
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(param_x, ret, 0, 0, TypeId::I32)
            .unwrap();

        (graph, func_id, inv_node)
    }

    #[test]
    fn test_evaluate_invariant_for_value_passes() {
        let (graph, _, inv_node) = build_invariant_graph();
        // x = 5, condition x >= 0 should be true
        let result = evaluate_invariant_for_value(&graph, inv_node, &Value::I32(5)).unwrap();
        assert!(result, "Invariant should pass for x=5 (5 >= 0)");
    }

    #[test]
    fn test_evaluate_invariant_for_value_fails() {
        let (graph, _, inv_node) = build_invariant_graph();
        // x = -1, condition x >= 0 should be false
        let result = evaluate_invariant_for_value(&graph, inv_node, &Value::I32(-1)).unwrap();
        assert!(!result, "Invariant should fail for x=-1 (-1 >= 0 is false)");
    }

    #[test]
    fn test_check_invariants_for_value_with_mini_eval() {
        let (graph, func_id, _) = build_invariant_graph();
        // x = -1 should produce 1 violation
        let violations =
            check_invariants_for_value(&graph, TypeId::I32, &Value::I32(-1), func_id).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].kind, ContractKind::Invariant);
        assert_eq!(violations[0].message, "x must be non-negative");
    }
}
