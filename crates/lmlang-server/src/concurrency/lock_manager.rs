//! Per-function read-write lock manager for concurrent graph editing.
//!
//! [`LockManager`] provides function-level locking with TTL-based auto-expiry,
//! batch acquisition with all-or-nothing semantics, and a global write lock
//! for module structure changes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Serialize;

use lmlang_core::id::FunctionId;

use super::agent::AgentId;

/// Lock mode: read (shared) or write (exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockMode {
    Read,
    Write,
}

/// Information about a write lock holder.
#[derive(Debug, Clone)]
pub struct LockHolderInfo {
    /// Which agent holds the lock.
    pub agent_id: AgentId,
    /// Optional human-readable agent name.
    pub agent_name: Option<String>,
    /// Optional description of what the agent is doing.
    pub description: Option<String>,
    /// When the lock was acquired.
    pub acquired_at: Instant,
    /// When the lock expires automatically.
    pub expires_at: Instant,
}

/// Per-function lock state.
#[derive(Debug)]
pub enum FunctionLockState {
    /// No lock held.
    Unlocked,
    /// Shared read lock held by one or more agents.
    ReadLocked {
        readers: HashMap<AgentId, Instant>,
        expires_at: Instant,
    },
    /// Exclusive write lock held by one agent.
    WriteLocked {
        holder: LockHolderInfo,
        /// Agents that were denied the lock, in FIFO order.
        waiters: Vec<AgentId>,
    },
}

/// A successful lock acquisition.
#[derive(Debug, Clone, Serialize)]
pub struct LockGrant {
    pub function_id: FunctionId,
    pub mode: LockMode,
    pub expires_at: String,
}

/// A lock request that was denied.
#[derive(Debug, Clone, Serialize)]
pub struct LockDenial {
    pub function_id: FunctionId,
    pub holder_agent_id: AgentId,
    pub holder_name: Option<String>,
    pub holder_description: Option<String>,
    pub queue_position: usize,
}

/// Errors from lock operations.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Lock is already held by another agent.
    #[error("lock already held")]
    AlreadyHeldBy(LockDenial),

    /// Agent does not hold the requested lock.
    #[error("lock not held: function {function_id} by agent {agent_id}")]
    NotHeld {
        function_id: FunctionId,
        agent_id: AgentId,
    },

    /// Function does not exist in the graph.
    #[error("function not found: {0}")]
    FunctionNotFound(FunctionId),

    /// Batch acquire failed partway through.
    #[error("batch lock partially failed")]
    BatchPartialFailure {
        acquired: Vec<FunctionId>,
        failed: LockDenial,
    },
}

/// Status entry for a single function lock.
#[derive(Debug, Clone, Serialize)]
pub struct LockStatusEntry {
    pub function_id: FunctionId,
    pub state: String,
    pub holders: Vec<AgentId>,
    pub holder_description: Option<String>,
    pub expires_at: Option<String>,
}

/// Per-function read-write lock manager with TTL-based auto-expiry.
///
/// Uses `DashMap` for concurrent access. Each function can be in one of three
/// states: unlocked, read-locked (shared), or write-locked (exclusive).
pub struct LockManager {
    function_locks: DashMap<FunctionId, FunctionLockState>,
    /// Global write lock for module structure changes.
    pub global_write_lock: tokio::sync::RwLock<()>,
    default_ttl: Duration,
}

impl LockManager {
    /// Creates a new lock manager with the given default TTL.
    pub fn new(default_ttl: Duration) -> Self {
        LockManager {
            function_locks: DashMap::new(),
            global_write_lock: tokio::sync::RwLock::new(()),
            default_ttl,
        }
    }

