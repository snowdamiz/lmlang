//! Agent registration and lifecycle handlers.

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::error::ApiError;
use crate::schema::agents::{
    AgentView, ListAgentsResponse, RegisterAgentRequest, RegisterAgentResponse,
};
use crate::state::AppState;

/// `POST /agents/register`
pub async fn register_agent(
    State(state): State<AppState>,
    Json(req): Json<RegisterAgentRequest>,
) -> Result<Json<RegisterAgentResponse>, ApiError> {
    let agent_id = state.agent_registry.register(req.name.clone());

    let registered_at = chrono_like_now();
    Ok(Json(RegisterAgentResponse {
        agent_id: agent_id.0,
        name: req.name,
        registered_at,
    }))
}

/// `DELETE /agents/{agent_id}`
pub async fn deregister_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let parsed = Uuid::parse_str(&agent_id).map_err(|_| {
        ApiError::BadRequest(format!("invalid agent id '{}': expected UUID", agent_id))
    })?;
    let agent_id = crate::concurrency::AgentId(parsed);

    let removed = state.agent_registry.deregister(&agent_id);
    if !removed {
        return Err(ApiError::NotFound(format!("agent {} not found", parsed)));
    }

    let released = state.lock_manager.release_all(&agent_id);
    Ok(Json(serde_json::json!({
        "success": true,
        "released_locks": released.iter().map(|f| f.0).collect::<Vec<_>>(),
    })))
}

/// `GET /agents`
pub async fn list_agents(
    State(state): State<AppState>,
) -> Result<Json<ListAgentsResponse>, ApiError> {
    let mut agents = state
        .agent_registry
        .list()
        .into_iter()
        .map(|session| AgentView {
            agent_id: session.id.0,
            name: session.name,
        })
        .collect::<Vec<_>>();
    agents.sort_by_key(|a| a.agent_id);

    Ok(Json(ListAgentsResponse { agents }))
}

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}
