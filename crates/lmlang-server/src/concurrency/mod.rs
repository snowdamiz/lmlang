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
