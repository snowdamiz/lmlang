//! Interpreter state machine with step-by-step execution.
//!
//! The [`Interpreter`] uses an explicit state machine for execution, supporting
//! pause/resume and step-by-step debugging. The state transitions are:
//! `Ready -> Running -> (Paused | Completed | Error)`.
//!
//! Execution uses a work-list algorithm: nodes are evaluated when all their
//! data inputs are ready. Control flow nodes (Branch, IfElse, Loop) determine
//! which successor nodes enter the work list.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::visit::EdgeRef;
use petgraph::Direction;

use lmlang_core::edge::FlowEdge;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::{FunctionId, NodeId};
use lmlang_core::ops::{ComputeNodeOp, ComputeOp};

use super::error::RuntimeError;
use super::trace::TraceEntry;
use super::value::Value;

/// Execution state of the interpreter state machine.
#[derive(Debug)]
pub enum ExecutionState {
    /// Ready to start execution (initial state).
    Ready,
    /// Currently executing (between steps).
    Running,
    /// Paused after evaluating a node -- can inspect and resume.
    Paused {
        last_node: NodeId,
        last_value: Option<Value>,
    },
    /// Execution completed successfully with a result value.
    Completed { result: Value },
    /// Execution halted due to a runtime error, with partial results.
    Error {
        error: RuntimeError,
        partial_results: HashMap<NodeId, Value>,
    },
    /// Execution halted due to a contract violation (development-time feedback).
    /// Distinct from Error (runtime crash) -- this is expected feedback about
    /// violated behavioral contracts.
    ContractViolation {
        violation: crate::contracts::ContractViolation,
    },
}

/// A single call frame on the interpreter's call stack.
#[derive(Debug)]
pub struct CallFrame {
    /// Which function this frame is executing.
    pub function_id: FunctionId,
    /// Values produced by each node in this function.
    pub node_values: HashMap<NodeId, Value>,
    /// Arguments passed to this function call.
    pub arguments: Vec<Value>,
    /// Where to store the return value in the caller's frame.
    /// (caller_node_id, port) -- the Call node that will receive the result.
    pub return_target: Option<(NodeId, u16)>,
    /// Nodes ready to evaluate (have all inputs).
    pub work_list: VecDeque<NodeId>,
    /// Readiness counter: node -> number of data inputs that have been provided.
    pub readiness: HashMap<NodeId, usize>,
    /// Closure captures available in this frame (for CaptureAccess ops).
    pub captures: Vec<Value>,
    /// Nodes that have been control-unblocked (have at least one incoming
    /// control edge whose source has been evaluated). Nodes with no incoming
    /// control edges are always control-ready.
    pub control_ready: HashSet<NodeId>,
    /// Set of nodes that require control-gating (have at least one incoming
    /// control edge). Nodes NOT in this set are always control-ready.
    pub control_gated: HashSet<NodeId>,
    /// Tracks which nodes have been evaluated (to avoid double-evaluation).
    pub evaluated: HashSet<NodeId>,
}

/// Configuration for the interpreter.
#[derive(Debug, Clone)]
pub struct InterpreterConfig {
    /// Whether to record execution traces.
    pub trace_enabled: bool,
    /// Maximum recursion depth (call stack frames). Default: 256.
    pub max_recursion_depth: usize,
}

impl Default for InterpreterConfig {
    fn default() -> Self {
        InterpreterConfig {
            trace_enabled: false,
            max_recursion_depth: 256,
        }
    }
}

/// The graph interpreter with state machine execution.
///
/// Holds a reference to the [`ProgramGraph`] and executes its computational
/// graph using a work-list algorithm. Supports step-by-step execution,
/// pause/resume, and optional execution tracing.
pub struct Interpreter<'g> {
    /// The program graph being interpreted.
    graph: &'g ProgramGraph,
    /// Current execution state.
    state: ExecutionState,
    /// Call stack (last element is the current frame).
    call_stack: Vec<CallFrame>,
    /// Flat memory for Alloc/Load/Store operations.
    memory: Vec<Value>,
    /// Execution trace (when enabled).
    trace: Option<Vec<TraceEntry>>,
    /// Configuration.
    config: InterpreterConfig,
    /// Whether a pause has been requested (for pause-after-step).
    pause_requested: bool,
    /// I/O log for capturing Print output.
    pub(crate) io_log: Vec<Value>,
}

impl<'g> Interpreter<'g> {
    /// Creates a new interpreter in the Ready state.
    pub fn new(graph: &'g ProgramGraph, config: InterpreterConfig) -> Self {
        let trace = if config.trace_enabled {
            Some(Vec::new())
        } else {
            None
        };

        Interpreter {
            graph,
            state: ExecutionState::Ready,
            call_stack: Vec::new(),
            memory: Vec::new(),
            trace,
            config,
            pause_requested: false,
            io_log: Vec::new(),
        }
    }

    /// Starts execution of a function with the given arguments.
    ///
    /// Transitions from Ready to Running, initializes the first call frame,
    /// and seeds the work list with Parameter nodes and Const nodes.
    pub fn start(&mut self, function_id: FunctionId, args: Vec<Value>) {
        self.state = ExecutionState::Running;

        let frame = self.create_call_frame(function_id, args, None, Vec::new());
        self.call_stack.push(frame);
    }

