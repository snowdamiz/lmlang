//! Simulation handler for function interpretation.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::simulate::{SimulateRequest, SimulateResponse};
use crate::state::AppState;

/// Runs the interpreter on a function with provided inputs.
///
/// `POST /programs/{id}/simulate`
pub async fn simulate(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<SimulateRequest>,
) -> Result<Json<SimulateResponse>, ApiError> {
    let service = state.service.lock().unwrap();

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.simulate(req)?;
    Ok(Json(response))
}
