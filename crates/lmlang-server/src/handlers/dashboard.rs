//! Unified dashboard handlers for Operate + Observe UX.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::Json;
use lmlang_storage::ProgramId;

use crate::concurrency::{AgentId, AgentLlmConfig};
use crate::error::ApiError;
use crate::project_agent::ProjectAgentMessage;
use crate::schema::agent_control::{
    AgentChatMessageView, ExecutionDiagnosticsView, ExecutionSummaryView,
};
use crate::schema::dashboard::{
    DashboardAiChatRequest, DashboardAiChatResponse, DashboardOpenRouterStatusQuery,
    DashboardOpenRouterStatusResponse,
};
use crate::state::AppState;

use super::agent_control::{
    execute_program_agent_chat, to_execution_attempt_views, to_latest_execution_view,
};

/// Serves the top-level unified dashboard shell.
///
/// `GET /dashboard`
pub async fn ui_root_index() -> Html<String> {
    let html =
        include_str!("../../static/dashboard/index.html").replace("__INITIAL_PROGRAM_ID__", "");
    Html(html)
}

/// Serves the unified dashboard shell.
///
/// `GET /programs/{id}/dashboard`
pub async fn ui_index(Path(program_id): Path<i64>) -> Html<String> {
    let html = include_str!("../../static/dashboard/index.html")
        .replace("__INITIAL_PROGRAM_ID__", &program_id.to_string());
    Html(html)
}

/// Serves dashboard client JavaScript.
///
/// `GET /dashboard/app.js`
pub async fn ui_root_app_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/dashboard/app.js"),
    )
}

/// Serves dashboard client JavaScript.
///
/// `GET /programs/{id}/dashboard/app.js`
pub async fn ui_app_js(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../../static/dashboard/app.js"),
    )
}

/// Serves dashboard client CSS.
///
/// `GET /dashboard/styles.css`
pub async fn ui_root_styles_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/dashboard/styles.css"),
    )
}

/// Serves dashboard client CSS.
///
/// `GET /programs/{id}/dashboard/styles.css`
pub async fn ui_styles_css(Path(_program_id): Path<i64>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../static/dashboard/styles.css"),
    )
}