    /// Advances execution by one node.
    ///
    /// Pops a ready node from the work list, evaluates it, stores the result,
    /// and updates readiness of successor nodes. Returns the new state.
    pub fn step(&mut self) -> &ExecutionState {
        // Only step if Running
        match &self.state {
            ExecutionState::Running => {}
            ExecutionState::Paused { .. } => {
                // Resume from paused state before stepping
                self.state = ExecutionState::Running;
            }
            _ => return &self.state,
        }

        // Check if call stack is empty
        if self.call_stack.is_empty() {
            self.state = ExecutionState::Error {
                error: RuntimeError::InternalError {
                    message: "empty call stack".into(),
                },
                partial_results: HashMap::new(),
            };
            return &self.state;
        }

        // Find next ready node
        let node_id = match self.find_next_ready_node() {
            Some(id) => id,
            None => {
                // No ready nodes -- check if we should be completed or error
                // If we have no more work but also didn't hit a Return, that's an error
                if self.call_stack.is_empty() {
                    // Should have been completed already
                    return &self.state;
                }
                self.state = ExecutionState::Error {
                    error: RuntimeError::InternalError {
                        message: "no ready nodes in work list (possible deadlock)".into(),
                    },
                    partial_results: self.collect_partial_results(),
                };
                return &self.state;
            }
        };

        // Get the node's op
        let node = match self.graph.get_compute_node(node_id) {
            Some(n) => n,
            None => {
                self.state = ExecutionState::Error {
                    error: RuntimeError::InternalError {
                        message: format!("node {node_id} not found in graph"),
                    },
                    partial_results: self.collect_partial_results(),
                };
                return &self.state;
            }
        };
        let op = node.op.clone();

        // Gather input values
        let inputs = self.gather_inputs(node_id);

        // Evaluate the op
        match self.eval_node(&op, &inputs, node_id) {
            Ok(EvalResult::Value(value)) => {
                // Store result in current frame and mark evaluated
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.node_values.insert(node_id, value.clone());
                    frame.evaluated.insert(node_id);
                }

                // Record trace
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op_description: format!("{:?}", op),
                        inputs: inputs.clone(),
                        output: Some(value.clone()),
                    });
                }

                // Propagate readiness to successors
                self.propagate_readiness(node_id, &op);

                // Check if pause was requested
                if self.pause_requested {
                    self.pause_requested = false;
                    self.state = ExecutionState::Paused {
                        last_node: node_id,
                        last_value: Some(value),
                    };
                } else {
                    self.state = ExecutionState::Running;
                }
            }
            Ok(EvalResult::NoValue) => {
                // Mark as evaluated
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.evaluated.insert(node_id);
                }
                // Ops like Store, Branch produce no value
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op_description: format!("{:?}", op),
                        inputs: inputs.clone(),
                        output: None,
                    });
                }

                self.propagate_readiness(node_id, &op);

                if self.pause_requested {
                    self.pause_requested = false;
                    self.state = ExecutionState::Paused {
                        last_node: node_id,
                        last_value: None,
                    };
                } else {
                    self.state = ExecutionState::Running;
                }
            }
            Ok(EvalResult::Return(value)) => {
                // Record trace before popping
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op_description: format!("{:?}", op),
                        inputs: inputs.clone(),
                        output: Some(value.clone()),
                    });
                }

                // Pop current frame
                let frame = self.call_stack.pop().unwrap();

                if self.call_stack.is_empty() {
                    // Top-level function returned -- execution complete
                    self.state = ExecutionState::Completed {
                        result: value,
                    };
                } else {
                    // Store return value in caller's frame at return_target
                    if let Some((target_node, _target_port)) = frame.return_target {
                        if let Some(caller_frame) = self.call_stack.last_mut() {
                            caller_frame.node_values.insert(target_node, value);
                            // The Call node now has its value; propagate readiness
                            // from the Call node to its successors
                            self.propagate_readiness_for_call_return(target_node);
                        }
                    }
                    self.state = ExecutionState::Running;
                }
            }
            Ok(EvalResult::Call {
                target,
                args,
                return_target,
                captures,
            }) => {
                // Record trace
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op_description: format!("{:?}", op),
                        inputs: inputs.clone(),
                        output: None,
                    });
                }

                // Check recursion depth
                if self.call_stack.len() >= self.config.max_recursion_depth {
                    self.state = ExecutionState::Error {
                        error: RuntimeError::RecursionLimitExceeded {
                            node: node_id,
                            limit: self.config.max_recursion_depth,
                        },
                        partial_results: self.collect_partial_results(),
                    };
                    return &self.state;
                }

                // --- Module-boundary invariant checking ---
                // Check invariants for arguments crossing module boundaries.
                // Must happen BEFORE pushing the callee frame because invariant
                // subgraphs are evaluated on-the-fly (the callee frame doesn't
                // exist yet).
                if let Some(caller_func_id) = self.call_stack.last().map(|f| f.function_id) {
                    if let (Some(caller_func), Some(target_func)) = (
                        self.graph.get_function(caller_func_id),
                        self.graph.get_function(target),
                    ) {
                        if caller_func.module != target_func.module {
                            // Cross-module call: check invariants for each typed argument
                            for (idx, arg_value) in args.iter().enumerate() {
                                if let Some((_, type_id)) = target_func.params.get(idx) {
                                    let violations = crate::contracts::check::check_invariants_for_value(
                                        self.graph, *type_id, arg_value, target,
                                    );
                                    match violations {
                                        Ok(v) => {
                                            if let Some(violation) = v.into_iter().next() {
                                                self.state = ExecutionState::ContractViolation { violation };
                                                return &self.state;
                                            }
                                        }
                                        Err(error) => {
                                            self.state = ExecutionState::Error {
                                                error,
                                                partial_results: self.collect_partial_results(),
                                            };
                                            return &self.state;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // --- End module-boundary invariant checking ---

                // Push new frame
                let frame = self.create_call_frame(target, args, Some(return_target), captures);
                self.call_stack.push(frame);
                self.state = ExecutionState::Running;
            }
            Ok(EvalResult::ContractViolated { violation }) => {
                // Record trace before halting
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op_description: format!("{:?}", op),
                        inputs: inputs.clone(),
                        output: None,
                    });
                }

                self.state = ExecutionState::ContractViolation { violation };
            }
            Err(error) => {
                self.state = ExecutionState::Error {
                    partial_results: self.collect_partial_results(),
                    error,
                };
            }
        }

        &self.state
    }

    /// Runs execution until Completed, Error, or Paused.
    pub fn run(&mut self) -> &ExecutionState {
        loop {
            self.step();
            match &self.state {
                ExecutionState::Running => continue,
                _ => return &self.state,
            }
        }
    }

    /// Requests a pause after the current/next step completes.
    pub fn pause(&mut self) {
        self.pause_requested = true;
    }

    /// Resumes execution from a Paused state.
    pub fn resume(&mut self) {
        if matches!(&self.state, ExecutionState::Paused { .. }) {
            self.state = ExecutionState::Running;
        }
    }

    /// Returns the current execution state.
    pub fn state(&self) -> &ExecutionState {
        &self.state
    }

    /// Returns the execution trace (if tracing was enabled).
    pub fn trace(&self) -> Option<&[TraceEntry]> {
        self.trace.as_deref()
    }

    /// Returns the I/O log (values printed via Print ops).
    pub fn io_log(&self) -> &[Value] {
        &self.io_log
    }

    /// Returns a reference to the interpreter's memory.
    pub fn memory(&self) -> &[Value] {
        &self.memory
    }

    /// Returns a mutable reference to the interpreter's memory.
    pub(crate) fn memory_mut(&mut self) -> &mut Vec<Value> {
        &mut self.memory
    }

    /// Returns the call stack depth.
    pub fn call_depth(&self) -> usize {
        self.call_stack.len()
    }

    /// Returns the program graph reference.
    pub(crate) fn graph(&self) -> &'g ProgramGraph {
        self.graph
    }

    /// Returns the config.
    pub fn config(&self) -> &InterpreterConfig {
        &self.config
    }

    // -----------------------------------------------------------------------
    // Internal methods
    // -----------------------------------------------------------------------

    /// Creates a new call frame for a function, seeding the work list.
    fn create_call_frame(
        &self,
        function_id: FunctionId,
        args: Vec<Value>,
        return_target: Option<(NodeId, u16)>,
        captures: Vec<Value>,
    ) -> CallFrame {
        let mut frame = CallFrame {
            function_id,
            node_values: HashMap::new(),
            arguments: args.clone(),
            return_target,
            work_list: VecDeque::new(),
            readiness: HashMap::new(),
            captures,
            control_ready: HashSet::new(),
            control_gated: HashSet::new(),
            evaluated: HashSet::new(),
        };

        // Find all nodes owned by this function
        let function_nodes = self.graph.function_nodes(function_id);

        // First pass: identify control-gated nodes
        for &node_id in &function_nodes {
            let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
            let has_control_input = self
                .graph
                .compute()
                .edges_directed(node_idx, Direction::Incoming)
                .any(|e| e.weight().is_control());
            if has_control_input {
                frame.control_gated.insert(node_id);
            }
        }

        // Second pass: seed the work list
        for &node_id in &function_nodes {
            if let Some(node) = self.graph.get_compute_node(node_id) {
                match &node.op {
                    ComputeNodeOp::Core(ComputeOp::Parameter { index }) => {
                        // Store argument value immediately
                        if let Some(arg) = args.get(*index as usize) {
                            frame.node_values.insert(node_id, arg.clone());
                        }
                        frame.work_list.push_back(node_id);
                    }
                    ComputeNodeOp::Core(ComputeOp::Const { .. }) => {
                        // Const nodes are always ready -- but if control-gated,
                        // they wait for control
                        if !frame.control_gated.contains(&node_id) {
                            frame.work_list.push_back(node_id);
                        }
                    }
                    ComputeNodeOp::Core(ComputeOp::CaptureAccess { .. }) => {
                        if !frame.control_gated.contains(&node_id) {
                            frame.work_list.push_back(node_id);
                        }
                    }
                    // Nodes that are seedable with no data inputs
                    ComputeNodeOp::Core(ComputeOp::Alloc)
                    | ComputeNodeOp::Core(ComputeOp::ReadLine) => {
                        if !frame.control_gated.contains(&node_id) {
                            frame.work_list.push_back(node_id);
                        }
                        frame.readiness.insert(node_id, 0);
                    }
                    _ => {
                        // All other nodes require data inputs to become ready
                        frame.readiness.insert(node_id, 0);
                    }
                }
            }
        }

        frame
    }

    /// Finds the next ready node from the current frame's work list.
    fn find_next_ready_node(&mut self) -> Option<NodeId> {
        let frame = self.call_stack.last_mut()?;

        // Pop from work list -- nodes in the work list should be ready
        // but we verify readiness for non-seed nodes
        let mut deferred = VecDeque::new();

        while let Some(node_id) = frame.work_list.pop_front() {
            // Skip already evaluated nodes
            if frame.evaluated.contains(&node_id) {
                continue;
            }

            // Check control readiness
            let is_control_ready = !frame.control_gated.contains(&node_id)
                || frame.control_ready.contains(&node_id);

            if !is_control_ready {
                deferred.push_back(node_id);
                continue;
            }

            // Check data readiness
            let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
            let expected_inputs = self
                .graph
                .compute()
                .edges_directed(node_idx, Direction::Incoming)
                .filter(|e| e.weight().is_data())
                .count();

            if expected_inputs == 0 {
                // Put deferred back
                while let Some(d) = deferred.pop_front() {
                    frame.work_list.push_back(d);
                }
                return Some(node_id);
            }

            let ready_count = frame.readiness.get(&node_id).copied().unwrap_or(0);
            if ready_count >= expected_inputs {
                while let Some(d) = deferred.pop_front() {
                    frame.work_list.push_back(d);
                }
                return Some(node_id);
            }
            // Not yet ready; defer
            deferred.push_back(node_id);
        }

        // Put all deferred nodes back
        while let Some(d) = deferred.pop_front() {
            frame.work_list.push_back(d);
        }

        None
    }

    /// Gathers input values for a node from the current frame's node_values.
    fn gather_inputs(&self, node_id: NodeId) -> Vec<(u16, Value)> {
        let frame = match self.call_stack.last() {
            Some(f) => f,
            None => return Vec::new(),
        };

        let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
        let mut inputs: Vec<(u16, Value)> = self
            .graph
            .compute()
            .edges_directed(node_idx, Direction::Incoming)
            .filter_map(|edge_ref| match edge_ref.weight() {
                FlowEdge::Data { target_port, .. } => {
                    let source_id = NodeId::from(edge_ref.source());
                    frame
                        .node_values
                        .get(&source_id)
                        .map(|v| (*target_port, v.clone()))
                }
                FlowEdge::Control { .. } => None,
            })
            .collect();

        inputs.sort_by_key(|(port, _)| *port);
        inputs
    }

    /// Evaluates a single node operation and returns the result.
    fn eval_node(
        &mut self,
        op: &ComputeNodeOp,
        inputs: &[(u16, Value)],
        node_id: NodeId,
    ) -> Result<EvalResult, RuntimeError> {
        use super::eval::eval_op;

        match op {
            ComputeNodeOp::Core(ComputeOp::Return) => {
                // Return takes port 0 as the return value
                let value = inputs
                    .iter()
                    .find(|(port, _)| *port == 0)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(Value::Unit);
                Ok(EvalResult::Return(value))
            }
            ComputeNodeOp::Core(ComputeOp::Call { target }) => {
                // Gather arguments from data inputs (sorted by port)
                let args: Vec<Value> = inputs.iter().map(|(_, v)| v.clone()).collect();
                Ok(EvalResult::Call {
                    target: *target,
                    args,
                    return_target: (node_id, 0),
                    captures: Vec::new(),
                })
            }
            ComputeNodeOp::Core(ComputeOp::IndirectCall) => {
                // Port 0 is the function reference, remaining ports are args
                if inputs.is_empty() {
                    return Err(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    });
                }
                let func_val = &inputs[0].1;
                match func_val {
                    Value::FunctionRef(fid) => {
                        let args: Vec<Value> =
                            inputs.iter().skip(1).map(|(_, v)| v.clone()).collect();
                        Ok(EvalResult::Call {
                            target: *fid,
                            args,
                            return_target: (node_id, 0),
                            captures: Vec::new(),
                        })
                    }
                    Value::Closure { function, captures } => {
                        let args: Vec<Value> =
                            inputs.iter().skip(1).map(|(_, v)| v.clone()).collect();
                        Ok(EvalResult::Call {
                            target: *function,
                            args,
                            return_target: (node_id, 0),
                            captures: captures.clone(),
                        })
                    }
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "FunctionRef or Closure".into(),
                        got: func_val.type_name().into(),
                    }),
                }
            }
            ComputeNodeOp::Core(ComputeOp::Parameter { index }) => {
                // Parameter values are pre-seeded in the frame
                let frame = self.call_stack.last().ok_or_else(|| {
                    RuntimeError::InternalError {
                        message: "no call frame for Parameter".into(),
                    }
                })?;
                let value = frame
                    .arguments
                    .get(*index as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                Ok(EvalResult::Value(value))
            }
            ComputeNodeOp::Core(ComputeOp::CaptureAccess { index }) => {
                let frame = self.call_stack.last().ok_or_else(|| {
                    RuntimeError::InternalError {
                        message: "no call frame for CaptureAccess".into(),
                    }
                })?;
                let value = frame
                    .captures
                    .get(*index as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                Ok(EvalResult::Value(value))
            }
            ComputeNodeOp::Core(ComputeOp::Print) => {
                // Log the value to the I/O log
                if let Some((_, v)) = inputs.first() {
                    self.io_log.push(v.clone());
                }
                Ok(EvalResult::Value(Value::Unit))
            }
            ComputeNodeOp::Core(ComputeOp::ReadLine) => {
                // Placeholder: return I64(0) as per plan
                Ok(EvalResult::Value(Value::I64(0)))
            }
            ComputeNodeOp::Core(
                ComputeOp::FileOpen
                | ComputeOp::FileRead
                | ComputeOp::FileWrite
                | ComputeOp::FileClose,
            ) => {
                // Placeholder I/O ops
                Ok(EvalResult::Value(Value::I64(0)))
            }
            ComputeNodeOp::Core(ComputeOp::Alloc) => {
                let addr = self.memory.len();
                self.memory.push(Value::Unit);
                Ok(EvalResult::Value(Value::Pointer(addr)))
            }
            ComputeNodeOp::Core(ComputeOp::Load) => {
                // Port 0 is the pointer
                let ptr = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    })?;
                match ptr {
                    Value::Pointer(addr) => {
                        if *addr >= self.memory.len() {
                            Err(RuntimeError::OutOfBoundsAccess {
                                node: node_id,
                                index: *addr,
                                size: self.memory.len(),
                            })
                        } else {
                            Ok(EvalResult::Value(self.memory[*addr].clone()))
                        }
                    }
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "Pointer".into(),
                        got: ptr.type_name().into(),
                    }),
                }
            }
            ComputeNodeOp::Core(ComputeOp::Store) => {
                // Port 0: pointer, Port 1: value
                let ptr = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    })?;
                let val = inputs
                    .iter()
                    .find(|(p, _)| *p == 1)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 1,
                    })?;
                match ptr {
                    Value::Pointer(addr) => {
                        if *addr >= self.memory.len() {
                            Err(RuntimeError::OutOfBoundsAccess {
                                node: node_id,
                                index: *addr,
                                size: self.memory.len(),
                            })
                        } else {
                            self.memory[*addr] = val.clone();
                            Ok(EvalResult::NoValue)
                        }
                    }
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "Pointer".into(),
                        got: ptr.type_name().into(),
                    }),
                }
            }
            ComputeNodeOp::Core(ComputeOp::GetElementPtr) => {
                // Port 0: base pointer, Port 1: index
                let base = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    })?;
                let index = inputs
                    .iter()
                    .find(|(p, _)| *p == 1)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 1,
                    })?;
                match (base, index) {
                    (Value::Pointer(addr), idx) => {
                        let offset = value_to_usize(idx, node_id)?;
                        Ok(EvalResult::Value(Value::Pointer(addr + offset)))
                    }
                    _ => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "Pointer".into(),
                        got: base.type_name().into(),
                    }),
                }
            }
            ComputeNodeOp::Core(ComputeOp::MakeClosure { function }) => {
                let captures: Vec<Value> = inputs.iter().map(|(_, v)| v.clone()).collect();
                Ok(EvalResult::Value(Value::Closure {
                    function: *function,
                    captures,
                }))
            }
            // Control flow ops -- Branch/IfElse/Loop/Match/Jump/Phi handled here
            ComputeNodeOp::Core(ComputeOp::Branch) | ComputeNodeOp::Core(ComputeOp::IfElse) => {
                // Condition at port 0
                let cond = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    })?;
                let taken = match cond {
                    Value::Bool(b) => *b,
                    _ => {
                        return Err(RuntimeError::TypeMismatchAtRuntime {
                            node: node_id,
                            expected: "Bool".into(),
                            got: cond.type_name().into(),
                        })
                    }
                };
                // Store the branch decision so propagate_readiness can use it
                if let Some(frame) = self.call_stack.last_mut() {
                    // Store a value indicating which branch was taken
                    // true = branch_index 0, false = branch_index 1
                    frame
                        .node_values
                        .insert(node_id, Value::Bool(taken));
                }
                Ok(EvalResult::NoValue)
            }
            ComputeNodeOp::Core(ComputeOp::Loop) => {
                // Loop condition at port 0
                let cond = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v);
                if let Some(cond_val) = cond {
                    if let Some(frame) = self.call_stack.last_mut() {
                        frame.node_values.insert(node_id, cond_val.clone());
                    }
                }
                Ok(EvalResult::NoValue)
            }
            ComputeNodeOp::Core(ComputeOp::Match) => {
                // Discriminant at port 0
                let disc = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v)
                    .ok_or(RuntimeError::MissingValue {
                        node: node_id,
                        port: 0,
                    })?;
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.node_values.insert(node_id, disc.clone());
                }
                Ok(EvalResult::NoValue)
            }
            ComputeNodeOp::Core(ComputeOp::Jump) => {
                // No-op for evaluation; control flow handled by propagate_readiness
                Ok(EvalResult::NoValue)
            }
            ComputeNodeOp::Core(ComputeOp::Phi) => {
                // Phi: select value from the control flow path that was actually taken.
                //
                // Convention: incoming control edges from a Branch node carry
                // branch_index. The branch node stores its Bool decision as a
                // node_value. We map: true -> branch_index=0 -> data port 0,
                // false -> branch_index=1 -> data port 1.
                let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();

                let frame = self.call_stack.last().ok_or_else(|| {
                    RuntimeError::InternalError {
                        message: "no call frame for Phi".into(),
                    }
                })?;

                let mut selected_port: Option<u16> = None;

                // Look at incoming control edges from branch nodes
                for edge_ref in self.graph.compute().edges_directed(node_idx, Direction::Incoming) {
                    if let FlowEdge::Control { branch_index: Some(_) } = edge_ref.weight() {
                        let source_id = NodeId::from(edge_ref.source());
                        if let Some(branch_val) = frame.node_values.get(&source_id) {
                            match branch_val {
                                Value::Bool(true) => { selected_port = Some(0); }
                                Value::Bool(false) => { selected_port = Some(1); }
                                _ => {}
                            }
                        }
                    }
                }

                let value = if let Some(port) = selected_port {
                    inputs
                        .iter()
                        .find(|(p, _)| *p == port)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Unit)
                } else {
                    // No control info available -- take first input
                    inputs
                        .iter()
                        .find(|(_, _)| true)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Unit)
                };
                Ok(EvalResult::Value(value))
            }
            // Contract ops: check condition and halt with ContractViolation if false
            ComputeNodeOp::Core(
                ComputeOp::Precondition { message }
                | ComputeOp::Postcondition { message }
            ) => {
                // Port 0 is the condition (Bool)
                let condition = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v);
                match condition {
                    Some(Value::Bool(true)) => Ok(EvalResult::NoValue),
                    Some(Value::Bool(false)) => {
                        let kind = match op {
                            ComputeNodeOp::Core(ComputeOp::Precondition { .. }) => {
                                crate::contracts::ContractKind::Precondition
                            }
                            _ => crate::contracts::ContractKind::Postcondition,
                        };
                        let frame = self.call_stack.last().ok_or_else(|| {
                            RuntimeError::InternalError {
                                message: "no call frame for contract check".into(),
                            }
                        })?;
                        let counterexample = crate::contracts::check::collect_counterexample(
                            self.graph,
                            node_id,
                            &frame.node_values,
                        );
                        Ok(EvalResult::ContractViolated {
                            violation: crate::contracts::ContractViolation {
                                kind,
                                contract_node: node_id,
                                function_id: frame.function_id,
                                message: message.clone(),
                                inputs: frame.arguments.clone(),
                                actual_return: None,
                                counterexample,
                            },
                        })
                    }
                    Some(other) => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "Bool".into(),
                        got: other.type_name().into(),
                    }),
                    None => {
                        // No condition input -- treat as passed
                        Ok(EvalResult::NoValue)
                    }
                }
            }
            ComputeNodeOp::Core(ComputeOp::Invariant { message, .. }) => {
                let condition = inputs
                    .iter()
                    .find(|(p, _)| *p == 0)
                    .map(|(_, v)| v);
                match condition {
                    Some(Value::Bool(true)) => Ok(EvalResult::NoValue),
                    Some(Value::Bool(false)) => {
                        let frame = self.call_stack.last().ok_or_else(|| {
                            RuntimeError::InternalError {
                                message: "no call frame for contract check".into(),
                            }
                        })?;
                        let counterexample = crate::contracts::check::collect_counterexample(
                            self.graph,
                            node_id,
                            &frame.node_values,
                        );
                        Ok(EvalResult::ContractViolated {
                            violation: crate::contracts::ContractViolation {
                                kind: crate::contracts::ContractKind::Invariant,
                                contract_node: node_id,
                                function_id: frame.function_id,
                                message: message.clone(),
                                inputs: frame.arguments.clone(),
                                actual_return: None,
                                counterexample,
                            },
                        })
                    }
                    Some(other) => Err(RuntimeError::TypeMismatchAtRuntime {
                        node: node_id,
                        expected: "Bool".into(),
                        got: other.type_name().into(),
                    }),
                    None => Ok(EvalResult::NoValue),
                }
            }

            // Delegate all other ops (arithmetic, logic, comparison, structured) to eval_op
            _ => {
                match eval_op(op, inputs, node_id, self.graph)? {
                    Some(value) => Ok(EvalResult::Value(value)),
                    None => Ok(EvalResult::NoValue),
                }
            }
        }
    }

    /// Propagates readiness to successor nodes after a node evaluation.
    fn propagate_readiness(&mut self, node_id: NodeId, op: &ComputeNodeOp) {
        let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();

        // Check if this is a control flow node that selects branches
        let is_branch = matches!(
            op,
            ComputeNodeOp::Core(
                ComputeOp::Branch
                    | ComputeOp::IfElse
                    | ComputeOp::Loop
                    | ComputeOp::Match
            )
        );

        if is_branch {
            self.propagate_control_flow(node_id, op);
        } else {
            // Collect successors first (no mutable borrow)
            let successors: Vec<(NodeId, bool)> = self
                .graph
                .compute()
                .edges_directed(node_idx, Direction::Outgoing)
                .filter_map(|edge_ref| match edge_ref.weight() {
                    FlowEdge::Data { .. } => {
                        Some((NodeId::from(edge_ref.target()), true))
                    }
                    FlowEdge::Control { .. } => {
                        Some((NodeId::from(edge_ref.target()), false))
                    }
                })
                .collect();

            // Update readiness counters and control flags
            if let Some(frame) = self.call_stack.last_mut() {
                for &(succ_id, is_data) in &successors {
                    if is_data {
                        let count = frame.readiness.entry(succ_id).or_insert(0);
                        *count += 1;
                    } else {
                        frame.control_ready.insert(succ_id);
                    }
                }
            }

            // Now schedule (separate borrow scope)
            for (succ_id, _) in successors {
                self.try_schedule_node(succ_id);
            }
        }
    }

    /// Tries to schedule a node onto the work list if it is both data-ready
    /// and control-ready. Avoids duplicate scheduling.
    fn try_schedule_node(&mut self, node_id: NodeId) {
        let frame = match self.call_stack.last_mut() {
            Some(f) => f,
            None => return,
        };

        // Already evaluated?
        if frame.evaluated.contains(&node_id) {
            return;
        }

        // Control ready?
        let is_control_ready = !frame.control_gated.contains(&node_id)
            || frame.control_ready.contains(&node_id);
        if !is_control_ready {
            return;
        }

        // Data ready?
        let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();
        let expected_data = self
            .graph
            .compute()
            .edges_directed(node_idx, Direction::Incoming)
            .filter(|e| e.weight().is_data())
            .count();
        let ready_count = frame.readiness.get(&node_id).copied().unwrap_or(0);

        // For seed nodes (Const, Alloc, etc.) expected_data is 0
        if expected_data == 0 || ready_count >= expected_data {
            // Avoid duplicate scheduling
            if !frame.work_list.contains(&node_id) {
                frame.work_list.push_back(node_id);
            }
        }
    }

    /// Handles control flow propagation for Branch/IfElse/Loop/Match.
    fn propagate_control_flow(&mut self, node_id: NodeId, op: &ComputeNodeOp) {
        let node_idx: petgraph::graph::NodeIndex<u32> = node_id.into();

        // Get the branch decision from stored node_values
        let branch_value = self
            .call_stack
            .last()
            .and_then(|f| f.node_values.get(&node_id))
            .cloned();

        // Determine which branch index is taken
        let taken_branch: Option<u16> = match op {
            ComputeNodeOp::Core(ComputeOp::Branch | ComputeOp::IfElse) => {
                match &branch_value {
                    Some(Value::Bool(true)) => Some(0),  // then branch
                    Some(Value::Bool(false)) => Some(1), // else branch
                    _ => None,
                }
            }
            ComputeNodeOp::Core(ComputeOp::Loop) => {
                match &branch_value {
                    Some(Value::Bool(true)) => Some(0),  // continue loop (body)
                    Some(Value::Bool(false)) => Some(1), // exit loop
                    _ => Some(1), // default: exit
                }
            }
            ComputeNodeOp::Core(ComputeOp::Match) => {
                match &branch_value {
                    Some(Value::I32(v)) => Some(*v as u16),
                    Some(Value::I8(v)) => Some(*v as u16),
                    Some(Value::I16(v)) => Some(*v as u16),
                    Some(Value::I64(v)) => Some(*v as u16),
                    Some(Value::Enum { variant, .. }) => Some(*variant as u16),
                    _ => Some(0),
                }
            }
            _ => None,
        };

        // Collect outgoing control edges
        let control_successors: Vec<(NodeId, Option<u16>)> = self
            .graph
            .compute()
            .edges_directed(node_idx, Direction::Outgoing)
            .filter_map(|edge_ref| match edge_ref.weight() {
                FlowEdge::Control { branch_index } => {
                    Some((NodeId::from(edge_ref.target()), *branch_index))
                }
                _ => None,
            })
            .collect();

        // Also collect outgoing data edges
        let data_successors: Vec<NodeId> = self
            .graph
            .compute()
            .edges_directed(node_idx, Direction::Outgoing)
            .filter_map(|edge_ref| match edge_ref.weight() {
                FlowEdge::Data { .. } => Some(NodeId::from(edge_ref.target())),
                _ => None,
            })
            .collect();

        // Collect the activated successors for this branch
        let mut activated_successors = Vec::new();

        // Activate only the taken control branch
        for (succ_id, edge_branch) in &control_successors {
            let should_activate = match (taken_branch, *edge_branch) {
                (Some(taken), Some(edge)) => taken == edge,
                (Some(_), None) => true,
                (None, _) => true,
            };

            if should_activate {
                activated_successors.push(*succ_id);
                // Mark as control-ready and try to schedule
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.control_ready.insert(*succ_id);
                }
            }
        }

        // Loop back-edge re-evaluation: when Loop takes branch 0 (continue),
        // clear the evaluated/value/readiness state of all loop body nodes so
        // they can be re-scheduled and re-evaluated on the next iteration.
        let is_loop_continue = matches!(op, ComputeNodeOp::Core(ComputeOp::Loop))
            && taken_branch == Some(0);

        if is_loop_continue {
            // BFS from activated branch-0 successors through control edges to
            // find all loop body nodes. Stop when hitting the Loop node itself
            // or nodes outside the function.
            let mut body_nodes: Vec<NodeId> = Vec::new();
            let mut visited: HashSet<NodeId> = HashSet::new();
            let mut queue: VecDeque<NodeId> = VecDeque::new();

            for &succ in &activated_successors {
                if !visited.contains(&succ) {
                    visited.insert(succ);
                    queue.push_back(succ);
                    body_nodes.push(succ);
                }
            }

            while let Some(current) = queue.pop_front() {
                let cur_idx: petgraph::graph::NodeIndex<u32> = current.into();
                // Follow outgoing control AND data edges to discover the full loop body
                for edge_ref in self.graph.compute().edges_directed(cur_idx, Direction::Outgoing) {
                    let target = NodeId::from(edge_ref.target());
                    // Don't include the Loop node itself in the body reset set
                    if target == node_id {
                        continue;
                    }
                    if !visited.contains(&target) {
                        visited.insert(target);
                        queue.push_back(target);
                        body_nodes.push(target);
                    }
                }
            }

            // Build a set for O(1) membership checks
            let body_set: HashSet<NodeId> = body_nodes.iter().copied().collect();

            // Pre-compute external readiness for each body node (using
            // immutable borrows of graph and frame). Data edges from nodes
            // OUTSIDE the body that still have values won't re-fire, so we
            // pre-credit them in the readiness counter.
            let frame_values: HashSet<NodeId> = self
                .call_stack
                .last()
                .map(|f| f.node_values.keys().copied().collect())
                .unwrap_or_default();

            let external_ready_counts: Vec<(NodeId, usize)> = body_nodes
                .iter()
                .map(|&body_node| {
                    let bn_idx: petgraph::graph::NodeIndex<u32> = body_node.into();
                    let count = self
                        .graph
                        .compute()
                        .edges_directed(bn_idx, Direction::Incoming)
                        .filter(|e| e.weight().is_data())
                        .filter(|e| {
                            let src = NodeId::from(e.source());
                            !body_set.contains(&src)
                                && src != node_id
                                && frame_values.contains(&src)
                        })
                        .count();
                    (body_node, count)
                })
                .collect();

            // Clear state for all body nodes so they can be re-evaluated
            if let Some(frame) = self.call_stack.last_mut() {
                for &body_node in &body_nodes {
                    frame.evaluated.remove(&body_node);
                    frame.node_values.remove(&body_node);
                    frame.control_ready.remove(&body_node);
                }
                for (body_node, ext_ready) in &external_ready_counts {
                    frame.readiness.insert(*body_node, *ext_ready);
                }
                // The Loop node itself must be re-evaluable when its condition
                // input arrives from the back-edge
                frame.evaluated.remove(&node_id);
                frame.node_values.remove(&node_id);
                frame.readiness.insert(node_id, 0);
            }
        }

        // Re-apply control-ready for activated successors (may have been
        // cleared by loop body reset above)
        if let Some(frame) = self.call_stack.last_mut() {
            for &succ_id in &activated_successors {
                frame.control_ready.insert(succ_id);
            }
        }

        // Now schedule activated successors (after state reset for loops)
        for succ_id in &activated_successors {
            self.try_schedule_node(*succ_id);
        }

        // Data successors from control flow nodes
        for succ_id in data_successors {
            if let Some(frame) = self.call_stack.last_mut() {
                let count = frame.readiness.entry(succ_id).or_insert(0);
                *count += 1;
            }
            self.try_schedule_node(succ_id);
        }
    }

    /// Propagates readiness from a Call node after it receives its return value.
    fn propagate_readiness_for_call_return(&mut self, call_node_id: NodeId) {
        let node_idx: petgraph::graph::NodeIndex<u32> = call_node_id.into();

        let successors: Vec<NodeId> = self
            .graph
            .compute()
            .edges_directed(node_idx, Direction::Outgoing)
            .filter_map(|edge_ref| match edge_ref.weight() {
                FlowEdge::Data { .. } => Some(NodeId::from(edge_ref.target())),
                _ => None,
            })
            .collect();

        for succ_id in successors {
            if let Some(frame) = self.call_stack.last_mut() {
                let count = frame.readiness.entry(succ_id).or_insert(0);
                *count += 1;
            }
            self.try_schedule_node(succ_id);
        }
    }

    /// Collects all partial results from all call frames.
    fn collect_partial_results(&self) -> HashMap<NodeId, Value> {
        let mut results = HashMap::new();
        for frame in &self.call_stack {
            results.extend(frame.node_values.iter().map(|(k, v)| (*k, v.clone())));
        }
        results
    }
}

