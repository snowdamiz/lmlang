//! LLVM code generation for lmlang programs.
//!
//! This crate provides the compilation pipeline that transforms lmlang
//! computational graphs into native executables via LLVM/inkwell.
//!
//! # Modules
//!
//! - [`error`] -- Error types for all compilation failure modes
//! - [`types`] -- Mapping from lmlang types to LLVM IR types
//! - [`runtime`] -- Runtime function declarations (error handling, I/O)
//! - [`linker`] -- Object file to executable linking via system `cc`

pub mod codegen;
pub mod compiler;
pub mod error;
pub mod incremental;
pub mod linker;
pub mod runtime;
pub mod types;

pub use compiler::{compile, compile_incremental, compile_to_ir};

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Optimization level for LLVM compilation passes.
///
/// Maps directly to LLVM's `default<ON>` pass pipeline.
/// Default is `O0` (no optimization, fastest compilation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptLevel {
    /// No optimization (fastest compilation, easiest debugging).
    O0,
    /// Basic optimizations (inlining, simple loop opts).
    O1,
    /// Standard optimizations (most optimizations enabled).
    O2,
    /// Aggressive optimizations (including vectorization).
    O3,
}

impl Default for OptLevel {
    fn default() -> Self {
        OptLevel::O0
    }
}

/// Options controlling the compilation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileOptions {
    /// Directory for build output (object files, executables).
    pub output_dir: PathBuf,

    /// LLVM optimization level.
    pub opt_level: OptLevel,

    /// Target triple for cross-compilation.
    /// `None` means use the host machine's native triple.
    pub target_triple: Option<String>,

    /// Whether to include debug symbols in the output binary.
    pub debug_symbols: bool,

    /// Name of the entry function to call from main().
    /// `None` means auto-detect (first public function).
    pub entry_function: Option<String>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            output_dir: PathBuf::from("./build/"),
            opt_level: OptLevel::O0,
            target_triple: None,
            debug_symbols: false,
            entry_function: None,
        }
    }
}

/// Result of a successful compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResult {
    /// Path to the output executable binary.
    pub binary_path: PathBuf,

    /// LLVM target triple used for compilation.
    pub target_triple: String,

    /// Size of the output binary in bytes.
    pub binary_size: u64,

    /// Time taken for compilation in milliseconds.
    pub compilation_time_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_opt_level_is_o0() {
        assert_eq!(OptLevel::default(), OptLevel::O0);
    }

    #[test]
    fn default_compile_options() {
        let opts = CompileOptions::default();
        assert_eq!(opts.output_dir, PathBuf::from("./build/"));
        assert_eq!(opts.opt_level, OptLevel::O0);
        assert!(opts.target_triple.is_none());
        assert!(!opts.debug_symbols);
        assert!(opts.entry_function.is_none());
    }

    #[test]
    fn opt_level_serde_roundtrip() {
        for level in [OptLevel::O0, OptLevel::O1, OptLevel::O2, OptLevel::O3] {
            let json = serde_json::to_string(&level).unwrap();
            let back: OptLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn compile_options_serde_roundtrip() {
        let opts = CompileOptions {
            output_dir: PathBuf::from("/tmp/build"),
            opt_level: OptLevel::O2,
            target_triple: Some("aarch64-apple-darwin".to_string()),
            debug_symbols: true,
            entry_function: Some("my_main".to_string()),
        };
        let json = serde_json::to_string(&opts).unwrap();
        let back: CompileOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back.output_dir, opts.output_dir);
        assert_eq!(back.opt_level, opts.opt_level);
        assert_eq!(back.target_triple, opts.target_triple);
        assert_eq!(back.debug_symbols, opts.debug_symbols);
        assert_eq!(back.entry_function, opts.entry_function);
    }

    #[test]
    fn compile_result_serde_roundtrip() {
        let result = CompileResult {
            binary_path: PathBuf::from("/tmp/build/output"),
            target_triple: "aarch64-apple-darwin".to_string(),
            binary_size: 12345,
            compilation_time_ms: 500,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: CompileResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.binary_path, result.binary_path);
        assert_eq!(back.target_triple, result.target_triple);
        assert_eq!(back.binary_size, result.binary_size);
        assert_eq!(back.compilation_time_ms, result.compilation_time_ms);
    }
}