/// Chat-first orchestration endpoint for the unified dashboard.
///
/// `POST /dashboard/ai/chat`
pub async fn ai_chat(
    State(state): State<AppState>,
    Json(req): Json<DashboardAiChatRequest>,
) -> Result<Json<DashboardAiChatResponse>, ApiError> {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return Err(ApiError::BadRequest(
            "message must not be empty".to_string(),
        ));
    }

    let lower = message.to_lowercase();
    let mut ctx = DashboardContext {
        selected_program_id: req.selected_program_id,
        selected_agent_id: req.selected_agent_id.map(AgentId),
        selected_project_agent_id: req.selected_project_agent_id.map(AgentId),
    };
    let mut actions = Vec::new();
    let mut transcript = None;
    let mut planner = None;
    let mut execution: Option<ExecutionSummaryView> = None;
    let mut execution_attempts = Vec::new();
    let mut diagnostics: Option<ExecutionDiagnosticsView> = None;

    let reply = if lower.contains("create project") || lower.contains("new project") {
        let name = parse_project_name(&message, &lower)
            .unwrap_or_else(|| format!("ai-project-{}", unix_seconds()));
        let program_id = create_and_load_program(&state, &name).await?;
        ctx.selected_program_id = Some(program_id);
        actions.push(format!("Created project '{}' (#{}).", name, program_id));
        format!("Created and selected project '{}' (#{}).", name, program_id)
    } else if lower.contains("register agent") || lower.contains("add agent") {
        let name = parse_agent_name(&message, &lower);
        let llm = parse_llm_config_from_message(&message, &lower, AgentLlmConfig::default());
        let agent_id = state.agent_registry.register(name.clone(), llm.clone());
        state
            .agent_config_store
            .upsert(agent_id, name.clone(), &llm)?;
        ctx.selected_agent_id = Some(agent_id);
        actions.push(format!("Registered agent {}.", agent_id.0));
        if llm.provider.is_some() {
            actions.push("Applied provider config from chat request.".to_string());
        }
        match name {
            Some(n) => format!("Registered agent '{}' ({}) and selected it.", n, agent_id.0),
            None => format!("Registered agent {} and selected it.", agent_id.0),
        }
    } else if lower.contains("configure agent")
        || lower.contains("set provider")
        || lower.contains("set model")
        || lower.contains("set api key")
        || lower.contains("set base url")
        || lower.contains("set system prompt")
    {
        let agent_id = ensure_agent_selected(&state, &mut ctx, &mut actions).await?;
        let current = state
            .agent_registry
            .get(&agent_id)
            .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?
            .llm;
        let updated = parse_llm_config_from_message(&message, &lower, current);
        let session = state
            .agent_registry
            .set_llm_config(&agent_id, updated)
            .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;
        state
            .agent_config_store
            .upsert(agent_id, session.name.clone(), &session.llm)?;
        ctx.selected_agent_id = Some(agent_id);
        actions.push(format!("Updated provider config for agent {}.", agent_id.0));
        format!("Updated provider config for agent {}.", agent_id.0)
    } else if lower.contains("assign agent") {
        let program_id = ensure_program_selected(&state, &mut ctx, &mut actions).await?;
        let agent_id = ensure_agent_selected(&state, &mut ctx, &mut actions).await?;
        ensure_assignment(&state, &mut ctx, &mut actions, program_id, agent_id).await?;
        format!("Assigned agent {} to project {}.", agent_id.0, program_id)
    } else if lower.contains("start build") {
        let program_id = ensure_program_selected(&state, &mut ctx, &mut actions).await?;
        let agent_id = ensure_agent_selected(&state, &mut ctx, &mut actions).await?;
        ensure_assignment(&state, &mut ctx, &mut actions, program_id, agent_id).await?;

        let goal = parse_start_goal(&message, &lower)
            .unwrap_or_else(|| "AI-directed build run".to_string());
        {
            let mut service = state.service.lock().await;
            service.load_program(ProgramId(program_id))?;
        }
        let session = state
            .project_agent_manager
            .start(program_id, agent_id, goal.clone())
            .await
            .map_err(ApiError::BadRequest)?;
        execution = to_latest_execution_view(&session);
        execution_attempts = to_execution_attempt_views(&session);
        diagnostics = execution
            .as_ref()
            .and_then(|value| value.diagnostics.clone());
        state
            .autonomous_runner
            .start(state.clone(), program_id, agent_id);
        ctx.selected_project_agent_id = Some(session.agent_id);
        actions.push(format!(
            "Started build run on project {} for agent {}.",
            program_id, agent_id.0
        ));
        format!("Started build: {}.", goal)
    } else if lower.contains("stop build") {
        let program_id = ensure_program_selected(&state, &mut ctx, &mut actions).await?;
        let agent_id = ensure_project_agent_selected(&state, &mut ctx, &mut actions).await?;
        let session = state
            .project_agent_manager
            .stop(
                program_id,
                agent_id,
                Some("Stopped via dashboard AI chat".to_string()),
            )
            .await
            .map_err(ApiError::BadRequest)?;
        execution = to_latest_execution_view(&session);
        execution_attempts = to_execution_attempt_views(&session);
        diagnostics = execution
            .as_ref()
            .and_then(|value| value.diagnostics.clone());
        state.autonomous_runner.stop(program_id, agent_id);
        ctx.selected_project_agent_id = Some(session.agent_id);
        actions.push(format!(
            "Stopped build run on project {} for agent {}.",
            program_id, agent_id.0
        ));
        "Build stopped.".to_string()
    } else {
        let program_id = ensure_program_selected(&state, &mut ctx, &mut actions).await?;
        let agent_id = ensure_agent_selected(&state, &mut ctx, &mut actions).await?;
        ensure_assignment(&state, &mut ctx, &mut actions, program_id, agent_id).await?;

        if lower.contains("create hello world")
            || lower.contains("compile")
            || lower.contains("run")
            || lower.contains("execute")
        {
            ensure_running(
                &state,
                &mut ctx,
                &mut actions,
                program_id,
                agent_id,
                "AI orchestration run",
            )
            .await?;
        }

        let (session, reply, planner_outcome) =
            execute_program_agent_chat(&state, program_id, agent_id, message).await?;
        ctx.selected_project_agent_id = Some(session.agent_id);
        transcript = Some(to_transcript_view(&session.transcript));
        planner = planner_outcome;
        execution = to_latest_execution_view(&session);
        execution_attempts = to_execution_attempt_views(&session);
        diagnostics = execution
            .as_ref()
            .and_then(|value| value.diagnostics.clone());
        actions.push(format!(
            "Delegated chat to agent {} for project {}.",
            agent_id.0, program_id
        ));
        reply
    };

    Ok(Json(DashboardAiChatResponse {
        success: true,
        reply,
        selected_program_id: ctx.selected_program_id,
        selected_agent_id: ctx.selected_agent_id.map(|id| id.0),
        selected_project_agent_id: ctx.selected_project_agent_id.map(|id| id.0),
        actions,
        transcript,
        planner,
        execution,
        execution_attempts,
        diagnostics,
    }))
}

