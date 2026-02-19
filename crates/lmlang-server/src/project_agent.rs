//! In-memory project-scoped agent assignment, run state, and chat transcript store.

use std::collections::HashMap;

use tokio::sync::Mutex;

use crate::concurrency::AgentId;
use crate::schema::autonomy_execution::{
    AutonomyExecutionAttemptSummary, AutonomyExecutionOutcome, AutonomyExecutionStatus, StopReason,
    StopReasonCode,
};

/// Chat entry in a project-agent transcript.
#[derive(Debug, Clone)]
pub struct ProjectAgentMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// Runtime state for one `(program_id, agent_id)` assignment.
#[derive(Debug, Clone)]
pub struct ProjectAgentSession {
    pub program_id: i64,
    pub agent_id: AgentId,
    pub name: Option<String>,
    pub run_status: String,
    pub active_goal: Option<String>,
    pub assigned_at: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub updated_at: String,
    pub transcript: Vec<ProjectAgentMessage>,
    pub stop_reason: Option<StopReason>,
    pub execution: Option<AutonomyExecutionOutcome>,
    pub execution_attempts: Vec<AutonomyExecutionAttemptSummary>,
}

type ProgramAgentKey = (i64, AgentId);

/// Manages all project-agent sessions for the running server process.
pub struct ProjectAgentManager {
    sessions: Mutex<HashMap<ProgramAgentKey, ProjectAgentSession>>,
}

