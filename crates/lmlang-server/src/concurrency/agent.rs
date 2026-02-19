//! Agent session management for multi-agent concurrency.
//!
//! [`AgentRegistry`] tracks connected agents via UUID-based session identifiers.
//! Agents register to receive an [`AgentId`], which is required for lock
//! acquisition and mutation operations.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique agent identifier (UUID v4 newtype).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An active agent session with metadata.
#[derive(Debug, Clone)]
pub struct AgentSession {
    /// The agent's unique identifier.
    pub id: AgentId,
    /// Optional human-readable agent name.
    pub name: Option<String>,
    /// When the agent registered.
    pub registered_at: Instant,
    /// When the agent last performed an action.
    pub last_active: Instant,
}

/// Registry of active agent sessions.
///
/// Backed by `DashMap` for concurrent lock-free access from multiple
/// async handler tasks.
pub struct AgentRegistry {
    sessions: DashMap<AgentId, AgentSession>,
}

impl AgentRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        AgentRegistry {
            sessions: DashMap::new(),
        }
    }

    /// Registers a new agent, returning its assigned ID.
    pub fn register(&self, name: Option<String>) -> AgentId {
        let id = AgentId(Uuid::new_v4());
        let now = Instant::now();
        let session = AgentSession {
            id,
            name,
            registered_at: now,
            last_active: now,
        };
        self.sessions.insert(id, session);
        id
    }

    /// Removes an agent session. Returns `true` if the agent was registered.
    pub fn deregister(&self, id: &AgentId) -> bool {
        self.sessions.remove(id).is_some()
    }

    /// Returns a clone of the agent session, if it exists.
    pub fn get(&self, id: &AgentId) -> Option<AgentSession> {
        self.sessions.get(id).map(|entry| entry.clone())
    }

    /// Returns all active agent sessions.
    pub fn list(&self) -> Vec<AgentSession> {
        self.sessions.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Updates the `last_active` timestamp for an agent.
    pub fn touch(&self, id: &AgentId) {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            entry.last_active = Instant::now();
        }
    }

    /// Removes sessions that have been inactive longer than `timeout`.
    ///
    /// Returns the number of sessions removed.
    pub fn sweep_inactive(&self, timeout: Duration) -> usize {
        let now = Instant::now();
        let mut removed = 0;
        self.sessions.retain(|_, session| {
            let active = now.duration_since(session.last_active) < timeout;
            if !active {
                removed += 1;
            }
            active
        });
        removed
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
