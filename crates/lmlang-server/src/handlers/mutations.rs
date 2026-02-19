//! Mutation handler for proposing graph edits.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use lmlang_core::id::FunctionId;

use crate::concurrency::{affected_functions, check_hashes, extract_agent_id};
use crate::error::ApiError;
use crate::schema::mutations::{ProposeEditRequest, ProposeEditResponse};
use crate::state::AppState;

/// Proposes one or more graph mutations.
///
/// `POST /programs/{id}/mutations`
pub async fn propose_edit(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<ProposeEditRequest>,
) -> Result<Json<ProposeEditResponse>, ApiError> {
    let maybe_agent_id = headers
        .get("X-Agent-Id")
        .map(|_| extract_agent_id(&headers))
        .transpose()?;

    let mut service = state.service.lock().await;

    // Verify the request targets the active program.
    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(format!(
            "program {} is not the active program (active: {})",
            program_id, active_id.0
        )));
    }

    // Mode A: non-agent callers stay backward compatible.
    let Some(agent_id) = maybe_agent_id else {
        let response = service.propose_edit(req)?;
        return Ok(Json(response));
    };

    let (affected, structure_change) = affected_functions(&req.mutations, service.graph());

    let _global_write_guard = if structure_change {
        Some(state.lock_manager.global_write_lock.write().await)
    } else {
        None
    };

    if !structure_change && !affected.is_empty() {
        state
            .lock_manager
            .verify_write_locks(&agent_id, &affected)
            .map_err(|_| {
                ApiError::LockRequired(
                    "agent must hold write locks for all affected functions".to_string(),
                )
            })?;
    }

    if let Some(expected_hashes) = &req.expected_hashes {
        let expected = expected_hashes
            .iter()
            .map(|(k, v)| (FunctionId(*k), v.clone()))
            .collect::<HashMap<FunctionId, String>>();

        if let Err(conflicts) = check_hashes(service.graph(), &expected) {
            return Err(ApiError::ConflictWithDetails {
                message: "function hash conflict detected".to_string(),
                details: serde_json::to_value(conflicts).unwrap_or(serde_json::Value::Null),
            });
        }
    }

    let response = service.propose_edit(req.clone())?;

    // Keep graph verified after agent commits.
    if response.committed && !req.dry_run {
        let verify =
            crate::concurrency::verify::run_incremental_verification(service.graph(), &affected);
        if !verify.valid {
            let _ = service.undo();
            return Ok(Json(ProposeEditResponse {
                valid: false,
                created: Vec::new(),
                errors: verify.errors,
                warnings: verify.warnings,
                committed: false,
            }));
        }
    }

    state.agent_registry.touch(&agent_id);
    Ok(Json(response))
}
