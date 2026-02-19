//! API schema types for property testing and contract violation views.
//!
//! Agents use [`PropertyTestRequest`] to trigger property-based testing
//! of function contracts, and receive [`PropertyTestResponse`] with
//! structured failure details including counterexample values.

use lmlang_core::id::NodeId;
use serde::{Deserialize, Serialize};

/// Request to run property-based tests on a function's contracts.
#[derive(Debug, Deserialize)]
pub struct PropertyTestRequest {
    /// Function to test.
    pub function_id: u32,
    /// Agent-provided seed inputs (the "interesting" cases).
    pub seeds: Vec<Vec<serde_json::Value>>,
    /// Number of randomized iterations (required, no default).
    pub iterations: u32,
    /// Random seed for reproducibility (optional -- system generates if absent).
    #[serde(default)]
    pub random_seed: Option<u64>,
    /// Whether to include execution traces for failures.
    #[serde(default)]
    pub trace_failures: bool,
}

/// Response from a property test run.
#[derive(Debug, Serialize)]
pub struct PropertyTestResponse {
    /// Total tests run (seeds + random variations).
    pub total_run: u32,
    /// Number of passing tests.
    pub passed: u32,
    /// Number of failing tests.
    pub failed: u32,
    /// The random seed used (for reproducibility).
    pub random_seed: u64,
    /// All failures with full details.
    pub failures: Vec<PropertyTestFailureView>,
}

/// A single property test failure for API responses.
#[derive(Debug, Serialize)]
pub struct PropertyTestFailureView {
    /// The inputs that caused the failure.
    pub inputs: Vec<serde_json::Value>,
    /// The contract violation that occurred.
    pub violation: ContractViolationView,
    /// Execution trace for this test case (None if trace_failures was false).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Vec<TraceEntryView>>,
}

/// A contract violation for API responses.
#[derive(Debug, Serialize)]
pub struct ContractViolationView {
    /// Kind of contract: "precondition", "postcondition", or "invariant".
    pub kind: String,
    /// The contract node that failed.
    pub contract_node: u32,
    /// The function containing the contract.
    pub function_id: u32,
    /// Human-readable description from the contract op.
    pub message: String,
    /// Function inputs that triggered the violation.
    pub inputs: Vec<serde_json::Value>,
    /// For postconditions, the actual return value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_return: Option<serde_json::Value>,
    /// Node values from the failing evaluation, sorted by NodeId for determinism.
    pub counterexample: Vec<(u32, serde_json::Value)>,
}

/// A single trace entry for API responses (reuse pattern from simulate).
#[derive(Debug, Serialize)]
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
