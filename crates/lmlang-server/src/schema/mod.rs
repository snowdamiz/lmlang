//! API schema types for request/response definitions.
//!
//! Each sub-module defines the request and response types for a specific
//! API domain. Types use serde derives for JSON serialization/deserialization.

pub mod common;
pub mod compile;
pub mod diagnostics;
pub mod history;
pub mod mutations;
pub mod programs;
pub mod queries;
pub mod simulate;
pub mod verify;
