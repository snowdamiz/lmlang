//! API error types with HTTP status code mapping.
//!
//! [`ApiError`] is the unified error type for all API endpoints. It implements
//! `axum::response::IntoResponse` to produce structured JSON error responses
//! with appropriate HTTP status codes.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::concurrency::lock_manager::{LockDenial, LockError};
use crate::schema::diagnostics::DiagnosticError;

/// Structured error detail in API responses.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorDetail {
    /// Machine-readable error code (e.g., "NOT_FOUND", "BAD_REQUEST").
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured details (e.g., validation errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// API errors with HTTP status code mapping.
///
/// Each variant maps to a specific HTTP status code and produces a structured
/// JSON error response body.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Entity not found (404).
    #[error("not found: {0}")]
    NotFound(String),

    /// Invalid request (400).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// Validation failed with diagnostic errors (422).
    #[error("validation failed")]
    ValidationFailed(Vec<DiagnosticError>),

    /// Internal server error (500).
    #[error("internal error: {0}")]
    InternalError(String),

    /// Resource conflict (409).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Resource conflict with structured details (409).
    #[error("conflict: {message}")]
    ConflictWithDetails {
        message: String,
        details: serde_json::Value,
    },

    /// Lock denied -- another agent holds the lock (423 Locked).
    #[error("lock denied")]
    LockDenied(LockDenial),

    /// Lock required but not held (428 Precondition Required).
    #[error("lock required: {0}")]
    LockRequired(String),

    /// Missing X-Agent-Id header (400).
    #[error("agent required: {0}")]
    AgentRequired(String),

    /// Too many retries (429).
    #[error("too many retries: {0}")]
    TooManyRetries(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, detail) = match &self {
            ApiError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                ApiErrorDetail {
                    code: "NOT_FOUND".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                ApiErrorDetail {
                    code: "BAD_REQUEST".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::ValidationFailed(errors) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiErrorDetail {
                    code: "VALIDATION_FAILED".to_string(),
                    message: format!("{} validation error(s)", errors.len()),
                    details: serde_json::to_value(errors).ok(),
                },
            ),
            ApiError::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorDetail {
                    code: "INTERNAL_ERROR".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::Conflict(msg) => (
                StatusCode::CONFLICT,
                ApiErrorDetail {
                    code: "CONFLICT".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::ConflictWithDetails { message, details } => (
                StatusCode::CONFLICT,
                ApiErrorDetail {
                    code: "CONFLICT".to_string(),
                    message: message.clone(),
                    details: Some(details.clone()),
                },
            ),
            ApiError::LockDenied(denial) => (
                StatusCode::LOCKED,
                ApiErrorDetail {
                    code: "LOCK_DENIED".to_string(),
                    message: format!(
                        "function {} is locked by another agent",
                        denial.function_id.0
                    ),
                    details: serde_json::to_value(denial).ok(),
                },
            ),
            ApiError::LockRequired(msg) => (
                StatusCode::from_u16(428).unwrap_or(StatusCode::BAD_REQUEST),
                ApiErrorDetail {
                    code: "LOCK_REQUIRED".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::AgentRequired(msg) => (
                StatusCode::BAD_REQUEST,
                ApiErrorDetail {
                    code: "AGENT_REQUIRED".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
            ApiError::TooManyRetries(msg) => (
                StatusCode::TOO_MANY_REQUESTS,
                ApiErrorDetail {
                    code: "TOO_MANY_RETRIES".to_string(),
                    message: msg.clone(),
                    details: None,
                },
            ),
        };

        let body = serde_json::json!({
            "success": false,
            "error": detail,
        });

        (status, axum::Json(body)).into_response()
    }
}

impl From<lmlang_core::CoreError> for ApiError {
    fn from(err: lmlang_core::CoreError) -> Self {
        match &err {
            lmlang_core::CoreError::NodeNotFound { .. }
            | lmlang_core::CoreError::FunctionNotFound { .. }
            | lmlang_core::CoreError::ModuleNotFound { .. }
            | lmlang_core::CoreError::TypeNotFound { .. } => {
                ApiError::NotFound(err.to_string())
            }
            lmlang_core::CoreError::DuplicateTypeName { .. } => {
                ApiError::Conflict(err.to_string())
            }
            lmlang_core::CoreError::InvalidEdge { .. }
            | lmlang_core::CoreError::GraphInconsistency { .. } => {
                ApiError::BadRequest(err.to_string())
            }
        }
    }
}

impl From<lmlang_storage::StorageError> for ApiError {
    fn from(err: lmlang_storage::StorageError) -> Self {
        match &err {
            lmlang_storage::StorageError::ProgramNotFound(_)
            | lmlang_storage::StorageError::NodeNotFound { .. }
            | lmlang_storage::StorageError::EdgeNotFound { .. }
            | lmlang_storage::StorageError::FunctionNotFound { .. }
            | lmlang_storage::StorageError::ModuleNotFound { .. }
            | lmlang_storage::StorageError::TypeNotFound { .. } => {
                ApiError::NotFound(err.to_string())
            }
            lmlang_storage::StorageError::IntegrityError { .. } => {
                ApiError::Conflict(err.to_string())
            }
            _ => ApiError::InternalError(err.to_string()),
        }
    }
}

impl From<LockError> for ApiError {
    fn from(err: LockError) -> Self {
        match err {
            LockError::AlreadyHeldBy(denial) => ApiError::LockDenied(denial),
            LockError::NotHeld {
                function_id,
                agent_id,
            } => ApiError::BadRequest(format!(
                "agent {} does not hold lock on function {}",
                agent_id, function_id
            )),
            LockError::FunctionNotFound(func_id) => {
                ApiError::NotFound(format!("function {} not found", func_id))
            }
            LockError::BatchPartialFailure { failed, .. } => ApiError::LockDenied(failed),
        }
    }
}
