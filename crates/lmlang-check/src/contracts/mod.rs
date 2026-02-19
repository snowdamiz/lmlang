//! Contract types for behavioral contracts on functions and data structures.
//!
//! Contracts are first-class graph nodes checked during interpretation
//! (development-time) and stripped during compilation (zero overhead).
//! Violations produce structured diagnostics with counterexample values.

pub mod check;

use lmlang_core::id::{FunctionId, NodeId};
use serde::{Deserialize, Serialize};

use crate::interpreter::value::Value;

/// The kind of contract that was violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractKind {
    /// A precondition checked at function entry.
    Precondition,
    /// A postcondition checked at function return.
    Postcondition,
    /// A data structure invariant checked at module boundaries.
    Invariant,
}

/// A structured contract violation diagnostic.
///
/// Contains all information needed for an agent to understand and fix
/// the violated contract: what kind, which node, which function, the
/// human-readable message, the inputs that triggered the violation,
/// the actual return value (for postconditions), and counterexample
/// node values from the failing evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractViolation {
    /// What kind of contract was violated.
    pub kind: ContractKind,
    /// The contract node that failed.
    pub contract_node: NodeId,
    /// The function containing the contract.
    pub function_id: FunctionId,
    /// Human-readable description from the contract op.
    pub message: String,
    /// Function inputs that triggered the violation.
    pub inputs: Vec<Value>,
    /// For postconditions, the actual return value.
    pub actual_return: Option<Value>,
    /// Node values from the failing evaluation, sorted by NodeId for determinism.
    pub counterexample: Vec<(NodeId, Value)>,
}
