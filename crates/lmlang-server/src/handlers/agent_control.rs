//! Project-scoped agent assignment, start/stop, and chat handlers.

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::autonomy_planner::{plan_for_prompt, PlannerOutcome};
use crate::concurrency::AgentId;
use crate::error::ApiError;
use crate::project_agent::{ProjectAgentMessage, ProjectAgentSession};
use crate::schema::agent_control::{
    AgentChatMessageView, ChatWithProgramAgentRequest, ChatWithProgramAgentResponse,
    ExecutionActionView, ExecutionDiagnosticsView, ExecutionStopReasonView, ExecutionSummaryView,
    ListProgramAgentsResponse, PlannerActionView, PlannerFailureView, PlannerOutcomeView,
    PlannerValidationErrorView, ProgramAgentActionResponse, ProgramAgentDetailResponse,
    ProgramAgentSessionView, StartProgramAgentRequest, StopProgramAgentRequest,
};
use crate::schema::autonomy_execution::{bounded_attempt_history, AutonomyExecutionAttemptSummary};
use crate::state::AppState;

const MAX_EXECUTION_TIMELINE_ATTEMPTS: usize = 8;

/// `GET /programs/{id}/agents`
pub async fn list_program_agents(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
) -> Result<Json<ListProgramAgentsResponse>, ApiError> {
    ensure_program_exists(&state, program_id).await?;
    let sessions = state
        .project_agent_manager
        .list_for_program(program_id)
        .await
        .into_iter()
        .map(to_session_view)
        .collect::<Vec<_>>();

    Ok(Json(ListProgramAgentsResponse {
        program_id,
        agents: sessions,
    }))
}

/// `POST /programs/{id}/agents/{agent_id}/assign`
pub async fn assign_program_agent(
    State(state): State<AppState>,
    Path((program_id, agent_id)): Path<(i64, String)>,
) -> Result<Json<ProgramAgentActionResponse>, ApiError> {
    ensure_program_exists(&state, program_id).await?;
    let agent_id = parse_agent_id(&agent_id)?;
    let agent = state
        .agent_registry
        .get(&agent_id)
        .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;

    let session = state
        .project_agent_manager
        .assign(program_id, agent_id, agent.name.clone())
        .await;

    Ok(Json(ProgramAgentActionResponse {
        success: true,
        session: to_session_view(session),
        system_message: Some("Agent assigned to project".to_string()),
    }))
}

/// `GET /programs/{id}/agents/{agent_id}`
pub async fn get_program_agent(
    State(state): State<AppState>,
    Path((program_id, agent_id)): Path<(i64, String)>,
) -> Result<Json<ProgramAgentDetailResponse>, ApiError> {
    ensure_program_exists(&state, program_id).await?;
    let agent_id = parse_agent_id(&agent_id)?;
    let session = state
        .project_agent_manager
        .get(program_id, agent_id)
        .await
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "agent {} is not assigned to project {}",
                agent_id.0, program_id
            ))
        })?;

    Ok(Json(ProgramAgentDetailResponse {
        session: to_session_view(session.clone()),
        transcript: to_transcript_view(&session.transcript),
    }))
}

/// `POST /programs/{id}/agents/{agent_id}/start`
pub async fn start_program_agent(
    State(state): State<AppState>,
    Path((program_id, agent_id)): Path<(i64, String)>,
    Json(req): Json<StartProgramAgentRequest>,
) -> Result<Json<ProgramAgentActionResponse>, ApiError> {
    ensure_program_exists(&state, program_id).await?;
    let agent_id = parse_agent_id(&agent_id)?;
    ensure_agent_exists(&state, agent_id)?;

    if req.goal.trim().is_empty() {
        return Err(ApiError::BadRequest("goal must not be empty".to_string()));
    }

    {
        let mut service = state.service.lock().await;
        service.load_program(lmlang_storage::ProgramId(program_id))?;
    }

    let session = state
        .project_agent_manager
        .start(program_id, agent_id, req.goal)
        .await
        .map_err(ApiError::BadRequest)?;
    state
        .autonomous_runner
        .start(state.clone(), program_id, agent_id);

    Ok(Json(ProgramAgentActionResponse {
        success: true,
        session: to_session_view(session),
        system_message: Some("Build run started".to_string()),
    }))
}

/// `POST /programs/{id}/agents/{agent_id}/stop`
pub async fn stop_program_agent(
    State(state): State<AppState>,
    Path((program_id, agent_id)): Path<(i64, String)>,
    Json(req): Json<StopProgramAgentRequest>,
) -> Result<Json<ProgramAgentActionResponse>, ApiError> {
    ensure_program_exists(&state, program_id).await?;
    let agent_id = parse_agent_id(&agent_id)?;
    ensure_agent_exists(&state, agent_id)?;

    let session = state
        .project_agent_manager
        .stop(program_id, agent_id, req.reason)
        .await
        .map_err(ApiError::BadRequest)?;
    state.autonomous_runner.stop(program_id, agent_id);

    Ok(Json(ProgramAgentActionResponse {
        success: true,
        session: to_session_view(session),
        system_message: Some("Build run stopped".to_string()),
    }))
}