impl ProjectAgentManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn assign(
        &self,
        program_id: i64,
        agent_id: AgentId,
        name: Option<String>,
    ) -> ProjectAgentSession {
        let mut guard = self.sessions.lock().await;
        let now = now_string();
        let key = (program_id, agent_id);

        if let Some(existing) = guard.get_mut(&key) {
            existing.name = name;
            existing.updated_at = now;
            return existing.clone();
        }

        let session = ProjectAgentSession {
            program_id,
            agent_id,
            name,
            run_status: "idle".to_string(),
            active_goal: None,
            assigned_at: now.clone(),
            started_at: None,
            stopped_at: None,
            updated_at: now,
            transcript: Vec::new(),
            stop_reason: None,
            execution: None,
            execution_attempts: Vec::new(),
        };
        guard.insert(key, session.clone());
        session
    }

    pub async fn list_for_program(&self, program_id: i64) -> Vec<ProjectAgentSession> {
        let guard = self.sessions.lock().await;
        let mut sessions = guard
            .values()
            .filter(|s| s.program_id == program_id)
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        sessions.reverse();
        sessions
    }

    pub async fn get(&self, program_id: i64, agent_id: AgentId) -> Option<ProjectAgentSession> {
        let guard = self.sessions.lock().await;
        guard.get(&(program_id, agent_id)).cloned()
    }

    pub async fn start(
        &self,
        program_id: i64,
        agent_id: AgentId,
        goal: String,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        session.run_status = "running".to_string();
        session.active_goal = Some(goal.clone());
        session.started_at = Some(now.clone());
        session.stopped_at = None;
        session.stop_reason = None;
        session.execution = None;
        session.execution_attempts.clear();
        session.updated_at = now.clone();
        session.transcript.push(ProjectAgentMessage {
            role: "system".to_string(),
            content: format!("Build started: {}", goal),
            timestamp: now,
        });
        Ok(session.clone())
    }

    pub async fn stop(
        &self,
        program_id: i64,
        agent_id: AgentId,
        reason: Option<String>,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        let detail = reason.unwrap_or_else(|| "no reason provided".to_string());
        session.run_status = "stopped".to_string();
        session.stopped_at = Some(now.clone());
        session.stop_reason = Some(StopReason::new(
            StopReasonCode::OperatorStopped,
            detail.clone(),
        ));
        session.updated_at = now.clone();
        session.transcript.push(ProjectAgentMessage {
            role: "system".to_string(),
            content: format!("Build stopped: {}", detail),
            timestamp: now,
        });
        Ok(session.clone())
    }

    pub async fn set_run_status(
        &self,
        program_id: i64,
        agent_id: AgentId,
        run_status: &str,
        note: Option<String>,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        session.run_status = run_status.to_string();
        if run_status == "stopped" {
            session.stopped_at = Some(now.clone());
        } else if run_status == "running" {
            session.stopped_at = None;
            session.stop_reason = None;
            if session.started_at.is_none() {
                session.started_at = Some(now.clone());
            }
        }
        session.updated_at = now.clone();

        if let Some(note) = note {
            session.transcript.push(ProjectAgentMessage {
                role: "system".to_string(),
                content: note,
                timestamp: now,
            });
        }

        Ok(session.clone())
    }

    pub async fn append_message(
        &self,
        program_id: i64,
        agent_id: AgentId,
        role: &str,
        content: String,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        session.transcript.push(ProjectAgentMessage {
            role: role.to_string(),
            content,
            timestamp: now.clone(),
        });
        session.updated_at = now;
        Ok(session.clone())
    }

    pub async fn chat(
        &self,
        program_id: i64,
        agent_id: AgentId,
        user_message: String,
        action_note: Option<String>,
        assistant_override: Option<String>,
    ) -> Result<(ProjectAgentSession, String), String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        session.transcript.push(ProjectAgentMessage {
            role: "user".to_string(),
            content: user_message.clone(),
            timestamp: now.clone(),
        });

        let base_reply = if session.run_status == "running" {
            match session.active_goal.as_deref() {
                Some(goal) => format!(
                    "Acknowledged. Continuing work on '{}'. Next step: inspect graph state and apply a safe mutation batch.",
                    goal
                ),
                None => {
                    "Acknowledged. Build is running; collecting context before proposing edits."
                        .to_string()
                }
            }
        } else if session.run_status == "stopped" {
            "Agent is stopped. Start the build run before requesting new work.".to_string()
        } else {
            "Agent is idle. Start a run to begin building, then continue chat for iteration."
                .to_string()
        };

        let reply = match assistant_override {
            Some(custom) => custom,
            None => match action_note {
                Some(note) => format!("{}\n\nAction result: {}", base_reply, note),
                None => base_reply,
            },
        };

        let reply_ts = now_string();
        session.transcript.push(ProjectAgentMessage {
            role: "assistant".to_string(),
            content: reply.clone(),
            timestamp: reply_ts.clone(),
        });
        session.updated_at = reply_ts;
        Ok((session.clone(), reply))
    }

    pub async fn append_execution_attempt(
        &self,
        program_id: i64,
        agent_id: AgentId,
        attempt: AutonomyExecutionAttemptSummary,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        session.execution_attempts.push(attempt.clone());
        session.updated_at = now.clone();
        session.transcript.push(ProjectAgentMessage {
            role: "system".to_string(),
            content: format!(
                "Execution attempt {}/{} recorded ({} action(s), {} succeeded).",
                attempt.attempt,
                attempt.max_attempts,
                attempt.action_count,
                attempt.succeeded_actions
            ),
            timestamp: now,
        });
        Ok(session.clone())
    }

    pub async fn set_execution_outcome(
        &self,
        program_id: i64,
        agent_id: AgentId,
        outcome: AutonomyExecutionOutcome,
    ) -> Result<ProjectAgentSession, String> {
        let mut guard = self.sessions.lock().await;
        let key = (program_id, agent_id);
        let session = guard
            .get_mut(&key)
            .ok_or_else(|| "agent is not assigned to this project".to_string())?;

        let now = now_string();
        let stop_reason = outcome.stop_reason.clone();
        let status = outcome.status;
        session.stop_reason = Some(stop_reason.clone());
        session.execution_attempts = outcome.attempts.clone();
        session.execution = Some(outcome);
        if status == AutonomyExecutionStatus::Failed {
            session.run_status = "stopped".to_string();
            session.stopped_at = Some(now.clone());
        }
        session.updated_at = now.clone();
        session.transcript.push(ProjectAgentMessage {
            role: "system".to_string(),
            content: format!(
                "Autonomous execution {:?}: [{}] {}",
                status,
                stop_reason_code_string(stop_reason.code),
                stop_reason.message
            ),
            timestamp: now,
        });
        Ok(session.clone())
    }
}

