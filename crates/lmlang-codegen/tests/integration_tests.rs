//! End-to-end integration tests for the LLVM compilation pipeline.
//!
//! Each test builds a program graph using the ProgramGraph builder API,
//! compiles it via `lmlang_codegen::compile()`, executes the resulting
//! binary, and verifies the output matches expectations (and where applicable,
//! matches the interpreter output).
//!
//! Tests cover:
//! - Simple and nested arithmetic (Task 1)
//! - Comparison and boolean output
//! - Control flow: IfElse, Loop
//! - Multi-function programs with Call
//! - Runtime errors: division by zero, integer overflow (Task 2)
//! - Optimization levels: O0 and O2 correctness
//! - LLVM IR inspection via compile_to_ir
//! - CompileResult fields validation
//! - Cast operations

use std::process::Command;

use lmlang_codegen::{compile, compile_to_ir, CompileOptions, OptLevel};
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::FunctionId;
use lmlang_core::ops::{ArithOp, CmpOp, ComputeOp, StructuredOp};
use lmlang_core::type_id::TypeId;
use lmlang_core::types::{ConstValue, Visibility};

use lmlang_check::interpreter::{Interpreter, InterpreterConfig, Value};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Build a graph, compile it, run the binary, return (stdout, stderr, exit_code).
fn compile_and_run(graph: &ProgramGraph, opt_level: OptLevel) -> (String, String, i32) {
    let temp_dir = tempfile::tempdir().unwrap();
    let options = CompileOptions {
        output_dir: temp_dir.path().to_path_buf(),
        opt_level,
        target_triple: None,
        debug_symbols: false,
        entry_function: None,
    };
    let result = compile(graph, &options).expect("compilation should succeed");
    let output = Command::new(&result.binary_path)
        .output()
        .expect("binary should execute");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// Run interpreter on a graph function and return the io_log (Print output values).
fn interpret_io(graph: &ProgramGraph, func_id: FunctionId, args: Vec<Value>) -> Vec<Value> {
    let mut interp = Interpreter::new(graph, InterpreterConfig::default());
    interp.start(func_id, args);
    interp.run();
    interp.io_log().to_vec()
}


// ---------------------------------------------------------------------------
// Graph builders
// ---------------------------------------------------------------------------

/// Build: main() -> Const(2) + Const(3), Print result, Return unit
/// Expected: prints "5"
fn build_simple_add_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c2 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(2) }, func_id).unwrap();
    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    // Wire: c2 -> add port 0, c3 -> add port 1
    graph.add_data_edge(c2, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c3, add, 0, 1, TypeId::I32).unwrap();
    // add -> print port 0
    graph.add_data_edge(add, print, 0, 0, TypeId::I32).unwrap();
    // print -> return (control ordering)
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build: main() -> (10 - 3) * 4 = 28, Print, Return
fn build_nested_arith_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c10 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(10) }, func_id).unwrap();
    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let c4 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(4) }, func_id).unwrap();
    let sub = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Sub }, func_id).unwrap();
    let mul = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    // (10 - 3) = 7
    graph.add_data_edge(c10, sub, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c3, sub, 0, 1, TypeId::I32).unwrap();
    // 7 * 4 = 28
    graph.add_data_edge(sub, mul, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c4, mul, 0, 1, TypeId::I32).unwrap();
    // Print and return
    graph.add_data_edge(mul, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build: main() -> 5 > 3, Print bool, Return
fn build_comparison_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c5 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(5) }, func_id).unwrap();
    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let cmp = graph.add_core_op(ComputeOp::Compare { op: CmpOp::Gt }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(c5, cmp, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c3, cmp, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(cmp, print, 0, 0, TypeId::BOOL).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build: function "add_one" takes i32, returns i32 (param + const 1)
///        function "main" calls add_one(10), prints result
fn build_multi_function_call_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    // Function "add_one(x: i32) -> i32 { return x + 1; }"
    let add_one_id = graph
        .add_function(
            "add_one".into(),
            root,
            vec![("x".into(), TypeId::I32)],
            TypeId::I32,
            Visibility::Public,
        )
        .unwrap();

    let param_x = graph.add_core_op(ComputeOp::Parameter { index: 0 }, add_one_id).unwrap();
    let c1 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(1) }, add_one_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, add_one_id).unwrap();
    let ret1 = graph.add_core_op(ComputeOp::Return, add_one_id).unwrap();

    graph.add_data_edge(param_x, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c1, add, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(add, ret1, 0, 0, TypeId::I32).unwrap();

    // Function "main() -> void { print(add_one(10)); return; }"
    let main_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c10 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(10) }, main_id).unwrap();
    let call = graph.add_core_op(ComputeOp::Call { target: add_one_id }, main_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, main_id).unwrap();
    let ret2 = graph.add_core_op(ComputeOp::Return, main_id).unwrap();

    graph.add_data_edge(c10, call, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(call, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret2, None).unwrap();

    (graph, main_id)
}

