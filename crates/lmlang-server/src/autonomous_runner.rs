//! Background autonomous run loop for assigned project agents.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use lmlang_storage::ProgramId;
use tokio::task::JoinHandle;

use crate::autonomy_executor::execute_plan;
use crate::autonomy_planner::{plan_for_prompt, PlannerOutcome};
use crate::concurrency::AgentId;
use crate::handlers::agent_control::maybe_execute_agent_chat_command;
use crate::project_agent::ProjectAgentSession;
use crate::schema::autonomy_execution::{
    AutonomyActionExecutionResult, AutonomyDiagnostics, AutonomyDiagnosticsClass,
    AutonomyExecutionAttemptSummary, AutonomyExecutionError, AutonomyExecutionErrorCode,
    AutonomyExecutionOutcome, AutonomyExecutionStatus, StopReason, StopReasonCode,
};
use crate::schema::verify::VerifyScope;
use crate::state::AppState;

const DEFAULT_MAX_ATTEMPTS: u32 = 3;
const MAX_ATTEMPTS_ENV: &str = "LMLANG_AUTONOMY_MAX_ATTEMPTS";

pub struct AutonomousRunner {
    tasks: DashMap<(i64, AgentId), JoinHandle<()>>,
}

impl AutonomousRunner {
    pub fn new() -> Self {
        Self {
            tasks: DashMap::new(),
        }
    }

    pub fn start(self: &Arc<Self>, state: AppState, program_id: i64, agent_id: AgentId) {
        let key = (program_id, agent_id);
        if self.tasks.contains_key(&key) {
            return;
        }

        let manager = Arc::clone(self);
        let handle = tokio::spawn(async move {
            manager.run_loop(state, program_id, agent_id).await;
            manager.tasks.remove(&(program_id, agent_id));
        });

        self.tasks.insert(key, handle);
    }

    pub fn stop(&self, program_id: i64, agent_id: AgentId) {
        if let Some((_, handle)) = self.tasks.remove(&(program_id, agent_id)) {
            handle.abort();
        }
    }

