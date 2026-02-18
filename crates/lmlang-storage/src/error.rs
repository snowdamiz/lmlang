//! Storage error types for lmlang-storage.
//!
//! [`StorageError`] covers all anticipated failure modes in the storage layer:
//! serialization, entity-not-found variants for each graph element type,
//! integrity violations, and reconstruction failures.

use thiserror::Error;

/// Errors produced by storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// A program with the given ID was not found.
    #[error("program not found: {0}")]
    ProgramNotFound(i64),

    /// A node was not found in the given program.
    #[error("node not found: program={program}, node={node}")]
    NodeNotFound { program: i64, node: u32 },

    /// An edge was not found in the given program.
    #[error("edge not found: program={program}, edge={edge}")]
    EdgeNotFound { program: i64, edge: u32 },

    /// A function was not found in the given program.
    #[error("function not found: program={program}, function={function}")]
    FunctionNotFound { program: i64, function: u32 },

    /// A module was not found in the given program.
    #[error("module not found: program={program}, module={module}")]
    ModuleNotFound { program: i64, module: u32 },

    /// A type was not found in the given program.
    #[error("type not found: program={program}, type_id={type_id}")]
    TypeNotFound { program: i64, type_id: u32 },

    /// A data integrity violation was detected.
    #[error("integrity error: {reason}")]
    IntegrityError { reason: String },

    /// Failed to reconstruct a ProgramGraph from stored data.
    #[error("reconstruction error: {reason}")]
    ReconstructionError { reason: String },
}