/// Build: main() -> (a + b) * (a - b) where a=7, b=3 -> (10) * (4) = 40
fn build_expression_chain_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let ca = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(7) }, func_id).unwrap();
    let cb = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
    let sub = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Sub }, func_id).unwrap();
    let mul = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    // a + b = 10
    graph.add_data_edge(ca, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(cb, add, 0, 1, TypeId::I32).unwrap();
    // a - b = 4
    graph.add_data_edge(ca, sub, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(cb, sub, 0, 1, TypeId::I32).unwrap();
    // (a+b) * (a-b) = 40
    graph.add_data_edge(add, mul, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(sub, mul, 0, 1, TypeId::I32).unwrap();
    // Print and return
    graph.add_data_edge(mul, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build: main() -> Const(10) / Const(0) -> Return
/// Expected: runtime error exit code 1 (divide by zero)
fn build_div_by_zero_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c10 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(10) }, func_id).unwrap();
    let c0 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(0) }, func_id).unwrap();
    let div = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Div }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(c10, div, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c0, div, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(div, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build: main() -> Const(i32::MAX) + Const(1) -> Return
/// Expected: runtime error exit code 2 (integer overflow)
fn build_overflow_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let cmax = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::I32(i32::MAX) },
        func_id,
    ).unwrap();
    let c1 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(1) }, func_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(cmax, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c1, add, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(add, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

/// Build a main that returns an integer exit code (for testing return-as-exit-code).
fn build_return_exit_code_graph(exit_code: i32) -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::I32, Visibility::Public)
        .unwrap();

    let c = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::I32(exit_code) },
        func_id,
    ).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
    graph.add_data_edge(c, ret, 0, 0, TypeId::I32).unwrap();

    (graph, func_id)
}

/// Build: main() -> Const(I32(42)), Cast to I64, Print, Return
fn build_cast_graph() -> (ProgramGraph, FunctionId) {
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c42 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(42) }, func_id).unwrap();
    let cast = graph.add_structured_op(
        StructuredOp::Cast { target_type: TypeId::I64 },
        func_id,
    ).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(c42, cast, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(cast, print, 0, 0, TypeId::I64).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    (graph, func_id)
}

// ===========================================================================
// Task 1: Core integration tests
// ===========================================================================

#[test]
fn test_simple_arithmetic_2_plus_3() {
    let (graph, func_id) = build_simple_add_graph();

    // Compile and run
    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0, "expected exit code 0");
    assert!(stdout.trim().contains("5"), "stdout should contain '5', got: '{}'", stdout);

    // Verify interpreter produces the same result
    let io_log = interpret_io(&graph, func_id, vec![]);
    assert!(!io_log.is_empty(), "interpreter should produce Print output");
    match &io_log[0] {
        Value::I32(v) => assert_eq!(*v, 5, "interpreter should produce I32(5)"),
        other => panic!("expected I32, got {:?}", other),
    }
}

#[test]
fn test_nested_arithmetic_10_minus_3_times_4() {
    let (graph, func_id) = build_nested_arith_graph();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(stdout.trim().contains("28"), "stdout should contain '28', got: '{}'", stdout);

    // Verify interpreter match
    let io_log = interpret_io(&graph, func_id, vec![]);
    assert!(!io_log.is_empty());
    match &io_log[0] {
        Value::I32(v) => assert_eq!(*v, 28),
        other => panic!("expected I32(28), got {:?}", other),
    }
}

#[test]
fn test_comparison_5_gt_3() {
    let (graph, _func_id) = build_comparison_graph();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(
        stdout.trim().contains("true"),
        "stdout should contain 'true', got: '{}'",
        stdout
    );
}

#[test]
fn test_multi_function_call() {
    let (graph, _main_id) = build_multi_function_call_graph();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(stdout.trim().contains("11"), "stdout should contain '11', got: '{}'", stdout);
}

