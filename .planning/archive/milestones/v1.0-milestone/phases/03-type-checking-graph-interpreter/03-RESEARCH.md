# Phase 3: Type Checking & Graph Interpreter - Research

**Researched:** 2026-02-18
**Domain:** Static type verification for graph edges, graph interpreter with step-by-step execution, runtime value representation
**Confidence:** HIGH

## Summary

Phase 3 adds two capabilities to lmlang: (1) static type checking that verifies every data flow edge carries a type compatible with what the target op expects, triggered eagerly on every graph edit, and (2) a graph interpreter that can execute computational graphs with provided inputs, producing results for arithmetic, logic, control flow, memory operations, and function calls without any LLVM dependency.

The type checker is architecturally simple because the graph already carries type information on every data edge (`FlowEdge::Data { value_type: TypeId }`). The checker's job is to define, for each op, what types it expects on each input port and what type it produces, then verify that every edge's `value_type` matches the target port's expected type. The core data structure is a per-op type rule table. Type checking hooks into `ProgramGraph`'s mutation methods (`add_data_edge`, `add_compute_node`) so that errors are caught immediately. All errors in the graph are reported at once (not stop-at-first), with rich diagnostics including nodes, edges, function boundary, and fix suggestions.

The interpreter requires more design: a `Value` enum for runtime values, a `CallFrame` struct for the call stack, an `InterpreterState` state machine for pause/resume, and a topological-sort-based evaluation strategy within each function. The state machine approach (not recursive evaluation) is mandatory because the user requires step-by-step execution with pause/inspect/resume. petgraph's `algo::toposort` provides evaluation order for the data flow DAG. Control flow (IfElse, Loop, Match) requires special handling -- the interpreter cannot simply evaluate all nodes in topological order because branch selection determines which nodes actually execute. The interpreter will use a "work list" approach: start from entry, evaluate ready nodes, follow taken control flow edges, repeat until Return.

**Primary recommendation:** Implement in a new `lmlang-check` crate that depends on `lmlang-core`. Type checker as a module with per-op type rules and an `add_data_edge` wrapper. Interpreter as a separate module with `Value` enum, `CallFrame`, `InterpreterState`, and `step()`/`run()` methods. Keep both stateless relative to `ProgramGraph` -- they read the graph but don't modify it (type errors are returned, not stored in the graph).

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Type error diagnostics:**
- Rich context in diagnostics: nodes involved, surrounding edges, function boundary, and fix suggestions
- Report all type errors in the graph at once (no stop-at-first)
- Include actionable fix suggestions when the fix is obvious (e.g., "insert a cast node from i32 to i64 here")
- Type checking happens eagerly on every graph edit (add_edge, modify_node) -- errors caught immediately

**Type coercion rules:**
- Bool-to-integer implicit conversion allowed (true=1, false=0)

**Interpreter execution model:**
- Step-by-step execution supported -- can pause after each node, inspect intermediate values, then continue
- Optional execution trace -- tracing off by default, enabled via flag to log every node evaluation and its result
- Proper call stack with frames -- each function call pushes a frame, return pops it (supports recursion)
- Configurable recursion depth limit -- default limit with option to increase, error on exceed

**Runtime error handling:**
- Integer overflow: trap (stop execution with overflow error, like Rust debug mode)
- Divide-by-zero: trap (stop execution with error, include the node that caused it)
- Out-of-bounds array access: trap (stop execution with bounds-check error including index and array size)

### Claude's Discretion

- Implicit widening conversion rules (strict vs safe widening)
- Pointer/reference mutability checking scope
- Nominal vs structural struct typing
- Whether runtime errors include partial results for debugging
- Exact recursion depth default value

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CNTR-01 | Static type checking verifies that edge source types match edge sink expected types on every edit | Per-op type rule table, eager checking hooked into ProgramGraph mutation methods, diagnostic error type with rich context |
| EXEC-01 | Graph interpreter walks the computational graph and executes op nodes for development-time feedback without LLVM | Value enum, CallFrame, InterpreterState state machine, topological evaluation within functions, control flow handling, trap-on-error semantics |

</phase_requirements>

## Standard Stack

