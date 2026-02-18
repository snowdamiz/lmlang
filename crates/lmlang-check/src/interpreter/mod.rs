//! Graph interpreter for development-time execution without LLVM.
//!
//! Executes computational graphs with provided inputs, producing correct output
//! values for arithmetic, logic, control flow, memory operations, and function
//! calls.
//!
//! # Architecture
//!
//! The interpreter uses a state machine execution model with a work-list
//! algorithm for evaluation ordering:
//!
//! - [`Interpreter`] holds a reference to a [`ProgramGraph`] and manages
//!   execution state, call stack, memory, and optional execution traces.
//! - [`ExecutionState`] tracks the interpreter's lifecycle:
//!   `Ready -> Running -> (Paused | Completed | Error)`.
//! - [`CallFrame`] represents a function invocation on the call stack.
//! - [`Value`] is the runtime representation of all values.
//! - [`RuntimeError`] captures trap conditions (overflow, div-by-zero, etc.)
//!   with the node ID that caused the error.
//! - [`TraceEntry`] records each node evaluation when tracing is enabled.
//!
//! # Usage
//!
//! ```ignore
//! let interp = Interpreter::new(&graph, InterpreterConfig::default());
//! interp.start(function_id, vec![Value::I32(3), Value::I32(5)]);
//! interp.run();
//! match interp.state() {
//!     ExecutionState::Completed { result } => { /* use result */ }
//!     ExecutionState::Error { error, partial_results } => { /* handle error */ }
//!     _ => {}
//! }
//! ```

pub mod error;
pub mod eval;
pub mod state;
pub mod trace;
pub mod value;

pub use error::RuntimeError;
pub use state::{CallFrame, ExecutionState, Interpreter, InterpreterConfig};
pub use trace::TraceEntry;
pub use value::Value;
