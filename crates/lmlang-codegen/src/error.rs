//! Codegen error types covering all compilation failure modes.

use lmlang_check::typecheck::TypeError;

/// Errors that can occur during LLVM code generation and compilation.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// Unsupported or unresolvable type during LLVM type mapping.
    #[error("type mapping error: {0}")]
    TypeMapping(String),

    /// Op node not yet implemented in codegen.
    #[error("unsupported op: {0}")]
    UnsupportedOp(String),

    /// No entry point function found in the program graph.
    #[error("no entry function found")]
    NoEntryFunction,

    /// Graph structure issue preventing compilation.
    #[error("invalid graph: {0}")]
    InvalidGraph(String),

    /// LLVM API failure (module verification, pass failures).
    #[error("LLVM error: {0}")]
    LlvmError(String),

    /// System linker (cc) subprocess failure.
    #[error("linker failed: {0}")]
    LinkerFailed(String),

    /// Filesystem I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Pre-codegen type checking found errors.
    #[error("type check failed with {} error(s)", .0.len())]
    TypeCheckFailed(Vec<TypeError>),
}
