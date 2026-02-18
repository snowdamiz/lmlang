//! Runtime error types with trap semantics for the graph interpreter.
//!
//! All runtime errors include the [`NodeId`] of the node that caused the error,
//! enabling precise error reporting. Errors are designed to be informative for
//! AI agent debugging.

use lmlang_core::id::{FunctionId, NodeId};
use serde::{Deserialize, Serialize};

/// Runtime errors produced by the interpreter.
///
/// Each variant represents a trap condition that halts execution. All variants
/// include the node ID where the error occurred for precise diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum RuntimeError {
    #[error("integer overflow at node {node}")]
    IntegerOverflow { node: NodeId },

    #[error("divide by zero at node {node}")]
    DivideByZero { node: NodeId },

    #[error("out of bounds access at node {node}: index {index}, size {size}")]
    OutOfBoundsAccess {
        node: NodeId,
        index: usize,
        size: usize,
    },

    #[error("recursion depth limit ({limit}) exceeded at node {node}")]
    RecursionLimitExceeded { node: NodeId, limit: usize },

    #[error("type mismatch at runtime: node {node}, expected {expected}, got {got}")]
    TypeMismatchAtRuntime {
        node: NodeId,
        expected: String,
        got: String,
    },

    #[error("missing value: node {node} input port {port} has no value")]
    MissingValue { node: NodeId, port: u16 },

    #[error("function not found: {id:?}")]
    FunctionNotFound { id: FunctionId },

    #[error("no return node in function {function:?}")]
    NoReturnNode { function: FunctionId },

    #[error("internal error: {message}")]
    InternalError { message: String },
}
