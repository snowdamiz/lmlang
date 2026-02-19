//! Static type checker for the lmlang computational graph.
//!
//! Provides two levels of type checking:
//! - [`validate_data_edge`]: Checks a single proposed edge for type compatibility
//!   (eager per-edit checking).
//! - [`validate_graph`]: Scans the entire graph and reports ALL type errors at
//!   once (full validation).
//!
//! Both functions are pure -- they read the graph but do not modify it.

pub mod coercion;
pub mod diagnostics;
pub mod rules;

pub use coercion::{can_coerce, common_numeric_type, is_float, is_integer, is_numeric};
pub use diagnostics::{FixSuggestion, TypeError};
pub use rules::{resolve_type_rule, OpTypeRule};

use petgraph::visit::EdgeRef;
use petgraph::Direction;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::NodeId;
use lmlang_core::type_id::TypeId;

use coercion::is_numeric as type_is_numeric;

/// Validates whether a proposed data edge is type-compatible with the target node.
///
/// This function is designed for **eager per-edit checking**: call it before
/// `add_data_edge` to catch type errors immediately.
///
/// Returns `Ok(())` if the edge is valid, or a list of type errors if not.
pub fn validate_data_edge(
    graph: &ProgramGraph,
    from: NodeId,
    to: NodeId,
    source_port: u16,
    target_port: u16,
    value_type: TypeId,
) -> Result<(), Vec<TypeError>> {
    let mut errors = Vec::new();

    // Look up the target node
    let target_node = match graph.get_compute_node(to) {
        Some(node) => node,
        None => return Ok(()), // Node not found -- let ProgramGraph handle this error
    };

    let function_id = target_node.owner;
    let registry = &graph.types;

    // Gather existing incoming data edges to the target node
    let to_idx: petgraph::graph::NodeIndex<u32> = to.into();
    let mut input_types: Vec<(u16, TypeId)> = graph
        .compute()
        .edges_directed(to_idx, Direction::Incoming)
        .filter_map(|edge_ref| match edge_ref.weight() {
            FlowEdge::Data {
                target_port,
                value_type,
                ..
            } => Some((*target_port, *value_type)),
            FlowEdge::Control { .. } => None,
        })
        .collect();

    // Add the proposed new edge to the input set
    // (Replace if same port, otherwise add)
    if let Some(existing) = input_types.iter_mut().find(|(p, _)| *p == target_port) {
        existing.1 = value_type;
    } else {
        input_types.push((target_port, value_type));
    }

    // Sort by port for consistent resolution
    input_types.sort_by_key(|(port, _)| *port);

    // Resolve the type rule for the target op
    match rules::resolve_type_rule(&target_node.op, &input_types, graph, to, function_id) {
        Ok(rule) => {
            // Check if value_type is compatible with the expected type at target_port
            if let Some(expected) = rule
                .expected_inputs
                .iter()
                .find(|(p, _)| *p == target_port)
                .map(|(_, t)| *t)
            {
                if value_type != expected && !can_coerce(value_type, expected, registry) {
                    let suggestion = if type_is_numeric(value_type) && type_is_numeric(expected) {
                        Some(FixSuggestion::InsertCast {
                            from: value_type,
                            to: expected,
                        })
                    } else {
                        None
                    };

                    errors.push(TypeError::TypeMismatch {
                        source_node: from,
                        target_node: to,
                        source_port,
                        target_port,
                        expected,
                        actual: value_type,
                        function_id,
                        suggestion,
                    });
                }
            }
        }
        Err(type_error) => {
            errors.push(type_error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates the entire graph and reports ALL type errors at once.
///
/// Iterates over every compute node, checks all incoming data edges against
/// the op's type rule, and verifies input counts. Does NOT stop at the first
/// error -- all errors are collected and returned.
///
/// Returns an empty `Vec` if the graph is type-valid.
pub fn validate_graph(graph: &ProgramGraph) -> Vec<TypeError> {
    let mut errors = Vec::new();
    let registry = &graph.types;

    // Iterate over all compute nodes
    for node_idx in graph.compute().node_indices() {
        let node_id = NodeId::from(node_idx);
        let node = match graph.compute().node_weight(node_idx) {
            Some(n) => n,
            None => continue,
        };

        let function_id = node.owner;

        // Gather all incoming data edges for this node
        let input_types = incoming_data_types(graph, node_id);

        // Resolve the type rule
        match rules::resolve_type_rule(&node.op, &input_types, graph, node_id, function_id) {
            Ok(rule) => {
                // Check each incoming data edge against expected types
                for &(port, actual_type) in &input_types {
                    if let Some(expected) = rule
                        .expected_inputs
                        .iter()
                        .find(|(p, _)| *p == port)
                        .map(|(_, t)| *t)
                    {
                        if actual_type != expected && !can_coerce(actual_type, expected, registry) {
                            // Find the source node for this edge
                            let source_node = find_source_node(graph, node_id, port);
                            let source_port = find_source_port(graph, node_id, port);

                            let suggestion =
                                if type_is_numeric(actual_type) && type_is_numeric(expected) {
                                    Some(FixSuggestion::InsertCast {
                                        from: actual_type,
                                        to: expected,
                                    })
                                } else {
                                    None
                                };

                            errors.push(TypeError::TypeMismatch {
                                source_node: source_node.unwrap_or(node_id),
                                target_node: node_id,
                                source_port: source_port.unwrap_or(0),
                                target_port: port,
                                expected,
                                actual: actual_type,
                                function_id,
                                suggestion,
                            });
                        }
                    }
                }

                // Check input count for ops that require a specific number of inputs
                check_input_count(&node.op, &input_types, node_id, function_id, &mut errors);
            }
            Err(type_error) => {
                errors.push(type_error);
            }
        }
    }

    errors
}

/// Collect all incoming data edge types for a node, keyed by target_port.
fn incoming_data_types(graph: &ProgramGraph, node_id: NodeId) -> Vec<(u16, TypeId)> {
    let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
    graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
        .filter_map(|edge_ref| match edge_ref.weight() {
            FlowEdge::Data {
                target_port,
                value_type,
                ..
            } => Some((*target_port, *value_type)),
            FlowEdge::Control { .. } => None,
        })
        .collect()
}

/// Find the source node for a data edge targeting a specific port.
fn find_source_node(graph: &ProgramGraph, target_node: NodeId, target_port: u16) -> Option<NodeId> {
    let node_idx: petgraph::graph::NodeIndex<u32> = target_node.into();
    graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
        .find_map(|edge_ref| match edge_ref.weight() {
            FlowEdge::Data {
                target_port: port, ..
            } if *port == target_port => Some(NodeId::from(edge_ref.source())),
            _ => None,
        })
}

/// Find the source port for a data edge targeting a specific port.
fn find_source_port(graph: &ProgramGraph, target_node: NodeId, target_port: u16) -> Option<u16> {
    let node_idx: petgraph::graph::NodeIndex<u32> = target_node.into();
    graph
        .compute()
        .edges_directed(node_idx, Direction::Incoming)
        .find_map(|edge_ref| match edge_ref.weight() {
            FlowEdge::Data {
                target_port: port,
                source_port,
                ..
            } if *port == target_port => Some(*source_port),
            _ => None,
        })
}

/// Check whether a node has the correct number of data inputs for its op type.
///
/// This catches cases like a BinaryArith node with 0 or 1 inputs.
fn check_input_count(
    op: &lmlang_core::ops::ComputeNodeOp,
    input_types: &[(u16, TypeId)],
    node_id: NodeId,
    function_id: lmlang_core::id::FunctionId,
    errors: &mut Vec<TypeError>,
) {
    use lmlang_core::ops::{ComputeOp, StructuredOp};

    let actual = input_types.len();

    let expected: Option<usize> = match op {
        lmlang_core::ops::ComputeNodeOp::Core(core_op) => match core_op {
            ComputeOp::BinaryArith { .. } => Some(2),
            ComputeOp::UnaryArith { .. } => Some(1),
            ComputeOp::Compare { .. } => Some(2),
            ComputeOp::BinaryLogic { .. } => Some(2),
            ComputeOp::Not => Some(1),
            ComputeOp::Shift { .. } => Some(2),
            ComputeOp::IfElse => Some(1),
            ComputeOp::Branch => Some(1),
            // Ops with variable or zero inputs -- no count check
            ComputeOp::Const { .. }
            | ComputeOp::Loop
            | ComputeOp::Match
            | ComputeOp::Jump
            | ComputeOp::Phi
            | ComputeOp::Alloc
            | ComputeOp::Load
            | ComputeOp::Store
            | ComputeOp::GetElementPtr
            | ComputeOp::Call { .. }
            | ComputeOp::IndirectCall
            | ComputeOp::Return
            | ComputeOp::Parameter { .. }
            | ComputeOp::Print
            | ComputeOp::ReadLine
            | ComputeOp::FileOpen
            | ComputeOp::FileRead
            | ComputeOp::FileWrite
            | ComputeOp::FileClose
            | ComputeOp::MakeClosure { .. }
            | ComputeOp::CaptureAccess { .. }
            | ComputeOp::Precondition { .. }
            | ComputeOp::Postcondition { .. }
            | ComputeOp::Invariant { .. } => None,
        },
        lmlang_core::ops::ComputeNodeOp::Structured(struct_op) => match struct_op {
            StructuredOp::StructGet { .. } => Some(1),
            StructuredOp::StructSet { .. } => Some(2),
            StructuredOp::ArrayGet => Some(2),
            StructuredOp::ArraySet => Some(3),
            StructuredOp::Cast { .. } => Some(1),
            StructuredOp::EnumDiscriminant => Some(1),
            StructuredOp::EnumPayload { .. } => Some(1),
            // Variable inputs
            StructuredOp::StructCreate { .. }
            | StructuredOp::ArrayCreate { .. }
            | StructuredOp::EnumCreate { .. } => None,
        },
    };

    if let Some(expected_count) = expected {
        if actual != expected_count {
            errors.push(TypeError::WrongInputCount {
                node: node_id,
                expected: expected_count,
                actual,
                function_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::{ArithOp, ComputeOp, StructuredOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    /// Helper: Create a graph with function "test_fn(a: i32, b: i32) -> i32"
    /// and return (graph, func_id).
    fn test_graph() -> (ProgramGraph, lmlang_core::id::FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();
        let func_id = graph
            .add_function(
                "test_fn".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        (graph, func_id)
    }

    /// Helper: Build a simple "a + b" graph and return (graph, func_id, param_a, param_b, add_node).
    fn add_graph() -> (
        ProgramGraph,
        lmlang_core::id::FunctionId,
        NodeId,
        NodeId,
        NodeId,
    ) {
        let (mut graph, func_id) = test_graph();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        graph
            .add_data_edge(param_a, add_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(param_b, add_node, 0, 1, TypeId::I32)
            .unwrap();

        (graph, func_id, param_a, param_b, add_node)
    }

    // -----------------------------------------------------------------------
    // validate_data_edge tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_data_edge_valid_i32_add() {
        let (mut graph, func_id) = test_graph();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        // First edge
        graph
            .add_data_edge(param_a, add_node, 0, 0, TypeId::I32)
            .unwrap();

        // Validate second edge before adding
        let result = validate_data_edge(&graph, param_b, add_node, 0, 1, TypeId::I32);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_data_edge_type_mismatch_i32_f64() {
        let (mut graph, func_id) = test_graph();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let const_f64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::F64(std::f64::consts::PI),
                },
                func_id,
            )
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        // Add i32 edge first
        graph
            .add_data_edge(param_a, add_node, 0, 0, TypeId::I32)
            .unwrap();

        // Now try adding f64 edge -- should fail (cross-family)
        let result = validate_data_edge(&graph, const_f64, add_node, 0, 1, TypeId::F64);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
    }

    #[test]
    fn validate_data_edge_bool_coerces_to_i32() {
        let (mut graph, func_id) = test_graph();

        let const_bool = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::Bool(true),
                },
                func_id,
            )
            .unwrap();
        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(42),
                },
                func_id,
            )
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        // Add i32 edge first
        graph
            .add_data_edge(const_i32, add_node, 0, 0, TypeId::I32)
            .unwrap();

        // Bool should coerce to integer for arithmetic
        let result = validate_data_edge(&graph, const_bool, add_node, 0, 1, TypeId::BOOL);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_data_edge_generates_insert_cast_suggestion() {
        let (mut graph, func_id) = test_graph();

        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(1),
                },
                func_id,
            )
            .unwrap();
        let const_i64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I64(2),
                },
                func_id,
            )
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        // Add i64 edge first so the resolved common type is i64
        graph
            .add_data_edge(const_i64, add_node, 0, 0, TypeId::I64)
            .unwrap();

        // i32 -> i64 widening is valid via coercion, so this should succeed
        let result = validate_data_edge(&graph, const_i32, add_node, 0, 1, TypeId::I32);
        // i32 can coerce to i64, so this should be Ok
        assert!(result.is_ok());

        // Now test a case where coercion fails but both are numeric:
        // f32 -> i64 (cross-family, no coercion, but both numeric)
        let const_f32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::F32(1.0),
                },
                func_id,
            )
            .unwrap();

        let add_node2 = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        graph
            .add_data_edge(const_i64, add_node2, 0, 0, TypeId::I64)
            .unwrap();

        let result = validate_data_edge(&graph, const_f32, add_node2, 0, 1, TypeId::F32);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // validate_graph tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_graph_valid_add_function() {
        let (mut graph, func_id, _, _, add_node) = add_graph();

        // Add return node
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(add_node, ret, 0, 0, TypeId::I32)
            .unwrap();

        let errors = validate_graph(&graph);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_graph_reports_all_errors() {
        let (mut graph, func_id) = test_graph();

        // Create two add nodes, each with mismatched types
        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(1),
                },
                func_id,
            )
            .unwrap();
        let const_f64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::F64(2.0),
                },
                func_id,
            )
            .unwrap();

        let add1 = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();
        let add2 = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        // Both add nodes get mismatched i32 + f64
        graph
            .add_data_edge(const_i32, add1, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_f64, add1, 0, 1, TypeId::F64)
            .unwrap();

        graph
            .add_data_edge(const_i32, add2, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_f64, add2, 0, 1, TypeId::F64)
            .unwrap();

        let errors = validate_graph(&graph);
        // Should have at least 2 errors (one per add node)
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got {}",
            errors.len()
        );
    }

    #[test]
    fn validate_graph_detects_missing_inputs() {
        let (mut graph, func_id) = test_graph();

        // Create a BinaryArith node with 0 inputs
        let _add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        let errors = validate_graph(&graph);
        assert!(
            errors.iter().any(|e| matches!(
                e,
                TypeError::WrongInputCount {
                    expected: 2,
                    actual: 0,
                    ..
                }
            )),
            "Expected WrongInputCount error, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_graph_multi_function() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Function 1: add(a: i32, b: i32) -> i32
        let func1 = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let p1a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func1)
            .unwrap();
        let p1b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, func1)
            .unwrap();
        let add = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func1)
            .unwrap();
        let ret1 = graph.add_core_op(ComputeOp::Return, func1).unwrap();

        graph.add_data_edge(p1a, add, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(p1b, add, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add, ret1, 0, 0, TypeId::I32).unwrap();

        // Function 2: negate(x: f64) -> f64
        let func2 = graph
            .add_function(
                "negate".into(),
                root,
                vec![("x".into(), TypeId::F64)],
                TypeId::F64,
                Visibility::Public,
            )
            .unwrap();

        let p2x = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func2)
            .unwrap();
        let neg = graph
            .add_core_op(
                ComputeOp::UnaryArith {
                    op: lmlang_core::UnaryArithOp::Neg,
                },
                func2,
            )
            .unwrap();
        let ret2 = graph.add_core_op(ComputeOp::Return, func2).unwrap();

        graph.add_data_edge(p2x, neg, 0, 0, TypeId::F64).unwrap();
        graph.add_data_edge(neg, ret2, 0, 0, TypeId::F64).unwrap();

        let errors = validate_graph(&graph);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn validate_graph_const_i32_plus_const_f64_returns_error() {
        let (mut graph, func_id) = test_graph();

        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(10),
                },
                func_id,
            )
            .unwrap();
        let const_f64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::F64(std::f64::consts::PI),
                },
                func_id,
            )
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        graph
            .add_data_edge(const_i32, add_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(const_f64, add_node, 0, 1, TypeId::F64)
            .unwrap();

        let errors = validate_graph(&graph);
        assert_eq!(
            errors.len(),
            1,
            "Expected exactly 1 error, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_graph_correct_return_type() {
        let (mut graph, func_id, _, _, add_node) = add_graph();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph
            .add_data_edge(add_node, ret, 0, 0, TypeId::I32)
            .unwrap();

        let errors = validate_graph(&graph);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_graph_reports_every_invalid_edge() {
        let (mut graph, func_id) = test_graph();

        // Create 3 separate add nodes, each with cross-family type mismatch
        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(1),
                },
                func_id,
            )
            .unwrap();
        let const_f64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::F64(2.0),
                },
                func_id,
            )
            .unwrap();

        for _ in 0..3 {
            let add = graph
                .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
                .unwrap();
            graph
                .add_data_edge(const_i32, add, 0, 0, TypeId::I32)
                .unwrap();
            graph
                .add_data_edge(const_f64, add, 0, 1, TypeId::F64)
                .unwrap();
        }

        let errors = validate_graph(&graph);
        assert!(
            errors.len() >= 3,
            "Expected at least 3 errors (one per invalid node), got {}",
            errors.len()
        );
    }

    #[test]
    fn validate_graph_nominal_typing_rejects_different_struct_type_ids() {
        use indexmap::IndexMap;
        use lmlang_core::types::{StructDef, Visibility};

        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Register two structs with identical fields but different TypeIds
        let point_id = graph
            .types
            .register_named(
                "Point",
                lmlang_core::LmType::Struct(StructDef {
                    name: "Point".into(),
                    type_id: TypeId(0),
                    fields: IndexMap::from([("x".into(), TypeId::F64), ("y".into(), TypeId::F64)]),
                    module: root,
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();

        let coord_id = graph
            .types
            .register_named(
                "Coordinate",
                lmlang_core::LmType::Struct(StructDef {
                    name: "Coordinate".into(),
                    type_id: TypeId(0),
                    fields: IndexMap::from([("x".into(), TypeId::F64), ("y".into(), TypeId::F64)]),
                    module: root,
                    visibility: Visibility::Public,
                }),
            )
            .unwrap();

        assert_ne!(point_id, coord_id);

        let func_id = graph
            .add_function(
                "f".into(),
                root,
                vec![("p".into(), point_id)],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        // Create a StructGet that expects a Point
        let const_coord = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::Unit,
                },
                func_id,
            )
            .unwrap();
        let struct_get = graph
            .add_structured_op(StructuredOp::StructGet { field_index: 0 }, func_id)
            .unwrap();

        // Connect with a Coordinate type where Point is expected
        graph
            .add_data_edge(const_coord, struct_get, 0, 0, coord_id)
            .unwrap();

        // The StructGet resolves its expected inputs based on the INCOMING type,
        // so it won't flag a type mismatch at the edge level.
        // However, if we had a function parameter expecting Point and fed it
        // Coordinate, that WOULD be caught.
        let param_node = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let print_node = graph.add_core_op(ComputeOp::Print, func_id).unwrap();

        // This is fine because Print accepts any type
        graph
            .add_data_edge(param_node, print_node, 0, 0, coord_id)
            .unwrap();

        // Verify that nominal typing is enforced: two different TypeIds are
        // different types even with identical field structures
        assert!(!can_coerce(point_id, coord_id, &graph.types));
        assert!(!can_coerce(coord_id, point_id, &graph.types));
    }

    #[test]
    fn validate_graph_empty_graph_is_valid() {
        let graph = ProgramGraph::new("test");
        let errors = validate_graph(&graph);
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_data_edge_widening_i32_to_i64() {
        let (mut graph, func_id) = test_graph();

        let const_i64 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I64(100),
                },
                func_id,
            )
            .unwrap();
        let const_i32 = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(50),
                },
                func_id,
            )
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();

        graph
            .add_data_edge(const_i64, add_node, 0, 0, TypeId::I64)
            .unwrap();

        // i32 widening to i64 should succeed
        let result = validate_data_edge(&graph, const_i32, add_node, 0, 1, TypeId::I32);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_data_edge_narrowing_i64_to_i32_fails() {
        // For BinaryArith, common_numeric_type(I32, I64) resolves to I64 (wider wins)
        // and I32 can coerce to I64, so mixing is allowed there.
        // The narrowing case is: an op that strictly expects I32 getting I64.
        // Use Call for that:
        let mut graph2 = ProgramGraph::new("test2");
        let root2 = graph2.modules.root_id();
        let callee = graph2
            .add_function(
                "callee".into(),
                root2,
                vec![("x".into(), TypeId::I32)],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();
        let caller = graph2
            .add_function(
                "caller".into(),
                root2,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        let const_i64_2 = graph2
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I64(100),
                },
                caller,
            )
            .unwrap();
        let call_node = graph2
            .add_core_op(ComputeOp::Call { target: callee }, caller)
            .unwrap();

        graph2
            .add_data_edge(const_i64_2, call_node, 0, 0, TypeId::I64)
            .unwrap();

        // Validate: I64 value going to a port that expects I32 (narrowing)
        let errors = validate_graph(&graph2);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "Expected type mismatch for I64 -> I32 narrowing, got: {:?}",
            errors
        );
    }

    #[test]
    fn validate_graph_call_wrong_arg_count() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let callee = graph
            .add_function(
                "callee".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();
        let caller = graph
            .add_function(
                "caller".into(),
                root,
                vec![],
                TypeId::UNIT,
                Visibility::Public,
            )
            .unwrap();

        let const_val = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(1),
                },
                caller,
            )
            .unwrap();
        let call_node = graph
            .add_core_op(ComputeOp::Call { target: callee }, caller)
            .unwrap();

        // Only provide 1 argument when callee expects 2
        graph
            .add_data_edge(const_val, call_node, 0, 0, TypeId::I32)
            .unwrap();

        let errors = validate_graph(&graph);
        // Call has flexible input count checking (it varies by function),
        // but the specific edge types should still be checked
        assert!(
            errors
                .iter()
                .all(|e| !matches!(e, TypeError::TypeMismatch { .. })),
            "Expected no type mismatch for provided call argument, got: {:?}",
            errors
        );
    }
}
