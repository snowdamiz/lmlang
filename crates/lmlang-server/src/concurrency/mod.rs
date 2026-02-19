//! Multi-agent concurrency infrastructure.
//!
//! Provides the core building blocks for concurrent multi-agent editing:
//! - [`agent::AgentRegistry`] for session management
//! - [`lock_manager::LockManager`] for per-function read-write locks
//! - [`conflict`] for hash-based conflict detection

pub mod agent;
pub mod conflict;
pub mod lock_manager;

pub use agent::{AgentId, AgentRegistry, AgentSession};
pub use conflict::{check_hashes, build_function_diff, ConflictDetail, FunctionDiff};
pub use lock_manager::{
    FunctionLockState, LockDenial, LockError, LockGrant, LockHolderInfo, LockManager, LockMode,
    LockStatusEntry,
};

use uuid::Uuid;

use crate::error::ApiError;

/// Extracts the agent ID from the `X-Agent-Id` HTTP header.
///
/// Returns `ApiError::AgentRequired` if the header is missing or malformed.
/// Used by lock and agent-aware handlers in Plan 02.
pub fn extract_agent_id(headers: &axum::http::HeaderMap) -> Result<AgentId, ApiError> {
    headers
        .get("X-Agent-Id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(AgentId)
        .ok_or_else(|| ApiError::AgentRequired("X-Agent-Id header required".to_string()))
}
