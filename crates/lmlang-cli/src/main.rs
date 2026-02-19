//! LM Language compiler CLI.
//!
//! Provides the `lmlang` binary with subcommands for working with lmlang
//! programs. Currently supports `compile` which compiles a program graph
//! stored in a SQLite database to a native executable.
//!
//! Uses the same `lmlang_codegen::compile()` pipeline as the HTTP server
//! endpoint, ensuring identical compilation behavior from both entry points.

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use lmlang_codegen::{CompileOptions, OptLevel};
use lmlang_storage::traits::GraphStore;
use lmlang_storage::types::ProgramId;
use lmlang_storage::SqliteStore;

/// LM Language compiler and tools.
#[derive(Parser)]
#[command(name = "lmlang", about = "LM Language compiler and tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Compile a program to a native binary.
    Compile {
        /// Path to the program database file.
        #[arg(short, long)]
        db: String,

        /// Program ID to compile.
        #[arg(short, long)]
        program: i64,

        /// Optimization level: O0, O1, O2, O3.
        #[arg(short, long, default_value = "O0")]
        opt_level: String,

        /// LLVM target triple for cross-compilation (default: host).
        #[arg(short, long)]
        target: Option<String>,

        /// Include debug symbols.
        #[arg(long)]
        debug_symbols: bool,

        /// Entry function name (default: auto-detect).
        #[arg(long)]
        entry: Option<String>,

        /// Output directory (default: ./build/).
        #[arg(short = 'O', long, default_value = "./build")]
        output_dir: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Compile {
            db,
            program,
            opt_level,
            target,
            debug_symbols,
            entry,
            output_dir,
        } => {
            let exit_code = run_compile(
                &db,
                program,
                &opt_level,
                target,
                debug_symbols,
                entry,
                output_dir,
            );
            process::exit(exit_code);
        }
    }
}

/// Execute the compile subcommand.
///
/// Returns exit code: 0 = success, 1 = compilation error,
/// 2 = type check failure, 3 = I/O error.
fn run_compile(
    db_path: &str,
    program_id: i64,
    opt_level_str: &str,
    target: Option<String>,
    debug_symbols: bool,
    entry: Option<String>,
    output_dir: PathBuf,
) -> i32 {
    // Parse optimization level
    let opt_level = match parse_opt_level(opt_level_str) {
        Ok(level) => level,
        Err(msg) => {
            eprintln!("Error: {}", msg);
            return 1;
        }
    };

    // Open the database and load the program
    let store = match SqliteStore::new(db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to open database '{}': {}", db_path, e);
            return 3;
        }
    };

    let pid = ProgramId(program_id);
    let graph = match store.load_program(pid) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: failed to load program {}: {}", program_id, e);
            return 3;
        }
    };

    // Build compile options
    let options = CompileOptions {
        output_dir,
        opt_level,
        target_triple: target,
        debug_symbols,
        entry_function: entry,
    };

    // Compile -- same function the HTTP handler uses
    match lmlang_codegen::compile(&graph, &options) {
        Ok(result) => {
            // Print CompileResult as JSON to stdout for machine-readable output
            let json = serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
                format!("{{\"error\": \"failed to serialize result: {}\"}}", e)
            });
            println!("{}", json);
            0
        }
        Err(lmlang_codegen::error::CodegenError::TypeCheckFailed(errors)) => {
            eprintln!("Type check failed with {} error(s):", errors.len());
            for err in &errors {
                eprintln!("  - {}", err);
            }
            2
        }
        Err(lmlang_codegen::error::CodegenError::IoError(e)) => {
            eprintln!("I/O error: {}", e);
            3
        }
        Err(e) => {
            eprintln!("Compilation error: {}", e);
            1
        }
    }
}

/// Parse an optimization level string to `OptLevel`.
fn parse_opt_level(s: &str) -> Result<OptLevel, String> {
    match s {
        "O0" | "o0" => Ok(OptLevel::O0),
        "O1" | "o1" => Ok(OptLevel::O1),
        "O2" | "o2" => Ok(OptLevel::O2),
        "O3" | "o3" => Ok(OptLevel::O3),
        _ => Err(format!(
            "invalid optimization level '{}', expected O0/O1/O2/O3",
            s
        )),
    }
}