impl Default for ProjectAgentManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_string() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

fn stop_reason_code_string(code: StopReasonCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::schema::autonomy_execution::{AutonomyExecutionStatus, StopReasonCode};

    #[tokio::test]
    async fn execution_evidence_can_be_recorded_on_session() {
        let manager = ProjectAgentManager::new();
        let program_id = 42;
        let agent_id = AgentId(Uuid::new_v4());
        manager
            .assign(program_id, agent_id, Some("runner".to_string()))
            .await;
        manager
            .start(program_id, agent_id, "build calculator".to_string())
            .await
            .expect("start");

        let attempt = AutonomyExecutionAttemptSummary {
            attempt: 1,
            max_attempts: 3,
            planner_status: "accepted".to_string(),
            action_count: 2,
            succeeded_actions: 1,
            action_results: vec![
                crate::schema::autonomy_execution::AutonomyActionExecutionResult::succeeded(
                    0,
                    "mutate_batch",
                    "applied 1 mutation",
                ),
            ],
            stop_reason: None,
        };
        let session = manager
            .append_execution_attempt(program_id, agent_id, attempt)
            .await
            .expect("append attempt");
        assert_eq!(session.execution_attempts.len(), 1);
        assert!(session.stop_reason.is_none());

        let stop_reason = StopReason::new(StopReasonCode::Completed, "all actions complete");
        let outcome = AutonomyExecutionOutcome::from_single_attempt(
            "build calculator",
            "2026-02-19",
            AutonomyExecutionStatus::Succeeded,
            session.execution_attempts[0]
                .clone()
                .with_stop_reason(stop_reason.clone()),
            stop_reason,
        );
        let session = manager
            .set_execution_outcome(program_id, agent_id, outcome)
            .await
            .expect("set outcome");
        assert_eq!(
            session.stop_reason.as_ref().map(|reason| reason.code),
            Some(StopReasonCode::Completed)
        );
        assert!(session.execution.is_some());
        assert_eq!(session.execution_attempts.len(), 1);
    }

    #[tokio::test]
    async fn transcript_behavior_remains_compatible_with_execution_evidence() {
        let manager = ProjectAgentManager::new();
        let program_id = 7;
        let agent_id = AgentId(Uuid::new_v4());
        manager.assign(program_id, agent_id, None).await;

        let attempt = AutonomyExecutionAttemptSummary {
            attempt: 1,
            max_attempts: 1,
            planner_status: "accepted".to_string(),
            action_count: 1,
            succeeded_actions: 0,
            action_results: vec![
                crate::schema::autonomy_execution::AutonomyActionExecutionResult::failed(
                    0,
                    "verify",
                    "verification failed",
                    crate::schema::autonomy_execution::AutonomyExecutionError::new(
                        crate::schema::autonomy_execution::AutonomyExecutionErrorCode::ValidationFailed,
                        "invalid graph",
                        true,
                    ),
                ),
            ],
            stop_reason: Some(StopReason::new(
                StopReasonCode::VerifyFailed,
                "verify returned diagnostics",
            )),
        };
        manager
            .append_execution_attempt(program_id, agent_id, attempt)
            .await
            .expect("append attempt");
        manager
            .append_message(
                program_id,
                agent_id,
                "assistant",
                "continuing autonomous loop".to_string(),
            )
            .await
            .expect("append message");

        let session = manager
            .get(program_id, agent_id)
            .await
            .expect("session exists");
        assert!(session
            .transcript
            .iter()
            .any(|msg| { msg.role == "assistant" && msg.content == "continuing autonomous loop" }));
    }
}
