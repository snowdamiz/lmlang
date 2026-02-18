//! Diagnostic types for API error and warning reporting.
//!
//! These types provide structured diagnostic information for type errors and
//! validation warnings. Per the CONTEXT.md locked decision, errors describe
//! the problem only -- no fix suggestions are included in API output.

use lmlang_core::id::{EdgeId, FunctionId, NodeId};
use lmlang_core::type_id::TypeId;
use lmlang_check::typecheck::diagnostics::TypeError;
use serde::Serialize;

/// A structured diagnostic error returned in API responses.
///
/// Each error has a machine-readable code, human-readable message, and
/// optional structured details containing graph context.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticError {
    /// Machine-readable error code (e.g., "TYPE_MISMATCH", "MISSING_INPUT").
    pub code: String,
    /// Human-readable error description.
    pub message: String,
    /// Optional structured details with graph context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<DiagnosticDetails>,
}

/// A non-blocking diagnostic warning.
///
/// Warnings are informational -- the agent can commit changes that produce
/// warnings (e.g., unreachable nodes, unused parameters).
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticWarning {
    /// Machine-readable warning code.
    pub code: String,
    /// Human-readable warning description.
    pub message: String,
}

/// Structured context for a diagnostic error.
///
/// Fields are optional because different error types provide different
/// amounts of context (e.g., UnknownType has no source/target nodes).
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticDetails {
    /// Source node producing the problematic value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_node: Option<NodeId>,
    /// Target node consuming the problematic value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_node: Option<NodeId>,
    /// Relevant edge path for multi-hop errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_path: Option<Vec<EdgeId>>,
    /// Expected type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_type: Option<TypeId>,
    /// Actual type found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_type: Option<TypeId>,
    /// Function where the error occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_id: Option<FunctionId>,
    /// Relevant port number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

impl From<TypeError> for DiagnosticError {
    fn from(err: TypeError) -> Self {
        match &err {
            TypeError::TypeMismatch {
                source_node,
                target_node,
                target_port,
                expected,
                actual,
                function_id,
                ..
            } => DiagnosticError {
                code: "TYPE_MISMATCH".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: Some(*source_node),
                    target_node: Some(*target_node),
                    edge_path: None,
                    expected_type: Some(*expected),
                    actual_type: Some(*actual),
                    function_id: Some(*function_id),
                    port: Some(*target_port),
                }),
            },
            TypeError::MissingInput {
                node,
                port,
                function_id,
            } => DiagnosticError {
                code: "MISSING_INPUT".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: None,
                    target_node: Some(*node),
                    edge_path: None,
                    expected_type: None,
                    actual_type: None,
                    function_id: Some(*function_id),
                    port: Some(*port),
                }),
            },
            TypeError::WrongInputCount {
                node,
                expected: _,
                actual: _,
                function_id,
            } => DiagnosticError {
                code: "WRONG_INPUT_COUNT".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: None,
                    target_node: Some(*node),
                    edge_path: None,
                    expected_type: None,
                    actual_type: None,
                    function_id: Some(*function_id),
                    port: None,
                }),
            },
            TypeError::UnknownType { type_id } => DiagnosticError {
                code: "UNKNOWN_TYPE".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: None,
                    target_node: None,
                    edge_path: None,
                    expected_type: Some(*type_id),
                    actual_type: None,
                    function_id: None,
                    port: None,
                }),
            },
            TypeError::NonNumericArithmetic {
                node,
                type_id,
                function_id,
            } => DiagnosticError {
                code: "NON_NUMERIC_ARITHMETIC".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: None,
                    target_node: Some(*node),
                    edge_path: None,
                    expected_type: None,
                    actual_type: Some(*type_id),
                    function_id: Some(*function_id),
                    port: None,
                }),
            },
            TypeError::NonBooleanCondition {
                node,
                actual,
                function_id,
            } => DiagnosticError {
                code: "NON_BOOLEAN_CONDITION".to_string(),
                message: err.to_string(),
                details: Some(DiagnosticDetails {
                    source_node: None,
                    target_node: Some(*node),
                    edge_path: None,
                    expected_type: None,
                    actual_type: Some(*actual),
                    function_id: Some(*function_id),
                    port: None,
                }),
            },
        }
    }
}
