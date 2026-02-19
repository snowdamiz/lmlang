//! Program management handlers (create, list, delete, load).

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::programs::{
    CreateProgramRequest, CreateProgramResponse, ProgramListResponse,
};
use crate::state::AppState;

/// Lists all programs.
///
/// `GET /programs`
pub async fn list_programs(
    State(state): State<AppState>,
) -> Result<Json<ProgramListResponse>, ApiError> {
    let service = state.service.lock().await;
    let programs = service.list_programs()?;
    Ok(Json(ProgramListResponse { programs }))
}

/// Creates a new program.
///
/// `POST /programs`
pub async fn create_program(
    State(state): State<AppState>,
    Json(req): Json<CreateProgramRequest>,
) -> Result<Json<CreateProgramResponse>, ApiError> {
    let mut service = state.service.lock().await;
    let id = service.create_program(&req.name)?;
    Ok(Json(CreateProgramResponse {
        id,
        name: req.name,
    }))
}

/// Deletes a program by ID.
///
/// `DELETE /programs/{id}`
pub async fn delete_program(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut service = state.service.lock().await;
    service.delete_program(lmlang_storage::ProgramId(id))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Loads a program as the active graph.
///
/// `POST /programs/{id}/load`
pub async fn load_program(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut service = state.service.lock().await;
    service.load_program(lmlang_storage::ProgramId(id))?;
    Ok(Json(serde_json::json!({
        "success": true,
        "program_id": id
    })))
}
