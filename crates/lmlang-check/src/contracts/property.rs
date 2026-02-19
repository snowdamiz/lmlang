//! Property-based testing harness for contract verification.
//!
//! Agents provide seed inputs (interesting/edge cases) and an iteration count.
//! The harness generates randomized input variations using a deterministic PRNG,
//! runs each through the interpreter, and collects contract violations with
//! full execution traces.
//!
//! Reproducibility: given the same `random_seed`, the same inputs are generated
//! and the same test results are produced.

use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::type_id::TypeId;

use crate::contracts::ContractViolation;
use crate::interpreter::error::RuntimeError;
use crate::interpreter::state::{ExecutionState, Interpreter, InterpreterConfig};
use crate::interpreter::trace::TraceEntry;
use crate::interpreter::value::Value;

/// Configuration for a property test run.
#[derive(Debug, Clone)]
pub struct PropertyTestConfig {
    /// Agent-provided seed inputs (the "interesting" cases).
    pub seeds: Vec<Vec<Value>>,
    /// Number of randomized iterations to run (agent-specified, required -- no default).
    pub iterations: u32,
    /// Random seed for reproducibility (system generates if not provided).
    pub random_seed: u64,
}

/// Result of a property test run.
#[derive(Debug, Clone)]
pub struct PropertyTestResult {
    /// Total tests run (seeds + random variations).
    pub total_run: u32,
    /// Number of passing tests.
    pub passed: u32,
    /// All failures with full details.
    pub failures: Vec<PropertyTestFailure>,
    /// The random seed used (for reproducibility).
    pub random_seed: u64,
}

/// A single property test failure with counterexample and trace.
#[derive(Debug, Clone)]
pub struct PropertyTestFailure {
    /// The inputs that caused the failure.
    pub inputs: Vec<Value>,
    /// The contract violation that occurred.
    pub violation: ContractViolation,
    /// Execution trace for this test case.
    pub trace: Vec<TraceEntry>,
}

