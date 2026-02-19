//! Compilation and dirty status handlers.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::compile::{CompileRequest, CompileResponse, DirtyStatusResponse};
use crate::state::AppState;

/// Compiles the active program graph to a native executable.
///
/// `POST /programs/{id}/compile`
pub async fn compile_program(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(request): Json<CompileRequest>,
) -> Result<Json<CompileResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.compile(&request)?;
    Ok(Json(response))
}

/// Returns the dirty status of all functions in the active program.
///
/// Shows which functions have changed since the last incremental compilation,
/// which are dirty due to dependency changes, and which are cached.
///
/// `GET /programs/{id}/dirty`
pub async fn dirty_status(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<DirtyStatusResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.dirty_status()?;
    Ok(Json(response))
}
