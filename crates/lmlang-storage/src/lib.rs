//! Storage abstraction for lmlang program graphs.
//!
//! Provides the [`GraphStore`] trait defining the storage contract that all
//! backends implement, plus the [`InMemoryStore`] and [`SqliteStore`] as
//! first-class backends.
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
//! - [`schema`]: SQL schema constants and migration setup
//! - [`sqlite`]: SqliteStore implementation

pub mod convert;
pub mod dirty;
pub mod error;
pub mod hash;
pub mod memory;
pub mod schema;
pub mod sqlite;
pub mod traits;
pub mod types;

// Re-export key types for ergonomic use.
pub use dirty::{compute_dirty_set, DirtySet};
pub use error::StorageError;
pub use hash::{hash_all_functions, hash_function, hash_node_content, hash_node_with_edges};
pub use hash::{hash_all_functions_for_compilation, hash_function_for_compilation};
pub use memory::InMemoryStore;
pub use sqlite::SqliteStore;
pub use traits::GraphStore;
pub use types::{ProgramId, ProgramSummary};
