//! Lock management handlers.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;

use lmlang_core::id::FunctionId;

use crate::concurrency::{extract_agent_id, LockMode};
use crate::error::ApiError;
use crate::schema::locks::{
    AcquireLockRequest, AcquireLockResponse, LockGrantView, LockStatusResponse, LockStatusView,
    ReleaseLockRequest, ReleaseLockResponse,
};
use crate::state::AppState;

/// `POST /programs/{id}/locks/acquire`
pub async fn acquire_locks(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<AcquireLockRequest>,
) -> Result<Json<AcquireLockResponse>, ApiError> {
    let agent_id = extract_agent_id(&headers)?;

    {
        let service = state.service.lock().await;
        let active_id = service.program_id();
        if active_id.0 != program_id {
            return Err(ApiError::BadRequest(format!(
                "program {} is not the active program (active: {})",
                program_id, active_id.0
            )));
        }
    }

    let function_ids = req
        .function_ids
        .iter()
        .copied()
        .map(FunctionId)
        .collect::<Vec<_>>();

    let grants = match req.mode.to_lowercase().as_str() {
        "write" => {
            if function_ids.len() > 1 {
                state.lock_manager.batch_acquire_write(
                    &agent_id,
                    &function_ids,
                    req.description.clone(),
                )?
            } else {
                let mut grants = Vec::new();
                for func_id in function_ids {
                    let grant = state.lock_manager.try_acquire_write(
                        &agent_id,
                        func_id,
                        req.description.clone(),
                    )?;
                    grants.push(grant);
                }
                grants
            }
        }
        "read" => {
            let mut grants = Vec::new();
            for func_id in function_ids {
                let grant = state.lock_manager.try_acquire_read(&agent_id, func_id)?;
                grants.push(grant);
            }
            grants
        }
        other => {
            return Err(ApiError::BadRequest(format!(
                "invalid lock mode '{}': expected 'read' or 'write'",
                other
            )));
        }
    };

    state.agent_registry.touch(&agent_id);

    Ok(Json(AcquireLockResponse {
        grants: grants
            .into_iter()
            .map(|g| LockGrantView {
                function_id: g.function_id.0,
                mode: match g.mode {
                    LockMode::Read => "read".to_string(),
                    LockMode::Write => "write".to_string(),
                },
                expires_at: g.expires_at,
            })
            .collect(),
    }))
}

/// `POST /programs/{id}/locks/release`
pub async fn release_locks(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<ReleaseLockRequest>,
) -> Result<Json<ReleaseLockResponse>, ApiError> {
    let agent_id = extract_agent_id(&headers)?;

    {
        let service = state.service.lock().await;
        let active_id = service.program_id();
        if active_id.0 != program_id {
            return Err(ApiError::BadRequest(format!(
                "program {} is not the active program (active: {})",
                program_id, active_id.0
            )));
        }
    }

    let mut released = Vec::new();
    for function_id in req.function_ids {
        state
            .lock_manager
            .release(&agent_id, FunctionId(function_id))?;
        released.push(function_id);
    }

    state.agent_registry.touch(&agent_id);

    Ok(Json(ReleaseLockResponse { released }))
}

/// `GET /programs/{id}/locks`
pub async fn lock_status(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<LockStatusResponse>, ApiError> {
    {
        let service = state.service.lock().await;
        let active_id = service.program_id();
        if active_id.0 != program_id {
            return Err(ApiError::BadRequest(format!(
                "program {} is not the active program (active: {})",
                program_id, active_id.0
            )));
        }
    }

    let locks = state
        .lock_manager
        .status()
        .into_iter()
        .map(|entry| LockStatusView {
            function_id: entry.function_id.0,
            state: entry.state,
            holders: entry.holders.into_iter().map(|a| a.to_string()).collect(),
            holder_description: entry.holder_description,
            expires_at: entry.expires_at,
        })
        .collect();

    Ok(Json(LockStatusResponse { locks }))
}
