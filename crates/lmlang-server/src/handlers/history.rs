//! History handlers for undo/redo, checkpoints, and diff.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::history::{
    CreateCheckpointRequest, CreateCheckpointResponse, DiffRequest, DiffResponse,
    ListCheckpointsResponse, ListHistoryResponse, RedoResponse, RestoreCheckpointResponse,
    UndoResponse,
};
use crate::state::AppState;

/// Lists the edit history for a program.
///
/// `GET /programs/{id}/history`
pub async fn list_history(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<ListHistoryResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.list_history()?;
    Ok(Json(response))
}

/// Undoes the last committed mutation.
///
/// `POST /programs/{id}/undo`
pub async fn undo(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<UndoResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.undo()?;
    Ok(Json(response))
}

/// Redoes the last undone mutation.
///
/// `POST /programs/{id}/redo`
pub async fn redo(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<RedoResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.redo()?;
    Ok(Json(response))
}

/// Creates a named checkpoint of the current graph state.
///
/// `POST /programs/{id}/checkpoints`
pub async fn create_checkpoint(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<CreateCheckpointRequest>,
) -> Result<Json<CreateCheckpointResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.create_checkpoint(&req.name, req.description.as_deref())?;
    Ok(Json(response))
}

/// Lists all checkpoints for a program.
///
/// `GET /programs/{id}/checkpoints`
pub async fn list_checkpoints(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<ListCheckpointsResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.list_checkpoints()?;
    Ok(Json(response))
}

/// Restores a named checkpoint, replacing the current graph.
///
/// `POST /programs/{id}/checkpoints/{name}/restore`
pub async fn restore_checkpoint(
    State(state): State<AppState>,
    Path((program_id, name)): Path<(i64, String)>,
) -> Result<Json<RestoreCheckpointResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.restore_checkpoint(&name)?;
    Ok(Json(response))
}

/// Diffs between two checkpoints or current state.
///
/// `POST /programs/{id}/diff`
pub async fn diff_versions(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<DiffRequest>,
) -> Result<Json<DiffResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.diff_versions(
        req.from_checkpoint.as_deref(),
        req.to_checkpoint.as_deref(),
    )?;
    Ok(Json(response))
}