/// Internal enum for node evaluation results.
pub(crate) enum EvalResult {
    /// Node produced a value.
    Value(Value),
    /// Node produced no value (e.g., Store, Branch).
    NoValue,
    /// Node is a Return -- pop frame.
    Return(Value),
    /// Node is a Call -- push new frame.
    Call {
        target: FunctionId,
        args: Vec<Value>,
        return_target: (NodeId, u16),
        captures: Vec<Value>,
    },
    /// A contract check failed -- halt with violation.
    ContractViolated {
        violation: crate::contracts::ContractViolation,
    },
}

/// Helper to convert a Value to usize (for index/pointer arithmetic).
fn value_to_usize(v: &Value, node_id: NodeId) -> Result<usize, RuntimeError> {
    match v {
        Value::I8(n) => Ok(*n as usize),
        Value::I16(n) => Ok(*n as usize),
        Value::I32(n) => Ok(*n as usize),
        Value::I64(n) => Ok(*n as usize),
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "integer".into(),
            got: v.type_name().into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lmlang_core::ops::{ArithOp, ComputeOp};
    use lmlang_core::type_id::TypeId;
    use lmlang_core::types::Visibility;

    /// Helper: build a graph with a single function containing Const(42) -> Return.
    fn const_return_graph() -> (ProgramGraph, FunctionId) {
        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        let func_id = graph
            .add_function(
                "const_fn".into(),
                root,
                vec![],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        let const_node = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(42),
                },
                func_id,
            )
            .unwrap();
        let ret_node = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph
            .add_data_edge(const_node, ret_node, 0, 0, TypeId::I32)
            .unwrap();

        graph.get_function_mut(func_id).unwrap().entry_node = Some(const_node);

        (graph, func_id)
    }

    /// Helper: build add(a: i32, b: i32) -> i32.
    fn add_graph() -> (ProgramGraph, FunctionId) {
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
            .add_core_op(
                ComputeOp::BinaryArith { op: ArithOp::Add },
                func_id,
            )
            .unwrap();
        let ret_node = graph.add_core_op(ComputeOp::Return, func_id).unwrap();

        graph
            .add_data_edge(param_a, add_node, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(param_b, add_node, 0, 1, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(add_node, ret_node, 0, 0, TypeId::I32)
            .unwrap();

        graph.get_function_mut(func_id).unwrap().entry_node = Some(param_a);

        (graph, func_id)
    }

    #[test]
    fn config_default_values() {
        let config = InterpreterConfig::default();
        assert!(!config.trace_enabled);
        assert_eq!(config.max_recursion_depth, 256);
    }

    #[test]
    fn new_interpreter_is_ready() {
        let (graph, _) = const_return_graph();
        let interp = Interpreter::new(&graph, InterpreterConfig::default());
        assert!(matches!(interp.state(), ExecutionState::Ready));
    }

    #[test]
    fn start_transitions_to_running() {
        let (graph, func_id) = const_return_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);
        assert!(matches!(interp.state(), ExecutionState::Running));
        assert_eq!(interp.call_depth(), 1);
    }

    #[test]
    fn step_single_node_const_return() {
        let (graph, func_id) = const_return_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);

        // Step through: Const -> Return
        // First step evaluates the Const node
        interp.step();
        // May need additional steps depending on work list ordering
        // Run to completion
        interp.run();

        match interp.state() {
            ExecutionState::Completed { result } => {
                match result {
                    Value::I32(v) => assert_eq!(*v, 42),
                    _ => panic!("Expected I32(42), got {:?}", result),
                }
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    #[test]
    fn run_const_return_to_completion() {
        let (graph, func_id) = const_return_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![]);
        interp.run();

        match interp.state() {
            ExecutionState::Completed { result } => {
                match result {
                    Value::I32(v) => assert_eq!(*v, 42),
                    _ => panic!("Expected I32(42), got {:?}", result),
                }
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    #[test]
    fn pause_resume_cycle() {
        let (graph, func_id) = add_graph();
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(func_id, vec![Value::I32(3), Value::I32(5)]);

        // Request pause
        interp.pause();

        // Step -- should pause after evaluating one node
        interp.step();

        assert!(
            matches!(interp.state(), ExecutionState::Paused { .. }),
            "Expected Paused, got {:?}",
            interp.state()
        );

        // Resume and run to completion
        interp.resume();
        interp.run();

        match interp.state() {
            ExecutionState::Completed { result } => {
                match result {
                    Value::I32(v) => assert_eq!(*v, 8),
                    _ => panic!("Expected I32(8), got {:?}", result),
                }
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    #[test]
    fn trace_enabled_records_entries() {
        let (graph, func_id) = const_return_graph();
        let config = InterpreterConfig {
            trace_enabled: true,
            max_recursion_depth: 256,
        };
        let mut interp = Interpreter::new(&graph, config);
        interp.start(func_id, vec![]);
        interp.run();

        let trace = interp.trace().expect("trace should be Some when enabled");
        assert!(
            !trace.is_empty(),
            "trace should contain at least one entry"
        );
    }

    #[test]
    fn trace_disabled_produces_no_trace() {
        let (graph, func_id) = const_return_graph();
        let config = InterpreterConfig {
            trace_enabled: false,
            max_recursion_depth: 256,
        };
        let mut interp = Interpreter::new(&graph, config);
        interp.start(func_id, vec![]);
        interp.run();

        assert!(
            interp.trace().is_none(),
            "trace should be None when disabled"
        );
    }

    #[test]
    fn value_from_const_all_variants() {
        use lmlang_core::types::ConstValue;

        assert!(matches!(
            Value::from_const(&ConstValue::Bool(true)),
            Value::Bool(true)
        ));
        assert!(matches!(
            Value::from_const(&ConstValue::I8(42)),
            Value::I8(42)
        ));
        assert!(matches!(
            Value::from_const(&ConstValue::I16(1000)),
            Value::I16(1000)
        ));
        assert!(matches!(
            Value::from_const(&ConstValue::I32(100_000)),
            Value::I32(100_000)
        ));
        assert!(matches!(
            Value::from_const(&ConstValue::I64(1_000_000)),
            Value::I64(1_000_000)
        ));
        // F32: stored as f64, converted to f32
        let f32_val = Value::from_const(&ConstValue::F32(3.14));
        match f32_val {
            Value::F32(v) => assert!((v - 3.14f32).abs() < 0.01),
            _ => panic!("Expected F32"),
        }
        assert!(matches!(
            Value::from_const(&ConstValue::F64(2.718)),
            Value::F64(v) if (v - 2.718).abs() < 0.001
        ));
        assert!(matches!(
            Value::from_const(&ConstValue::Unit),
            Value::Unit
        ));
    }

    #[test]
    fn test_cross_module_invariant_violation() {
        use lmlang_core::ops::CmpOp;

        let mut graph = ProgramGraph::new("test");
        let root = graph.modules.root_id();

        // Create module B (separate from root module A)
        let mod_b = graph
            .add_module("mod_b".into(), root, Visibility::Public)
            .unwrap();

        // Function "callee" in module B, takes one param (x: I32)
        let callee_id = graph
            .add_function(
                "callee".into(),
                mod_b,
                vec![("x".into(), TypeId::I32)],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // callee: Parameter(0) for x
        let callee_param = graph
            .add_core_op(ComputeOp::Parameter { index: 0 }, callee_id)
            .unwrap();

        // callee: Const(0) for the invariant condition
        let callee_const_zero = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(0),
                },
                callee_id,
            )
            .unwrap();

        // callee: Compare x >= 0
        let callee_cmp = graph
            .add_core_op(ComputeOp::Compare { op: CmpOp::Ge }, callee_id)
            .unwrap();
        graph
            .add_data_edge(callee_param, callee_cmp, 0, 0, TypeId::I32)
            .unwrap();
        graph
            .add_data_edge(callee_const_zero, callee_cmp, 0, 1, TypeId::I32)
            .unwrap();

        // callee: Invariant node targeting I32
        let callee_inv = graph
            .add_core_op(
                ComputeOp::Invariant {
                    target_type: TypeId::I32,
                    message: "x must be non-negative".into(),
                },
                callee_id,
            )
            .unwrap();
        graph
            .add_data_edge(callee_cmp, callee_inv, 0, 0, TypeId::BOOL)
            .unwrap();

        // callee: Return node returning x
        let callee_ret = graph
            .add_core_op(ComputeOp::Return, callee_id)
            .unwrap();
        graph
            .add_data_edge(callee_param, callee_ret, 0, 0, TypeId::I32)
            .unwrap();

        // Function "caller" in root module (module A)
        let caller_id = graph
            .add_function(
                "caller".into(),
                root,
                vec![],
                TypeId::I32,
                Visibility::Public,
            )
            .unwrap();

        // caller: Const(-1) -- will violate the invariant
        let caller_const = graph
            .add_core_op(
                ComputeOp::Const {
                    value: lmlang_core::ConstValue::I32(-1),
                },
                caller_id,
            )
            .unwrap();

        // caller: Call callee with -1
        let caller_call = graph
            .add_core_op(
                ComputeOp::Call { target: callee_id },
                caller_id,
            )
            .unwrap();
        graph
            .add_data_edge(caller_const, caller_call, 0, 0, TypeId::I32)
            .unwrap();

        // caller: Return the call result
        let caller_ret = graph
            .add_core_op(ComputeOp::Return, caller_id)
            .unwrap();
        graph
            .add_data_edge(caller_call, caller_ret, 0, 0, TypeId::I32)
            .unwrap();

        // Run the interpreter starting at caller
        let mut interp = Interpreter::new(&graph, InterpreterConfig::default());
        interp.start(caller_id, vec![]);
        interp.run();

        // Should halt with ContractViolation (invariant)
        match interp.state() {
            ExecutionState::ContractViolation { violation } => {
                assert_eq!(violation.kind, crate::contracts::ContractKind::Invariant);
                assert_eq!(violation.message, "x must be non-negative");
            }
            other => panic!(
                "Expected ContractViolation for cross-module invariant, got {:?}",
                other
            ),
        }
    }
}
