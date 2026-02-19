//! Background autonomous run loop for assigned project agents.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::task::JoinHandle;

use crate::autonomy_planner::{plan_for_prompt, PlannerOutcome};
use crate::concurrency::AgentId;
use crate::handlers::agent_control::maybe_execute_agent_chat_command;
use crate::project_agent::ProjectAgentSession;
use crate::state::AppState;

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
        let mut failures = 0u8;

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
                            failures = 0;
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
                            failures = failures.saturating_add(1);
                            let _ = state
                                .project_agent_manager
                                .append_message(
                                    program_id,
                                    agent_id,
                                    "system",
                                    format!("Autonomous step `{}` failed: {}", command, err),
                                )
                                .await;
                            if failures >= 3 {
                                let _ = state
                                    .project_agent_manager
                                    .set_run_status(
                                        program_id,
                                        agent_id,
                                        "stopped",
                                        Some(
                                            "Autonomous loop stopped after repeated failures."
                                                .to_string(),
                                        ),
                                    )
                                    .await;
                                break;
                            }
                        }
                    }
                }
                StepDecision::PlannerAccepted(summary) => {
                    let _ = state
                        .project_agent_manager
                        .append_message(
                            program_id,
                            agent_id,
                            "assistant",
                            format!("Autonomous planner accepted structured plan: {}", summary),
                        )
                        .await;
                    let _ = state
                        .project_agent_manager
                        .set_run_status(
                            program_id,
                            agent_id,
                            "idle",
                            Some(
                                "Autonomous planner produced structured plan. Execution awaits generic planner action executor."
                                    .to_string(),
                            ),
                        )
                        .await;
                    break;
                }
                StepDecision::PlannerRejected(reason) => {
                    failures = failures.saturating_add(1);
                    let _ = state
                        .project_agent_manager
                        .append_message(
                            program_id,
                            agent_id,
                            "system",
                            format!("Autonomous planner rejected current goal: {}", reason),
                        )
                        .await;
                    if failures >= 3 {
                        let _ = state
                            .project_agent_manager
                            .set_run_status(
                                program_id,
                                agent_id,
                                "stopped",
                                Some(
                                    "Autonomous loop stopped after repeated planner rejections."
                                        .to_string(),
                                ),
                            )
                            .await;
                        break;
                    }
                }
                StepDecision::Done => {
                    let _ = state
                        .project_agent_manager
                        .set_run_status(
                            program_id,
                            agent_id,
                            "idle",
                            Some("Autonomous run reached done state.".to_string()),
                        )
                        .await;
                    break;
                }
                StepDecision::Noop => {}
            }
        }
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

        match plan_for_prompt(&agent.llm, goal, &transcript).await {
            PlannerOutcome::Accepted(accepted) => StepDecision::PlannerAccepted(format!(
                "{} action(s) validated for '{}': {}",
                accepted.action_count,
                accepted.goal,
                accepted
                    .actions
                    .iter()
                    .map(|action| format!("{} ({})", action.kind, action.summary))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
            PlannerOutcome::Rejected(rejected) => {
                StepDecision::PlannerRejected(format!("[{}] {}", rejected.code, rejected.message))
            }
        }
    }
}

impl Default for AutonomousRunner {
    fn default() -> Self {
        Self::new()
    }
}

enum StepDecision {
    Command(String),
    PlannerAccepted(String),
    PlannerRejected(String),
    Done,
    Noop,
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
