//! System linker integration for producing executables from object files.
//!
//! Links LLVM-generated `.o` files into standalone executables using
//! the system `cc` command with platform-specific flags.
//!
//! Platform-specific behavior:
//! - **macOS:** `-lSystem` for minimal system linkage (kernel syscall interface).
//!   Full static linking (`-static`) is not supported by the Apple linker.
//! - **Linux:** `-static` for fully static linking (no runtime dependencies).

use std::path::Path;

use crate::error::CodegenError;

/// Link an object file into a standalone executable.
///
/// Invokes the system `cc` compiler driver with platform-specific flags.
/// Creates the output directory if it doesn't exist.
///
/// # Arguments
///
/// * `obj_path` - Path to the input object file (.o)
/// * `output_path` - Path for the output executable
/// * `debug_symbols` - If false, strips debug symbols from output
///
/// # Errors
///
/// Returns `CodegenError::LinkerFailed` if `cc` exits with non-zero status
/// or cannot be invoked. Returns `CodegenError::IoError` if the output
/// directory cannot be created.
pub fn link_executable(
    obj_path: &Path,
    output_path: &Path,
    debug_symbols: bool,
) -> Result<(), CodegenError> {
    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cmd = std::process::Command::new("cc");
    cmd.arg(obj_path);
    cmd.arg("-o").arg(output_path);

    // Platform-specific linking flags
    if cfg!(target_os = "linux") {
        // Static linking for self-contained binaries on Linux
        cmd.arg("-static");
    } else if cfg!(target_os = "macos") {
        // Minimal system linkage on macOS (kernel syscall interface)
        // macOS does not support -static; -lSystem provides libSystem.dylib
        // which is the minimum dynamic dependency and is guaranteed present
        cmd.arg("-lSystem");
    }

    // Strip debug symbols if not requested
    if !debug_symbols {
        cmd.arg("-Wl,-S");
    }

    let output = cmd
        .output()
        .map_err(|e| CodegenError::LinkerFailed(format!("failed to invoke cc: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError::LinkerFailed(format!(
            "cc exited with status {}: {}",
            output.status, stderr
        )));
    }

    Ok(())
}

/// Link multiple object files into a standalone executable.
///
/// Same as [`link_executable`] but accepts multiple `.o` files,
/// used by incremental compilation to link per-function object files.
pub fn link_objects(
    obj_paths: &[&Path],
    output_path: &Path,
    debug_symbols: bool,
) -> Result<(), CodegenError> {
    if obj_paths.is_empty() {
        return Err(CodegenError::LinkerFailed(
            "no object files to link".to_string(),
        ));
    }

    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cmd = std::process::Command::new("cc");
    for obj_path in obj_paths {
        cmd.arg(obj_path);
    }
    cmd.arg("-o").arg(output_path);

    if cfg!(target_os = "linux") {
        cmd.arg("-static");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-lSystem");
    }

    if !debug_symbols {
        cmd.arg("-Wl,-S");
    }

    let output = cmd
        .output()
        .map_err(|e| CodegenError::LinkerFailed(format!("failed to invoke cc: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CodegenError::LinkerFailed(format!(
            "cc exited with status {}: {}",
            output.status, stderr
        )));
    }

    Ok(())
}

/// Build the linker command for inspection/testing without executing.
///
/// Returns the `Command` that would be invoked by `link_executable`.
/// Useful for verifying the correct flags are set without actually linking.
pub fn build_link_command(
    obj_path: &Path,
    output_path: &Path,
    debug_symbols: bool,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("cc");
    cmd.arg(obj_path);
    cmd.arg("-o").arg(output_path);

    if cfg!(target_os = "linux") {
        cmd.arg("-static");
    } else if cfg!(target_os = "macos") {
        cmd.arg("-lSystem");
    }

    if !debug_symbols {
        cmd.arg("-Wl,-S");
    }

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_link_command_includes_obj_and_output() {
        let cmd = build_link_command(
            Path::new("/tmp/test.o"),
            Path::new("/tmp/test_binary"),
            false,
        );
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();

        // Should contain the object file path and -o output_path
        assert!(args.contains(&std::ffi::OsStr::new("/tmp/test.o")));
        assert!(args.contains(&std::ffi::OsStr::new("-o")));
        assert!(args.contains(&std::ffi::OsStr::new("/tmp/test_binary")));
    }

    #[test]
    fn build_link_command_strips_symbols_by_default() {
        let cmd = build_link_command(
            Path::new("/tmp/test.o"),
            Path::new("/tmp/test_binary"),
            false,
        );
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();

        // debug_symbols=false should add -Wl,-S
        assert!(args.contains(&std::ffi::OsStr::new("-Wl,-S")));
    }

    #[test]
    fn build_link_command_keeps_symbols_when_requested() {
        let cmd = build_link_command(
            Path::new("/tmp/test.o"),
            Path::new("/tmp/test_binary"),
            true, // keep debug symbols
        );
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();

        // debug_symbols=true should NOT add -Wl,-S
        assert!(!args.contains(&std::ffi::OsStr::new("-Wl,-S")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn build_link_command_uses_lsystem_on_macos() {
        let cmd = build_link_command(
            Path::new("/tmp/test.o"),
            Path::new("/tmp/test_binary"),
            false,
        );
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();

        assert!(args.contains(&std::ffi::OsStr::new("-lSystem")));
        // macOS should NOT use -static
        assert!(!args.contains(&std::ffi::OsStr::new("-static")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn build_link_command_uses_static_on_linux() {
        let cmd = build_link_command(
            Path::new("/tmp/test.o"),
            Path::new("/tmp/test_binary"),
            false,
        );
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();

        assert!(args.contains(&std::ffi::OsStr::new("-static")));
    }

    #[test]
    fn link_nonexistent_object_file_returns_error() {
        let result = link_executable(
            Path::new("/tmp/nonexistent_file_12345.o"),
            Path::new("/tmp/nonexistent_output_12345"),
            false,
        );
        // Should fail because the object file doesn't exist
        assert!(result.is_err());
    }

    #[test]
    fn link_creates_output_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested_output = dir.path().join("nested").join("deep").join("output");

        // This will fail at linking (no real .o file) but should create the directory
        let _ = link_executable(Path::new("/tmp/nonexistent.o"), &nested_output, false);

        // The parent directories should have been created
        assert!(nested_output.parent().unwrap().exists());
    }
}
