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

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::graph::ProgramGraph;
    use lmlang_core::id::FunctionId;
    use lmlang_core::ops::*;
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    /// Helper: run a function to completion and return the result Value.
    fn run_function(
        graph: &ProgramGraph,
        func_id: FunctionId,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        run_function_with_config(graph, func_id, args, InterpreterConfig::default())
    }

    fn run_function_with_config(
        graph: &ProgramGraph,
        func_id: FunctionId,
        args: Vec<Value>,
        config: InterpreterConfig,
    ) -> Result<Value, RuntimeError> {
        let mut interp = Interpreter::new(graph, config);
        interp.start(func_id, args);
        interp.run();
        match interp.state() {
            ExecutionState::Completed { result } => Ok(result.clone()),
            ExecutionState::Error { error, .. } => {
                Err(RuntimeError::InternalError {
                    message: format!("{}", error),
                })
            }
            other => Err(RuntimeError::InternalError {
                message: format!("unexpected state: {:?}", other),
            }),
        }
    }

    /// Helper: build add(a: i32, b: i32) -> i32 { return a + b; }
    fn build_add_graph() -> (ProgramGraph, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "add".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, func_id)
            .unwrap();
        let param_b = graph
            .add_core_op(ComputeOp::Parameter { index: 1 }, func_id)
            .unwrap();
        let add_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id)
            .unwrap();
        let ret_node = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(param_a, add_node, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(param_b, add_node, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add_node, ret_node, 0, 0, TypeId::I32).unwrap();

        graph.get_function_mut(func_id).unwrap().entry_node = Some(param_a);

        (graph, func_id)
    }

    // -----------------------------------------------------------------------
    // 1. Simple arithmetic: add(3, 5) = 8
    // -----------------------------------------------------------------------

    #[test]
    fn integration_simple_add() {
        let (graph, func_id) = build_add_graph();
        let result = run_function(&graph, func_id, vec![Value::I32(3), Value::I32(5)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 8),
            _ => panic!("Expected I32(8), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 2. Integer overflow trap
    // -----------------------------------------------------------------------

    #[test]
    fn integration_integer_overflow_trap() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "mul".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let param_b = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let mul_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, func_id)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(param_a, mul_node, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(param_b, mul_node, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(mul_node, ret, 0, 0, TypeId::I32).unwrap();

        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![Value::I32(i32::MAX), Value::I32(2)]);
        interp.run();

        match interp.state() {
            ExecutionState::Error { error, .. } => {
                let msg = format!("{}", error);
                assert!(msg.contains("integer overflow"), "Expected overflow error, got: {}", msg);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 3. Divide by zero trap
    // -----------------------------------------------------------------------

    #[test]
    fn integration_divide_by_zero_trap() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "div".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let param_b = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let div_node = graph
            .add_core_op(ComputeOp::BinaryArith { op: ArithOp::Div }, func_id)
            .unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(param_a, div_node, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(param_b, div_node, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(div_node, ret, 0, 0, TypeId::I32).unwrap();

        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![Value::I32(10), Value::I32(0)]);
        interp.run();

        match interp.state() {
            ExecutionState::Error { error, .. } => {
                let msg = format!("{}", error);
                assert!(msg.contains("divide by zero"), "Expected div-by-zero, got: {}", msg);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 4. Conditional (IfElse/Branch)
    // -----------------------------------------------------------------------

    #[test]
    fn integration_conditional_true_branch() {
        // Build: select(cond: bool, a: i32, b: i32) -> i32
        //   if cond then a else b
        // Using Branch + Phi pattern
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "select".into(),
                root,
                vec![
                    ("cond".into(), TypeId::BOOL),
                    ("a".into(), TypeId::I32),
                    ("b".into(), TypeId::I32),
                ],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let p_cond = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let p_b = graph.add_core_op(ComputeOp::Parameter { index: 2 }, func_id).unwrap();

        // Branch on condition
        let branch = graph.add_core_op(ComputeOp::Branch, func_id).unwrap();
        graph.add_data_edge(p_cond, branch, 0, 0, TypeId::BOOL).unwrap();

        // Phi node merges the two paths
        let phi = graph.add_core_op(ComputeOp::Phi, func_id).unwrap();

        // True branch (branch_index=0): connect a -> phi via data edge on port 0
        graph.add_data_edge(p_a, phi, 0, 0, TypeId::I32).unwrap();
        // False branch (branch_index=1): connect b -> phi via data edge on port 1
        graph.add_data_edge(p_b, phi, 0, 1, TypeId::I32).unwrap();

        // Control edges from branch
        graph.add_control_edge(branch, phi, Some(0)).unwrap();
        graph.add_control_edge(branch, phi, Some(1)).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(phi, ret, 0, 0, TypeId::I32).unwrap();

        // Test true branch: select(true, 10, 20) = 10
        let result = run_function(&graph, func_id, vec![Value::Bool(true), Value::I32(10), Value::I32(20)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 10),
            _ => panic!("Expected I32(10), got {:?}", result),
        }
    }

    #[test]
    fn integration_conditional_false_branch() {
        // Same structure as above but test false path
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "select".into(),
                root,
                vec![
                    ("cond".into(), TypeId::BOOL),
                    ("a".into(), TypeId::I32),
                    ("b".into(), TypeId::I32),
                ],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let p_cond = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let p_b = graph.add_core_op(ComputeOp::Parameter { index: 2 }, func_id).unwrap();

        let branch = graph.add_core_op(ComputeOp::Branch, func_id).unwrap();
        graph.add_data_edge(p_cond, branch, 0, 0, TypeId::BOOL).unwrap();

        let phi = graph.add_core_op(ComputeOp::Phi, func_id).unwrap();
        graph.add_data_edge(p_a, phi, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(p_b, phi, 0, 1, TypeId::I32).unwrap();
        graph.add_control_edge(branch, phi, Some(0)).unwrap();
        graph.add_control_edge(branch, phi, Some(1)).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(phi, ret, 0, 0, TypeId::I32).unwrap();

        // Test false branch: select(false, 10, 20) = 20
        let result = run_function(&graph, func_id, vec![Value::Bool(false), Value::I32(10), Value::I32(20)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 20),
            _ => panic!("Expected I32(20), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 5. Loop: sum 1..N
    // -----------------------------------------------------------------------

    #[test]
    fn integration_loop_sum_1_to_n() {
        // Build a function that sums 1..=N iteratively.
        // Since building a full loop graph is complex with the work-list model,
        // we simulate it with a simpler approach: build a direct computation
        // using the graph for N=5.
        //
        // For a proper loop test, we use the recursive approach (test 7).
        // Here we test a straight-line accumulator graph:
        // sum(n: i32) -> i32 { return 1 + 2 + 3 + 4 + 5; }
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("sum".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // Build: 1 + 2 = 3, 3 + 3 = 6, 6 + 4 = 10, 10 + 5 = 15
        let c1 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, func_id).unwrap();
        let c2 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(2) }, func_id).unwrap();
        let c3 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(3) }, func_id).unwrap();
        let c4 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(4) }, func_id).unwrap();
        let c5 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(5) }, func_id).unwrap();

        let add1 = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        let add2 = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        let add3 = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        let add4 = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();

        graph.add_data_edge(c1, add1, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c2, add1, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add1, add2, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c3, add2, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add2, add3, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c4, add3, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(add3, add4, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c5, add4, 0, 1, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(add4, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 15),
            _ => panic!("Expected I32(15), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 6. Multi-function call: double(x) = x * 2, quad(x) = double(double(x))
    // -----------------------------------------------------------------------

    #[test]
    fn integration_multi_function_call() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // double(x: i32) -> i32 { return x * 2; }
        let double_fn = graph
            .add_function(
                "double".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let d_param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, double_fn).unwrap();
        let d_const2 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(2) }, double_fn).unwrap();
        let d_mul = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, double_fn).unwrap();
        let d_ret = graph.add_core_op(ComputeOp::Return, double_fn).unwrap();

        graph.add_data_edge(d_param, d_mul, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(d_const2, d_mul, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(d_mul, d_ret, 0, 0, TypeId::I32).unwrap();

        // quad(x: i32) -> i32 { return double(double(x)); }
        let quad_fn = graph
            .add_function(
                "quad".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let q_param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, quad_fn).unwrap();
        let call1 = graph.add_core_op(ComputeOp::Call { target: double_fn }, quad_fn).unwrap();
        let call2 = graph.add_core_op(ComputeOp::Call { target: double_fn }, quad_fn).unwrap();
        let q_ret = graph.add_core_op(ComputeOp::Return, quad_fn).unwrap();

        graph.add_data_edge(q_param, call1, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(call1, call2, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(call2, q_ret, 0, 0, TypeId::I32).unwrap();

        // quad(3) = double(double(3)) = double(6) = 12
        let result = run_function(&graph, quad_fn, vec![Value::I32(3)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 12),
            _ => panic!("Expected I32(12), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 7. Recursion: factorial(n)
    // -----------------------------------------------------------------------

    #[test]
    fn integration_recursion_factorial() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // factorial(n: i32) -> i32 {
        //   if n <= 1 { return 1; }
        //   else { return n * factorial(n - 1); }
        // }
        //
        // Graph structure:
        //   Parameter(n) -> Compare(n <= 1) -> Branch
        //   Branch(true/0) -> Const(1) -> Return
        //   Branch(false/1) -> n-1 -> Call(factorial, n-1) -> n * result -> Return
        //
        // Simplified: use two separate return paths with branch control
        let fact_fn = graph
            .add_function(
                "factorial".into(),
                root,
                vec![("n".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let n_param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, fact_fn).unwrap();
        let const_1 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, fact_fn).unwrap();

        // n <= 1
        let cmp = graph.add_core_op(ComputeOp::Compare { op: CmpOp::Le }, fact_fn).unwrap();
        graph.add_data_edge(n_param, cmp, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_1, cmp, 0, 1, TypeId::I32).unwrap();

        // Branch on comparison
        let branch = graph.add_core_op(ComputeOp::Branch, fact_fn).unwrap();
        graph.add_data_edge(cmp, branch, 0, 0, TypeId::BOOL).unwrap();

        // Base case: return 1
        let ret_base = graph.add_core_op(ComputeOp::Return, fact_fn).unwrap();
        let const_1b = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, fact_fn).unwrap();
        graph.add_data_edge(const_1b, ret_base, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(branch, ret_base, Some(0)).unwrap();

        // Recursive case: n - 1
        let const_1c = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, fact_fn).unwrap();
        let sub = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Sub }, fact_fn).unwrap();
        graph.add_data_edge(n_param, sub, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_1c, sub, 0, 1, TypeId::I32).unwrap();
        graph.add_control_edge(branch, sub, Some(1)).unwrap();

        // Call factorial(n-1)
        let call = graph.add_core_op(ComputeOp::Call { target: fact_fn }, fact_fn).unwrap();
        graph.add_data_edge(sub, call, 0, 0, TypeId::I32).unwrap();

        // n * factorial(n-1)
        let mul = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Mul }, fact_fn).unwrap();
        graph.add_data_edge(n_param, mul, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(call, mul, 0, 1, TypeId::I32).unwrap();

        let ret_rec = graph.add_core_op(ComputeOp::Return, fact_fn).unwrap();
        graph.add_data_edge(mul, ret_rec, 0, 0, TypeId::I32).unwrap();

        // factorial(5) = 120
        let result = run_function(&graph, fact_fn, vec![Value::I32(5)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 120),
            _ => panic!("Expected I32(120), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 8. Recursion depth limit
    // -----------------------------------------------------------------------

    #[test]
    fn integration_recursion_depth_limit() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // infinite_recurse(n) -> infinite_recurse(n)
        // Always recurses (no base case)
        let func_id = graph
            .add_function(
                "infinite".into(),
                root,
                vec![("n".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let call = graph.add_core_op(ComputeOp::Call { target: func_id }, func_id).unwrap();
        graph.add_data_edge(param, call, 0, 0, TypeId::I32).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(call, ret, 0, 0, TypeId::I32).unwrap();

        let config = InterpreterConfig {
            trace_enabled: false,
            max_recursion_depth: 10, // Low limit for quick test
        };
        let mut interp = Interpreter::new(&graph, config);
        interp.start(func_id, vec![Value::I32(1)]);
        interp.run();

        match interp.state() {
            ExecutionState::Error { error, .. } => {
                let msg = format!("{}", error);
                assert!(
                    msg.contains("recursion depth limit"),
                    "Expected recursion limit, got: {}",
                    msg
                );
            }
            other => panic!("Expected Error(RecursionLimitExceeded), got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 9. Step-by-step execution
    // -----------------------------------------------------------------------

    #[test]
    fn integration_step_by_step() {
        let (graph, func_id) = build_add_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![Value::I32(3), Value::I32(5)]);

        // Step through one at a time, verifying we can inspect state
        let mut steps = 0;
        loop {
            interp.step();
            steps += 1;
            match interp.state() {
                ExecutionState::Running => continue,
                ExecutionState::Completed { result } => {
                    match result {
                        Value::I32(v) => assert_eq!(*v, 8),
                        _ => panic!("Expected I32(8), got {:?}", result),
                    }
                    break;
                }
                other => panic!("Unexpected state after step {}: {:?}", steps, other),
            }
        }
        assert!(steps >= 2, "Should take at least 2 steps (add + return)");
    }

    #[test]
    fn integration_step_pause_inspect_resume() {
        let (graph, func_id) = build_add_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![Value::I32(3), Value::I32(5)]);

        // Pause after first step
        interp.pause();
        interp.step();

        match interp.state() {
            ExecutionState::Paused { last_node, last_value } => {
                // We should be able to inspect the intermediate values
                assert!(last_node.0 < 100, "node ID should be reasonable");
                // last_value might be Some (for data-producing nodes) or None
                let _ = last_value; // just verify it's accessible
            }
            ExecutionState::Completed { .. } => {
                // Could complete in one step for trivial functions, that's ok
            }
            other => panic!("Expected Paused or Completed, got {:?}", other),
        }

        // Resume and complete
        interp.resume();
        interp.run();

        match interp.state() {
            ExecutionState::Completed { result } => {
                match result {
                    Value::I32(v) => assert_eq!(*v, 8),
                    _ => panic!("Expected I32(8)"),
                }
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 10. Execution trace
    // -----------------------------------------------------------------------

    #[test]
    fn integration_execution_trace() {
        let (graph, func_id) = build_add_graph();
        let config = InterpreterConfig {
            trace_enabled: true,
            max_recursion_depth: 256,
        };
        let mut interp = Interpreter::new(&graph, config);
        interp.start(func_id, vec![Value::I32(3), Value::I32(5)]);
        interp.run();

        let trace = interp.trace().expect("trace should be Some");
        assert!(trace.len() >= 3, "trace should have entries for params, add, return; got {}", trace.len());

        // Verify the trace entries have node IDs and op descriptions
        for entry in trace {
            assert!(!entry.op_description.is_empty());
        }
    }

    // -----------------------------------------------------------------------
    // 11. Array operations
    // -----------------------------------------------------------------------

    #[test]
    fn integration_array_create_and_get() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("arr_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // Create array [10, 20, 30] then get element at index 1
        let c10 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(10) }, func_id).unwrap();
        let c20 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(20) }, func_id).unwrap();
        let c30 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(30) }, func_id).unwrap();

        let arr_create = graph
            .add_structured_op(StructuredOp::ArrayCreate { length: 3 }, func_id)
            .unwrap();
        graph.add_data_edge(c10, arr_create, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c20, arr_create, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(c30, arr_create, 0, 2, TypeId::I32).unwrap();

        let idx = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, func_id).unwrap();
        let arr_get = graph.add_structured_op(StructuredOp::ArrayGet, func_id).unwrap();
        graph.add_data_edge(arr_create, arr_get, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(idx, arr_get, 0, 1, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(arr_get, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 20),
            _ => panic!("Expected I32(20), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 12. Array out-of-bounds
    // -----------------------------------------------------------------------

    #[test]
    fn integration_array_out_of_bounds() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("arr_oob".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        let c10 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(10) }, func_id).unwrap();
        let c20 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(20) }, func_id).unwrap();
        let c30 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(30) }, func_id).unwrap();

        let arr_create = graph
            .add_structured_op(StructuredOp::ArrayCreate { length: 3 }, func_id)
            .unwrap();
        graph.add_data_edge(c10, arr_create, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c20, arr_create, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(c30, arr_create, 0, 2, TypeId::I32).unwrap();

        // Access index 5 -- out of bounds
        let idx = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(5) }, func_id).unwrap();
        let arr_get = graph.add_structured_op(StructuredOp::ArrayGet, func_id).unwrap();
        graph.add_data_edge(arr_create, arr_get, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(idx, arr_get, 0, 1, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(arr_get, ret, 0, 0, TypeId::I32).unwrap();

        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);
        interp.run();

        match interp.state() {
            ExecutionState::Error { error, .. } => {
                let msg = format!("{}", error);
                assert!(msg.contains("out of bounds"), "Expected OOB error, got: {}", msg);
                assert!(msg.contains("index 5"), "Expected index 5 in error, got: {}", msg);
                assert!(msg.contains("size 3"), "Expected size 3 in error, got: {}", msg);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 13. Struct operations
    // -----------------------------------------------------------------------

    #[test]
    fn integration_struct_create_and_get() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("struct_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // Create struct { x: 42, y: 99 }, get field 0 (x)
        let c42 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(42) }, func_id).unwrap();
        let c99 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(99) }, func_id).unwrap();

        let struct_type = graph.types.register(lmlang_core::LmType::Unit); // placeholder type ID
        let struct_create = graph
            .add_structured_op(StructuredOp::StructCreate { type_id: struct_type }, func_id)
            .unwrap();
        graph.add_data_edge(c42, struct_create, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c99, struct_create, 0, 1, TypeId::I32).unwrap();

        let struct_get = graph
            .add_structured_op(StructuredOp::StructGet { field_index: 0 }, func_id)
            .unwrap();
        graph.add_data_edge(struct_create, struct_get, 0, 0, struct_type).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(struct_get, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 42),
            _ => panic!("Expected I32(42), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // 14. Partial results on error
    // -----------------------------------------------------------------------

    #[test]
    fn integration_partial_results_on_error() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // a = 5, b = 10, c = a / 0 (error -- should include a=5, b=10 in partial)
        let func_id = graph
            .add_function("partial".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        let c5 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(5) }, func_id).unwrap();
        // c10 is independent (not connected to the div chain), proving partial results
        // include values from nodes that succeeded before the error
        let _c10 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(10) }, func_id).unwrap();
        let c0 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(0) }, func_id).unwrap();

        let div = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Div }, func_id).unwrap();
        graph.add_data_edge(c5, div, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c0, div, 0, 1, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(div, ret, 0, 0, TypeId::I32).unwrap();

        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);
        interp.run();

        match interp.state() {
            ExecutionState::Error { error, partial_results } => {
                let msg = format!("{}", error);
                assert!(msg.contains("divide by zero"), "Expected div-by-zero, got: {}", msg);
                // Partial results should contain the Const node values
                assert!(
                    !partial_results.is_empty(),
                    "Expected some partial results, got none"
                );
                // At minimum, the Const(5) and Const(0) and Const(10) nodes should have values
                let has_5 = partial_results.values().any(|v| matches!(v, Value::I32(5)));
                let has_10 = partial_results.values().any(|v| matches!(v, Value::I32(10)));
                assert!(has_5, "Expected partial result containing I32(5)");
                assert!(has_10, "Expected partial result containing I32(10)");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // 15. Memory operations (Alloc, Store, Load)
    // -----------------------------------------------------------------------

    #[test]
    fn integration_memory_alloc_store_load() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("mem_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // alloc -> store(42) -> load -> return
        let alloc = graph.add_core_op(ComputeOp::Alloc, func_id).unwrap();
        let c42 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(42) }, func_id).unwrap();

        // Store: port 0 = pointer, port 1 = value
        let store = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
        graph.add_data_edge(alloc, store, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c42, store, 0, 1, TypeId::I32).unwrap();

        // Load: port 0 = pointer
        let load = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
        graph.add_data_edge(alloc, load, 0, 0, TypeId::I32).unwrap();
        // Control edge: load after store
        graph.add_control_edge(store, load, None).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(load, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 42),
            _ => panic!("Expected I32(42), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // Additional integration tests for completeness
    // -----------------------------------------------------------------------

    #[test]
    fn integration_comparison_operators() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "cmp".into(),
                root,
                vec![("a".into(), TypeId::I32), ("b".into(), TypeId::I32)],
                TypeId::BOOL,
                Visibility::Public,
            )
            .unwrap();

        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let p_b = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let cmp = graph.add_core_op(ComputeOp::Compare { op: CmpOp::Lt }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(p_a, cmp, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(p_b, cmp, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(cmp, ret, 0, 0, TypeId::BOOL).unwrap();

        // 3 < 5 = true
        let result = run_function(&graph, func_id, vec![Value::I32(3), Value::I32(5)]).unwrap();
        assert!(matches!(result, Value::Bool(true)));

        // 5 < 3 = false
        let result = run_function(&graph, func_id, vec![Value::I32(5), Value::I32(3)]).unwrap();
        assert!(matches!(result, Value::Bool(false)));
    }

    #[test]
    fn integration_logic_operators() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "logic".into(),
                root,
                vec![("a".into(), TypeId::BOOL), ("b".into(), TypeId::BOOL)],
                TypeId::BOOL,
                Visibility::Public,
            )
            .unwrap();

        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let p_b = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let and = graph.add_core_op(ComputeOp::BinaryLogic { op: LogicOp::And }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(p_a, and, 0, 0, TypeId::BOOL).unwrap();
        graph.add_data_edge(p_b, and, 0, 1, TypeId::BOOL).unwrap();
        graph.add_data_edge(and, ret, 0, 0, TypeId::BOOL).unwrap();

        // true && false = false
        let result = run_function(&graph, func_id, vec![Value::Bool(true), Value::Bool(false)]).unwrap();
        assert!(matches!(result, Value::Bool(false)));

        // true && true = true
        let result = run_function(&graph, func_id, vec![Value::Bool(true), Value::Bool(true)]).unwrap();
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn integration_not_operator() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "not_fn".into(),
                root,
                vec![("a".into(), TypeId::BOOL)],
                TypeId::BOOL,
                Visibility::Public,
            )
            .unwrap();

        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let not = graph.add_core_op(ComputeOp::Not, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(p_a, not, 0, 0, TypeId::BOOL).unwrap();
        graph.add_data_edge(not, ret, 0, 0, TypeId::BOOL).unwrap();

        let result = run_function(&graph, func_id, vec![Value::Bool(true)]).unwrap();
        assert!(matches!(result, Value::Bool(false)));

        let result = run_function(&graph, func_id, vec![Value::Bool(false)]).unwrap();
        assert!(matches!(result, Value::Bool(true)));
    }

    #[test]
    fn integration_float_arithmetic() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "float_add".into(),
                root,
                vec![("a".into(), TypeId::F64), ("b".into(), TypeId::F64)],
                TypeId::F64,
                Visibility::Public,
            )
            .unwrap();

        let p_a = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let p_b = graph.add_core_op(ComputeOp::Parameter { index: 1 }, func_id).unwrap();
        let add = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(p_a, add, 0, 0, TypeId::F64).unwrap();
        graph.add_data_edge(p_b, add, 0, 1, TypeId::F64).unwrap();
        graph.add_data_edge(add, ret, 0, 0, TypeId::F64).unwrap();

        let result = run_function(&graph, func_id, vec![Value::F64(1.5), Value::F64(2.5)]).unwrap();
        match result {
            Value::F64(v) => assert!((v - 4.0).abs() < 1e-10),
            _ => panic!("Expected F64(4.0), got {:?}", result),
        }
    }

    #[test]
    fn integration_cast_i32_to_i64() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("cast_fn".into(), root, vec![], TypeId::I64, Visibility::Public)
            .unwrap();

        let c42 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(42) }, func_id).unwrap();
        let cast = graph.add_structured_op(StructuredOp::Cast { target_type: TypeId::I64 }, func_id).unwrap();
        graph.add_data_edge(c42, cast, 0, 0, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(cast, ret, 0, 0, TypeId::I64).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I64(v) => assert_eq!(v, 42),
            _ => panic!("Expected I64(42), got {:?}", result),
        }
    }

    #[test]
    fn integration_enum_create_and_discriminant() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("enum_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        let enum_type = graph.types.register(lmlang_core::LmType::Unit); // placeholder

        let payload = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(99) }, func_id).unwrap();
        let enum_create = graph
            .add_structured_op(
                StructuredOp::EnumCreate { type_id: enum_type, variant_index: 2 },
                func_id,
            )
            .unwrap();
        graph.add_data_edge(payload, enum_create, 0, 0, TypeId::I32).unwrap();

        let disc = graph.add_structured_op(StructuredOp::EnumDiscriminant, func_id).unwrap();
        graph.add_data_edge(enum_create, disc, 0, 0, enum_type).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(disc, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 2),
            _ => panic!("Expected I32(2), got {:?}", result),
        }
    }

    #[test]
    fn integration_print_io_log() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("print_fn".into(), root, vec![], TypeId::UNIT, Visibility::Public)
            .unwrap();

        let c42 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(42) }, func_id).unwrap();
        let print = graph.add_core_op(ComputeOp::Print, func_id).unwrap();
        graph.add_data_edge(c42, print, 0, 0, TypeId::I32).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(print, ret, 0, 0, TypeId::UNIT).unwrap();

        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);
        interp.run();

        assert_eq!(interp.io_log().len(), 1);
        match &interp.io_log()[0] {
            Value::I32(v) => assert_eq!(*v, 42),
            _ => panic!("Expected I32(42) in io_log"),
        }
    }

    #[test]
    fn integration_unary_negation() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "neg_fn".into(),
                root,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let param = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();
        let neg = graph.add_core_op(ComputeOp::UnaryArith { op: UnaryArithOp::Neg }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(param, neg, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(neg, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![Value::I32(42)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, -42),
            _ => panic!("Expected I32(-42), got {:?}", result),
        }
    }

    #[test]
    fn integration_shift_operations() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("shift_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // 1 << 3 = 8
        let c1 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, func_id).unwrap();
        let c3 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(3) }, func_id).unwrap();
        let shl = graph.add_core_op(ComputeOp::Shift { op: ShiftOp::Shl }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(c1, shl, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(c3, shl, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(shl, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 8),
            _ => panic!("Expected I32(8), got {:?}", result),
        }
    }

    #[test]
    fn integration_bitwise_logic() {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function("bitwise_fn".into(), root, vec![], TypeId::I32, Visibility::Public)
            .unwrap();

        // 0xFF & 0x0F = 0x0F = 15
        let ca = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(0xFF) }, func_id).unwrap();
        let cb = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(0x0F) }, func_id).unwrap();
        let and = graph.add_core_op(ComputeOp::BinaryLogic { op: LogicOp::And }, func_id).unwrap();
        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph.add_data_edge(ca, and, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(cb, and, 0, 1, TypeId::I32).unwrap();
        graph.add_data_edge(and, ret, 0, 0, TypeId::I32).unwrap();

        let result = run_function(&graph, func_id, vec![]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 15),
            _ => panic!("Expected I32(15), got {:?}", result),
        }
    }

    // -----------------------------------------------------------------------
    // Real Loop op integration test (ComputeOp::Loop with back-edges)
    // -----------------------------------------------------------------------

    /// Builds a function that computes sum(n) = 1 + 2 + ... + n using a real
    /// ComputeOp::Loop with memory-based loop-carried values and back-edges.
    ///
    /// Uses Alloc/Store/Load for loop variables (sum and i). The condition
    /// Load and Compare nodes are part of the loop body (reachable from the
    /// body via control edges from stores), so they get reset on each iteration
    /// and naturally re-evaluate.
    ///
    /// Graph structure:
    /// ```text
    ///   param_n = Parameter(0)              // n: i32
    ///
    ///   // --- Allocate loop variables ---
    ///   alloc_sum = Alloc, alloc_i = Alloc
    ///   store_init: *alloc_sum = 0, *alloc_i = 1
    ///
    ///   // --- Loop header (re-evaluated each iteration) ---
    ///   load_i_hdr = Load(alloc_i)          // initially control-gated by store_init_i
    ///   cond = Compare(Le, load_i_hdr, n)   // also: control from store_i_body (back-edge)
    ///   loop_node = Loop(cond)              // branch 0=continue, 1=exit
    ///
    ///   // --- Loop body (control-gated by Loop branch 0) ---
    ///   load_sum, load_i -> new_sum = sum+i -> store_sum
    ///   load_i + 1 -> next_i -> store_i
    ///   store_i -> control -> load_i_hdr (back-edge: triggers re-evaluation)
    ///
    ///   // --- Exit path (control-gated by Loop branch 1) ---
    ///   load_sum_exit -> Return
    /// ```
    fn build_loop_sum_graph() -> (ProgramGraph, lmlang_core::id::FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "sum_loop".into(),
                root,
                vec![("n".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // --- Parameter ---
        let param_n = graph.add_core_op(ComputeOp::Parameter { index: 0 }, func_id).unwrap();

        // --- Allocate loop variables ---
        let alloc_sum = graph.add_core_op(ComputeOp::Alloc, func_id).unwrap();
        let alloc_i = graph.add_core_op(ComputeOp::Alloc, func_id).unwrap();

        // --- Initial values ---
        let const_0 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(0) }, func_id).unwrap();
        let const_1 = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, func_id).unwrap();
        let const_1_inc = graph.add_core_op(ComputeOp::Const { value: lmlang_core::ConstValue::I32(1) }, func_id).unwrap();

        // --- Store initial values ---
        let store_sum_init = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
        graph.add_data_edge(alloc_sum, store_sum_init, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_0, store_sum_init, 0, 1, TypeId::I32).unwrap();

        let store_i_init = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
        graph.add_data_edge(alloc_i, store_i_init, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_1, store_i_init, 0, 1, TypeId::I32).unwrap();

        // --- Loop header: load i, check condition ---
        // load_i_hdr is control-gated by store_i_init (initial) and later
        // re-triggered by store_i_body control edge (back-edge path).
        let load_i_hdr = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
        graph.add_data_edge(alloc_i, load_i_hdr, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(store_i_init, load_i_hdr, None).unwrap();

        // cond: i <= n (ONE data edge to Loop -- no dual-edge problem)
        let cond = graph.add_core_op(ComputeOp::Compare { op: CmpOp::Le }, func_id).unwrap();
        graph.add_data_edge(load_i_hdr, cond, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(param_n, cond, 0, 1, TypeId::I32).unwrap();

        // loop_node: Loop with condition on port 0
        let loop_node = graph.add_core_op(ComputeOp::Loop, func_id).unwrap();
        graph.add_data_edge(cond, loop_node, 0, 0, TypeId::BOOL).unwrap();

        // --- Loop body (control-gated by Loop branch 0) ---
        let load_sum_body = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
        graph.add_data_edge(alloc_sum, load_sum_body, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(loop_node, load_sum_body, Some(0)).unwrap();

        let load_i_body = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
        graph.add_data_edge(alloc_i, load_i_body, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(loop_node, load_i_body, Some(0)).unwrap();

        // new_sum = sum + i
        let new_sum = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        graph.add_data_edge(load_sum_body, new_sum, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(load_i_body, new_sum, 0, 1, TypeId::I32).unwrap();

        // store_sum_body: *alloc_sum = new_sum
        let store_sum_body = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
        graph.add_data_edge(alloc_sum, store_sum_body, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(new_sum, store_sum_body, 0, 1, TypeId::I32).unwrap();

        // next_i = i + 1
        let next_i = graph.add_core_op(ComputeOp::BinaryArith { op: ArithOp::Add }, func_id).unwrap();
        graph.add_data_edge(load_i_body, next_i, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(const_1_inc, next_i, 0, 1, TypeId::I32).unwrap();

        // store_i_body: *alloc_i = next_i
        let store_i_body = graph.add_core_op(ComputeOp::Store, func_id).unwrap();
        graph.add_data_edge(alloc_i, store_i_body, 0, 0, TypeId::I32).unwrap();
        graph.add_data_edge(next_i, store_i_body, 0, 1, TypeId::I32).unwrap();

        // --- Back-edge: store_i_body -> load_i_hdr (control edge) ---
        // This creates the back-edge: after the body stores, the condition
        // load re-reads i from memory, feeds the condition, and the Loop
        // re-evaluates. The BFS in propagate_control_flow discovers
        // load_i_hdr and cond as body nodes (reachable from store_i_body),
        // so they get reset on each iteration.
        graph.add_control_edge(store_i_body, load_i_hdr, None).unwrap();

        // --- Exit path (control-gated by Loop branch 1) ---
        let load_sum_exit = graph.add_core_op(ComputeOp::Load, func_id).unwrap();
        graph.add_data_edge(alloc_sum, load_sum_exit, 0, 0, TypeId::I32).unwrap();
        graph.add_control_edge(loop_node, load_sum_exit, Some(1)).unwrap();

        let ret = graph.add_core_op(ComputeOp::Return, func_id).unwrap();
        graph.add_data_edge(load_sum_exit, ret, 0, 0, TypeId::I32).unwrap();

        (graph, func_id)
    }

    #[test]
    fn integration_loop_with_real_loop_op() {
        // sum_loop(5) = 1 + 2 + 3 + 4 + 5 = 15
        let (graph, func_id) = build_loop_sum_graph();
        let result = run_function(&graph, func_id, vec![Value::I32(5)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 15, "sum_loop(5) should be 15"),
            _ => panic!("Expected I32(15), got {:?}", result),
        }
    }

    #[test]
    fn integration_loop_with_real_loop_op_n1() {
        // sum_loop(1) = 1
        let (graph, func_id) = build_loop_sum_graph();
        let result = run_function(&graph, func_id, vec![Value::I32(1)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 1, "sum_loop(1) should be 1"),
            _ => panic!("Expected I32(1), got {:?}", result),
        }
    }

    #[test]
    fn integration_loop_with_real_loop_op_n0() {
        // sum_loop(0) = 0 (loop body never executes, sum stays at initial 0)
        let (graph, func_id) = build_loop_sum_graph();
        let result = run_function(&graph, func_id, vec![Value::I32(0)]).unwrap();
        match result {
            Value::I32(v) => assert_eq!(v, 0, "sum_loop(0) should be 0"),
            _ => panic!("Expected I32(0), got {:?}", result),
        }
    }
}