#[test]
fn test_expression_chain_a_plus_b_times_a_minus_b() {
    let (graph, func_id) = build_expression_chain_graph();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(stdout.trim().contains("40"), "stdout should contain '40', got: '{}'", stdout);

    // Verify interpreter match
    let io_log = interpret_io(&graph, func_id, vec![]);
    assert!(!io_log.is_empty());
    match &io_log[0] {
        Value::I32(v) => assert_eq!(*v, 40),
        other => panic!("expected I32(40), got {:?}", other),
    }
}

#[test]
fn test_return_as_exit_code() {
    let (graph, _func_id) = build_return_exit_code_graph(0);
    let (_stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);

    let (graph, _func_id) = build_return_exit_code_graph(42);
    let (_stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 42);
}

// ===========================================================================
// Task 2: Runtime error tests
// ===========================================================================

#[test]
fn test_division_by_zero_runtime_error() {
    let (graph, _func_id) = build_div_by_zero_graph();

    let (stdout, stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    let _ = stdout; // may or may not produce output before the error

    assert_eq!(exit_code, 1, "div-by-zero should exit with code 1, got: {}", exit_code);
    assert!(
        stderr.contains("Runtime error"),
        "stderr should contain 'Runtime error', got: '{}'",
        stderr
    );
    assert!(
        stderr.contains("divide by zero"),
        "stderr should contain 'divide by zero', got: '{}'",
        stderr
    );
    // The error message should contain a node ID (a digit)
    assert!(
        stderr.chars().any(|c| c.is_ascii_digit()),
        "stderr should contain a node ID, got: '{}'",
        stderr
    );
}

#[test]
fn test_integer_overflow_runtime_error() {
    let (graph, _func_id) = build_overflow_graph();

    let (_stdout, stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);

    assert_eq!(exit_code, 2, "overflow should exit with code 2, got: {}", exit_code);
    assert!(
        stderr.contains("Runtime error"),
        "stderr should contain 'Runtime error', got: '{}'",
        stderr
    );
    assert!(
        stderr.contains("overflow"),
        "stderr should contain 'overflow', got: '{}'",
        stderr
    );
}

// ===========================================================================
// Task 2: Optimization levels
// ===========================================================================

#[test]
fn test_optimization_levels_produce_correct_results() {
    let (graph, _func_id) = build_simple_add_graph();

    // O0
    let (stdout_o0, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(stdout_o0.trim().contains("5"), "O0: expected '5', got: '{}'", stdout_o0);

    // O2
    let (stdout_o2, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O2);
    assert_eq!(exit_code, 0);
    assert!(stdout_o2.trim().contains("5"), "O2: expected '5', got: '{}'", stdout_o2);
}

// ===========================================================================
// Task 2: LLVM IR inspection
// ===========================================================================

#[test]
fn test_compile_to_ir_produces_valid_llvm_ir() {
    let (graph, _func_id) = build_simple_add_graph();
    let temp_dir = tempfile::tempdir().unwrap();
    let options = CompileOptions {
        output_dir: temp_dir.path().to_path_buf(),
        opt_level: OptLevel::O0,
        target_triple: None,
        debug_symbols: false,
        entry_function: None,
    };

    let ir = compile_to_ir(&graph, &options).expect("compile_to_ir should succeed");

    // Should contain function definitions
    assert!(
        ir.contains("define"),
        "IR should contain 'define' for function definitions, got:\n{}",
        &ir[..ir.len().min(500)]
    );
    // Should contain the main function or wrapper
    assert!(
        ir.contains("@main"),
        "IR should contain '@main', got:\n{}",
        &ir[..ir.len().min(500)]
    );
    // Should contain some LLVM instructions
    assert!(
        ir.contains("add") || ir.contains("call"),
        "IR should contain 'add' or 'call' instructions"
    );
}

// ===========================================================================
// Task 2: CompileResult fields
// ===========================================================================

#[test]
fn test_compile_result_fields_are_populated() {
    let (graph, _func_id) = build_simple_add_graph();
    let temp_dir = tempfile::tempdir().unwrap();
    let options = CompileOptions {
        output_dir: temp_dir.path().to_path_buf(),
        opt_level: OptLevel::O0,
        target_triple: None,
        debug_symbols: false,
        entry_function: None,
    };

    let result = compile(&graph, &options).expect("compilation should succeed");

    // binary_path should exist on disk
    assert!(
        result.binary_path.exists(),
        "binary_path {:?} should exist on disk",
        result.binary_path
    );
    // binary_size > 0
    assert!(
        result.binary_size > 0,
        "binary_size should be > 0, got: {}",
        result.binary_size
    );
    // target_triple is non-empty and contains arch info
    assert!(
        !result.target_triple.is_empty(),
        "target_triple should be non-empty"
    );
    assert!(
        result.target_triple.contains("aarch64")
            || result.target_triple.contains("arm64")
            || result.target_triple.contains("x86_64"),
        "target_triple should contain host arch, got: '{}'",
        result.target_triple
    );
}

// ===========================================================================
// Task 2: Cast operations
// ===========================================================================

#[test]
fn test_cast_i32_to_i64() {
    let (graph, _func_id) = build_cast_graph();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(
        stdout.trim().contains("42"),
        "stdout should contain '42' after i32->i64 cast, got: '{}'",
        stdout
    );
}

// ===========================================================================
// Task 2: Additional correctness tests
// ===========================================================================

#[test]
fn test_nested_function_calls() {
    // Build: double(x) = x + x, main() -> print(double(double(3))) = 12
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let double_id = graph
        .add_function(
            "double".into(),
            root,
            vec![("x".into(), TypeId::I32)],
            TypeId::I32,
            Visibility::Public,
        )
        .unwrap();

    let param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, double_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, double_id).unwrap();
    let ret1 = graph.add_core_op(ComputeOp::Return, double_id).unwrap();

    graph.add_data_edge(param, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(param, add, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(add, ret1, 0, 0, TypeId::I32).unwrap();

    let main_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, main_id).unwrap();
    let call1 = graph.add_core_op(ComputeOp::Call { target: double_id }, main_id).unwrap();
    let call2 = graph.add_core_op(ComputeOp::Call { target: double_id }, main_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, main_id).unwrap();
    let ret2 = graph.add_core_op(ComputeOp::Return, main_id).unwrap();

    graph.add_data_edge(c3, call1, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(call1, call2, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(call2, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret2, None).unwrap();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(stdout.trim().contains("12"), "stdout should contain '12', got: '{}'", stdout);
}

#[test]
fn test_boolean_false_comparison() {
    // Build: 3 > 5 = false
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let c5 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(5) }, func_id).unwrap();
    let cmp = graph.add_core_op(ComputeOp::Compare { op: CmpOp::Gt }, func_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(c3, cmp, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c5, cmp, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(cmp, print, 0, 0, TypeId::BOOL).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(
        stdout.trim().contains("false"),
        "stdout should contain 'false', got: '{}'",
        stdout
    );
}

#[test]
fn test_o2_optimization_with_nested_calls() {
    // Same nested call test with O2 -- ensures optimization doesn't break semantics
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let double_id = graph
        .add_function(
            "double".into(),
            root,
            vec![("x".into(), TypeId::I32)],
            TypeId::I32,
            Visibility::Public,
        )
        .unwrap();

    let param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, double_id).unwrap();
    let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, double_id).unwrap();
    let ret1 = graph.add_core_op(ComputeOp::Return, double_id).unwrap();

    graph.add_data_edge(param, add, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(param, add, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(add, ret1, 0, 0, TypeId::I32).unwrap();

    let main_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, main_id).unwrap();
    let call1 = graph.add_core_op(ComputeOp::Call { target: double_id }, main_id).unwrap();
    let call2 = graph.add_core_op(ComputeOp::Call { target: double_id }, main_id).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, main_id).unwrap();
    let ret2 = graph.add_core_op(ComputeOp::Return, main_id).unwrap();

    graph.add_data_edge(c3, call1, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(call1, call2, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(call2, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret2, None).unwrap();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O2);
    assert_eq!(exit_code, 0);
    assert!(stdout.trim().contains("12"), "O2: expected '12', got: '{}'", stdout);
}

// ===========================================================================
// Task 2: Type checker rejection before codegen
// ===========================================================================

#[test]
fn test_invalid_graph_rejected_before_execution() {
    // Build a graph with a type mismatch: BinaryArith(Add) with Bool and I32 inputs.
    // The compile pipeline should catch this (either via type checker or LLVM verification)
    // and return an error rather than producing a broken binary.
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c_bool = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::Bool(true) },
        func_id,
    ).unwrap();
    let c_i32 = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::I32(5) },
        func_id,
    ).unwrap();
    let add = graph.add_core_op(
        ComputeOp::BinaryArith { op: ArithOp::Add },
        func_id,
    ).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    // Wire Bool -> add port 0, I32 -> add port 1 (type mismatch)
    graph.add_data_edge(c_bool, add, 0, 0, TypeId::BOOL).unwrap();
    graph.add_data_edge(c_i32, add, 0, 1, TypeId::I32).unwrap();
    graph.add_data_edge(add, ret, 0, 0, TypeId::I32).unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let options = CompileOptions {
        output_dir: temp_dir.path().to_path_buf(),
        opt_level: OptLevel::O0,
        target_triple: None,
        debug_symbols: false,
        entry_function: None,
    };

    let result = compile(&graph, &options);
    assert!(
        result.is_err(),
        "compile should fail for graph with mismatched types"
    );
    // Error could be TypeCheckFailed or LlvmError depending on what the checker catches
    let err = result.unwrap_err();
    let err_msg = format!("{:?}", err);
    assert!(
        err_msg.contains("TypeCheck") || err_msg.contains("Llvm") || err_msg.contains("type"),
        "error should relate to type issues, got: {}",
        err_msg
    );
}