/// Provider connectivity + credits probe for dashboard badges.
///
/// `GET /dashboard/openrouter/status`
pub async fn openrouter_status(
    State(state): State<AppState>,
    Query(query): Query<DashboardOpenRouterStatusQuery>,
) -> Json<DashboardOpenRouterStatusResponse> {
    let selected = query.selected_agent_id.map(AgentId);
    let selected_session = selected.and_then(|id| state.agent_registry.get(&id));

    let session = selected_session.or_else(|| {
        state.agent_registry.list().into_iter().find(|entry| {
            entry.llm.provider.as_deref() == Some("openrouter") && entry.llm.api_key.is_some()
        })
    });

    let Some(session) = session else {
        return Json(DashboardOpenRouterStatusResponse {
            success: true,
            connected: false,
            provider: Some("openrouter".to_string()),
            message: Some("No OpenRouter-configured agent found.".to_string()),
            credit_balance: None,
            total_credits: None,
            total_usage: None,
        });
    };

    if session.llm.provider.as_deref() != Some("openrouter") {
        return Json(DashboardOpenRouterStatusResponse {
            success: true,
            connected: false,
            provider: session.llm.provider.clone(),
            message: Some("Selected agent is not using OpenRouter.".to_string()),
            credit_balance: None,
            total_credits: None,
            total_usage: None,
        });
    }

    let Some(api_key) = session.llm.api_key.clone() else {
        return Json(DashboardOpenRouterStatusResponse {
            success: true,
            connected: false,
            provider: Some("openrouter".to_string()),
            message: Some("OpenRouter API key is not configured.".to_string()),
            credit_balance: None,
            total_credits: None,
            total_usage: None,
        });
    };

    let base_url = session
        .llm
        .api_base_url
        .clone()
        .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());

    let probe = probe_openrouter(&base_url, &api_key).await;
    Json(DashboardOpenRouterStatusResponse {
        success: true,
        connected: probe.connected,
        provider: Some("openrouter".to_string()),
        message: probe.message,
        credit_balance: probe.credit_balance,
        total_credits: probe.total_credits,
        total_usage: probe.total_usage,
    })
}

#[derive(Debug, Default)]
struct DashboardContext {
    selected_program_id: Option<i64>,
    selected_agent_id: Option<AgentId>,
    selected_project_agent_id: Option<AgentId>,
}

#[derive(Debug, Default)]
struct OpenRouterProbe {
    connected: bool,
    message: Option<String>,
    credit_balance: Option<f64>,
    total_credits: Option<f64>,
    total_usage: Option<f64>,
}

