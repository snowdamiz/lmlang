//! Storage-layer types for program identity and metadata.
//!
//! [`ProgramId`] is defined here (not in lmlang-core) because program identity
//! is a storage concern -- programs only gain an ID when persisted.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Unique identifier for a stored program.
///
/// This is a storage-layer concern: programs gain an ID when persisted.
/// The inner `i64` aligns with SQLite's `INTEGER PRIMARY KEY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProgramId(pub i64);

impl fmt::Display for ProgramId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProgramId({})", self.0)
    }
}

/// Summary of a stored program (for listing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramSummary {
    /// Program identifier.
    pub id: ProgramId,
    /// Program name.
    pub name: String,
}