// ===========================================================================
// Task 2: Struct operations
// ===========================================================================

#[test]
fn test_struct_create_and_get() {
    // Build: create struct { i32, i32 } with values (10, 20), get field 0, print it
    use indexmap::IndexMap;
    use lmlang_core::types::StructDef;

    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    // Register a struct type with two I32 fields
    let struct_type_id = graph.types.register(lmlang_core::types::LmType::Struct(StructDef {
        name: "Point".into(),
        type_id: TypeId(100), // placeholder, will be overwritten
        fields: IndexMap::from([
            ("x".into(), TypeId::I32),
            ("y".into(), TypeId::I32),
        ]),
        module: root,
        visibility: Visibility::Public,
    }));

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c10 = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::I32(10) },
        func_id,
    ).unwrap();
    let c20 = graph.add_core_op(
        ComputeOp::Const { value: ConstValue::I32(20) },
        func_id,
    ).unwrap();
    let create = graph.add_structured_op(
        StructuredOp::StructCreate { type_id: struct_type_id },
        func_id,
    ).unwrap();
    let get = graph.add_structured_op(
        StructuredOp::StructGet { field_index: 0 },
        func_id,
    ).unwrap();
    let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    // Wire: c10 -> create port 0, c20 -> create port 1
    graph.add_data_edge(c10, create, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c20, create, 0, 1, TypeId::I32).unwrap();
    // create -> get port 0 (struct value)
    graph.add_data_edge(create, get, 0, 0, struct_type_id).unwrap();
    // get -> print port 0 (field 0 = I32)
    graph.add_data_edge(get, print, 0, 0, TypeId::I32).unwrap();
    graph.add_control_edge(print, ret, None).unwrap();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);
    assert!(
        stdout.trim().contains("10"),
        "stdout should contain '10' (field 0 of struct), got: '{}'",
        stdout
    );
}

