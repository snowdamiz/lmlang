//! Verification scope and response types.
//!
//! [`VerifyScope`] and [`VerifyResponse`] are defined in the schema layer
//! (not in handlers) so that the service layer in Plan 02 can import them
//! at compile time before handlers exist.

use serde::{Deserialize, Serialize};

use super::diagnostics::{DiagnosticError, DiagnosticWarning};

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