### Core (Phase 3)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| petgraph | 0.8 (already in workspace) | `edges_directed(node, Direction::Incoming)` for gathering input types/values, `toposort` for evaluation order | Already the graph foundation. Provides the traversal primitives needed for type checking and interpretation. |
| thiserror | 2.0 (already in workspace) | Type error and runtime error enums | Structured, matchable errors with rich context fields. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| lmlang-core | workspace | All type definitions, ops, edges, graph structure | Foundation -- the type checker and interpreter operate ON the core data model. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| New `lmlang-check` crate | Adding modules to `lmlang-core` | Separate crate maintains core as pure data model, avoids circular dependencies, keeps compilation units smaller. Clear architectural boundary. |
| Manual topological walk | petgraph `toposort` | petgraph's toposort handles all graph edge cases, cycle detection, and workspace reuse. |
| Recursive interpreter | State machine interpreter | Recursive is simpler but cannot support pause/resume (user requirement). State machine is mandatory. |

### Cargo.toml for lmlang-check

```toml
[package]
name = "lmlang-check"
version = "0.1.0"
edition = "2021"

[dependencies]
lmlang-core = { path = "../lmlang-core" }
petgraph = { version = "0.8", features = ["serde-1"] }
thiserror = "2.0"

[dev-dependencies]
proptest = "1.10"
```

## Architecture Patterns

### Recommended Project Structure (Phase 3)

```
lmlang/
  crates/
    lmlang-core/          # Existing: types, ops, edges, graph
    lmlang-storage/       # Existing: SQLite persistence
    lmlang-check/         # NEW: type checking + interpretation
      src/
        lib.rs            # Re-exports
        typecheck/
          mod.rs          # Type checker public API
          rules.rs        # Per-op type rules (input types, output type)
          diagnostics.rs  # TypeError, TypeDiagnostic, fix suggestions
          coercion.rs     # Coercion/widening rules
        interpreter/
          mod.rs          # Interpreter public API (run, step)
          value.rs        # Value enum for runtime values
          state.rs        # InterpreterState, CallFrame, execution state machine
          eval.rs         # Per-op evaluation logic
          error.rs        # RuntimeError with trap semantics
          trace.rs        # Execution trace logging
      Cargo.toml
```

### Pattern 1: Per-Op Type Rules Table

**What:** A function that, given a `ComputeNodeOp` and the types of its input edges, returns the expected input port types and the output type. This is the core type checking logic -- it defines the "type signature" of every operation.

**When to use:** Called by the type checker on every edge addition to verify compatibility, and called by the interpreter to validate before execution.

**Example:**
```rust
use lmlang_core::{TypeId, ComputeNodeOp, ComputeOp, ArithOp, LmType, TypeRegistry};

/// Result of resolving the type rule for an op
pub struct OpTypeRule {
    /// Expected types at each input port (port index -> TypeId)
    /// None means "any type" or "type determined by other inputs"
    pub inputs: Vec<PortConstraint>,
    /// Output type (None for ops that produce no value, e.g., Store, Return)
    pub output: Option<TypeId>,
}

pub enum PortConstraint {
    /// Must be exactly this type
    Exact(TypeId),
    /// Must match the type on another port (for binary ops: both inputs same type)
    SameAs(u16),
    /// Must be a boolean
    Bool,
    /// Must be any numeric (integer or float) type
    Numeric,
    /// Must be any integer type
    Integer,
    /// Must be a pointer type
    Pointer,
    /// Unconstrained (any type)
    Any,
}

/// Resolve the type rule for an op given its incoming edge types.
///
/// `input_types` maps target_port -> value_type for all incoming data edges.
pub fn resolve_type_rule(
    op: &ComputeNodeOp,
    input_types: &[(u16, TypeId)],
    registry: &TypeRegistry,
) -> Result<OpTypeRule, TypeError> {
    // ... match on op variant and return constraints + output type
}
```

**Key insight:** The type rules are STATIC per op variant. `BinaryArith { op: ArithOp::Add }` always expects two numeric inputs of the same type and produces the same type as output. `Compare { op: CmpOp::Eq }` expects two inputs of the same type and always produces `Bool`. `Cast { target_type }` expects one input and produces `target_type`. This table can be defined exhaustively.

### Pattern 2: Eager Type Checking via Graph Mutation Hooks

**What:** Type checking hooks into `ProgramGraph`'s mutation methods. When `add_data_edge` is called, the type checker verifies that the edge's `value_type` is compatible with what the target node expects at that port. Rather than modifying `ProgramGraph` directly (which would create a circular dependency from core to check), the approach uses a wrapper type or validation function.