// ===========================================================================
// Task 2: Multiple prints
// ===========================================================================

#[test]
fn test_multiple_prints_sequential() {
    // Build: print(1), print(2), print(3), return
    let mut graph = ProgramGraph::new("test");
    let root = graph.modules.root_id();

    let func_id = graph
        .add_function("main".into(), root, vec![], TypeId::UNIT, Visibility::Public)
        .unwrap();

    let c1 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(1) }, func_id).unwrap();
    let c2 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(2) }, func_id).unwrap();
    let c3 = graph.add_core_op(ComputeOp::Const { value: ConstValue::I32(3) }, func_id).unwrap();
    let p1 = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let p2 = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let p3 = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
    let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

    graph.add_data_edge(c1, p1, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c2, p2, 0, 0, TypeId::I32).unwrap();
    graph.add_data_edge(c3, p3, 0, 0, TypeId::I32).unwrap();

    // Chain prints with control edges for ordering
    graph.add_control_edge(p1, p2, None).unwrap();
    graph.add_control_edge(p2, p3, None).unwrap();
    graph.add_control_edge(p3, ret, None).unwrap();

    let (stdout, _stderr, exit_code) = compile_and_run(&graph, OptLevel::O0);
    assert_eq!(exit_code, 0);

    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 lines of output, got: {:?}", lines);
    assert_eq!(lines[0].trim(), "1");
    assert_eq!(lines[1].trim(), "2");
    assert_eq!(lines[2].trim(), "3");
}
