//! Verification scope and response types.
//!
//! [`VerifyScope`] and [`VerifyResponse`] are defined in the schema layer
//! (not in handlers) so that the service layer in Plan 02 can import them
//! at compile time before handlers exist.

use serde::{Deserialize, Serialize};

use super::diagnostics::{DiagnosticError, DiagnosticWarning, PropagationConflictDiagnosticView};

/// Controls the scope of verification.
///
/// The agent selects the scope based on confidence:
/// - `Local`: check only the affected subgraph and immediate dependents.
/// - `Full`: re-verify the entire program from scratch.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum VerifyScope {
    /// Check affected subgraph and immediate dependents only.
    Local,
    /// Re-verify the entire program.
    Full,
}

/// Response from a verification operation.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResponse {
    /// Whether the program (or subgraph) is valid.
    pub valid: bool,
    /// Type errors found during verification.
    pub errors: Vec<DiagnosticError>,
    /// Non-blocking warnings.
    pub warnings: Vec<DiagnosticWarning>,
}

/// Request for explicit propagation queue flush.
#[derive(Debug, Clone, Deserialize)]
pub struct FlushPropagationRequest {
    /// Optional no-op payload for forward compatibility.
    #[serde(default)]
    pub dry_run: bool,
    /// Optional events to enqueue before flush.
    #[serde(default)]
    pub events: Vec<PropagationEventSeed>,
}

/// Seed propagation event for explicit flush control.
#[derive(Debug, Clone, Deserialize)]
pub struct PropagationEventSeed {
    /// Event kind:
    /// - "semantic.function_created"
    /// - "semantic.function_signature_changed"
    /// - "semantic.contract_added"
    /// - "compute.node_inserted"
    /// - "compute.node_modified"
    /// - "compute.node_removed"
    /// - "compute.control_flow_changed"
    pub kind: String,
    pub function_id: u32,
    #[serde(default)]
    pub node_id: Option<u32>,
    #[serde(default)]
    pub op_kind: Option<String>,
    #[serde(default)]
    pub contract_name: Option<String>,
}

/// Response from explicit propagation flush.
#[derive(Debug, Clone, Serialize)]
pub struct FlushPropagationResponse {
    pub processed_events: usize,
    pub applied_events: usize,
    pub skipped_events: usize,
    pub generated_events: usize,
    pub remaining_queue: usize,
    pub refreshed_semantic_nodes: Vec<u32>,
    pub refreshed_summary_nodes: Vec<u32>,
    pub diagnostics: Vec<PropagationConflictDiagnosticView>,
}