**When to use:** On every graph edit that could affect type validity.

**Two implementation approaches:**

**Approach A: Validation function (recommended):**
```rust
/// Validates whether adding a data edge would be type-valid.
/// Returns Ok(()) if valid, or a list of type errors if not.
/// Called BEFORE the actual edge addition.
pub fn validate_data_edge(
    graph: &ProgramGraph,
    from: NodeId,
    to: NodeId,
    source_port: u16,
    target_port: u16,
    value_type: TypeId,
) -> Result<(), Vec<TypeError>> {
    // 1. Look up the target node's op
    // 2. Look up existing edges to the target node
    // 3. Determine what type the target expects at target_port
    // 4. Check if value_type matches (considering coercion rules)
    // 5. Return errors with full diagnostic context
}

/// Validates the entire graph at once (for initial load or full recheck).
/// Returns all type errors found.
pub fn validate_graph(graph: &ProgramGraph) -> Vec<TypeError> {
    // For each node: verify all incoming edges match expected types
}
```

**Approach B: Wrapper type (alternative):**
```rust
/// A type-checked program graph that validates mutations.
pub struct CheckedProgramGraph {
    inner: ProgramGraph,
}

impl CheckedProgramGraph {
    pub fn add_data_edge(
        &mut self, from: NodeId, to: NodeId,
        source_port: u16, target_port: u16, value_type: TypeId,
    ) -> Result<EdgeId, TypeError> {
        // Validate first, then delegate to inner.add_data_edge
    }
}
```

**Recommendation:** Approach A (validation functions). It keeps the check crate independent of core's mutation API, avoids wrapping the entire ProgramGraph API, and allows calling validation before or after mutations as needed. The user's locked decision is "type checking happens eagerly on every graph edit" -- validation functions support this while leaving the orchestration to the caller (Phase 4 API layer will call validate then mutate).

### Pattern 3: State Machine Interpreter

**What:** An interpreter that uses an explicit state machine for execution, supporting pause/resume and step-by-step execution. The state is an enum that transitions: `Ready -> Running -> (Paused | Completed | Error)`. A `step()` method advances execution by one node. A `run()` method calls `step()` in a loop until completion or error.

**When to use:** Always for the interpreter -- recursive evaluation cannot support pause/resume.

**Example:**
```rust
/// Runtime value type for the interpreter
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),  // Actual f32 at runtime, not f64 storage like ConstValue
    F64(f64),
    Unit,
    Array(Vec<Value>),
    Struct(Vec<Value>),          // Fields in declaration order
    Enum { variant: u32, payload: Box<Value> },
    Pointer(MemoryAddress),      // Index into interpreter's memory
    FunctionRef(FunctionId),     // For function pointers / closures
    Closure { function: FunctionId, captures: Vec<Value> },
}

/// Execution state of the interpreter
pub enum ExecutionState {
    /// Ready to start execution
    Ready,
    /// Currently running (between steps)
    Running,
    /// Paused after evaluating a node -- can inspect and resume
    Paused { last_node: NodeId, last_value: Option<Value> },
    /// Execution completed successfully
    Completed { result: Value },
    /// Execution halted due to a runtime error
    Error(RuntimeError),
}

/// A single call frame on the interpreter's call stack
pub struct CallFrame {
    pub function_id: FunctionId,
    /// Values produced by each node in this function (NodeId -> Value)
    pub node_values: HashMap<NodeId, Value>,
    /// Arguments passed to this function call
    pub arguments: Vec<Value>,
    /// Where to put the return value in the CALLER's frame
    pub return_target: Option<(NodeId, u16)>,
}

/// The graph interpreter
pub struct Interpreter<'g> {
    graph: &'g ProgramGraph,
    state: ExecutionState,
    call_stack: Vec<CallFrame>,
    /// Interpreter memory (for Alloc/Load/Store)
    memory: Vec<Value>,
    /// Execution trace (when enabled)
    trace: Option<Vec<TraceEntry>>,
    /// Configuration
    config: InterpreterConfig,
}

pub struct InterpreterConfig {
    pub trace_enabled: bool,
    pub max_recursion_depth: usize,
}

impl<'g> Interpreter<'g> {
    /// Advance execution by one node. Returns the new state.
    pub fn step(&mut self) -> &ExecutionState { ... }

    /// Run until completion, error, or pause.
    pub fn run(&mut self) -> &ExecutionState {
        loop {
            match self.step() {
                ExecutionState::Running => continue,
                _ => return &self.state,
            }
        }
    }
}
```

