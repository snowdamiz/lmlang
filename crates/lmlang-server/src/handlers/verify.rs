//! Verification handler for type checking.

use axum::extract::{Path, State};
use axum::Json;
use lmlang_core::id::NodeId;
use serde::Deserialize;

use crate::error::ApiError;
use crate::schema::verify::{VerifyResponse, VerifyScope};
use crate::state::AppState;

/// Handler-local request wrapper for verification.
///
/// Parses the scope string ("local" or "full") into `VerifyScope`.
#[derive(Debug, Clone, Deserialize)]
pub struct VerifyRequest {
    /// Verification scope: "local" or "full".
    pub scope: String,
    /// Nodes affected by recent mutations (used when scope is "local").
    #[serde(default)]
    pub affected_nodes: Option<Vec<NodeId>>,
}

/// Runs type verification on the active program graph.
///
/// `POST /programs/{id}/verify`
pub async fn verify(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let scope = match req.scope.to_lowercase().as_str() {
        "local" => VerifyScope::Local,
        "full" => VerifyScope::Full,
        other => {
            return Err(ApiError::BadRequest(format!(
                "invalid scope '{}': expected 'local' or 'full'",
                other
            )));
        }
    };

    let service = state.service.lock().await;

    // Verify the request targets the active program
    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.verify(scope, req.affected_nodes)?;
    Ok(Json(response))
}
