//! Project-scoped agent assignment, start/stop, and chat handlers.

use std::process::Command;

use axum::extract::{Path, State};
use axum::Json;
use lmlang_core::id::{FunctionId, ModuleId};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp};
use lmlang_core::type_id::TypeId;
use lmlang_core::types::Visibility;
use uuid::Uuid;

use crate::autonomy_planner::{plan_for_prompt, PlannerOutcome};
use crate::concurrency::AgentId;
use crate::error::ApiError;
use crate::project_agent::{ProjectAgentMessage, ProjectAgentSession};
use crate::schema::agent_control::{
    AgentChatMessageView, ChatWithProgramAgentRequest, ChatWithProgramAgentResponse,
    ListProgramAgentsResponse, ProgramAgentActionResponse, ProgramAgentDetailResponse,
    PlannerActionView, PlannerFailureView, PlannerOutcomeView, PlannerValidationErrorView,
    ProgramAgentSessionView, StartProgramAgentRequest, StopProgramAgentRequest,
};
use crate::schema::compile::CompileRequest;
use crate::schema::mutations::{CreatedEntity, Mutation, ProposeEditRequest};
use crate::schema::verify::VerifyScope;
use crate::state::AppState;

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

    let action_note = maybe_execute_agent_chat_command(state, program_id, message.as_str()).await?;
    let mut planner = None;
    let assistant_override = if action_note.is_none() {
        let (reply, planner_outcome) =
            plan_non_command_prompt(state, program_id, agent_id, message.as_str()).await?;
        planner = Some(planner_outcome);
        Some(reply)
    } else {
        None
    };

    let (session, reply) = state
        .project_agent_manager
        .chat(
            program_id,
            agent_id,
            message,
            action_note,
            assistant_override,
        )
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

pub(crate) async fn maybe_execute_agent_chat_command(
    state: &AppState,
    program_id: i64,
    message: &str,
) -> Result<Option<String>, ApiError> {
    let lower = message.to_lowercase();

    if lower.contains("hello world")
        && (lower.contains("create") || lower.contains("build") || lower.contains("scaffold"))
    {
        let summary = scaffold_hello_world_program(state, program_id).await?;
        return Ok(Some(summary));
    }

    if lower.contains("run") || lower.contains("execute") {
        let summary = compile_and_run_hello_world(state, program_id).await?;
        return Ok(Some(summary));
    }

    if lower.contains("compile") {
        let summary = compile_hello_world(state, program_id).await?;
        return Ok(Some(summary));
    }

    Ok(None)
}

async fn scaffold_hello_world_program(
    state: &AppState,
    program_id: i64,
) -> Result<String, ApiError> {
    let mut service = state.service.lock().await;
    service.load_program(lmlang_storage::ProgramId(program_id))?;

    let existing_function_id = service
        .graph()
        .functions()
        .iter()
        .find(|(_, def)| def.name == "hello_world")
        .map(|(id, _)| *id);

    let function_id = match existing_function_id {
        Some(id) => id,
        None => {
            let create_response = service.propose_edit(ProposeEditRequest {
                mutations: vec![Mutation::AddFunction {
                    name: "hello_world".to_string(),
                    module: ModuleId(0),
                    params: Vec::new(),
                    return_type: TypeId::UNIT,
                    visibility: Visibility::Public,
                }],
                dry_run: false,
                expected_hashes: None,
            })?;

            if !create_response.valid || !create_response.committed {
                return Err(ApiError::BadRequest(
                    "failed to create hello_world function".to_string(),
                ));
            }

            create_response
                .created
                .iter()
                .find_map(|entity| match entity {
                    CreatedEntity::Function { id } => Some(*id),
                    _ => None,
                })
                .ok_or_else(|| {
                    ApiError::InternalError(
                        "hello_world creation succeeded but function id was missing".to_string(),
                    )
                })?
        }
    };

    let has_return = service.graph().compute().node_weights().any(|node| {
        node.owner == function_id && matches!(&node.op, ComputeNodeOp::Core(ComputeOp::Return))
    });

    if !has_return {
        let node_response = service.propose_edit(ProposeEditRequest {
            mutations: vec![Mutation::InsertNode {
                op: ComputeNodeOp::Core(ComputeOp::Return),
                owner: function_id,
            }],
            dry_run: false,
            expected_hashes: None,
        })?;

        if !node_response.valid || !node_response.committed {
            return Err(ApiError::BadRequest(
                "failed to add return node to hello_world".to_string(),
            ));
        }
    }

    let verify = service.verify(VerifyScope::Full, None)?;
    if !verify.valid {
        return Err(ApiError::BadRequest(
            "hello_world scaffold failed verification".to_string(),
        ));
    }

    Ok(format!(
        "Hello world scaffold ready in program {} (function id {}).",
        program_id, function_id.0
    ))
}

