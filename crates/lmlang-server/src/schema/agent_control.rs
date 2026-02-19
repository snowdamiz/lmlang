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
}

/// Detailed session view for one assigned agent.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramAgentDetailResponse {
    pub session: ProgramAgentSessionView,
    pub transcript: Vec<AgentChatMessageView>,
}
