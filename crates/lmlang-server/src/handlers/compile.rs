//! Compilation handler for compiling program graphs to native binaries.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::compile::{CompileRequest, CompileResponse};
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
