//! Simulation request/response types for graph interpretation.
//!
//! Allows agents to execute functions with provided inputs and optionally
//! record an execution trace for debugging.

use lmlang_core::id::{FunctionId, NodeId};
use serde::{Deserialize, Serialize};

use super::diagnostics::DiagnosticError;

/// Request to simulate (interpret) a function.
#[derive(Debug, Clone, Deserialize)]
pub struct SimulateRequest {
    /// The function to execute.
    pub function_id: FunctionId,
    /// Input values as JSON (service layer converts to interpreter Value).
    pub inputs: Vec<serde_json::Value>,
    /// Whether to record an execution trace.
    #[serde(default)]
    pub trace_enabled: Option<bool>,
}

/// Response from a simulation run.
#[derive(Debug, Clone, Serialize)]
pub struct SimulateResponse {
    /// Whether the simulation completed without error.
    pub success: bool,
    /// The result value (None if simulation errored).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Execution trace (None if tracing was disabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Vec<TraceEntryView>>,
    /// Error if simulation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DiagnosticError>,
    /// I/O operations logged during execution (Print outputs, etc.).
    pub io_log: Vec<serde_json::Value>,
}

/// A single trace entry for API responses.
///
/// Simplified view of the interpreter's TraceEntry, with values serialized
/// as JSON for agent consumption.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEntryView {
    /// The node that was evaluated.
    pub node_id: NodeId,
    /// Human-readable operation description.
    pub op: String,
    /// Input values gathered for this node, keyed by port number.
    pub inputs: Vec<(u16, serde_json::Value)>,
    /// Output value produced (None for void ops).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}
