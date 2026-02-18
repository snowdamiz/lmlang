//! System linker integration for producing executables from object files.
//!
//! Links LLVM-generated `.o` files into standalone executables using
//! the system `cc` command with platform-specific flags.