async fn probe_openrouter(base_url: &str, api_key: &str) -> OpenRouterProbe {
    let client = reqwest::Client::new();
    let base = base_url.trim_end_matches('/');
    let key_endpoint = format!("{}/key", base);

    let mut probe = OpenRouterProbe::default();

    let key_resp = match client
        .get(&key_endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("HTTP-Referer", "https://localhost:3000")
        .header("X-Title", "lmlang dashboard")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            probe.message = Some(format!("OpenRouter unreachable: {}", err));
            return probe;
        }
    };

    if !key_resp.status().is_success() {
        probe.message = Some(format!(
            "OpenRouter key check failed ({})",
            key_resp.status()
        ));
        return probe;
    }

    probe.connected = true;

    let key_json = key_resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or(serde_json::Value::Null);
    let key_data = key_json.get("data").unwrap_or(&key_json);
    let key_remaining = parse_json_number(key_data.get("limit_remaining"))
        .or_else(|| parse_json_number(key_data.get("remaining_credits")));
    let key_limit = parse_json_number(key_data.get("limit"));
    let key_usage = parse_json_number(key_data.get("usage"));

    let credits_endpoint = format!("{}/credits", base);
    let mut credits_error: Option<String> = None;
    if let Ok(credits_resp) = client
        .get(&credits_endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("HTTP-Referer", "https://localhost:3000")
        .header("X-Title", "lmlang dashboard")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        if credits_resp.status().is_success() {
            let credits_json = credits_resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or(serde_json::Value::Null);
            let credits_data = credits_json.get("data").unwrap_or(&credits_json);
            probe.total_credits = parse_json_number(credits_data.get("total_credits"));
            probe.total_usage = parse_json_number(credits_data.get("total_usage"));
        } else {
            credits_error = Some(format!(
                "credits endpoint returned {}",
                credits_resp.status()
            ));
        }
    } else {
        credits_error = Some("credits endpoint unreachable".to_string());
    }

    probe.credit_balance = probe
        .total_credits
        .zip(probe.total_usage)
        .map(|(total, used)| (total - used).max(0.0))
        .or(key_remaining)
        .or_else(|| {
            key_limit
                .zip(key_usage)
                .map(|(limit, used)| (limit - used).max(0.0))
        });

    if probe.credit_balance.is_none() {
        probe.message = credits_error;
    }

    probe
}

async fn create_and_load_program(state: &AppState, name: &str) -> Result<i64, ApiError> {
    let mut service = state.service.lock().await;
    let program_id = service.create_program(name)?.0;
    service.load_program(ProgramId(program_id))?;
    Ok(program_id)
}

async fn ensure_program_selected(
    state: &AppState,
    ctx: &mut DashboardContext,
    actions: &mut Vec<String>,
) -> Result<i64, ApiError> {
    if let Some(program_id) = ctx.selected_program_id {
        let mut service = state.service.lock().await;
        service.load_program(ProgramId(program_id))?;
        return Ok(program_id);
    }

    let name = format!("ai-project-{}", unix_seconds());
    let program_id = create_and_load_program(state, &name).await?;
    ctx.selected_program_id = Some(program_id);
    actions.push(format!(
        "Auto-created project '{}' (#{}).",
        name, program_id
    ));
    Ok(program_id)
}

async fn ensure_agent_selected(
    state: &AppState,
    ctx: &mut DashboardContext,
    actions: &mut Vec<String>,
) -> Result<AgentId, ApiError> {
    if let Some(agent_id) = ctx.selected_agent_id {
        if state.agent_registry.get(&agent_id).is_some() {
            return Ok(agent_id);
        }
        ctx.selected_agent_id = None;
    }

    if let Some(existing) = state.agent_registry.list().into_iter().next() {
        ctx.selected_agent_id = Some(existing.id);
        actions.push(format!("Selected existing agent {}.", existing.id.0));
        return Ok(existing.id);
    }

    let agent_id = state.agent_registry.register(
        Some("dashboard-ai-agent".to_string()),
        AgentLlmConfig::default(),
    );
    state.agent_config_store.upsert(
        agent_id,
        Some("dashboard-ai-agent".to_string()),
        &AgentLlmConfig::default(),
    )?;
    ctx.selected_agent_id = Some(agent_id);
    actions.push(format!("Auto-registered agent {}.", agent_id.0));
    Ok(agent_id)
}

