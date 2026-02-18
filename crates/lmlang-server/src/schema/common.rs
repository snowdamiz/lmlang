//! Common API response wrapper types.
//!
//! [`ApiResponse`] provides the standard envelope for all successful API
//! responses, with optional warnings for non-blocking diagnostic information.

use serde::Serialize;

use crate::error::ApiErrorDetail;
use super::diagnostics::DiagnosticWarning;

/// Standard API response envelope.
///
/// All successful responses wrap their payload in this structure. The `success`
/// field is always `true` for non-error responses. Warnings are non-blocking
/// diagnostic messages that the agent can choose to act on.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T: Serialize> {
    /// Always `true` for successful responses.
    pub success: bool,
    /// Response payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Error detail (only present in error responses constructed manually).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorDetail>,
    /// Non-blocking diagnostic warnings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<DiagnosticWarning>,
}

impl<T: Serialize> ApiResponse<T> {
    /// Create a successful response with data and no warnings.
    pub fn ok(data: T) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Create a successful response with data and warnings.
    pub fn ok_with_warnings(data: T, warnings: Vec<DiagnosticWarning>) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            error: None,
            warnings,
        }
    }
}