async fn compile_hello_world(state: &AppState, program_id: i64) -> Result<String, ApiError> {
    let mut service = state.service.lock().await;
    service.load_program(lmlang_storage::ProgramId(program_id))?;

    ensure_function_exists(service.graph().functions(), "hello_world")?;

    let response = service.compile(&CompileRequest {
        opt_level: "O0".to_string(),
        target_triple: None,
        debug_symbols: false,
        entry_function: Some("hello_world".to_string()),
        output_dir: None,
    })?;

    Ok(format!(
        "Compiled hello_world to {} ({} bytes, {} ms).",
        response.binary_path, response.binary_size, response.compilation_time_ms
    ))
}

async fn compile_and_run_hello_world(
    state: &AppState,
    program_id: i64,
) -> Result<String, ApiError> {
    let binary_path = {
        let mut service = state.service.lock().await;
        service.load_program(lmlang_storage::ProgramId(program_id))?;

        ensure_function_exists(service.graph().functions(), "hello_world")?;

        let response = service.compile(&CompileRequest {
            opt_level: "O0".to_string(),
            target_triple: None,
            debug_symbols: false,
            entry_function: Some("hello_world".to_string()),
            output_dir: None,
        })?;
        response.binary_path
    };

    let resolved_binary_path = resolve_binary_path(&binary_path)?;

    let output = Command::new(&resolved_binary_path)
        .output()
        .map_err(|err| ApiError::InternalError(format!("failed to run binary: {}", err)))?;

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        Ok(format!(
            "Program executed (exit code {}). stdout='{}' stderr='{}'.",
            code, stdout, stderr
        ))
    } else {
        Err(ApiError::InternalError(format!(
            "program run failed (exit code {}). stderr='{}'",
            code, stderr
        )))
    }
}

fn resolve_binary_path(binary_path: &str) -> Result<std::path::PathBuf, ApiError> {
    let direct = std::path::PathBuf::from(binary_path);
    if direct.exists() {
        return Ok(direct);
    }

    let cwd = std::env::current_dir()
        .map_err(|err| ApiError::InternalError(format!("failed to read current dir: {}", err)))?;
    let cwd_candidate = cwd.join(binary_path);
    if cwd_candidate.exists() {
        return Ok(cwd_candidate);
    }

    let manifest_candidate = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(binary_path);
    if manifest_candidate.exists() {
        return Ok(manifest_candidate);
    }

    Err(ApiError::InternalError(format!(
        "compiled binary not found at '{}' (checked '{}', '{}', '{}')",
        binary_path,
        direct.display(),
        cwd_candidate.display(),
        manifest_candidate.display()
    )))
}

fn ensure_function_exists(
    functions: &std::collections::HashMap<FunctionId, lmlang_core::function::FunctionDef>,
    name: &str,
) -> Result<(), ApiError> {
    if functions.values().any(|def| def.name == name) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "function '{}' does not exist; ask the agent to create hello world first",
            name
        )))
    }
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

    let outcome = plan_for_prompt(&agent.llm, user_message, &transcript_context).await;
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
            let mut message = format!("Planner rejected request [{}]: {}", rejected.code, rejected.message);
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

fn validation_code_to_string(
    code: &crate::schema::autonomy_plan::AutonomyPlanValidationCode,
) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown_validation_code".to_string())
}
