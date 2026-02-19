//! Schema types for lock management API.

use serde::{Deserialize, Serialize};

/// Request to acquire locks on one or more functions.
#[derive(Debug, Clone, Deserialize)]
pub struct AcquireLockRequest {
    /// Function IDs to lock.
    pub function_ids: Vec<u32>,
    /// Lock mode: "read" or "write".
    pub mode: String,
    /// Optional description of what the agent intends to do.
    pub description: Option<String>,
}

/// Response after successful lock acquisition.
#[derive(Debug, Clone, Serialize)]
pub struct AcquireLockResponse {
    pub grants: Vec<LockGrantView>,
}

/// View of a single lock grant.
#[derive(Debug, Clone, Serialize)]
pub struct LockGrantView {
    pub function_id: u32,
    pub mode: String,
    pub expires_at: String,
}

/// Request to release locks on one or more functions.
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseLockRequest {
    /// Function IDs to release.
    pub function_ids: Vec<u32>,
}

/// Response after releasing locks.
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseLockResponse {
    /// Function IDs that were released.
    pub released: Vec<u32>,
}

/// Response showing all current lock status.
#[derive(Debug, Clone, Serialize)]
pub struct LockStatusResponse {
    pub locks: Vec<LockStatusView>,
}

/// View of a single function's lock status.
#[derive(Debug, Clone, Serialize)]
pub struct LockStatusView {
    pub function_id: u32,
    pub state: String,
    pub holders: Vec<String>,
    pub holder_description: Option<String>,
    pub expires_at: Option<String>,
}
