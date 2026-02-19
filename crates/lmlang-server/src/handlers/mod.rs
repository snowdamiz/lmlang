//! HTTP handler modules for the lmlang API.
//!
//! Each sub-module implements thin handlers that parse requests, acquire the
//! service lock, delegate to [`ProgramService`], and return JSON responses.
//! No business logic lives in handlers.

pub mod agents;
pub mod compile;
pub mod contracts;
pub mod history;
pub mod locks;
pub mod mutations;
pub mod programs;
pub mod queries;
pub mod simulate;
pub mod verify;