### Pattern 4: Work-List Evaluation with Control Flow

**What:** Within a function, nodes are NOT evaluated in a simple topological order because control flow means not all nodes execute. Instead, the interpreter uses a work-list: start with the entry nodes (Parameters), then for each evaluated node, add its data-flow successors to the work list. For control flow nodes (IfElse, Branch), only add the taken branch's successors.

**When to use:** For all interpreter execution within a function.

**Why not plain toposort:** Toposort gives an order over ALL nodes, but control flow means some nodes may not execute (dead branches). The interpreter must follow the actual control flow path, skipping untaken branches.

**Execution within a function:**
```
1. Initialize: push Parameter nodes' values from arguments
2. Work list = data successors of Parameter nodes
3. While work list is not empty:
   a. Pop a node that has all inputs ready (all incoming data edges have values)
   b. Evaluate the node's op using input values
   c. Store the result in the call frame's node_values map
   d. For data edges: add target nodes to work list
   e. For control flow nodes: determine which branch is taken, add ONLY
      that branch's successors to the work list
   f. If node is Return: pop frame, pass value to caller
   g. If node is Call: push new frame, start evaluating callee
```

### Pattern 5: Memory Model for Alloc/Load/Store

**What:** The interpreter needs a simple memory model to support Alloc, Load, Store, and GetElementPtr operations. A flat vector of `Value` slots works as a simple heap. Each `Alloc` reserves a slot (or contiguous range for arrays/structs) and returns a `Pointer(address)`. `Load` reads from the address. `Store` writes to the address.

**When to use:** Whenever the graph uses memory operations.

**Example:**
```rust
type MemoryAddress = usize;

impl Interpreter {
    fn eval_alloc(&mut self) -> Value {
        let addr = self.memory.len();
        self.memory.push(Value::Unit); // placeholder
        Value::Pointer(addr)
    }

    fn eval_load(&self, addr: MemoryAddress) -> Result<Value, RuntimeError> {
        self.memory.get(addr)
            .cloned()
            .ok_or(RuntimeError::OutOfBoundsAccess { address: addr })
    }

    fn eval_store(&mut self, addr: MemoryAddress, value: Value) -> Result<(), RuntimeError> {
        if addr >= self.memory.len() {
            return Err(RuntimeError::OutOfBoundsAccess { address: addr });
        }
        self.memory[addr] = value;
        Ok(())
    }
}
```

### Anti-Patterns to Avoid

