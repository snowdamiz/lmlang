//! Schema types for project-scoped agent control and chat.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single chat message exchanged with a project agent.
#[derive(Debug, Clone, Serialize)]
pub struct AgentChatMessageView {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// Summary state for one agent assigned to a project.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramAgentSessionView {
    pub program_id: i64,
    pub agent_id: Uuid,
    pub name: Option<String>,
    pub run_status: String,
    pub active_goal: Option<String>,
    pub assigned_at: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub updated_at: String,
    pub message_count: usize,
    /// Machine-readable terminal stop reason (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<ExecutionStopReasonView>,
    /// Compact summary of latest autonomous execution attempt metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionSummaryView>,
}

/// Lists all agents assigned to a project.
#[derive(Debug, Clone, Serialize)]
pub struct ListProgramAgentsResponse {
    pub program_id: i64,
    pub agents: Vec<ProgramAgentSessionView>,
}

/// Shared response wrapper for assignment/start/stop operations.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramAgentActionResponse {
    pub success: bool,
    pub session: ProgramAgentSessionView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
}

/// Request body to start a build run for an assigned agent.
#[derive(Debug, Clone, Deserialize)]
pub struct StartProgramAgentRequest {
    pub goal: String,
}

/// Request body to stop a running build for an assigned agent.
#[derive(Debug, Clone, Deserialize)]
pub struct StopProgramAgentRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

/// Request body for chatting with an assigned agent.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatWithProgramAgentRequest {
    pub message: String,
}

/// Response body for chat operations.
#[derive(Debug, Clone, Serialize)]
pub struct ChatWithProgramAgentResponse {
    pub success: bool,
    pub session: ProgramAgentSessionView,
    pub reply: String,
    pub transcript: Vec<AgentChatMessageView>,
    /// Planner outcome metadata for non-command prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner: Option<PlannerOutcomeView>,
    /// Latest autonomous execution metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionSummaryView>,
}

/// Detailed session view for one assigned agent.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramAgentDetailResponse {
    pub session: ProgramAgentSessionView,
    pub transcript: Vec<AgentChatMessageView>,
}

/// Structured stop reason metadata projected for operators.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionStopReasonView {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Compact diagnostics metadata projected for operator triage.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionDiagnosticsView {
    pub class: String,
    pub retryable: bool,
    pub summary: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub key_diagnostics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// Compact action result row from latest autonomous attempt.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionActionView {
    pub action_index: usize,
    pub kind: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<ExecutionDiagnosticsView>,
}

/// Compact attempt-level execution evidence.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummaryView {
    pub attempt: u32,
    pub max_attempts: u32,
    pub planner_status: String,
    pub action_count: usize,
    pub succeeded_actions: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ExecutionActionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<ExecutionDiagnosticsView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<ExecutionStopReasonView>,
}

/// Structured planner outcome for chat responses.
#[derive(Debug, Clone, Serialize)]
pub struct PlannerOutcomeView {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PlannerActionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<PlannerFailureView>,
}

/// Planner action summary shown in API responses.
#[derive(Debug, Clone, Serialize)]
pub struct PlannerActionView {
    pub kind: String,
    pub summary: String,
}

/// Structured planner failure details.
#[derive(Debug, Clone, Serialize)]
pub struct PlannerFailureView {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub validation_errors: Vec<PlannerValidationErrorView>,
}

/// Planner validation error projection for response payloads.
#[derive(Debug, Clone, Serialize)]
pub struct PlannerValidationErrorView {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}
