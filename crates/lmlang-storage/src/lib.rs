//! Storage abstraction for lmlang program graphs.
//!
//! Provides the [`GraphStore`] trait defining the storage contract that all
//! backends implement, plus the [`InMemoryStore`] as a first-class backend
//! for tests and ephemeral sessions.
//!
//! # Architecture
//!
//! The storage layer has a two-layer API:
//! - **Low-level CRUD** methods (insert/get/update/delete for nodes, edges,
//!   types, functions, modules) serve as the incremental save mechanism.
//! - **High-level convenience** methods (`save_program`, `load_program`)
//!   provide bulk operations for initial save and full reconstruction.
//!
//! # Modules
//!
//! - [`error`]: StorageError enum with all failure modes
//! - [`types`]: ProgramId, ProgramSummary storage-layer types
//! - [`traits`]: GraphStore trait definition
//! - [`convert`]: ProgramGraph decompose/recompose functions
//! - [`memory`]: InMemoryStore implementation

pub mod error;
pub mod types;
pub mod traits;
pub mod convert;
pub mod memory;

// Re-export key types for ergonomic use.
pub use error::StorageError;
pub use types::{ProgramId, ProgramSummary};
pub use traits::GraphStore;
