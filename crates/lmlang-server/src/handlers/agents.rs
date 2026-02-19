//! Agent registration and lifecycle handlers.

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::concurrency::{AgentId, AgentLlmConfig, AgentSession};
use crate::error::ApiError;
use crate::schema::agents::{
    AgentDetailResponse, AgentLlmConfigView, AgentView, ListAgentsResponse, RegisterAgentRequest,
    RegisterAgentResponse, UpdateAgentConfigRequest, UpdateAgentConfigResponse,
};
use crate::state::AppState;

/// `POST /agents/register`
pub async fn register_agent(
    State(state): State<AppState>,
    Json(req): Json<RegisterAgentRequest>,
) -> Result<Json<RegisterAgentResponse>, ApiError> {
    let llm = llm_from_register(&req)?;
    let agent_id = state.agent_registry.register(req.name.clone(), llm);
    let session = state.agent_registry.get(&agent_id).ok_or_else(|| {
        ApiError::InternalError("registered agent missing from registry".to_string())
    })?;
    state
        .agent_config_store
        .upsert(agent_id, session.name.clone(), &session.llm)?;

    let registered_at = chrono_like_now();
    Ok(Json(RegisterAgentResponse {
        agent_id: agent_id.0,
        name: req.name,
        registered_at,
        llm: llm_view(&session),
    }))
}

/// `GET /agents/{agent_id}`
pub async fn get_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentDetailResponse>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    let session = state
        .agent_registry
        .get(&agent_id)
        .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;
    Ok(Json(AgentDetailResponse {
        agent: to_agent_view(session),
    }))
}

/// `DELETE /agents/{agent_id}`
pub async fn deregister_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;

    let removed = state.agent_registry.deregister(&agent_id);
    if !removed {
        return Err(ApiError::NotFound(format!(
            "agent {} not found",
            agent_id.0
        )));
    }
    state.agent_config_store.delete(agent_id)?;

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
        .map(to_agent_view)
        .collect::<Vec<_>>();
    agents.sort_by_key(|a| a.agent_id);

    Ok(Json(ListAgentsResponse { agents }))
}

/// `POST /agents/{agent_id}/config`
pub async fn update_agent_config(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(req): Json<UpdateAgentConfigRequest>,
) -> Result<Json<UpdateAgentConfigResponse>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    let llm = llm_from_update(&req)?;
    let session = state
        .agent_registry
        .set_llm_config(&agent_id, llm)
        .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;
    state
        .agent_config_store
        .upsert(agent_id, session.name.clone(), &session.llm)?;

    Ok(Json(UpdateAgentConfigResponse {
        success: true,
        agent: to_agent_view(session),
    }))
}

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

fn parse_agent_id(raw: &str) -> Result<AgentId, ApiError> {
    let parsed = Uuid::parse_str(raw)
        .map_err(|_| ApiError::BadRequest(format!("invalid agent id '{}': expected UUID", raw)))?;
    Ok(AgentId(parsed))
}

fn to_agent_view(session: AgentSession) -> AgentView {
    AgentView {
        agent_id: session.id.0,
        name: session.name.clone(),
        llm: llm_view(&session),
    }
}

fn llm_view(session: &AgentSession) -> AgentLlmConfigView {
    AgentLlmConfigView {
        provider: session.llm.provider.clone(),
        model: session.llm.model.clone(),
        api_base_url: session.llm.api_base_url.clone(),
        system_prompt: session.llm.system_prompt.clone(),
        api_key_configured: session.llm.api_key.is_some(),
    }
}

fn llm_from_register(req: &RegisterAgentRequest) -> Result<AgentLlmConfig, ApiError> {
    Ok(AgentLlmConfig {
        provider: normalize_provider(req.provider.as_deref())?,
        model: req.model.clone(),
        api_base_url: req.api_base_url.clone(),
        api_key: req.api_key.clone(),
        system_prompt: req.system_prompt.clone(),
    })
}

fn llm_from_update(req: &UpdateAgentConfigRequest) -> Result<AgentLlmConfig, ApiError> {
    Ok(AgentLlmConfig {
        provider: normalize_provider(req.provider.as_deref())?,
        model: req.model.clone(),
        api_base_url: req.api_base_url.clone(),
        api_key: req.api_key.clone(),
        system_prompt: req.system_prompt.clone(),
    })
}

fn normalize_provider(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.is_empty() {
        return Ok(None);
    }

    match normalized.as_str() {
        "openrouter" | "openai_compatible" => Ok(Some(normalized)),
        _ => Err(ApiError::BadRequest(format!(
            "unsupported provider '{}': use openrouter or openai_compatible",
            raw
        ))),
    }
}