    /// Creates a lock manager with the default 30-minute TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(30 * 60))
    }

    /// Formats an `Instant` as an ISO 8601 string (offset from now).
    fn format_expiry(expires_at: Instant) -> String {
        let now = Instant::now();
        if expires_at > now {
            let remaining = expires_at.duration_since(now);
            let system_expiry = std::time::SystemTime::now() + remaining;
            humanize_system_time(system_expiry)
        } else {
            "expired".to_string()
        }
    }

    /// Tries to acquire a read lock on a function for an agent.
    pub fn try_acquire_read(
        &self,
        agent_id: &AgentId,
        func_id: FunctionId,
    ) -> Result<LockGrant, LockError> {
        let expires_at = Instant::now() + self.default_ttl;

        let mut entry = self
            .function_locks
            .entry(func_id)
            .or_insert(FunctionLockState::Unlocked);

        match entry.value_mut() {
            FunctionLockState::Unlocked => {
                let mut readers = HashMap::new();
                readers.insert(*agent_id, expires_at);
                *entry.value_mut() = FunctionLockState::ReadLocked {
                    readers,
                    expires_at,
                };
                Ok(LockGrant {
                    function_id: func_id,
                    mode: LockMode::Read,
                    expires_at: Self::format_expiry(expires_at),
                })
            }
            FunctionLockState::ReadLocked {
                readers,
                expires_at: ref mut exp,
            } => {
                readers.insert(*agent_id, expires_at);
                // Extend expiry to the latest reader
                if expires_at > *exp {
                    *exp = expires_at;
                }
                Ok(LockGrant {
                    function_id: func_id,
                    mode: LockMode::Read,
                    expires_at: Self::format_expiry(expires_at),
                })
            }
            FunctionLockState::WriteLocked { holder, waiters } => {
                if holder.agent_id == *agent_id {
                    // Same agent: allow read alongside own write
                    Ok(LockGrant {
                        function_id: func_id,
                        mode: LockMode::Read,
                        expires_at: Self::format_expiry(holder.expires_at),
                    })
                } else {
                    // Different agent: deny and track waiter
                    if !waiters.contains(agent_id) {
                        waiters.push(*agent_id);
                    }
                    let queue_position = waiters
                        .iter()
                        .position(|a| a == agent_id)
                        .map(|p| p + 1)
                        .unwrap_or(waiters.len());

                    Err(LockError::AlreadyHeldBy(LockDenial {
                        function_id: func_id,
                        holder_agent_id: holder.agent_id,
                        holder_name: holder.agent_name.clone(),
                        holder_description: holder.description.clone(),
                        queue_position,
                    }))
                }
            }
        }
    }

    /// Tries to acquire a write lock on a function for an agent.
    pub fn try_acquire_write(
        &self,
        agent_id: &AgentId,
        func_id: FunctionId,
        description: Option<String>,
    ) -> Result<LockGrant, LockError> {
        let now = Instant::now();
        let expires_at = now + self.default_ttl;

        let mut entry = self
            .function_locks
            .entry(func_id)
            .or_insert(FunctionLockState::Unlocked);

        match entry.value_mut() {
            FunctionLockState::Unlocked => {
                *entry.value_mut() = FunctionLockState::WriteLocked {
                    holder: LockHolderInfo {
                        agent_id: *agent_id,
                        agent_name: None,
                        description,
                        acquired_at: now,
                        expires_at,
                    },
                    waiters: Vec::new(),
                };
                Ok(LockGrant {
                    function_id: func_id,
                    mode: LockMode::Write,
                    expires_at: Self::format_expiry(expires_at),
                })
            }
            FunctionLockState::ReadLocked { readers, .. } => {
                if readers.len() == 1 && readers.contains_key(agent_id) {
                    // Upgrade: sole reader -> writer
                    *entry.value_mut() = FunctionLockState::WriteLocked {
                        holder: LockHolderInfo {
                            agent_id: *agent_id,
                            agent_name: None,
                            description,
                            acquired_at: now,
                            expires_at,
                        },
                        waiters: Vec::new(),
                    };
                    Ok(LockGrant {
                        function_id: func_id,
                        mode: LockMode::Write,
                        expires_at: Self::format_expiry(expires_at),
                    })
                } else {
                    // Other readers present: cannot upgrade
                    // Pick any reader as "holder" for the denial
                    let (&holder_id, _) = readers.iter().next().unwrap();
                    Err(LockError::AlreadyHeldBy(LockDenial {
                        function_id: func_id,
                        holder_agent_id: holder_id,
                        holder_name: None,
                        holder_description: None,
                        queue_position: 1,
                    }))
                }
            }
            FunctionLockState::WriteLocked { holder, waiters } => {
                if holder.agent_id == *agent_id {
                    // Same agent: refresh expiry
                    holder.expires_at = expires_at;
                    if description.is_some() {
                        holder.description = description;
                    }
                    Ok(LockGrant {
                        function_id: func_id,
                        mode: LockMode::Write,
                        expires_at: Self::format_expiry(expires_at),
                    })
                } else {
                    // Different agent: deny and track waiter
                    if !waiters.contains(agent_id) {
                        waiters.push(*agent_id);
                    }
                    let queue_position = waiters
                        .iter()
                        .position(|a| a == agent_id)
                        .map(|p| p + 1)
                        .unwrap_or(waiters.len());

                    Err(LockError::AlreadyHeldBy(LockDenial {
                        function_id: func_id,
                        holder_agent_id: holder.agent_id,
                        holder_name: holder.agent_name.clone(),
                        holder_description: holder.description.clone(),
                        queue_position,
                    }))
                }
            }
        }
    }

    /// Acquires write locks on multiple functions atomically (all-or-nothing).
    ///
    /// Function IDs are sorted and deduplicated to prevent deadlocks.
    /// If any acquisition fails, all previously acquired locks are released.
    pub fn batch_acquire_write(
        &self,
        agent_id: &AgentId,
        function_ids: &[FunctionId],
        description: Option<String>,
    ) -> Result<Vec<LockGrant>, LockError> {
        // Sort and dedup by inner u32 to ensure consistent ordering
        let mut sorted_ids: Vec<FunctionId> = function_ids.to_vec();
        sorted_ids.sort_by_key(|f| f.0);
        sorted_ids.dedup_by_key(|f| f.0);

        let mut grants = Vec::new();
        let mut acquired = Vec::new();

        for &func_id in &sorted_ids {
            match self.try_acquire_write(agent_id, func_id, description.clone()) {
                Ok(grant) => {
                    grants.push(grant);
                    acquired.push(func_id);
                }
                Err(LockError::AlreadyHeldBy(denial)) => {
                    // Rollback: release all acquired locks
                    for &acq_id in &acquired {
                        let _ = self.release(agent_id, acq_id);
                    }
                    return Err(LockError::BatchPartialFailure {
                        acquired,
                        failed: denial,
                    });
                }
                Err(other) => {
                    // Rollback
                    for &acq_id in &acquired {
                        let _ = self.release(agent_id, acq_id);
                    }
                    return Err(other);
                }
            }
        }

        Ok(grants)
    }

    /// Releases a lock held by the given agent on the given function.
    pub fn release(&self, agent_id: &AgentId, func_id: FunctionId) -> Result<(), LockError> {
        let mut entry = match self.function_locks.get_mut(&func_id) {
            Some(e) => e,
            None => {
                return Err(LockError::NotHeld {
                    function_id: func_id,
                    agent_id: *agent_id,
                });
            }
        };

        match entry.value_mut() {
            FunctionLockState::Unlocked => {
                return Err(LockError::NotHeld {
                    function_id: func_id,
                    agent_id: *agent_id,
                });
            }
            FunctionLockState::ReadLocked { readers, .. } => {
                if !readers.contains_key(agent_id) {
                    return Err(LockError::NotHeld {
                        function_id: func_id,
                        agent_id: *agent_id,
                    });
                }
                readers.remove(agent_id);
                if readers.is_empty() {
                    // Drop the entry ref before removing from map
                    drop(entry);
                    self.function_locks.remove(&func_id);
                    return Ok(());
                }
            }
            FunctionLockState::WriteLocked { holder, .. } => {
                if holder.agent_id != *agent_id {
                    return Err(LockError::NotHeld {
                        function_id: func_id,
                        agent_id: *agent_id,
                    });
                }
                // Release: discard waiters, remove entry
                drop(entry);
                self.function_locks.remove(&func_id);
                return Ok(());
            }
        }

        Ok(())
    }

    /// Releases all locks held by the given agent.
    ///
    /// Returns a list of function IDs that were released.
    pub fn release_all(&self, agent_id: &AgentId) -> Vec<FunctionId> {
        let mut released = Vec::new();

        // Collect function IDs where this agent holds locks
        let func_ids: Vec<FunctionId> = self
            .function_locks
            .iter()
            .filter_map(|entry| {
                let func_id = *entry.key();
                match entry.value() {
                    FunctionLockState::ReadLocked { readers, .. } => {
                        if readers.contains_key(agent_id) {
                            Some(func_id)
                        } else {
                            None
                        }
                    }
                    FunctionLockState::WriteLocked { holder, .. } => {
                        if holder.agent_id == *agent_id {
                            Some(func_id)
                        } else {
                            None
                        }
                    }
                    FunctionLockState::Unlocked => None,
                }
            })
            .collect();

        for func_id in func_ids {
            if self.release(agent_id, func_id).is_ok() {
                released.push(func_id);
            }
        }

        released
    }

    /// Verifies that the given agent holds write locks for all specified functions.
    pub fn verify_write_locks(
        &self,
        agent_id: &AgentId,
        function_ids: &[FunctionId],
    ) -> Result<(), LockError> {
        for &func_id in function_ids {
            match self.function_locks.get(&func_id) {
                Some(entry) => match entry.value() {
                    FunctionLockState::WriteLocked { holder, .. } => {
                        if holder.agent_id != *agent_id {
                            return Err(LockError::NotHeld {
                                function_id: func_id,
                                agent_id: *agent_id,
                            });
                        }
                    }
                    _ => {
                        return Err(LockError::NotHeld {
                            function_id: func_id,
                            agent_id: *agent_id,
                        });
                    }
                },
                None => {
                    return Err(LockError::NotHeld {
                        function_id: func_id,
                        agent_id: *agent_id,
                    });
                }
            }
        }
        Ok(())
    }

    /// Returns the current lock status for all locked functions.
    pub fn status(&self) -> Vec<LockStatusEntry> {
        self.function_locks
            .iter()
            .filter_map(|entry| {
                let func_id = *entry.key();
                match entry.value() {
                    FunctionLockState::Unlocked => None,
                    FunctionLockState::ReadLocked {
                        readers,
                        expires_at,
                    } => Some(LockStatusEntry {
                        function_id: func_id,
                        state: "read".to_string(),
                        holders: readers.keys().copied().collect(),
                        holder_description: None,
                        expires_at: Some(Self::format_expiry(*expires_at)),
                    }),
                    FunctionLockState::WriteLocked { holder, .. } => Some(LockStatusEntry {
                        function_id: func_id,
                        state: "write".to_string(),
                        holders: vec![holder.agent_id],
                        holder_description: holder.description.clone(),
                        expires_at: Some(Self::format_expiry(holder.expires_at)),
                    }),
                }
            })
            .collect()
    }

    /// Removes expired locks and returns the list of released function IDs.
    pub fn sweep_expired_locks(&self) -> Vec<FunctionId> {
        let now = Instant::now();
        let mut released = Vec::new();

        // Collect expired entries
        let expired: Vec<FunctionId> = self
            .function_locks
            .iter()
            .filter_map(|entry| {
                let func_id = *entry.key();
                let is_expired = match entry.value() {
                    FunctionLockState::Unlocked => false,
                    FunctionLockState::ReadLocked { expires_at, .. } => now >= *expires_at,
                    FunctionLockState::WriteLocked { holder, .. } => now >= holder.expires_at,
                };
                if is_expired {
                    Some(func_id)
                } else {
                    None
                }
            })
            .collect();

        for func_id in expired {
            self.function_locks.remove(&func_id);
            released.push(func_id);
        }

        released
    }

    /// Spawns a background tokio task that periodically sweeps expired locks.
    pub fn start_expiry_sweep(self: &Arc<Self>, interval: Duration) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                let released = manager.sweep_expired_locks();
                if !released.is_empty() {
                    tracing::info!("Swept {} expired lock(s): {:?}", released.len(), released);
                }
            }
        });
    }
}

/// Formats a `SystemTime` as an ISO 8601 string.
fn humanize_system_time(time: std::time::SystemTime) -> String {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            // Simple ISO 8601 format without external crate
            let days_since_epoch = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;
            let seconds = time_of_day % 60;

            // Approximate date calculation (good enough for TTL display)
            let mut year = 1970i64;
            let mut remaining_days = days_since_epoch as i64;

            loop {
                let days_in_year = if is_leap_year(year) { 366 } else { 365 };
                if remaining_days < days_in_year {
                    break;
                }
                remaining_days -= days_in_year;
                year += 1;
            }

            let month_days: [i64; 12] = if is_leap_year(year) {
                [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            } else {
                [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            };

            let mut month = 1u32;
            for &md in &month_days {
                if remaining_days < md {
                    break;
                }
                remaining_days -= md;
                month += 1;
            }
            let day = remaining_days + 1;

            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                year, month, day, hours, minutes, seconds
            )
        }
        Err(_) => "unknown".to_string(),
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
