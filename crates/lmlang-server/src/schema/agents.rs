//! Schema types for agent registration API.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public view of model-provider settings for an agent.
#[derive(Debug, Clone, Serialize)]
pub struct AgentLlmConfigView {
    /// Provider key (for example `openrouter`).
    pub provider: Option<String>,
    /// Model identifier (for example `openai/gpt-4o-mini`).
    pub model: Option<String>,
    /// Base URL for OpenAI-compatible chat completions.
    pub api_base_url: Option<String>,
    /// Optional system prompt prepended to chat calls.
    pub system_prompt: Option<String>,
    /// Whether an API key is currently stored for this agent.
    pub api_key_configured: bool,
}

/// Request to register a new agent.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterAgentRequest {
    /// Optional human-readable agent name.
    pub name: Option<String>,
    /// Optional LLM provider key (`openrouter`, `openai_compatible`).
    pub provider: Option<String>,
    /// Optional model name for external chat.
    pub model: Option<String>,
    /// Optional API base URL.
    pub api_base_url: Option<String>,
    /// Optional API key. Persisted in SQLite and never returned in responses.
    pub api_key: Option<String>,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
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
    /// LLM provider configuration (API key redacted).
    pub llm: AgentLlmConfigView,
}

/// View of an agent for listing.
#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    /// The agent's UUID.
    pub agent_id: Uuid,
    /// The agent's name, if any.
    pub name: Option<String>,
    /// LLM provider configuration (API key redacted).
    pub llm: AgentLlmConfigView,
}

/// Response listing all active agents.
#[derive(Debug, Clone, Serialize)]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentView>,
}

/// Agent detail response.
#[derive(Debug, Clone, Serialize)]
pub struct AgentDetailResponse {
    pub agent: AgentView,
}

/// Request to update model-provider config on an existing agent.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAgentConfigRequest {
    /// Optional provider key (`openrouter`, `openai_compatible`).
    pub provider: Option<String>,
    /// Optional model identifier.
    pub model: Option<String>,
    /// Optional API base URL.
    pub api_base_url: Option<String>,
    /// Optional API key. Empty string clears.
    pub api_key: Option<String>,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
}

/// Response after updating agent config.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateAgentConfigResponse {
    pub success: bool,
    pub agent: AgentView,
}
