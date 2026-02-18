//! Application state with shared `ProgramService` for concurrent access.
//!
//! [`AppState`] wraps the service in `Arc<Mutex<>>` for use with axum handlers.
//! Handlers extract `AppState` and lock the mutex for each request.

use std::sync::{Arc, Mutex};

use crate::error::ApiError;
use crate::service::ProgramService;

/// Shared application state for the HTTP server.
///
/// Wraps `ProgramService` in `Arc<Mutex<>>` so it can be shared across
/// async handler tasks. Each handler locks the mutex for the duration
/// of its service call.
#[derive(Clone)]
pub struct AppState {
    /// The shared program service.
    pub service: Arc<Mutex<ProgramService>>,
}

impl AppState {
    /// Creates a new `AppState` with a `ProgramService` backed by the given
    /// SQLite database path.
    pub fn new(db_path: &str) -> Result<Self, ApiError> {
        let service = ProgramService::new(db_path)?;
        Ok(AppState {
            service: Arc::new(Mutex::new(service)),
        })
    }

    /// Creates a new `AppState` with an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, ApiError> {
        let service = ProgramService::in_memory()?;
        Ok(AppState {
            service: Arc::new(Mutex::new(service)),
        })
    }
}