async fn ensure_project_agent_selected(
    state: &AppState,
    ctx: &mut DashboardContext,
    actions: &mut Vec<String>,
) -> Result<AgentId, ApiError> {
    let program_id = ensure_program_selected(state, ctx, actions).await?;

    if let Some(agent_id) = ctx.selected_project_agent_id {
        if state
            .project_agent_manager
            .get(program_id, agent_id)
            .await
            .is_some()
        {
            return Ok(agent_id);
        }
        ctx.selected_project_agent_id = None;
    }

    if let Some(existing) = state
        .project_agent_manager
        .list_for_program(program_id)
        .await
        .into_iter()
        .next()
    {
        ctx.selected_project_agent_id = Some(existing.agent_id);
        ctx.selected_agent_id = Some(existing.agent_id);
        actions.push(format!(
            "Selected assigned agent {} for project {}.",
            existing.agent_id.0, program_id
        ));
        return Ok(existing.agent_id);
    }

    let agent_id = ensure_agent_selected(state, ctx, actions).await?;
    ensure_assignment(state, ctx, actions, program_id, agent_id).await?;
    Ok(agent_id)
}

async fn ensure_assignment(
    state: &AppState,
    ctx: &mut DashboardContext,
    actions: &mut Vec<String>,
    program_id: i64,
    agent_id: AgentId,
) -> Result<(), ApiError> {
    if state
        .project_agent_manager
        .get(program_id, agent_id)
        .await
        .is_none()
    {
        let agent = state
            .agent_registry
            .get(&agent_id)
            .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;
        state
            .project_agent_manager
            .assign(program_id, agent_id, agent.name)
            .await;
        actions.push(format!(
            "Assigned agent {} to project {}.",
            agent_id.0, program_id
        ));
    }
    ctx.selected_project_agent_id = Some(agent_id);
    ctx.selected_agent_id = Some(agent_id);
    Ok(())
}

async fn ensure_running(
    state: &AppState,
    ctx: &mut DashboardContext,
    actions: &mut Vec<String>,
    program_id: i64,
    agent_id: AgentId,
    default_goal: &str,
) -> Result<(), ApiError> {
    if let Some(session) = state.project_agent_manager.get(program_id, agent_id).await {
        if session.run_status == "running" {
            return Ok(());
        }
    }

    {
        let mut service = state.service.lock().await;
        service.load_program(ProgramId(program_id))?;
    }

    state
        .project_agent_manager
        .start(program_id, agent_id, default_goal.to_string())
        .await
        .map_err(ApiError::BadRequest)?;
    state
        .autonomous_runner
        .start(state.clone(), program_id, agent_id);
    ctx.selected_project_agent_id = Some(agent_id);
    actions.push(format!(
        "Auto-started build run for agent {} on project {}.",
        agent_id.0, program_id
    ));
    Ok(())
}