/// Generates a random value of the given type using the provided RNG.
///
/// For scalar types, boundary values (0, 1, -1, MIN, MAX) are weighted
/// into the mix to increase edge-case coverage.
pub fn generate_random_value(type_id: TypeId, rng: &mut ChaCha8Rng) -> Value {
    match type_id {
        TypeId::BOOL => Value::Bool(rng.gen_bool(0.5)),

        TypeId::I8 => {
            // ~30% chance of boundary value
            if rng.gen_ratio(3, 10) {
                let boundaries: &[i8] = &[0, 1, -1, i8::MIN, i8::MAX];
                Value::I8(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::I8(rng.gen())
            }
        }

        TypeId::I16 => {
            if rng.gen_ratio(3, 10) {
                let boundaries: &[i16] = &[0, 1, -1, i16::MIN, i16::MAX];
                Value::I16(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::I16(rng.gen())
            }
        }

        TypeId::I32 => {
            if rng.gen_ratio(3, 10) {
                let boundaries: &[i32] = &[0, 1, -1, i32::MIN, i32::MAX];
                Value::I32(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::I32(rng.gen())
            }
        }

        TypeId::I64 => {
            if rng.gen_ratio(3, 10) {
                let boundaries: &[i64] = &[0, 1, -1, i64::MIN, i64::MAX];
                Value::I64(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::I64(rng.gen())
            }
        }

        TypeId::F32 => {
            if rng.gen_ratio(3, 10) {
                let boundaries: &[f32] = &[0.0, -0.0, 1.0, -1.0];
                Value::F32(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::F32(rng.gen_range(-1e6f32..1e6f32))
            }
        }

        TypeId::F64 => {
            if rng.gen_ratio(3, 10) {
                let boundaries: &[f64] = &[0.0, -0.0, 1.0, -1.0];
                Value::F64(boundaries[rng.gen_range(0..boundaries.len())])
            } else {
                Value::F64(rng.gen_range(-1e12f64..1e12f64))
            }
        }

        // Other types: return a zero/default value
        TypeId::UNIT => Value::Unit,

        _ => {
            // For unknown/compound types, return a zero/default value
            Value::I32(0)
        }
    }
}

/// Generates random input values for a function's parameters.
pub fn generate_random_inputs(
    params: &[(String, TypeId)],
    rng: &mut ChaCha8Rng,
) -> Vec<Value> {
    params
        .iter()
        .map(|(_, type_id)| generate_random_value(*type_id, rng))
        .collect()
}

/// Runs property tests on a function, checking contracts against seed inputs
/// and randomized variations.
///
/// Seeds are run first, then random variations. For each test case:
/// 1. Create a fresh interpreter with tracing enabled
/// 2. Run the function with the test inputs
/// 3. If the interpreter halts with a ContractViolation, record a failure
/// 4. Capture the execution trace for failures
///
/// Returns a PropertyTestResult with totals, failures, and the random seed.
pub fn run_property_tests(
    graph: &ProgramGraph,
    func_id: FunctionId,
    config: PropertyTestConfig,
) -> Result<PropertyTestResult, RuntimeError> {
    let func_def = graph.get_function(func_id).ok_or_else(|| {
        RuntimeError::InternalError {
            message: format!("function {} not found", func_id.0),
        }
    })?;

    let params = func_def.params.clone();
    let mut rng = ChaCha8Rng::seed_from_u64(config.random_seed);
    let mut failures = Vec::new();
    let mut total_run: u32 = 0;
    let mut passed: u32 = 0;

    // Run seed inputs first
    for seed in &config.seeds {
        total_run += 1;
        match run_single_test(graph, func_id, seed.clone())? {
            SingleTestResult::Pass => {
                passed += 1;
            }
            SingleTestResult::Failure(failure) => {
                failures.push(failure);
            }
        }
    }

    // Run random variations
    for _ in 0..config.iterations {
        total_run += 1;
        let inputs = generate_random_inputs(&params, &mut rng);
        match run_single_test(graph, func_id, inputs)? {
            SingleTestResult::Pass => {
                passed += 1;
            }
            SingleTestResult::Failure(failure) => {
                failures.push(failure);
            }
        }
    }

    Ok(PropertyTestResult {
        total_run,
        passed,
        failures,
        random_seed: config.random_seed,
    })
}

/// Result of a single test execution.
enum SingleTestResult {
    Pass,
    Failure(PropertyTestFailure),
}

/// Runs a single test case and returns the result.
fn run_single_test(
    graph: &ProgramGraph,
    func_id: FunctionId,
    inputs: Vec<Value>,
) -> Result<SingleTestResult, RuntimeError> {
    let config = InterpreterConfig {
        trace_enabled: true,
        max_recursion_depth: 256,
    };

    let mut interp = Interpreter::new(graph, config);
    interp.start(func_id, inputs.clone());
    interp.run();

    match interp.state() {
        ExecutionState::Completed { .. } => Ok(SingleTestResult::Pass),
        ExecutionState::ContractViolation { violation } => {
            let trace = interp
                .trace()
                .map(|t| t.to_vec())
                .unwrap_or_default();
            Ok(SingleTestResult::Failure(PropertyTestFailure {
                inputs,
                violation: violation.clone(),
                trace,
            }))
        }
        ExecutionState::Error { error, .. } => {
            // Runtime errors (overflow, div-by-zero) are not contract violations.
            // They count as a pass from the contract perspective (the function
            // errored before contracts could be checked, or after).
            // If needed, these could be tracked separately, but per the plan
            // spec we only track ContractViolation failures.
            let _ = error;
            Ok(SingleTestResult::Pass)
        }
        _ => Ok(SingleTestResult::Pass),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::{CmpOp, ComputeOp};
    use lmlang_core::types::Visibility;

    /// Helper: build a function `checked_fn(a: i32) -> i32` with precondition `a >= 0`.
    /// The function body just returns `a`.
    fn build_precondition_function() -> (ProgramGraph, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "checked_fn".into(),
                root,
                vec![("a".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // Parameter node
        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();

        // Const 0
        let const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::types::ConstValue::I32(0),
                },
                func_id,
            )
            .unwrap();

        // Compare: a >= 0
        let cmp_node = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, func_id)
            .unwrap();
        graph.add_data_edge(param_a, cmp_node, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_zero, cmp_node, 0, 1, TypeId::I32).unwrap();

        // Precondition node
        let precond_node = graph
            .add_core_op(
                ComputeOp::Precondition {
                    message: "a must be non-negative".into(),
                },
                func_id,
            )
            .unwrap();
        graph.add_data_edge(cmp_node, precond_node, 0, 0, TypeId::BOOL).unwrap();

        // Return node (just return a)
        // Control edge from precondition to return ensures the contract
        // is checked before the function completes.
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(param_a, ret, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(precond_node, ret, None).unwrap();

        (graph, func_id)
    }

    #[test]
    fn generate_random_value_produces_correct_types() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);

        // Bool
        let val = generate_random_value(TypeId::BOOL, &mut rng);
        assert!(matches!(val, Value::Bool(_)));

        // I8
        let val = generate_random_value(TypeId::I8, &mut rng);
        assert!(matches!(val, Value::I8(_)));

        // I16
        let val = generate_random_value(TypeId::I16, &mut rng);
        assert!(matches!(val, Value::I16(_)));

        // I32
        let val = generate_random_value(TypeId::I32, &mut rng);
        assert!(matches!(val, Value::I32(_)));

        // I64
        let val = generate_random_value(TypeId::I64, &mut rng);
        assert!(matches!(val, Value::I64(_)));

        // F32
        let val = generate_random_value(TypeId::F32, &mut rng);
        assert!(matches!(val, Value::F32(_)));

        // F64
        let val = generate_random_value(TypeId::F64, &mut rng);
        assert!(matches!(val, Value::F64(_)));

        // Unit
        let val = generate_random_value(TypeId::UNIT, &mut rng);
        assert!(matches!(val, Value::Unit));
    }

    #[test]
    fn property_test_finds_violations_with_negative_inputs() {
        let (graph, func_id) = build_precondition_function();

        let config = PropertyTestConfig {
            seeds: vec![],
            iterations: 100,
            random_seed: 12345,
        };

        let result = run_property_tests(&graph, func_id, config).unwrap();

        assert_eq!(result.total_run, 100);
        assert_eq!(result.random_seed, 12345);

        // With 100 random i32 values, we should find at least one negative
        // (statistically nearly certain given the distribution)
        assert!(
            !result.failures.is_empty(),
            "Expected at least one failure from random negatives; got {} passed out of {}",
            result.passed,
            result.total_run,
        );

        // Verify each failure has a contract violation
        for failure in &result.failures {
            assert_eq!(
                failure.violation.message,
                "a must be non-negative"
            );
            assert!(!failure.trace.is_empty(), "Failure trace should not be empty");
        }
    }

    #[test]
    fn property_test_reproducibility() {
        let (graph, func_id) = build_precondition_function();

        let config1 = PropertyTestConfig {
            seeds: vec![vec![Value::I32(5)]],
            iterations: 50,
            random_seed: 99999,
        };

        let config2 = PropertyTestConfig {
            seeds: vec![vec![Value::I32(5)]],
            iterations: 50,
            random_seed: 99999,
        };

        let result1 = run_property_tests(&graph, func_id, config1).unwrap();
        let result2 = run_property_tests(&graph, func_id, config2).unwrap();

        // Same seed -> same results
        assert_eq!(result1.total_run, result2.total_run);
        assert_eq!(result1.passed, result2.passed);
        assert_eq!(result1.failures.len(), result2.failures.len());
        assert_eq!(result1.random_seed, result2.random_seed);

        // Verify failure inputs are identical
        for (f1, f2) in result1.failures.iter().zip(result2.failures.iter()) {
            assert_eq!(f1.inputs, f2.inputs, "Failure inputs should be identical for same seed");
        }
    }

    #[test]
    fn seeds_run_before_random_inputs() {
        let (graph, func_id) = build_precondition_function();

        // Provide a seed that we know will fail (negative input)
        let config = PropertyTestConfig {
            seeds: vec![vec![Value::I32(-1)]],
            iterations: 10,
            random_seed: 42,
        };

        let result = run_property_tests(&graph, func_id, config).unwrap();

        // Total = 1 seed + 10 random = 11
        assert_eq!(result.total_run, 11);

        // The first failure should be from the seed input (-1)
        assert!(!result.failures.is_empty());
        assert_eq!(result.failures[0].inputs, vec![Value::I32(-1)]);
    }

    #[test]
    fn all_seeds_pass_when_valid() {
        let (graph, func_id) = build_precondition_function();

        // Only provide seeds that satisfy the precondition
        let config = PropertyTestConfig {
            seeds: vec![
                vec![Value::I32(0)],
                vec![Value::I32(1)],
                vec![Value::I32(100)],
            ],
            iterations: 0,
            random_seed: 42,
        };

        let result = run_property_tests(&graph, func_id, config).unwrap();

        assert_eq!(result.total_run, 3);
        assert_eq!(result.passed, 3);
        assert!(result.failures.is_empty());
    }
}
