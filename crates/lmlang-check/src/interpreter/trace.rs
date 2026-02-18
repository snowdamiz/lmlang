//! Execution trace recording for the graph interpreter.
//!
//! When tracing is enabled via [`InterpreterConfig::trace_enabled`], the
//! interpreter records a [`TraceEntry`] for every node evaluation, capturing
//! the node ID, operation description, input values, and output value.

use lmlang_core::id::NodeId;

use super::value::Value;

/// A single entry in the execution trace, recording one node evaluation.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// The node that was evaluated.
    pub node_id: NodeId,
    /// Human-readable description of the operation.
    pub op_description: String,
    /// Input values gathered for this node, keyed by port number.
    pub inputs: Vec<(u16, Value)>,
    /// Output value produced (None for ops like Store, Branch that produce no value).
    pub output: Option<Value>,
}
