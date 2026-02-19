//! Schema types for dashboard AI orchestration chat.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::agent_control::AgentChatMessageView;

/// Request body for the dashboard AI chat endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct DashboardAiChatRequest {
    pub message: String,
    #[serde(default)]
    pub selected_program_id: Option<i64>,
    #[serde(default)]
    pub selected_agent_id: Option<Uuid>,
    #[serde(default)]
    pub selected_project_agent_id: Option<Uuid>,
}

/// Response body for the dashboard AI chat endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct DashboardAiChatResponse {
    pub success: bool,
    pub reply: String,
    pub selected_program_id: Option<i64>,
    pub selected_agent_id: Option<Uuid>,
    pub selected_project_agent_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<AgentChatMessageView>>,
}