/// `POST /programs/{id}/agents/{agent_id}/chat`
pub async fn chat_with_program_agent(
    State(state): State<AppState>,
    Path((program_id, agent_id)): Path<(i64, String)>,
    Json(req): Json<ChatWithProgramAgentRequest>,
) -> Result<Json<ChatWithProgramAgentResponse>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    let (session, reply, planner) =
        execute_program_agent_chat(&state, program_id, agent_id, req.message).await?;

    Ok(Json(ChatWithProgramAgentResponse {
        success: true,
        session: to_session_view(session.clone()),
        reply,
        transcript: to_transcript_view(&session.transcript),
        planner,
        execution: to_latest_execution_view(&session),
        execution_attempts: to_execution_attempt_views(&session),
    }))
}

pub(crate) async fn execute_program_agent_chat(
    state: &AppState,
    program_id: i64,
    agent_id: AgentId,
    message: String,
) -> Result<(ProjectAgentSession, String, Option<PlannerOutcomeView>), ApiError> {
    ensure_program_exists(state, program_id).await?;
    ensure_agent_exists(state, agent_id)?;

    if message.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "message must not be empty".to_string(),
        ));
    }

    let (reply, planner_outcome) =
        plan_non_command_prompt(state, program_id, agent_id, message.as_str()).await?;
    let planner = Some(planner_outcome);

    let (session, reply) = state
        .project_agent_manager
        .chat(program_id, agent_id, message, None, Some(reply))
        .await
        .map_err(ApiError::BadRequest)?;

    Ok((session, reply, planner))
}

fn parse_agent_id(raw: &str) -> Result<AgentId, ApiError> {
    let uuid = Uuid::parse_str(raw)
        .map_err(|_| ApiError::BadRequest(format!("invalid agent id '{}': expected UUID", raw)))?;
    Ok(AgentId(uuid))
}

fn ensure_agent_exists(state: &AppState, agent_id: AgentId) -> Result<(), ApiError> {
    if state.agent_registry.get(&agent_id).is_none() {
        return Err(ApiError::NotFound(format!(
            "agent {} not found",
            agent_id.0
        )));
    }
    Ok(())
}

async fn ensure_program_exists(state: &AppState, program_id: i64) -> Result<(), ApiError> {
    let service = state.service.lock().await;
    let exists = service
        .list_programs()?
        .into_iter()
        .any(|program| program.id.0 == program_id);
    if !exists {
        return Err(ApiError::NotFound(format!(
            "program {} not found",
            program_id
        )));
    }
    Ok(())
}