fn parse_project_name(message: &str, lower: &str) -> Option<String> {
    for phrase in ["create project", "new project"] {
        if let Some(rest) = text_after(message, lower, phrase) {
            let rest = rest
                .trim_start_matches("named ")
                .trim_start_matches("called ")
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim_matches('.');
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn parse_agent_name(message: &str, lower: &str) -> Option<String> {
    let rest = text_after(message, lower, "register agent")
        .or_else(|| text_after(message, lower, "add agent"))?;
    let mut parts = Vec::new();
    for token in rest.split_whitespace() {
        let t = token.to_ascii_lowercase();
        if matches!(
            t.as_str(),
            "provider"
                | "model"
                | "api"
                | "key"
                | "base"
                | "url"
                | "system"
                | "prompt"
                | "openrouter"
                | "openai_compatible"
                | "openai-compatible"
        ) {
            break;
        }
        parts.push(token);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn parse_start_goal(message: &str, lower: &str) -> Option<String> {
    let rest = text_after(message, lower, "start build")?;
    let goal = rest.trim().trim_matches('"').trim_matches('\'').trim();
    if goal.is_empty() {
        None
    } else {
        Some(goal.to_string())
    }
}

fn parse_llm_config_from_message(
    message: &str,
    lower: &str,
    mut cfg: AgentLlmConfig,
) -> AgentLlmConfig {
    if lower.contains("openrouter") {
        cfg.provider = Some("openrouter".to_string());
    }
    if lower.contains("openai_compatible") || lower.contains("openai-compatible") {
        cfg.provider = Some("openai_compatible".to_string());
    }
    if let Some(provider) = token_after(message, "provider") {
        let normalized = provider.to_ascii_lowercase().replace('-', "_");
        if normalized == "openrouter" || normalized == "openai_compatible" {
            cfg.provider = Some(normalized);
        }
    }

    if let Some(model) = token_after(message, "model") {
        cfg.model = Some(model);
    }
    if let Some(api_key) =
        token_after_pair(message, "api", "key").or_else(|| token_after(message, "key"))
    {
        cfg.api_key = Some(api_key);
    }
    if let Some(base_url) =
        token_after_pair(message, "base", "url").or_else(|| token_after(message, "url"))
    {
        cfg.api_base_url = Some(base_url);
    }
    if let Some(system_prompt) = text_after_pair(message, lower, "system", "prompt") {
        if !system_prompt.trim().is_empty() {
            cfg.system_prompt = Some(system_prompt.trim().to_string());
        }
    }
    if cfg.provider.as_deref() == Some("openrouter") && cfg.api_base_url.is_none() {
        cfg.api_base_url = Some("https://openrouter.ai/api/v1".to_string());
    }
    cfg.normalize()
}

fn token_after(message: &str, key: &str) -> Option<String> {
    let tokens = message.split_whitespace().collect::<Vec<_>>();
    let key = key.to_ascii_lowercase();
    for i in 0..tokens.len() {
        if tokens[i].to_ascii_lowercase() == key && i + 1 < tokens.len() {
            return Some(
                tokens[i + 1]
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim_matches(',')
                    .to_string(),
            );
        }
    }
    None
}

fn token_after_pair(message: &str, first: &str, second: &str) -> Option<String> {
    let tokens = message.split_whitespace().collect::<Vec<_>>();
    let first = first.to_ascii_lowercase();
    let second = second.to_ascii_lowercase();
    for i in 0..tokens.len() {
        if i + 2 < tokens.len()
            && tokens[i].to_ascii_lowercase() == first
            && tokens[i + 1].to_ascii_lowercase() == second
        {
            return Some(
                tokens[i + 2]
                    .trim_matches('"')
                    .trim_matches('\'')
                    .trim_matches(',')
                    .to_string(),
            );
        }
    }
    None
}

fn text_after<'a>(message: &'a str, lower: &str, phrase: &str) -> Option<&'a str> {
    let idx = lower.find(phrase)?;
    let start = idx + phrase.len();
    message.get(start..).map(str::trim)
}

fn text_after_pair<'a>(
    message: &'a str,
    lower: &str,
    first: &str,
    second: &str,
) -> Option<&'a str> {
    let phrase = format!("{} {}", first, second);
    text_after(message, lower, &phrase)
}

fn to_transcript_view(messages: &[ProjectAgentMessage]) -> Vec<AgentChatMessageView> {
    messages
        .iter()
        .map(|msg| AgentChatMessageView {
            role: msg.role.clone(),
            content: msg.content.clone(),
            timestamp: msg.timestamp.clone(),
        })
        .collect()
}

fn parse_json_number(value: Option<&serde_json::Value>) -> Option<f64> {
    match value {
        Some(serde_json::Value::Number(num)) => num.as_f64(),
        Some(serde_json::Value::String(text)) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
