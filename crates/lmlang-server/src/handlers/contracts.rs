//! Contract testing handlers.
//!
//! Implements the property-based testing endpoint for contract verification.

use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::schema::contracts::{PropertyTestRequest, PropertyTestResponse};
use crate::state::AppState;

/// Runs property-based tests on a function's contracts.
///
/// `POST /programs/{id}/property-test`
pub async fn property_test(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<PropertyTestRequest>,
) -> Result<Json<PropertyTestResponse>, ApiError> {
    let service = state.service.lock().await;

    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    let response = service.property_test(req)?;
    Ok(Json(response))
}
