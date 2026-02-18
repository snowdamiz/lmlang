//! Mutation handler for proposing graph edits.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::mutations::{ProposeEditRequest, ProposeEditResponse};
use crate::state::AppState;

/// Proposes one or more graph mutations.
///
/// `POST /programs/{id}/mutations`
///
/// The handler is deliberately thin -- all batch/dry_run/validation logic
/// lives in [`ProgramService::propose_edit`].
pub async fn propose_edit(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<ProposeEditRequest>,
) -> Result<Json<ProposeEditResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();

    // Verify the request targets the active program
    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.propose_edit(req)?;
    Ok(Json(response))
}
