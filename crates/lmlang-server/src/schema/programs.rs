//! Program management request/response types.
//!
//! Provides types for creating and listing programs.

use lmlang_storage::ProgramId;
use serde::{Deserialize, Serialize};

/// Request to create a new program.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProgramRequest {
    /// The name for the new program.
    pub name: String,
}

/// Response from creating a program.
#[derive(Debug, Clone, Serialize)]
pub struct CreateProgramResponse {
    /// The assigned program identifier.
    pub id: ProgramId,
    /// The program name.
    pub name: String,
}

/// Response for listing all programs.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramListResponse {
    /// All programs.
    pub programs: Vec<ProgramSummaryView>,
}

/// Summary view of a program for listing.
#[derive(Debug, Clone, Serialize)]
pub struct ProgramSummaryView {
    /// Program identifier.
    pub id: ProgramId,
    /// Program name.
    pub name: String,
}
