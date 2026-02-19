//! Background autonomous run loop for assigned project agents.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::task::JoinHandle;

use crate::concurrency::AgentId;
use crate::handlers::agent_control::maybe_execute_agent_chat_command;
use crate::llm_provider::run_external_chat;
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
                StepDecision::Clarification(question) => {
                    let assumption = default_assumption(&goal, &question);
                    let _ = state
                        .project_agent_manager
                        .append_message(
                            program_id,
                            agent_id,
                            "assistant",
                            format!("Autonomous clarification requested: {}", question),
                        )
                        .await;
                    let _ = state
                        .project_agent_manager
                        .append_message(
                            program_id,
                            agent_id,
                            "system",
                            format!(
                                "Autonomous assumption applied (no operator chat turn required): {}",
                                assumption
                            ),
                        )
                        .await;
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
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are running as an autonomous background build agent.\n\
             Goal: {}\n\
             Recent transcript:\n{}\n\
             Return exactly one line in one of these forms:\n\
             - create hello world program\n\
             - compile program\n\
             - run program\n\
             - done\n\
             - clarify: <single blocking question>\n\
             Prefer assumptions and progress over asking questions.",
            goal, transcript
        );

        let first = match run_external_chat(&agent.llm, &prompt).await {
            Ok(text) => text,
            Err(_) => return StepDecision::Noop,
        };

        match parse_step_decision(&first) {
            StepDecision::Clarification(question) => {
                let assumption = default_assumption(goal, &question);
                let followup = format!(
                    "Use this assumption and continue without operator intervention: {}\n\
                     Return one line command now.",
                    assumption
                );
                match run_external_chat(&agent.llm, &followup).await {
                    Ok(second) => parse_step_decision(&second),
                    Err(_) => StepDecision::Clarification(question),
                }
            }
            other => other,
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
    Clarification(String),
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

fn parse_step_decision(raw: &str) -> StepDecision {
    let text = raw.trim();
    if text.is_empty() {
        return StepDecision::Noop;
    }
    let lower = text.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("clarify:") {
        let question = rest.trim();
        if !question.is_empty() {
            return StepDecision::Clarification(question.to_string());
        }
    }
    if text.ends_with('?') {
        return StepDecision::Clarification(text.to_string());
    }
    if lower.contains("create hello world") {
        return StepDecision::Command("create hello world program".to_string());
    }
    if lower.contains("compile") {
        return StepDecision::Command("compile program".to_string());
    }
    if lower.contains("run") || lower.contains("execute") {
        return StepDecision::Command("run program".to_string());
    }
    if lower.contains("done") || lower.contains("complete") {
        return StepDecision::Done;
    }

    StepDecision::Noop
}

fn default_assumption(goal: &str, question: &str) -> String {
    format!(
        "For goal '{}', assume minimal safe defaults and proceed without waiting. Blocking question was: {}",
        goal, question
    )
}
