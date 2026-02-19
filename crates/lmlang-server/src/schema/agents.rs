//! Schema types for agent registration API.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Request to register a new agent.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterAgentRequest {
    /// Optional human-readable agent name.
    pub name: Option<String>,
}

/// Response after successful agent registration.
#[derive(Debug, Clone, Serialize)]
pub struct RegisterAgentResponse {
    /// The assigned agent UUID.
    pub agent_id: Uuid,
    /// The agent's name, if provided.
    pub name: Option<String>,
    /// ISO 8601 timestamp of registration.
    pub registered_at: String,
}

/// View of an agent for listing.
#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    /// The agent's UUID.
    pub agent_id: Uuid,
    /// The agent's name, if any.
    pub name: Option<String>,
}

/// Response listing all active agents.
#[derive(Debug, Clone, Serialize)]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentView>,
}
