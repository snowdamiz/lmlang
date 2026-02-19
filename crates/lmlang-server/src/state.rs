//! Application state with shared `ProgramService` for concurrent access.
//!
//! [`AppState`] wraps the service in `Arc<tokio::sync::Mutex<>>` for use with
//! axum handlers. Uses `tokio::sync::Mutex` (async-aware) instead of
//! `std::sync::Mutex` (blocking) so handlers await the lock without blocking
//! the tokio runtime.
//!
//! Note: `tokio::sync::RwLock` would allow concurrent reads, but
//! `ProgramService` contains `rusqlite::Connection` which is `!Sync`,
//! preventing it from being held behind an `RwLock`. The `Mutex` approach
//! is correct and non-blocking. Concurrent reads at the graph level happen
//! through the function-level `LockManager` instead.

use std::sync::Arc;
use std::time::Duration;

use crate::concurrency::{AgentRegistry, LockManager};
use crate::error::ApiError;
use crate::service::ProgramService;

/// Shared application state for the HTTP server.
///
/// Wraps `ProgramService` in `Arc<tokio::sync::Mutex<>>` so it can be shared
/// across async handler tasks. All handlers acquire the lock via `.lock().await`
/// (non-blocking to the tokio runtime, unlike `std::sync::Mutex`).
///
/// Concurrent multi-agent access is managed at the function level by
/// [`LockManager`], not at the `ProgramService` level.
#[derive(Clone)]
pub struct AppState {
    /// The shared program service (async Mutex -- non-blocking await).
    pub service: Arc<tokio::sync::Mutex<ProgramService>>,
    /// Per-function lock manager for multi-agent editing.
    pub lock_manager: Arc<LockManager>,
    /// Agent session registry.
    pub agent_registry: Arc<AgentRegistry>,
}

impl AppState {
    /// Creates a new `AppState` with a `ProgramService` backed by the given
    /// SQLite database path.
    pub fn new(db_path: &str) -> Result<Self, ApiError> {
        let service = ProgramService::new(db_path)?;
        let lock_manager = Arc::new(LockManager::with_default_ttl());
        let agent_registry = Arc::new(AgentRegistry::new());

        // Start the lock expiry sweep task (every 60 seconds)
        lock_manager.start_expiry_sweep(Duration::from_secs(60));

        Ok(AppState {
            service: Arc::new(tokio::sync::Mutex::new(service)),
            lock_manager,
            agent_registry,
        })
    }

    /// Creates a new `AppState` with an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, ApiError> {
        let service = ProgramService::in_memory()?;
        let lock_manager = Arc::new(LockManager::with_default_ttl());
        let agent_registry = Arc::new(AgentRegistry::new());

        // Start the lock expiry sweep task (every 60 seconds)
        lock_manager.start_expiry_sweep(Duration::from_secs(60));

        Ok(AppState {
            service: Arc::new(tokio::sync::Mutex::new(service)),
            lock_manager,
            agent_registry,
        })
    }
}
