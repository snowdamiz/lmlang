//! API schema types for the compilation and dirty status endpoints.
//!
//! Defines the request and response types for `POST /programs/{id}/compile`
//! and `GET /programs/{id}/dirty`.

use serde::{Deserialize, Serialize};

/// Default optimization level for compilation requests.
fn default_opt_level() -> String {
    "O0".to_string()
}

/// Request body for `POST /programs/{id}/compile`.
#[derive(Debug, Deserialize)]
pub struct CompileRequest {
    /// Optimization level: "O0", "O1", "O2", "O3" (default: "O0").
    #[serde(default = "default_opt_level")]
    pub opt_level: String,

    /// LLVM target triple for cross-compilation (default: host triple).
    pub target_triple: Option<String>,

    /// Include debug symbols in the output binary (default: false).
    #[serde(default)]
    pub debug_symbols: bool,

    /// Entry function name (default: auto-detect).
    pub entry_function: Option<String>,

    /// Output directory for the compiled binary (default: "./build/").
    pub output_dir: Option<String>,
}

/// Response body for `POST /programs/{id}/compile`.
#[derive(Debug, Serialize)]
pub struct CompileResponse {
    /// Path to the output executable binary.
    pub binary_path: String,

    /// LLVM target triple used for compilation.
    pub target_triple: String,

    /// Size of the output binary in bytes.
    pub binary_size: u64,

    /// Time taken for compilation in milliseconds.
    pub compilation_time_ms: u64,
}

/// Response body for `GET /programs/{id}/dirty`.
///
/// Shows which functions need recompilation, which are dirty dependents
/// (a callee changed), and which can use cached object files.
#[derive(Debug, Serialize)]
pub struct DirtyStatusResponse {
    /// Functions that have changed since last compilation.
    pub dirty_functions: Vec<DirtyFunctionView>,
    /// Functions that need recompilation due to dependency changes.
    pub dirty_dependents: Vec<DirtyFunctionView>,
    /// Functions that can use cached object files.
    pub cached_functions: Vec<CachedFunctionView>,
    /// Whether any recompilation is needed.
    pub needs_recompilation: bool,
}

/// A function that needs recompilation.
#[derive(Debug, Serialize)]
pub struct DirtyFunctionView {
    /// The function's ID.
    pub function_id: u32,
    /// The function's name.
    pub function_name: String,
    /// Why this function is dirty: "changed" or "dependent_of:<function_name>".
    pub reason: String,
}

/// A function that can use cached object files.
#[derive(Debug, Serialize)]
pub struct CachedFunctionView {
    /// The function's ID.
    pub function_id: u32,
    /// The function's name.
    pub function_name: String,
}