- **Storing type errors in the graph:** Type errors are returned to the caller, not stored as graph state. The graph data model should not know about type checking.
- **Recursive interpreter walk:** Cannot support pause/resume. Must use an explicit state machine with a work list.
- **Evaluating ALL nodes via toposort:** Control flow means some nodes are dead. Only evaluate along the actual execution path.
- **Single type error return:** User requires ALL errors at once. Use `Vec<TypeError>` not `Result<(), TypeError>`.
- **Mutable borrow of graph during interpretation:** The interpreter takes `&ProgramGraph` (immutable). Graph modification during execution is not supported (and doesn't make sense).
- **Floating-point comparison for type equality:** TypeId is a u32 newtype. Type equality is integer comparison, which is fine.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Graph traversal for input edges | Manual adjacency list walking | `graph.compute().edges_directed(node_idx, Direction::Incoming)` | petgraph handles index stability, tombstones, direction filtering |
| Evaluation order (data flow DAG) | Custom topological sort | `petgraph::algo::toposort` (as starting point for work list) | Handles cycle detection, workspace reuse, correctness |
| Checked arithmetic overflow | Manual overflow detection | Rust's `checked_add`, `checked_sub`, `checked_mul`, `checked_div` methods | Return `None` on overflow -- maps directly to trap semantics. Part of std library. |
| Error types with context | String-based errors | `thiserror` derive with structured fields | Matchable, displayable, preserves diagnostic context |

**Key insight:** petgraph's `edges_directed(node, Direction::Incoming)` is the workhorse for both type checking (gathering incoming edge types to verify against op rules) and interpretation (gathering input values to evaluate an op). This single API call replaces what would be complex custom traversal logic.

## Common Pitfalls

### Pitfall 1: Type Rule Completeness Gaps

**What goes wrong:** Some op variants are missed in the type rule table, causing the type checker to accept invalid programs or reject valid ones.

**Why it happens:** The op vocabulary has ~34 variants across ComputeOp and StructuredOp. Missing a variant in a match arm is easy, especially for ops with complex type rules (Cast, EnumCreate, IfElse).

**How to avoid:** Use exhaustive `match` statements (Rust compiler enforces this). Write a test that constructs one instance of every op variant and runs it through the type rule resolver. Property-based testing with proptest can generate random ops and verify the rule resolver handles them all.

**Warning signs:** Match arms with `_ => unimplemented!()` or `todo!()`. These will hide missing type rules until runtime.

### Pitfall 2: Control Flow Evaluation Path Errors

**What goes wrong:** The interpreter evaluates nodes in the wrong branch of an IfElse/Match, or fails to evaluate the correct branch at all. Loop back-edges cause infinite evaluation without proper termination.

**Why it happens:** Control flow nodes (IfElse, Loop, Match) have multiple outgoing control edges. The interpreter must choose the correct branch based on the condition value. Getting the branch_index mapping wrong (0=then, 1=else) causes subtle bugs.

**How to avoid:** Clearly document the control edge conventions (branch_index 0 = then/true path, 1 = else/false path). Write explicit tests for each control flow pattern: simple if-then-else, nested conditionals, while loops, counted loops, match with multiple arms. Verify outputs against hand-computed expected values.

**Warning signs:** Tests that pass for straight-line code but fail for code with conditionals or loops.

### Pitfall 3: ConstValue/Value Type Mismatch

**What goes wrong:** The `ConstValue` enum in lmlang-core stores F32 as f64 (for derive safety). The interpreter's `Value` enum stores F32 as actual f32. Converting between them requires care -- precision loss, NaN handling, etc.

**Why it happens:** Phase 1 made a deliberate decision to store F32 constants as f64 in `ConstValue` (to enable `PartialEq` derive). The interpreter needs actual f32 values for correct arithmetic behavior.

**How to avoid:** Explicit conversion in the `Const` evaluation path: `ConstValue::F32(bits) -> Value::F32(bits as f32)`. Document this conversion. Test with values near f32 precision boundaries.

**Warning signs:** Float tests passing with f64 precision but failing when narrowed to f32.

### Pitfall 4: Eager Type Checking Performance on Large Graphs

**What goes wrong:** Checking the ENTIRE graph on every edge addition becomes O(n) per edit where n is total edges, making graph construction O(n^2).

**Why it happens:** "Report all type errors at once" might be interpreted as "recheck everything on every edit."

**How to avoid:** On an individual edge addition, only check the LOCAL type compatibility of that specific edge (source output type matches target input expectation). Full graph validation (`validate_graph`) is separate and called on demand (initial load, explicit request). The "report all errors at once" requirement means that `validate_graph` reports all errors, not that every single `add_data_edge` call re-validates the entire graph.

**Warning signs:** Graph construction benchmarks showing quadratic time growth.

### Pitfall 5: Missing Edge Input During Evaluation

**What goes wrong:** The interpreter tries to evaluate a node before all its input edges have values, causing a "value not found" panic or error.

**Why it happens:** The work-list approach requires checking that ALL inputs are ready before evaluating a node. If a node has two inputs and only one is ready, it must wait.

**How to avoid:** Track readiness explicitly: a node is ready when `incoming_data_edge_count == available_values_count`. Use a readiness counter per node. When a node produces a value, increment the readiness counter of all its data flow successors. Only add a successor to the work list when its readiness counter equals its total incoming data edge count.

**Warning signs:** Non-deterministic test failures, or tests that only pass when nodes happen to be evaluated in a specific order.

## Code Examples

### Gathering Input Types for Type Checking

```rust
use petgraph::Direction;
use petgraph::graph::NodeIndex;
use lmlang_core::graph::ProgramGraph;
use lmlang_core::id::NodeId;
use lmlang_core::edge::FlowEdge;
use lmlang_core::type_id::TypeId;

/// Collect all incoming data edge types for a node, keyed by target_port.
fn incoming_data_types(
    graph: &ProgramGraph,
    node_id: NodeId,
) -> Vec<(u16, TypeId)> {
    let node_idx: NodeIndex<u32> = node_id.into();
    graph.compute()
        .edges_directed(node_idx, Direction::Incoming)
        .filter_map(|edge_ref| {
            match edge_ref.weight() {
                FlowEdge::Data { target_port, value_type, .. } => {
                    Some((*target_port, *value_type))
                }
                FlowEdge::Control { .. } => None,
            }
        })
        .collect()
}
```

### Op Type Rule Resolution (BinaryArith Example)

```rust
/// Determine the expected types and output for BinaryArith ops.
///
/// Rule: Both inputs must be the same numeric type. Output is that same type.
/// Exception: Bool inputs are allowed (implicit bool-to-integer).
fn binary_arith_rule(
    input_types: &[(u16, TypeId)],
    registry: &TypeRegistry,
) -> Result<OpTypeRule, TypeError> {
    // Expect exactly 2 data inputs: port 0 and port 1
    let port0_type = find_port_type(input_types, 0)?;
    let port1_type = find_port_type(input_types, 1)?;

    // Both must be numeric (or bool, which implicitly converts)
    verify_numeric_or_bool(port0_type, registry)?;
    verify_numeric_or_bool(port1_type, registry)?;

    // After coercion, types must match
    let resolved0 = coerce_to_numeric(port0_type);
    let resolved1 = coerce_to_numeric(port1_type);

    if resolved0 != resolved1 {
        return Err(TypeError::TypeMismatch {
            expected: resolved0,
            actual: resolved1,
            context: "binary arithmetic operands must have the same type",
        });
    }

    Ok(OpTypeRule {
        inputs: vec![
            PortConstraint::Exact(resolved0),
            PortConstraint::Exact(resolved1),
        ],
        output: Some(resolved0),
    })
}
```

### Interpreter Step Execution

```rust
impl<'g> Interpreter<'g> {
    /// Execute one node from the work list.
    pub fn step(&mut self) -> &ExecutionState {
        let frame = match self.call_stack.last_mut() {
            Some(f) => f,
            None => {
                self.state = ExecutionState::Error(
                    RuntimeError::InternalError("empty call stack".into())
                );
                return &self.state;
            }
        };

        // Find next ready node from work list
        let node_id = match self.find_next_ready_node() {
            Some(id) => id,
            None => {
                // No ready nodes -- check if we're done or stuck
                self.state = ExecutionState::Error(
                    RuntimeError::DeadlockDetected
                );
                return &self.state;
            }
        };

        let node = self.graph.get_compute_node(node_id).unwrap();

        // Gather input values from incoming edges
        let inputs = self.gather_inputs(node_id);

        // Evaluate the op
        match self.eval_op(&node.op, &inputs, node_id) {
            Ok(result) => {
                // Store result
                if let Some(value) = &result {
                    frame.node_values.insert(node_id, value.clone());
                }
                // Log trace if enabled
                if let Some(trace) = &mut self.trace {
                    trace.push(TraceEntry {
                        node_id,
                        op: format!("{:?}", node.op),
                        inputs: inputs.clone(),
                        output: result.clone(),
                    });
                }
                // Update readiness of successor nodes
                self.propagate_readiness(node_id);
                self.state = ExecutionState::Running;
            }
            Err(e) => {
                self.state = ExecutionState::Error(e);
            }
        }

        &self.state
    }
}
```

### Checked Arithmetic with Trap Semantics

```rust
/// Evaluate binary arithmetic with overflow trapping.
fn eval_binary_arith(
    op: &ArithOp,
    lhs: &Value,
    rhs: &Value,
    node_id: NodeId,
) -> Result<Value, RuntimeError> {
    match (lhs, rhs) {
        (Value::I32(a), Value::I32(b)) => {
            let result = match op {
                ArithOp::Add => a.checked_add(*b),
                ArithOp::Sub => a.checked_sub(*b),
                ArithOp::Mul => a.checked_mul(*b),
                ArithOp::Div => {
                    if *b == 0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a.checked_div(*b)
                }
                ArithOp::Rem => {
                    if *b == 0 {
                        return Err(RuntimeError::DivideByZero { node: node_id });
                    }
                    a.checked_rem(*b)
                }
            };
            result
                .map(Value::I32)
                .ok_or(RuntimeError::IntegerOverflow { node: node_id })
        }
        // ... similar for I8, I16, I64, F32, F64
        _ => Err(RuntimeError::TypeMismatchAtRuntime {
            node: node_id,
            expected: "matching numeric types",
        }),
    }
}
```

## Discretion Recommendations

### Implicit Widening: Allow Safe Widening (i8->i16->i32->i64, f32->f64)

**Recommendation:** Allow implicit widening conversions along the standard chain: i8->i16->i32->i64 and f32->f64. Do NOT allow implicit integer-to-float or float-to-integer conversion (these require explicit Cast nodes). Do NOT allow narrowing conversions implicitly.

**Rationale:**
1. **Safety:** Widening conversions are lossless -- no precision or data loss is possible. This is the same policy as Java, C#, and most typed languages.
2. **Agent ergonomics:** Without implicit widening, every operation mixing i32 and i64 requires the agent to insert an explicit Cast node. This creates graph noise without adding semantic value.
3. **Consistency with bool-to-integer:** If bool->integer is already implicit (locked decision), allowing i32->i64 follows the same "lossless promotion" principle.
4. **Type checker simplicity:** The coercion rules form a simple lattice: bool < i8 < i16 < i32 < i64 and f32 < f64. Integer and float chains are separate (no cross-chain implicit conversion).

### Pointer/Reference Mutability: Enforce &mut -> & (Not Reverse)

**Recommendation:** Allow implicit coercion from `&mut T` to `&T` (mutable to immutable) but NOT the reverse. This matches Rust's borrow semantics.

**Rationale:**
1. **Safety:** Allowing `&T` to flow where `&mut T` is expected would bypass mutability guarantees. The reverse (`&mut T` flowing as `&T`) is safe -- you're just not using the mutation capability.
2. **Familiar to Rust developers:** This is exactly Rust's coercion rule.
3. **Implementation simplicity:** One-directional coercion check on pointer types: if target expects `Pointer { mutable: false }`, accept `Pointer { mutable: true }`. If target expects `Pointer { mutable: true }`, only accept `Pointer { mutable: true }`.

### Struct Type Compatibility: Nominal (Strict)

**Recommendation:** Use strict nominal typing for structs. Two struct types are compatible only if they have the same `TypeId`. Even if two structs have identical fields and field types, they are distinct types unless they share the same TypeId.

**Rationale:**
1. **Consistency with Phase 1 decision:** Phase 1 locked "Nominal typing -- types have names and identity. Two structs with the same fields but different names are different types."
2. **Implementation simplicity:** Type checking is just `TypeId == TypeId`, which is a u32 comparison.
3. **LLVM alignment:** LLVM uses structural typing, but lmlang adds nominal semantics on top. This gives agents clearer type errors ("expected Point, got Coordinate") rather than structural compatibility surprises.
4. **No soundness risk:** Structural typing can silently accept incompatible types that happen to share a layout. Nominal typing prevents this.

### Runtime Errors: Include Partial Results

**Recommendation:** When a runtime error occurs, include the partial results (all values computed before the error) alongside the error. This aids debugging without complicating the error handling path.

**Rationale:**
1. **Debugging value:** When a division by zero occurs at node 15, knowing the values at nodes 0-14 helps the agent understand the state that led to the error.
2. **Implementation simplicity:** The `CallFrame` already stores `node_values: HashMap<NodeId, Value>`. On error, include a snapshot of all frames' node_values in the error. No extra bookkeeping needed.
3. **Development-time tool:** The interpreter is explicitly a development-time tool (not production runtime). Richer error information is always better in a development context.

### Recursion Depth Default: 256

**Recommendation:** Default recursion depth limit of 256 call frames.

**Rationale:**
1. **Generous for development:** Most test programs won't exceed 10-20 levels. 256 allows deeply recursive algorithms (quicksort, tree traversals) without arbitrary constraint.
2. **Finite by default:** Prevents stack overflow from infinite recursion. The error message includes the call stack at the point of limit exceeded.
3. **Configurable:** The user explicitly requested "configurable with option to increase." 256 as default, configurable via `InterpreterConfig::max_recursion_depth`.
4. **Precedent:** Python defaults to 1000, but Python's call frames are much larger. For an interpreter with lightweight frames, 256 is sufficient.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Recursive tree-walking interpreters | State machine interpreters with explicit continuation | Always been the approach for pause/resume | Must use state machine, not recursion, for step-by-step |
| Stop-at-first-error type checking | Collect-all-errors type checking | Modern compilers (Rust, TypeScript) | More helpful diagnostics, especially for AI agents that process all errors at once |
| Runtime type checking only | Eager static verification on edit | Graph-based IRs (Sea of Nodes, Click's approach) | Catches errors immediately, graph is always type-valid |

## Open Questions

1. **High-level control flow node semantics in the interpreter**
   - What we know: IfElse, Loop, and Match are "high-level" ops that lower to Branch/Jump/Phi for LLVM. The interpreter needs to handle both.
   - What's unclear: Should the interpreter handle high-level ops directly (interpreting IfElse as "evaluate condition, then evaluate one branch"), or should it first lower to Branch/Jump/Phi and interpret those? Both approaches work.
   - Recommendation: Interpret high-level ops directly. This is simpler and more aligned with how agents construct graphs. Low-level ops (Branch/Jump/Phi) should also be interpretable for completeness, but the common path uses high-level ops. The key convention: IfElse has control edges with branch_index 0 (then) and 1 (else), and data edges for the condition (port 0) and the results from each branch (ports 0 and 1 of the merge).

2. **Type checking on node addition vs. edge addition**
   - What we know: The user wants type checking "on every graph edit." Edges carry types, so edge addition is the primary check point.
   - What's unclear: Should adding a node alone trigger any type checks? A node with no edges has no type constraints yet.
   - Recommendation: Type check primarily on edge addition (`add_data_edge`). Node addition alone doesn't create type relationships. However, `validate_graph` (full recheck) should also verify that nodes expecting inputs (e.g., BinaryArith) actually have the correct number of incoming edges.

3. **Interpreter handling of I/O ops (Print, ReadLine, FileOpen, etc.)**
   - What we know: The interpreter needs to handle all ops including I/O.
   - What's unclear: Should the interpreter actually perform I/O (write to stdout, read from stdin), or should I/O be mocked/simulated?
   - Recommendation: Provide an I/O trait that the interpreter uses. Default implementation performs actual I/O for development-time testing. Trait allows mocking for automated tests. This enables Phase 4's `simulate` API to capture I/O output programmatically.

## Sources

### Primary (HIGH confidence)
- [petgraph StableGraph documentation](https://docs.rs/petgraph/latest/petgraph/stable_graph/struct.StableGraph.html) -- `edges_directed`, `Direction::Incoming`, `edges` API
- [petgraph toposort documentation](https://docs.rs/petgraph/latest/petgraph/algo/fn.toposort.html) -- topological sort for evaluation order
- Existing lmlang-core codebase (types.rs, ops.rs, edge.rs, graph.rs, node.rs) -- the actual data model this phase operates on
- [Rust std checked arithmetic](https://doc.rust-lang.org/std/primitive.i32.html#method.checked_add) -- `checked_add/sub/mul/div` for trap semantics

### Secondary (MEDIUM confidence)
- [Pretty State Machine Patterns in Rust](https://hoverbear.org/blog/rust-state-machine-pattern/) -- enum-based state machines for Rust
- [rs-graph-llm](https://github.com/a-agmon/rs-graph-llm) -- graph-based workflow with pause/resume execution patterns
- [Rust MIR RFC 1211](https://rust-lang.github.io/rfcs/1211-mir.html) -- SSA-style IR design, type checking at the MIR level

### Tertiary (LOW confidence)
- [graph-flow crate](https://crates.io/crates/graph-flow) -- human-in-the-loop graph execution (similar patterns but different domain)

## Metadata

**Confidence breakdown:**
- Type checker architecture: HIGH -- the graph data model is fully understood, type rules are deterministic per op, petgraph provides the traversal primitives
- Interpreter architecture: HIGH -- state machine pattern is well-established, work-list evaluation is standard for graph-based IRs, Rust's checked arithmetic maps directly to trap semantics
- Per-op type rules: MEDIUM-HIGH -- straightforward for arithmetic/logic/comparison, more complex for control flow nodes and structured ops (need careful convention definition)
- Discretion recommendations: HIGH -- all follow established language design precedent (Rust, Java, LLVM)
- Pitfalls: HIGH -- identified from analysis of the actual codebase and common compiler implementation mistakes

**Research date:** 2026-02-18
**Valid until:** 2026-03-18 (stable domain, 30 days)
