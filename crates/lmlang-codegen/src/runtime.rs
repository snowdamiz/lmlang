//! Runtime function declarations for compiled lmlang programs.
//!
//! Declares external C functions (printf, exit, fprintf) and emits
//! the `lmlang_runtime_error` function body in LLVM IR.
//! Also provides guard helpers for division-by-zero, overflow, and
//! bounds checking, plus Print op support via typed printf calls.
