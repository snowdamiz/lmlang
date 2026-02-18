//! Type error diagnostics with rich context fields and fix suggestions.
//!
//! [`TypeError`] captures full context for every type error: which nodes are
//! involved, which ports, expected vs actual types, function boundary, and
//! optional fix suggestions for AI agent consumption.

use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::type_id::TypeId;
use serde::{Deserialize, Serialize};

/// A type error detected during static type checking.
///
/// Every variant includes enough context for an AI agent to understand the
/// error and apply a fix without additional graph queries.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum TypeError {
    /// A data edge carries a type incompatible with what the target port expects.
    #[error(
        "type mismatch at node {target_node}: port {target_port} expects {expected}, got {actual}"
    )]
    TypeMismatch {
        /// Node producing the value.
        source_node: NodeId,
        /// Node consuming the value.
        target_node: NodeId,
        /// Output port of the source node.
        source_port: u16,
        /// Input port of the target node.
        target_port: u16,
        /// The type expected by the target port.
        expected: TypeId,
        /// The actual type provided by the source edge.
        actual: TypeId,
        /// Function containing this edge.
        function_id: FunctionId,
        /// Suggested fix, if one is obvious.
        suggestion: Option<FixSuggestion>,
    },

    /// A node requires an input on a port but no incoming data edge exists.
    #[error("missing input: node {node} port {port} has no incoming data edge")]
    MissingInput {
        /// The node missing an input.
        node: NodeId,
        /// The port that has no incoming data edge.
        port: u16,
        /// Function containing this node.
        function_id: FunctionId,
    },

    /// A node received the wrong number of data inputs.
    #[error(
        "unexpected input count: node {node} expects {expected} inputs, got {actual}"
    )]
    WrongInputCount {
        /// The node with the wrong input count.
        node: NodeId,
        /// Expected number of inputs.
        expected: usize,
        /// Actual number of inputs.
        actual: usize,
        /// Function containing this node.
        function_id: FunctionId,
    },

    /// A type ID referenced in the graph is not registered in the type registry.
    #[error("unknown type: {type_id} not found in registry")]
    UnknownType {
        /// The unregistered type ID.
        type_id: TypeId,
    },

    /// A non-numeric type was used where an arithmetic operation requires a
    /// numeric type.
    #[error(
        "non-numeric type {type_id} used in arithmetic operation at node {node}"
    )]
    NonNumericArithmetic {
        /// The node performing the arithmetic operation.
        node: NodeId,
        /// The non-numeric type provided.
        type_id: TypeId,
        /// Function containing this node.
        function_id: FunctionId,
    },

    /// A non-boolean type was used where a boolean condition is required.
    #[error("non-boolean condition at node {node}: expected Bool, got {actual}")]
    NonBooleanCondition {
        /// The node requiring a boolean condition.
        node: NodeId,
        /// The non-boolean type provided.
        actual: TypeId,
        /// Function containing this node.
        function_id: FunctionId,
    },
}

/// A suggested fix for a type error.
///
/// When the type checker can determine an obvious corrective action, it
/// attaches a `FixSuggestion` to the error. AI agents can apply these
/// suggestions automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixSuggestion {
    /// Insert a Cast node between the source and target to convert types.
    InsertCast {
        /// Source type to cast from.
        from: TypeId,
        /// Target type to cast to.
        to: TypeId,
    },
}
