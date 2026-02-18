//! Static type checker for the lmlang computational graph.
//!
//! Provides two levels of type checking:
//! - [`validate_data_edge`]: Checks a single proposed edge for type compatibility
//!   (eager per-edit checking).
//! - [`validate_graph`]: Scans the entire graph and reports ALL type errors at
//!   once (full validation).
//!
//! Both functions are pure -- they read the graph but do not modify it.

pub mod coercion;
pub mod diagnostics;
pub mod rules;

pub use coercion::{can_coerce, common_numeric_type, is_float, is_integer, is_numeric};
pub use diagnostics::{FixSuggestion, TypeError};
pub use rules::{resolve_type_rule, OpTypeRule};