    async fn run_loop(&self, state: AppState, program_id: i64, agent_id: AgentId) {
        let max_attempts = configured_max_attempts();
        let mut attempts = Vec::new();
        let mut last_version: Option<String> = None;
        let mut command_failures = 0u32;

        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let Some(session) = state.project_agent_manager.get(program_id, agent_id).await else {
                break;
            };
            if session.run_status != "running" {
                break;
            }

            let goal = session.active_goal.clone().unwrap_or_default();
            let decision = self
                .decide_next_step(&state, agent_id, &session, &goal)
                .await;

            match decision {
                StepDecision::Command(command) => {
                    match maybe_execute_agent_chat_command(&state, program_id, &command).await {
                        Ok(Some(summary)) => {
                            command_failures = 0;
                            let _ = state
                                .project_agent_manager
                                .append_message(
                                    program_id,
                                    agent_id,
                                    "assistant",
                                    format!("Autonomous step `{}` complete. {}", command, summary),
                                )
                                .await;
                        }
                        Ok(None) => {
                            let _ = state
                                .project_agent_manager
                                .append_message(
                                    program_id,
                                    agent_id,
                                    "system",
                                    format!(
                                        "Autonomous step skipped: command `{}` did not map to an action.",
                                        command
                                    ),
                                )
                                .await;
                        }
                        Err(err) => {
                            command_failures = command_failures.saturating_add(1);
                            let _ = state
                                .project_agent_manager
                                .append_message(
                                    program_id,
                                    agent_id,
                                    "system",
                                    format!("Autonomous step `{}` failed: {}", command, err),
                                )
                                .await;

                            if command_failures >= max_attempts {
                                let stop_reason = StopReason::new(
                                    StopReasonCode::RetryBudgetExhausted,
                                    format!(
                                        "deterministic command path exhausted {} retries",
                                        max_attempts
                                    ),
                                )
                                .with_detail(serde_json::json!({
                                    "command": command,
                                    "max_attempts": max_attempts,
                                }));
                                attempts.push(AutonomyExecutionAttemptSummary {
                                    attempt: command_failures,
                                    max_attempts,
                                    planner_status: "deterministic_command".to_string(),
                                    action_count: 0,
                                    succeeded_actions: 0,
                                    action_results: Vec::new(),
                                    stop_reason: Some(stop_reason.clone()),
                                });

                                self.finish_with_outcome(
                                    &state,
                                    program_id,
                                    agent_id,
                                    &goal,
                                    "deterministic-command".to_string(),
                                    attempts.clone(),
                                    AutonomyExecutionStatus::Failed,
                                    stop_reason,
                                    "Autonomous deterministic command path exhausted retries."
                                        .to_string(),
                                )
                                .await;
                                break;
                            }
                        }
                    }
                }
                StepDecision::Planner(outcome) => {
                    let attempt_number = attempts.len() as u32 + 1;
                    let resolution = self
                        .execute_planner_attempt(
                            &state,
                            program_id,
                            outcome,
                            attempt_number,
                            max_attempts,
                        )
                        .await;

                    if let Some(version) = resolution.version.clone() {
                        last_version = Some(version);
                    }

                    attempts.push(resolution.attempt.clone());
                    let _ = state
                        .project_agent_manager
                        .append_execution_attempt(program_id, agent_id, resolution.attempt.clone())
                        .await;

                    if let Some(note) = resolution.note {
                        let _ = state
                            .project_agent_manager
                            .append_message(program_id, agent_id, "system", note)
                            .await;
                    }

                    match resolution.transition {
                        AttemptTransition::Continue => {}
                        AttemptTransition::Terminal {
                            status,
                            stop_reason,
                            run_status_note,
                        } => {
                            self.finish_with_outcome(
                                &state,
                                program_id,
                                agent_id,
                                &goal,
                                last_version
                                    .clone()
                                    .unwrap_or_else(|| "2026-02-19".to_string()),
                                attempts.clone(),
                                status,
                                stop_reason,
                                run_status_note,
                            )
                            .await;
                            break;
                        }
                    }
                }
                StepDecision::Done => {
                    let stop_reason = StopReason::new(
                        StopReasonCode::Completed,
                        "Autonomous run reached deterministic done state.",
                    );
                    self.finish_with_outcome(
                        &state,
                        program_id,
                        agent_id,
                        &goal,
                        "deterministic-command".to_string(),
                        attempts.clone(),
                        AutonomyExecutionStatus::Succeeded,
                        stop_reason,
                        "Autonomous run reached done state.".to_string(),
                    )
                    .await;
                    break;
                }
                StepDecision::Noop => {}
            }
        }
    }

    async fn execute_planner_attempt(
        &self,
        state: &AppState,
        program_id: i64,
        planner_outcome: PlannerOutcome,
        attempt: u32,
        max_attempts: u32,
    ) -> AttemptResolution {
        match planner_outcome {
            PlannerOutcome::Rejected(rejected) => {
                let terminal = decide_transition(
                    AttemptEvent::PlannerRejected {
                        retryable: rejected.retryable,
                    },
                    attempt,
                    max_attempts,
                );
                let stop_reason = StopReason::new(
                    match terminal {
                        TransitionDecision::Continue => StopReasonCode::PlannerRejectedNonRetryable,
                        TransitionDecision::Terminal { code, .. } => code,
                    },
                    format!("[{}] {}", rejected.code, rejected.message),
                )
                .with_detail(serde_json::json!({
                    "planner_code": rejected.code,
                    "retryable": rejected.retryable,
                    "version": rejected.version,
                    "validation_errors": rejected.validation_errors,
                }));

                let attempt_summary = AutonomyExecutionAttemptSummary {
                    attempt,
                    max_attempts,
                    planner_status: "failed".to_string(),
                    action_count: 0,
                    succeeded_actions: 0,
                    action_results: Vec::new(),
                    stop_reason: if matches!(terminal, TransitionDecision::Continue) {
                        None
                    } else {
                        Some(stop_reason.clone())
                    },
                };

                match terminal {
                    TransitionDecision::Continue => AttemptResolution {
                        attempt: attempt_summary,
                        version: rejected.version.clone(),
                        note: Some(format!(
                            "Attempt {}/{}: planner rejected goal and is retryable; replanning.",
                            attempt, max_attempts
                        )),
                        transition: AttemptTransition::Continue,
                    },
                    TransitionDecision::Terminal { code, status } => AttemptResolution {
                        attempt: attempt_summary,
                        version: rejected.version.clone(),
                        note: None,
                        transition: AttemptTransition::Terminal {
                            status,
                            stop_reason: StopReason::new(code, rejected.message),
                            run_status_note: "Autonomous planner rejected current goal."
                                .to_string(),
                        },
                    },
                }
            }
            PlannerOutcome::Accepted(accepted) => {
                let execution_outcome = {
                    let mut service = state.service.lock().await;
                    if let Err(err) = service.load_program(ProgramId(program_id)) {
                        return AttemptResolution {
                            attempt: AutonomyExecutionAttemptSummary {
                                attempt,
                                max_attempts,
                                planner_status: "accepted".to_string(),
                                action_count: 0,
                                succeeded_actions: 0,
                                action_results: Vec::new(),
                                stop_reason: Some(stop_reason_from_api_error(
                                    StopReasonCode::RunnerInternalError,
                                    "failed to load active program for planner execution",
                                    err,
                                )),
                            },
                            version: Some(accepted.version),
                            note: None,
                            transition: AttemptTransition::Terminal {
                                status: AutonomyExecutionStatus::Failed,
                                stop_reason: StopReason::new(
                                    StopReasonCode::RunnerInternalError,
                                    "Failed to load active program before execution.",
                                ),
                                run_status_note: "Autonomous run stopped due to internal error."
                                    .to_string(),
                            },
                        };
                    }
                    execute_plan(&mut service, &accepted.envelope)
                };

                let mut attempt_summary = execution_outcome.attempts.first().cloned().unwrap_or(
                    AutonomyExecutionAttemptSummary {
                        attempt,
                        max_attempts,
                        planner_status: "accepted".to_string(),
                        action_count: accepted.action_count,
                        succeeded_actions: 0,
                        action_results: Vec::new(),
                        stop_reason: None,
                    },
                );
                attempt_summary.attempt = attempt;
                attempt_summary.max_attempts = max_attempts;
                attempt_summary.planner_status = "accepted".to_string();

                if execution_outcome.status == AutonomyExecutionStatus::Failed {
                    let retryable = matches!(
                        execution_outcome.stop_reason.code,
                        StopReasonCode::ActionFailedRetryable
                    );
                    let decision = decide_transition(
                        AttemptEvent::ActionFailed { retryable },
                        attempt,
                        max_attempts,
                    );

                    match decision {
                        TransitionDecision::Continue => {
                            attempt_summary.stop_reason =
                                Some(execution_outcome.stop_reason.clone());
                            AttemptResolution {
                                attempt: attempt_summary,
                                version: Some(accepted.version),
                                note: Some(format!(
                                    "Attempt {}/{} action failure is retryable; replanning.",
                                    attempt, max_attempts
                                )),
                                transition: AttemptTransition::Continue,
                            }
                        }
                        TransitionDecision::Terminal { code, status } => {
                            let terminal_reason = if code == StopReasonCode::RetryBudgetExhausted {
                                StopReason::new(
                                    code,
                                    format!(
                                        "retry budget exhausted after action failure: {}",
                                        execution_outcome.stop_reason.message
                                    ),
                                )
                                .with_detail(serde_json::json!({
                                    "last_stop_reason": execution_outcome.stop_reason,
                                    "attempt": attempt,
                                    "max_attempts": max_attempts,
                                }))
                            } else {
                                execution_outcome.stop_reason.clone()
                            };
                            attempt_summary.stop_reason = Some(terminal_reason.clone());
                            AttemptResolution {
                                attempt: attempt_summary,
                                version: Some(accepted.version),
                                note: None,
                                transition: AttemptTransition::Terminal {
                                    status,
                                    stop_reason: terminal_reason,
                                    run_status_note: "Autonomous execution failed.".to_string(),
                                },
                            }
                        }
                    }
                } else {
                    match self.run_verify_gate(state, program_id).await {
                        Ok(verify) if verify.valid => {
                            let verify_result = AutonomyActionExecutionResult::succeeded(
                                attempt_summary.action_results.len(),
                                "verify_gate",
                                "post-execution verify passed",
                            )
                            .with_detail(
                                serde_json::to_value(&verify).unwrap_or(serde_json::Value::Null),
                            );
                            attempt_summary.action_results.push(verify_result);
                            attempt_summary.action_count += 1;
                            attempt_summary.succeeded_actions += 1;

                            let decision =
                                decide_transition(AttemptEvent::Success, attempt, max_attempts);
                            let (status, code) = match decision {
                                TransitionDecision::Continue => (
                                    AutonomyExecutionStatus::Succeeded,
                                    StopReasonCode::Completed,
                                ),
                                TransitionDecision::Terminal { status, code } => (status, code),
                            };
                            let stop_reason =
                                StopReason::new(code, "autonomous execution completed");
                            attempt_summary.stop_reason = Some(stop_reason.clone());

                            AttemptResolution {
                                attempt: attempt_summary,
                                version: Some(accepted.version),
                                note: None,
                                transition: AttemptTransition::Terminal {
                                    status,
                                    stop_reason,
                                    run_status_note: "Autonomous loop completed successfully."
                                        .to_string(),
                                },
                            }
                        }
                        Ok(verify) => {
                            let diagnostic_messages = verify
                                .errors
                                .iter()
                                .take(3)
                                .map(|error| format!("[{}] {}", error.code, error.message))
                                .collect::<Vec<_>>();
                            let diagnostics = AutonomyDiagnostics::new(
                                AutonomyDiagnosticsClass::VerifyFailure,
                                true,
                                format!(
                                    "verify gate reported {} diagnostic(s)",
                                    verify.errors.len()
                                ),
                            )
                            .with_messages(diagnostic_messages)
                            .with_detail(serde_json::json!({
                                "error_count": verify.errors.len(),
                                "warning_count": verify.warnings.len(),
                            }));
                            let verify_error = AutonomyExecutionError::new(
                                AutonomyExecutionErrorCode::ValidationFailed,
                                "post-execution verify failed",
                                true,
                            )
                            .with_details(
                                serde_json::to_value(&verify.errors)
                                    .unwrap_or(serde_json::Value::Null),
                            )
                            .with_diagnostics(diagnostics.clone());
                            let verify_result = AutonomyActionExecutionResult::failed(
                                attempt_summary.action_results.len(),
                                "verify_gate",
                                format!(
                                    "post-execution verify failed with {} diagnostic(s)",
                                    verify.errors.len()
                                ),
                                verify_error,
                            )
                            .with_detail(
                                serde_json::to_value(&verify).unwrap_or(serde_json::Value::Null),
                            )
                            .with_diagnostics(diagnostics);
                            attempt_summary.action_results.push(verify_result);
                            attempt_summary.action_count += 1;

                            let reason = StopReason::new(
                                StopReasonCode::VerifyFailed,
                                "post-execution verify gate failed",
                            )
                            .with_detail(
                                serde_json::to_value(&verify).unwrap_or(serde_json::Value::Null),
                            );
                            let decision = decide_transition(
                                AttemptEvent::VerifyFailed,
                                attempt,
                                max_attempts,
                            );

                            match decision {
                                TransitionDecision::Continue => {
                                    attempt_summary.stop_reason = Some(reason);
                                    AttemptResolution {
                                        attempt: attempt_summary,
                                        version: Some(accepted.version),
                                        note: Some(format!(
                                            "Attempt {}/{} verify gate failed; replanning.",
                                            attempt, max_attempts
                                        )),
                                        transition: AttemptTransition::Continue,
                                    }
                                }
                                TransitionDecision::Terminal { status, code } => {
                                    let stop_reason = StopReason::new(
                                        code,
                                        format!(
                                            "retry budget exhausted after verify gate failure (attempt {}/{})",
                                            attempt, max_attempts
                                        ),
                                    );
                                    attempt_summary.stop_reason = Some(stop_reason.clone());
                                    AttemptResolution {
                                        attempt: attempt_summary,
                                        version: Some(accepted.version),
                                        note: None,
                                        transition: AttemptTransition::Terminal {
                                            status,
                                            stop_reason,
                                            run_status_note:
                                                "Autonomous run stopped after verify failures."
                                                    .to_string(),
                                        },
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            let stop_reason = stop_reason_from_api_error(
                                StopReasonCode::RunnerInternalError,
                                "post-execution verify gate failed to run",
                                err,
                            );
                            attempt_summary.stop_reason = Some(stop_reason.clone());
                            AttemptResolution {
                                attempt: attempt_summary,
                                version: Some(accepted.version),
                                note: None,
                                transition: AttemptTransition::Terminal {
                                    status: AutonomyExecutionStatus::Failed,
                                    stop_reason,
                                    run_status_note:
                                        "Autonomous run stopped due to verify gate error."
                                            .to_string(),
                                },
                            }
                        }
                    }
                }
            }
        }
    }

    async fn run_verify_gate(
        &self,
        state: &AppState,
        program_id: i64,
    ) -> Result<crate::schema::verify::VerifyResponse, crate::error::ApiError> {
        let mut service = state.service.lock().await;
        service.load_program(ProgramId(program_id))?;
        service.verify(VerifyScope::Full, None)
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_with_outcome(
        &self,
        state: &AppState,
        program_id: i64,
        agent_id: AgentId,
        goal: &str,
        version: String,
        attempts: Vec<AutonomyExecutionAttemptSummary>,
        status: AutonomyExecutionStatus,
        stop_reason: StopReason,
        run_status_note: String,
    ) {
        let outcome = AutonomyExecutionOutcome {
            goal: goal.to_string(),
            version,
            status,
            attempts,
            stop_reason,
        };
        let _ = state
            .project_agent_manager
            .set_execution_outcome(program_id, agent_id, outcome)
            .await;
        let _ = state
            .project_agent_manager
            .set_run_status(
                program_id,
                agent_id,
                if status == AutonomyExecutionStatus::Succeeded {
                    "idle"
                } else {
                    "stopped"
                },
                Some(run_status_note),
            )
            .await;
    }

    async fn decide_next_step(
        &self,
        state: &AppState,
        agent_id: AgentId,
        session: &ProjectAgentSession,
        goal: &str,
    ) -> StepDecision {
        if let Some(command) = deterministic_hello_world_step(goal, session) {
            return command;
        }

        let Some(agent) = state.agent_registry.get(&agent_id) else {
            return StepDecision::Noop;
        };
        if !agent.llm.is_configured() {
            return StepDecision::Noop;
        }

        let transcript = session
            .transcript
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>();

        StepDecision::Planner(plan_for_prompt(&agent.llm, goal, &transcript).await)
    }
}

impl Default for AutonomousRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::large_enum_variant)]
enum StepDecision {
    Command(String),
    Planner(PlannerOutcome),
    Done,
    Noop,
}

enum AttemptTransition {
    Continue,
    Terminal {
        status: AutonomyExecutionStatus,
        stop_reason: StopReason,
        run_status_note: String,
    },
}

struct AttemptResolution {
    attempt: AutonomyExecutionAttemptSummary,
    version: Option<String>,
    note: Option<String>,
    transition: AttemptTransition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptEvent {
    Success,
    PlannerRejected { retryable: bool },
    ActionFailed { retryable: bool },
    VerifyFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionDecision {
    Continue,
    Terminal {
        code: StopReasonCode,
        status: AutonomyExecutionStatus,
    },
}

fn configured_max_attempts() -> u32 {
    std::env::var(MAX_ATTEMPTS_ENV)
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_ATTEMPTS)
}

fn decide_transition(event: AttemptEvent, attempt: u32, max_attempts: u32) -> TransitionDecision {
    match event {
        AttemptEvent::Success => TransitionDecision::Terminal {
            code: StopReasonCode::Completed,
            status: AutonomyExecutionStatus::Succeeded,
        },
        AttemptEvent::PlannerRejected { retryable } => {
            if !retryable {
                TransitionDecision::Terminal {
                    code: StopReasonCode::PlannerRejectedNonRetryable,
                    status: AutonomyExecutionStatus::Failed,
                }
            } else if attempt < max_attempts {
                TransitionDecision::Continue
            } else {
                TransitionDecision::Terminal {
                    code: StopReasonCode::PlannerRejectedRetryBudgetExhausted,
                    status: AutonomyExecutionStatus::Failed,
                }
            }
        }
        AttemptEvent::ActionFailed { retryable } => {
            if !retryable {
                TransitionDecision::Terminal {
                    code: StopReasonCode::ActionFailedNonRetryable,
                    status: AutonomyExecutionStatus::Failed,
                }
            } else if attempt < max_attempts {
                TransitionDecision::Continue
            } else {
                TransitionDecision::Terminal {
                    code: StopReasonCode::RetryBudgetExhausted,
                    status: AutonomyExecutionStatus::Failed,
                }
            }
        }
        AttemptEvent::VerifyFailed => {
            if attempt < max_attempts {
                TransitionDecision::Continue
            } else {
                TransitionDecision::Terminal {
                    code: StopReasonCode::RetryBudgetExhausted,
                    status: AutonomyExecutionStatus::Failed,
                }
            }
        }
    }
}

fn stop_reason_from_api_error(
    code: StopReasonCode,
    context: &str,
    err: crate::error::ApiError,
) -> StopReason {
    StopReason::new(code, format!("{}: {}", context, err))
}

fn deterministic_hello_world_step(
    goal: &str,
    session: &ProjectAgentSession,
) -> Option<StepDecision> {
    let goal_lower = goal.to_ascii_lowercase();
    if !goal_lower.contains("hello world") {
        return None;
    }

    let transcript_text = session
        .transcript
        .iter()
        .map(|m| m.content.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");

    let has_scaffold = transcript_text.contains("hello world scaffold ready");
    let has_compile = transcript_text.contains("compiled hello_world");
    let has_run = transcript_text.contains("program executed");

    let wants_run = goal_lower.contains("run")
        || goal_lower.contains("execute")
        || goal_lower.contains("bootstrap");
    let wants_compile = wants_run || goal_lower.contains("compile");

    if !has_scaffold {
        return Some(StepDecision::Command(
            "create hello world program".to_string(),
        ));
    }
    if wants_compile && !has_compile {
        return Some(StepDecision::Command("compile program".to_string()));
    }
    if wants_run && !has_run {
        return Some(StepDecision::Command("run program".to_string()));
    }

    Some(StepDecision::Done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_success_terminates_with_completed_code() {
        assert_eq!(
            decide_transition(AttemptEvent::Success, 1, 3),
            TransitionDecision::Terminal {
                code: StopReasonCode::Completed,
                status: AutonomyExecutionStatus::Succeeded,
            }
        );
    }

    #[test]
    fn transition_retryable_planner_rejection_continues_before_budget_exhaustion() {
        assert_eq!(
            decide_transition(AttemptEvent::PlannerRejected { retryable: true }, 1, 3),
            TransitionDecision::Continue
        );
    }

    #[test]
    fn transition_retryable_planner_rejection_exhausts_budget_with_stable_code() {
        assert_eq!(
            decide_transition(AttemptEvent::PlannerRejected { retryable: true }, 3, 3),
            TransitionDecision::Terminal {
                code: StopReasonCode::PlannerRejectedRetryBudgetExhausted,
                status: AutonomyExecutionStatus::Failed,
            }
        );
    }

    #[test]
    fn transition_non_retryable_planner_rejection_is_terminal() {
        assert_eq!(
            decide_transition(AttemptEvent::PlannerRejected { retryable: false }, 1, 3),
            TransitionDecision::Terminal {
                code: StopReasonCode::PlannerRejectedNonRetryable,
                status: AutonomyExecutionStatus::Failed,
            }
        );
    }

    #[test]
    fn transition_retryable_action_failure_eventually_exhausts_budget() {
        assert_eq!(
            decide_transition(AttemptEvent::ActionFailed { retryable: true }, 2, 3),
            TransitionDecision::Continue
        );
        assert_eq!(
            decide_transition(AttemptEvent::ActionFailed { retryable: true }, 3, 3),
            TransitionDecision::Terminal {
                code: StopReasonCode::RetryBudgetExhausted,
                status: AutonomyExecutionStatus::Failed,
            }
        );
    }

    #[test]
    fn transition_non_retryable_action_failure_is_terminal() {
        assert_eq!(
            decide_transition(AttemptEvent::ActionFailed { retryable: false }, 1, 3),
            TransitionDecision::Terminal {
                code: StopReasonCode::ActionFailedNonRetryable,
                status: AutonomyExecutionStatus::Failed,
            }
        );
    }
}