fn to_session_view(session: ProjectAgentSession) -> ProgramAgentSessionView {
    let execution = to_latest_execution_view(&session);
    let execution_attempts = to_execution_attempt_views(&session);
    let stop_reason = session
        .stop_reason
        .as_ref()
        .map(to_stop_reason_view)
        .or_else(|| {
            execution
                .as_ref()
                .and_then(|value| value.stop_reason.clone())
        });

    ProgramAgentSessionView {
        program_id: session.program_id,
        agent_id: session.agent_id.0,
        name: session.name,
        run_status: session.run_status,
        active_goal: session.active_goal,
        assigned_at: session.assigned_at,
        started_at: session.started_at,
        stopped_at: session.stopped_at,
        updated_at: session.updated_at,
        message_count: session.transcript.len(),
        stop_reason,
        execution,
        execution_attempts,
    }
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

async fn plan_non_command_prompt(
    state: &AppState,
    program_id: i64,
    agent_id: AgentId,
    user_message: &str,
) -> Result<(String, PlannerOutcomeView), ApiError> {
    let agent = state
        .agent_registry
        .get(&agent_id)
        .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", agent_id.0)))?;
    let transcript_context = state
        .project_agent_manager
        .get(program_id, agent_id)
        .await
        .map(|session| {
            session
                .transcript
                .iter()
                .rev()
                .take(8)
                .rev()
                .map(|entry| format!("{}: {}", entry.role, entry.content))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let outcome = plan_for_prompt(&agent.llm, user_message, &transcript_context, None).await;
    let view = planner_outcome_to_view(&outcome);
    let reply = planner_outcome_to_reply(&outcome);
    Ok((reply, view))
}

fn planner_outcome_to_reply(outcome: &PlannerOutcome) -> String {
    match outcome {
        PlannerOutcome::Accepted(accepted) => {
            let summary = accepted
                .actions
                .iter()
                .map(|action| format!("{} ({})", action.kind, action.summary))
                .collect::<Vec<_>>()
                .join("; ");

            format!(
                "Planner accepted {} action(s) for goal '{}'. Contract version {}. {}",
                accepted.action_count,
                accepted.goal,
                accepted.version,
                if summary.is_empty() {
                    "No action summary available.".to_string()
                } else {
                    format!("Actions: {}", summary)
                }
            )
        }
        PlannerOutcome::Rejected(rejected) => {
            let mut message = format!(
                "Planner rejected request [{}]: {}",
                rejected.code, rejected.message
            );
            if !rejected.validation_errors.is_empty() {
                let first = &rejected.validation_errors[0];
                message.push_str(&format!(
                    " First validation error: {} ({:?}).",
                    first.message, first.field
                ));
            }
            message
        }
    }
}

fn planner_outcome_to_view(outcome: &PlannerOutcome) -> PlannerOutcomeView {
    match outcome {
        PlannerOutcome::Accepted(accepted) => PlannerOutcomeView {
            status: outcome.status().to_string(),
            version: Some(accepted.version.clone()),
            actions: accepted
                .actions
                .iter()
                .map(|action| PlannerActionView {
                    kind: action.kind.clone(),
                    summary: action.summary.clone(),
                })
                .collect(),
            failure: None,
        },
        PlannerOutcome::Rejected(rejected) => PlannerOutcomeView {
            status: outcome.status().to_string(),
            version: rejected.version.clone(),
            actions: Vec::new(),
            failure: Some(PlannerFailureView {
                code: rejected.code.clone(),
                message: rejected.message.clone(),
                retryable: rejected.retryable,
                validation_errors: rejected
                    .validation_errors
                    .iter()
                    .map(|error| PlannerValidationErrorView {
                        code: validation_code_to_string(&error.code),
                        message: error.message.clone(),
                        action_index: error.action_index,
                        field: error.field.clone(),
                    })
                    .collect(),
            }),
        },
    }
}

pub(crate) fn to_latest_execution_view(
    session: &ProjectAgentSession,
) -> Option<ExecutionSummaryView> {
    let latest = session.execution_attempts.last()?;
    Some(to_execution_summary_view(
        latest,
        session.stop_reason.as_ref(),
    ))
}

pub(crate) fn to_execution_attempt_views(
    session: &ProjectAgentSession,
) -> Vec<ExecutionSummaryView> {
    bounded_attempt_history(&session.execution_attempts, MAX_EXECUTION_TIMELINE_ATTEMPTS)
        .iter()
        .map(|attempt| to_execution_summary_view(attempt, None))
        .collect()
}

fn to_execution_summary_view(
    attempt: &AutonomyExecutionAttemptSummary,
    fallback_stop_reason: Option<&crate::schema::autonomy_execution::StopReason>,
) -> ExecutionSummaryView {
    ExecutionSummaryView {
        attempt: attempt.attempt,
        max_attempts: attempt.max_attempts,
        planner_status: attempt.planner_status.clone(),
        action_count: attempt.action_count,
        succeeded_actions: attempt.succeeded_actions,
        actions: attempt
            .action_results
            .iter()
            .map(|action| ExecutionActionView {
                action_index: action.action_index,
                kind: action.kind.clone(),
                status: enum_to_string(&action.status),
                summary: action.summary.clone(),
                error_code: action.error.as_ref().map(|err| enum_to_string(&err.code)),
                diagnostics: action
                    .diagnostics
                    .as_ref()
                    .or_else(|| {
                        action
                            .error
                            .as_ref()
                            .and_then(|error| error.diagnostics.as_ref())
                    })
                    .map(to_diagnostics_view),
            })
            .collect(),
        diagnostics: attempt.latest_diagnostics().map(to_diagnostics_view),
        stop_reason: attempt
            .terminal_stop_reason(fallback_stop_reason)
            .map(to_stop_reason_view),
    }
}

fn to_stop_reason_view(
    reason: &crate::schema::autonomy_execution::StopReason,
) -> ExecutionStopReasonView {
    ExecutionStopReasonView {
        code: enum_to_string(&reason.code),
        message: reason.message.clone(),
        detail: reason.detail.clone(),
    }
}

fn to_diagnostics_view(
    diagnostics: &crate::schema::autonomy_execution::AutonomyDiagnostics,
) -> ExecutionDiagnosticsView {
    ExecutionDiagnosticsView {
        class: enum_to_string(&diagnostics.class),
        retryable: diagnostics.retryable,
        summary: diagnostics.summary.clone(),
        key_diagnostics: diagnostics.messages.clone(),
        detail: diagnostics.detail.clone(),
    }
}

fn enum_to_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|raw| raw.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn validation_code_to_string(
    code: &crate::schema::autonomy_plan::AutonomyPlanValidationCode,
) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown_validation_code".to_string())
}
