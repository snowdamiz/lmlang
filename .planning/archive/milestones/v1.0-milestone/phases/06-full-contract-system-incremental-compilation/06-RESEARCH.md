# Phase 6: Full Contract System & Incremental Compilation - Research

**Researched:** 2026-02-18
**Domain:** Contract-based verification (pre/post-conditions, invariants, property-based testing) as graph nodes, incremental recompilation via function-level dirty tracking
**Confidence:** HIGH

## Summary

Phase 6 adds two interconnected capabilities to lmlang: (1) a contract system that lets functions declare behavioral constraints (pre-conditions, post-conditions, invariants) as first-class graph nodes checked at development time via the interpreter, and (2) an incremental recompilation system that tracks which functions have changed and only recompiles dirty functions and their dependents.

The contract system introduces three new node types (Precondition, Postcondition, Invariant) to `ComputeNodeOp`. These are full graph nodes whose bodies are subgraphs of existing operations -- any logic expressible in the main graph can appear in a contract. Contracts are development-time constructs: the interpreter evaluates them during simulation (Preconditions at function entry before any body nodes execute, Postconditions after the return value is computed but before it is returned, Invariants at module boundaries when data structures cross module interfaces). Contracts are invisible to the compiler -- the codegen pipeline skips contract nodes entirely, producing zero-overhead compiled binaries.

The incremental recompilation system builds on the existing `hash_function()` infrastructure in `lmlang-storage/src/hash.rs`, which already computes deterministic per-function blake3 hashes from node content + edges. Dirty tracking compares the current function hash against the last-compiled hash to identify which functions need recompilation. The existing compiler compiles functions in a single LLVM Module; incremental compilation compiles each function to its own object file, then links only changed object files with cached unchanged ones. The call graph provides dependent identification -- if function A calls function B and B changes, A must also be recompiled (its Call node targets may have changed signatures). The agent has full visibility into dirty state and can query the recompilation plan before triggering a build.

**Primary recommendation:** Add Precondition/Postcondition/Invariant variants to the existing `ComputeNodeOp` enum, wire them into the interpreter's evaluation loop with structured violation diagnostics, implement property-based testing as an interpreter-driven harness with agent-seeded inputs and system-generated variations, and build incremental compilation using per-function object file caching with blake3 hash-based dirty detection.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Contract model:**
- Contracts are special graph nodes (Precondition, Postcondition, Invariant) -- first-class op types like any other node
- Contract nodes can contain full graph expressions -- any logic expressible in the main graph can appear in a contract subgraph
- Agents build contract subgraphs using existing node/edge mutation API -- no separate contract-specific endpoints needed

**Violation feedback:**
- Contract violations produce structured diagnostics with counterexample values embedded -- agent sees node IDs, the contract that failed, actual vs expected values, and the specific inputs that triggered the failure
- Consistent with Phase 4 decision: errors describe the problem only, agent determines the fix (no fix suggestions)
- Invariant violations on data structures block compilation -- they are errors, not warnings
- Contracts are development-time only -- checked during interpretation/simulation, stripped from compiled binaries

**Property testing strategy:**
- Agent-seeded: agent provides seed inputs and interesting edge cases, system generates randomized variations to test contracts
- Agent-controlled iteration count -- no default, agent always specifies how many iterations per test run
- Test results include detailed execution trace for each failure showing the path through the graph
- Property tests run through the graph interpreter only -- no compiled execution for testing

**Incremental recompilation:**
- Function-level dirty tracking -- if any node in a function changes, the whole function recompiles (matches existing function-scoped LLVM codegen)
- Dirty status visible to agent -- agent can query which functions are dirty, see what will recompile, and get a recompilation plan before triggering
- Contract changes do NOT mark functions dirty for recompilation -- since contracts are dev-only, they don't affect compiled output

### Claude's Discretion
- Whether contract API uses dedicated endpoints for common patterns or purely existing mutations (leaning toward existing mutations given the "full graph expressions" decision)
- Whether contracts are fully separable from function logic or integrated -- pick based on graph architecture
- Dependent identification strategy for incremental recompilation -- call graph analysis vs content hash comparison vs hybrid

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CNTR-02 | Functions support pre-conditions as contract nodes checked at function entry | Precondition op variant in ComputeNodeOp, interpreter checks at function entry before body execution, structured ContractViolation diagnostics |
| CNTR-03 | Functions support post-conditions as contract nodes checked at function return | Postcondition op variant in ComputeNodeOp, interpreter checks after return value computation, diagnostics include actual return value vs contract expectation |
| CNTR-04 | Data structures support invariants checked at module boundaries | Invariant op variant associated with type definitions, checked when values cross module boundaries during interpretation, violations block compilation |
| CNTR-05 | Property-based tests auto-generated from contracts to verify graph behavior across input ranges | Property test harness using interpreter, agent-seeded inputs with system randomization, iteration count agent-controlled, detailed failure traces |
| STORE-05 | Incremental recompilation via red-green dirty node tracking -- only recompile functions whose subgraphs actually changed | Per-function blake3 hash comparison, function-level object file caching, call graph analysis for dependent identification, dirty status queryable by agent |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| blake3 | (already in lmlang-storage) | Deterministic content hashing for dirty detection | Already used for per-function hashing; same library for incremental tracking |
| petgraph | (already in lmlang-core) | Call graph construction for dependent analysis | Already the graph library used throughout the codebase |
| rand | 0.8+ | Random input generation for property-based testing | Standard Rust randomness; used by the system to generate variations from agent seeds |
| serde/serde_json | (already in workspace) | Contract violation diagnostics serialization | Already used throughout for API responses |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| rand_chacha | 0.3+ | Deterministic PRNG seeded from agent-provided seeds | Property tests need reproducibility -- given the same seed, generate the same test inputs |
| tempfile | 3.x (already in lmlang-codegen) | Per-function object file storage during incremental builds | Already used for temp object files; reuse for cached object files |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| blake3 for dirty detection | Content-hash comparison via serde_json | blake3 is already implemented, 10x faster, collision-resistant; serde_json comparison is simpler but slower |
| Call graph for dependents | Content hash of all call targets embedded in function hash | Call graph is more precise (only recompile when signatures change); hash embedding is simpler but triggers unnecessary recompilation on body-only changes in callees |
| rand for property testing | proptest/quickcheck crate | proptest/quickcheck are Rust-native property testing frameworks, but they work with Rust types, not lmlang graph values; the interpreter-based approach needs custom input generation from lmlang TypeIds |

**No new Cargo.toml dependencies required except `rand` and `rand_chacha` in `lmlang-check`.**

## Architecture Patterns

### Recommended Changes Per Crate

```
crates/lmlang-core/src/
  ops.rs              # Add Precondition/Postcondition/Invariant to ComputeOp
  (no new files)

crates/lmlang-check/src/
  interpreter/
    state.rs          # Contract checking hooks at function entry/return
    eval.rs           # Contract node evaluation (subgraph execution)
  contracts/
    mod.rs            # ContractViolation type, contract node discovery
    property.rs       # Property-based test harness (input generation, iteration)
  typecheck/
    mod.rs            # Skip contract nodes during type checking of compiled output

crates/lmlang-codegen/src/
  compiler.rs         # Skip contract nodes during codegen, per-function object caching
  incremental.rs      # Dirty tracking, function hash cache, recompilation plan

crates/lmlang-storage/src/
  hash.rs             # Already has hash_function() -- extend with non-contract hash variant
  incremental.rs      # Compiled hash cache (last-compiled function hashes)

crates/lmlang-server/src/
  schema/
    contracts.rs      # ContractViolation, PropertyTestRequest/Response schemas
    compile.rs        # Extend CompileRequest with incremental flag; DirtyStatus response
  handlers/
    contracts.rs      # Property test endpoint
    compile.rs        # Dirty query endpoint
  service.rs          # Contract checking integration, incremental compilation
```

### Pattern 1: Contract Nodes as ComputeOp Variants

**What:** Add three new variants to `ComputeOp`: `Precondition`, `Postcondition`, and `Invariant`. Each variant carries metadata identifying which function/type it constrains. The contract body is a subgraph of regular compute nodes owned by the same function, connected to the contract node via data edges.

**When to use:** Every contract definition.

**Why:** The user decision requires contracts to be "first-class op types like any other node" and use "existing node/edge mutation API." Adding variants to the existing enum is the natural fit -- no new graph structures, no new mutation endpoints, no new storage schemas.

```rust
// In lmlang-core/src/ops.rs, add to ComputeOp:

/// Precondition check at function entry.
/// The contract body is a subgraph connected to this node via data edges.
/// Port 0 receives a Bool value -- the contract condition.
/// If false at runtime (interpreter), produces a ContractViolation.
/// Ignored by the compiler (dev-only).
Precondition {
    /// Human-readable contract description for diagnostics.
    message: String,
},

/// Postcondition check at function return.
/// Port 0 receives a Bool value -- the contract condition.
/// Port 1 receives the function's return value (for inspection by the
/// contract subgraph).
/// If false at runtime, produces a ContractViolation.
/// Ignored by the compiler.
Postcondition {
    /// Human-readable contract description for diagnostics.
    message: String,
},

/// Data structure invariant.
/// Associated with a TypeId. Checked when values of this type cross
/// module boundaries during interpretation.
/// Port 0 receives a Bool value -- the invariant condition.
/// Port 1 receives the struct/data value being checked.
/// Violations block compilation (they are errors, not warnings).
Invariant {
    /// The type this invariant constrains.
    target_type: TypeId,
    /// Human-readable invariant description for diagnostics.
    message: String,
},
```

**Contract subgraph structure:**

A precondition for `fn add(a: i32, b: i32) -> i32` requiring `a >= 0` would look like:

```text
Parameter(0) ──data(i32)──> Compare(Ge) ──data(Bool)──> Precondition { message: "a must be non-negative" }
                                 ↑
Const(0) ──────data(i32)─────────┘
```

The agent builds this using existing mutations: InsertNode (Compare, Const, Precondition), AddEdge. No new mutation types needed.

### Pattern 2: Interpreter Contract Checking

**What:** The interpreter checks contracts at specific evaluation points -- preconditions at function entry (after parameters are bound but before body execution), postconditions after the return value is computed, invariants at module boundary crossings. Contract checking is implemented as subgraph evaluation within the interpreter.

**When to use:** During simulation (`POST /programs/{id}/simulate`) and property testing.

**Why:** User decision: "Contracts are development-time only -- checked during interpretation/simulation."

```rust
// In interpreter state.rs, when entering a function:

fn enter_function(&mut self, func_id: FunctionId, args: Vec<Value>) {
    // 1. Push call frame with parameters
    self.push_call_frame(func_id, args.clone());

    // 2. Find all Precondition nodes owned by this function
    let precondition_nodes = self.find_contract_nodes(func_id, ContractKind::Precondition);

    // 3. Evaluate each precondition subgraph with current parameter values
    for pre_node_id in precondition_nodes {
        let result = self.evaluate_contract_subgraph(pre_node_id, &args);
        match result {
            Ok(Value::Bool(true)) => { /* contract satisfied, continue */ }
            Ok(Value::Bool(false)) => {
                // Collect counterexample values
                let violation = ContractViolation {
                    kind: ContractKind::Precondition,
                    contract_node: pre_node_id,
                    function_id: func_id,
                    message: self.get_contract_message(pre_node_id),
                    inputs: args.clone(),
                    // The failing input values that triggered the violation
                    counterexample: self.collect_counterexample(pre_node_id),
                };
                self.set_state(ExecutionState::ContractViolation { violation });
                return;
            }
            _ => { /* type error in contract -- report as internal error */ }
        }
    }

    // 4. Proceed with body execution
    self.schedule_body_nodes(func_id);
}

// Similarly for postconditions, when Return node is reached:
fn eval_return(&mut self, return_value: Value) {
    let func_id = self.current_function();
    let postcondition_nodes = self.find_contract_nodes(func_id, ContractKind::Postcondition);

    for post_node_id in postcondition_nodes {
        let result = self.evaluate_contract_subgraph(post_node_id, &[return_value.clone()]);
        match result {
            Ok(Value::Bool(true)) => { /* satisfied */ }
            Ok(Value::Bool(false)) => {
                let violation = ContractViolation {
                    kind: ContractKind::Postcondition,
                    contract_node: post_node_id,
                    function_id: func_id,
                    message: self.get_contract_message(post_node_id),
                    inputs: self.current_args(),
                    actual_return: Some(return_value.clone()),
                    counterexample: self.collect_counterexample(post_node_id),
                };
                self.set_state(ExecutionState::ContractViolation { violation });
                return;
            }
            _ => { /* internal error */ }
        }
    }

    // Proceed with normal return
    self.pop_call_frame(return_value);
}
```

### Pattern 3: Contract Violation Diagnostics

**What:** Structured diagnostics for contract violations that include the contract node ID, the function, the specific inputs that triggered the failure, and actual vs expected values.

**When to use:** Every contract violation detected by the interpreter.

```rust
// In lmlang-check/src/contracts/mod.rs:

/// A contract violation detected during interpretation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractViolation {
    /// What kind of contract was violated.
    pub kind: ContractKind,
    /// The contract node that failed.
    pub contract_node: NodeId,
    /// The function containing the contract.
    pub function_id: FunctionId,
    /// Human-readable contract description.
    pub message: String,
    /// The function inputs that triggered the violation.
    pub inputs: Vec<Value>,
    /// For postconditions: the actual return value.
    pub actual_return: Option<Value>,
    /// Counterexample values from the contract subgraph evaluation.
    /// Maps node IDs to their computed values for the failing execution.
    pub counterexample: HashMap<NodeId, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ContractKind {
    Precondition,
    Postcondition,
    Invariant,
}
```

In the API response (server schema), this maps to:

```rust
// In lmlang-server/src/schema/contracts.rs:

#[derive(Debug, Clone, Serialize)]
pub struct ContractViolationView {
    pub kind: String,  // "precondition", "postcondition", "invariant"
    pub contract_node: NodeId,
    pub function_id: FunctionId,
    pub message: String,
    pub inputs: Vec<serde_json::Value>,
    pub actual_return: Option<serde_json::Value>,
    pub counterexample: Vec<(NodeId, serde_json::Value)>,
    /// Execution trace for the failing contract evaluation (if tracing enabled).
    pub trace: Option<Vec<TraceEntryView>>,
}
```

### Pattern 4: Property-Based Testing Harness

**What:** A test harness that runs a function many times with varied inputs to check if contracts hold. The agent provides seed inputs and edge cases; the system generates randomized variations. The harness uses the interpreter to execute each test case and checks all contracts.

**When to use:** When agent calls the property test endpoint.

```rust
// In lmlang-check/src/contracts/property.rs:

pub struct PropertyTestConfig {
    /// Agent-provided seed inputs (the "interesting" cases).
    pub seeds: Vec<Vec<Value>>,
    /// Number of randomized iterations to run (agent-specified, no default).
    pub iterations: u32,
    /// Random seed for reproducibility.
    pub random_seed: u64,
}

pub struct PropertyTestResult {
    /// Total tests run (seeds + random variations).
    pub total_run: u32,
    /// Number of passing tests.
    pub passed: u32,
    /// All failures, each with full details.
    pub failures: Vec<PropertyTestFailure>,
}

pub struct PropertyTestFailure {
    /// The inputs that caused the failure.
    pub inputs: Vec<Value>,
    /// The contract violation that occurred.
    pub violation: ContractViolation,
    /// Full execution trace for this test case.
    pub trace: Vec<TraceEntry>,
}

/// Generate randomized input variations from a type signature.
///
/// For each parameter type:
/// - Bool: random true/false
/// - I8-I64: random within type range, plus boundary values (0, 1, -1, MIN, MAX)
/// - F32/F64: random float, plus 0.0, -0.0, NaN, Inf, -Inf, epsilon
/// - Array: random elements of the element type
/// - Struct: random values for each field
fn generate_random_inputs(
    params: &[(String, TypeId)],
    registry: &TypeRegistry,
    rng: &mut ChaCha8Rng,
) -> Vec<Value> {
    params.iter().map(|(_, type_id)| generate_random_value(*type_id, registry, rng)).collect()
}
```

### Pattern 5: Incremental Compilation with Per-Function Object Files

**What:** Instead of compiling all functions into a single LLVM Module and emitting one object file, compile each function into its own Module, emit per-function object files, cache them with their blake3 hash, and only recompile functions whose hash has changed. Link all object files (cached + freshly compiled) into the final executable.

**When to use:** Every compilation after the first full build.

**Why:** User decision: "Function-level dirty tracking -- if any node in a function changes, the whole function recompiles."

```rust
// In lmlang-codegen/src/incremental.rs:

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use blake3::Hash;
use lmlang_core::id::FunctionId;

/// Tracks compilation state for incremental builds.
pub struct IncrementalState {
    /// Per-function hash from last successful compilation.
    /// FunctionId -> blake3 hash of the function's subgraph at compile time.
    last_compiled_hashes: HashMap<FunctionId, Hash>,
    /// Directory containing cached per-function object files.
    cache_dir: PathBuf,
}

/// A recompilation plan computed from dirty analysis.
#[derive(Debug, Clone, Serialize)]
pub struct RecompilationPlan {
    /// Functions that need recompilation (their hash changed).
    pub dirty: Vec<FunctionId>,
    /// Functions that are dirty because a dependency changed.
    pub dirty_dependents: Vec<FunctionId>,
    /// Functions that can use cached object files.
    pub cached: Vec<FunctionId>,
    /// Whether any functions are dirty at all.
    pub needs_recompilation: bool,
}

impl IncrementalState {
    /// Compute which functions are dirty by comparing current hashes
    /// against last-compiled hashes.
    pub fn compute_dirty(
        &self,
        current_hashes: &HashMap<FunctionId, Hash>,
        call_graph: &HashMap<FunctionId, Vec<FunctionId>>,
    ) -> RecompilationPlan {
        let mut dirty: Vec<FunctionId> = Vec::new();

        // Phase 1: Direct changes
        for (func_id, current_hash) in current_hashes {
            match self.last_compiled_hashes.get(func_id) {
                Some(last_hash) if last_hash == current_hash => { /* clean */ }
                _ => dirty.push(*func_id), // Changed or new
            }
        }

        // Phase 2: Transitive dependents via call graph
        // If B calls A, and A is dirty, B must recompile too
        let mut dirty_dependents = Vec::new();
        let dirty_set: HashSet<FunctionId> = dirty.iter().copied().collect();
        let reverse_call_graph = reverse_graph(call_graph);

        let mut queue: VecDeque<FunctionId> = dirty.iter().copied().collect();
        let mut visited = dirty_set.clone();

        while let Some(func_id) = queue.pop_front() {
            if let Some(callers) = reverse_call_graph.get(&func_id) {
                for caller in callers {
                    if visited.insert(*caller) {
                        dirty_dependents.push(*caller);
                        queue.push_back(*caller);
                    }
                }
            }
        }

        let all_dirty: HashSet<FunctionId> = dirty.iter()
            .chain(dirty_dependents.iter())
            .copied()
            .collect();

        let cached: Vec<FunctionId> = current_hashes.keys()
            .filter(|f| !all_dirty.contains(f))
            .copied()
            .collect();

        RecompilationPlan {
            needs_recompilation: !dirty.is_empty() || !dirty_dependents.is_empty(),
            dirty,
            dirty_dependents,
            cached,
        }
    }
}
```

### Pattern 6: Contract-Aware Hashing (Contract Changes Don't Dirty Functions)

**What:** The existing `hash_function()` in `lmlang-storage/src/hash.rs` includes ALL nodes owned by a function. For incremental compilation, we need a variant that EXCLUDES contract nodes, since contract changes should not trigger recompilation.

**When to use:** When computing hashes for dirty detection.

```rust
// In lmlang-storage/src/hash.rs, add:

/// Computes a function hash EXCLUDING contract nodes.
/// Used for incremental compilation dirty detection.
/// Contract changes do NOT mark functions dirty (contracts are dev-only).
pub fn hash_function_for_compilation(graph: &ProgramGraph, func_id: FunctionId) -> blake3::Hash {
    let mut func_nodes = graph.function_nodes(func_id);
    func_nodes.sort_by_key(|n| n.0);

    // Filter out contract nodes
    let func_nodes: Vec<NodeId> = func_nodes.into_iter()
        .filter(|node_id| {
            let node = graph.get_compute_node(*node_id).unwrap();
            !is_contract_op(&node.op)
        })
        .collect();

    // Same hashing logic as hash_function() but with filtered nodes
    // ...
}

fn is_contract_op(op: &ComputeNodeOp) -> bool {
    matches!(
        op,
        ComputeNodeOp::Core(ComputeOp::Precondition { .. })
        | ComputeNodeOp::Core(ComputeOp::Postcondition { .. })
        | ComputeNodeOp::Core(ComputeOp::Invariant { .. })
    )
}
```

### Pattern 7: Call Graph Construction for Dependent Identification

**What:** Build a call graph by scanning all `Call { target }` nodes in the program graph. This maps each function to the set of functions it calls, and the reverse map (which functions call it). Used to propagate dirty status.

**When to use:** During incremental compilation planning.

```rust
// In lmlang-codegen/src/incremental.rs:

/// Build the call graph from the program graph.
/// Returns a map from caller -> list of callees.
pub fn build_call_graph(graph: &ProgramGraph) -> HashMap<FunctionId, Vec<FunctionId>> {
    let mut call_graph: HashMap<FunctionId, Vec<FunctionId>> = HashMap::new();

    for (func_id, _func_def) in graph.functions() {
        let mut callees = Vec::new();
        for node_id in graph.function_nodes(*func_id) {
            if let Some(node) = graph.get_compute_node(node_id) {
                if let ComputeNodeOp::Core(ComputeOp::Call { target }) = &node.op {
                    callees.push(*target);
                }
            }
        }
        call_graph.insert(*func_id, callees);
    }

    call_graph
}
```

### Anti-Patterns to Avoid

- **Adding contract-specific mutation endpoints:** The user explicitly decided contracts use the existing node/edge mutation API. Do NOT create endpoints like `POST /programs/{id}/add-precondition`. Instead, agents use `InsertNode { op: Precondition { ... }, owner: func_id }` followed by `AddEdge` to connect the contract subgraph.
- **Checking contracts during compilation:** Contracts are development-time only. The compiler must skip all Precondition/Postcondition/Invariant nodes during codegen. Never emit LLVM IR for contract nodes.
- **Default iteration counts for property tests:** The user decided the agent always specifies the iteration count. There must be no default fallback.
- **Marking functions dirty when only contracts change:** Contract changes do NOT affect compiled output. The dirty detection hash must exclude contract nodes.
- **Providing fix suggestions in contract violations:** Per Phase 4 decision, errors describe the problem only. Contract violation diagnostics describe what failed and with what values, but never suggest how to fix it.

## Discretion Recommendations

### Contract API: Existing Mutations Only (No Dedicated Endpoints)

**Recommendation: Use existing mutations exclusively.**

Rationale: The user leaned toward this approach, and the "full graph expressions" decision makes dedicated endpoints redundant. A precondition is just: (1) InsertNode for the condition computation nodes (Compare, Const, etc.), (2) InsertNode for the Precondition node itself, (3) AddEdge to connect parameter nodes to the condition computation, (4) AddEdge to connect the condition result to the Precondition node's port 0. This is a normal batch mutation. A dedicated endpoint would add API surface for no gain.

However, the property test endpoint IS needed as a new endpoint (`POST /programs/{id}/property-test`) since it has unique semantics (iteration count, seed inputs, randomization) that don't map to existing patterns.

### Contract Separation: Integrated but Identifiable

**Recommendation: Contracts are integrated into the function's node set but identifiable via op type.**

Rationale: Contract nodes are owned by the same function (same `owner: FunctionId`) and live in the same flat compute graph. They are connected to the function's parameter nodes and other body nodes via data edges. They are "integrated" in the sense that they share the same graph space, but "identifiable" because their op types (Precondition/Postcondition/Invariant) make them trivially filterable. The alternative of a separate graph or separate storage would break the "contracts are regular nodes using existing mutation API" decision.

Key implementation detail: When the interpreter discovers contract nodes, it filters function nodes by op type. When the compiler processes a function, it filters OUT contract nodes. Both use the same `graph.function_nodes(func_id)` method, just with different filters applied.

### Dependent Identification: Call Graph Analysis

**Recommendation: Call graph analysis, not content hash embedding.**

Rationale: The call graph approach is more precise and aligns with the existing architecture. The program graph already has `Call { target: FunctionId }` nodes that explicitly encode inter-function dependencies. Building the call graph is a simple scan over these nodes. The content hash approach would embed callee hashes into the caller's hash, which would cause cascading recompilation even for body-only changes in callees that don't affect the caller's behavior (e.g., a callee's internal optimization that doesn't change its signature).

Call graph analysis only triggers recompilation of dependents when a function's *signature-affecting* hash changes. For the initial implementation, treat any change to a called function as requiring caller recompilation (conservative but correct). Future optimization could distinguish signature-affecting vs body-only changes.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Function content hashing | Custom hash function | Existing `hash_function()` in lmlang-storage | Already deterministic, blake3-based, covers nodes + edges |
| Random number generation | Custom PRNG | `rand` + `rand_chacha` crates | ChaCha8Rng is cryptographically suitable, seedable, deterministic given same seed |
| Call graph construction | Custom dependency tracker | Scan `Call { target }` nodes in petgraph | The call graph is already encoded in the compute graph structure |
| Object file caching | Custom binary cache | Filesystem directory with hash-named files | Simple, reliable, no additional dependencies needed |
| Type-aware value generation | Custom type-to-value mapper | Extension of existing `Value` enum + `TypeRegistry` | Value already supports all runtime types; TypeRegistry knows all type structures |

**Key insight:** The existing codebase already has most of the infrastructure needed. Function hashing, per-function node ownership, interpreter with tracing, typed value system -- these are all Phase 1-5 deliverables that Phase 6 builds on top of.

## Common Pitfalls

### Pitfall 1: Contract Subgraph Evaluation Circularity

**What goes wrong:** A contract node's subgraph accidentally references nodes that depend on the contract evaluation result, creating a circular dependency that causes the interpreter to loop or deadlock.

**Why it happens:** Contract nodes live in the same compute graph as body nodes. If a postcondition's subgraph references a node that references the return value, but the return value flows through the postcondition, you get a cycle.

**How to avoid:** Contract subgraphs must only reference: (1) Parameter nodes (for preconditions), (2) the return value (for postconditions, via a dedicated input port), (3) other body nodes that have already been evaluated and don't depend on the contract result. The interpreter should evaluate contracts as a separate subgraph invocation with explicit inputs, not as part of the normal work-list flow. Contracts don't produce values that flow to other nodes -- they only produce pass/fail.

**Warning signs:** Interpreter hangs when contracts are present. Topological sort fails for functions with contracts.

### Pitfall 2: Incremental Compilation Cache Invalidation

**What goes wrong:** Stale cached object files are linked into the binary because the dirty detection missed a dependency, producing a binary with inconsistent function implementations.

**Why it happens:** The call graph analysis only covers direct calls. If function A reads a global variable modified by function B, changing B should dirty A, but the call graph doesn't capture this relationship. Also, type definition changes affect all functions using that type.

**How to avoid:** Be conservative in the first implementation: if a type definition changes, invalidate all functions using that type. Track type dependencies per function (which TypeIds appear in a function's edges). For the initial version, if any type definition changes, mark all functions dirty. Optimize later with finer-grained dependency tracking.

**Warning signs:** Tests pass with full recompilation but fail with incremental builds. Binary behavior differs between clean and incremental builds.

### Pitfall 3: Contract Nodes Appearing in Compiled Output

**What goes wrong:** The compiler attempts to generate LLVM IR for Precondition/Postcondition/Invariant nodes, either crashing or generating code that checks contracts at runtime.

**Why it happens:** The codegen loop iterates over `graph.function_nodes(func_id)` and processes all nodes. If contract nodes aren't filtered out, they reach the IR emitter which doesn't know how to handle them.

**How to avoid:** Filter contract nodes at the start of `compile_function()` in `codegen.rs`. The filtered list is what gets topologically sorted and compiled. Contract nodes are excluded before the sort, not after.

**Warning signs:** `match` exhaustiveness errors in codegen for new op variants. Unexpected LLVM IR instructions in output.

### Pitfall 4: Property Test Reproducibility

**What goes wrong:** A property test finds a failure, but when the agent re-runs the test to debug it, the failure doesn't reproduce because different random inputs were generated.

**Why it happens:** The random seed was not captured or not used consistently. Or the RNG state drifted between runs due to different iteration counts or ordering.

**How to avoid:** Always use a deterministic PRNG (ChaCha8Rng) seeded from the agent-provided seed. Log the exact seed used for each test run in the response. The agent can replay any failure by re-running with the same seed and iteration count.

**Warning signs:** Flaky property test results. Agent reports "failure disappeared on re-run."

### Pitfall 5: Invariant Checking Timing

**What goes wrong:** Invariants are checked at the wrong time (e.g., on every struct access instead of at module boundaries), causing excessive overhead or false positives during intermediate construction of data structures.

**Why it happens:** "Module boundaries" is an abstract concept. In the interpreter, it means when a value of the invariant's target type is passed to a function in a different module, or returned from such a function.

**How to avoid:** Check invariants at two specific points: (1) when a Call node passes a value to a function in a different module (check the value against the type's invariants before the call), (2) when a Return node returns a value to a caller in a different module. Use `FunctionDef::module` to determine module membership.

**Warning signs:** Invariant checks firing during struct construction (before the struct is fully initialized). Performance degradation during simulation with many invariants.

### Pitfall 6: Object File ABI Compatibility

**What goes wrong:** A cached object file compiled with different LLVM settings (optimization level, target triple) is linked with freshly compiled object files, causing linker errors or undefined behavior.

**Why it happens:** The incremental cache doesn't track compilation settings alongside function hashes.

**How to avoid:** Include the compilation settings (opt level, target triple, debug symbols flag) in the cache key. When compilation settings change, invalidate the entire cache. Store settings alongside cached object files and verify before reuse.

**Warning signs:** Linker errors about symbol conflicts or incompatible object file formats. Crashes in optimized builds that work in debug builds.

## Code Examples

### Complete Contract Node Type Definition

```rust
// Addition to ComputeOp in lmlang-core/src/ops.rs

/// Precondition check at function entry.
/// Port 0 (input): Bool -- the condition that must be true.
/// No output -- this is a check node, not a value-producing node.
/// Evaluated by interpreter before function body. Skipped by compiler.
Precondition {
    /// Human-readable contract description for diagnostics.
    message: String,
},

/// Postcondition check at function return.
/// Port 0 (input): Bool -- the condition that must be true.
/// Port 1 (input): The return value being checked (for use by
/// the contract subgraph).
/// No output. Evaluated by interpreter after return value is computed.
/// Skipped by compiler.
Postcondition {
    /// Human-readable contract description for diagnostics.
    message: String,
},

/// Data structure invariant.
/// Port 0 (input): Bool -- the invariant condition.
/// Port 1 (input): The value being checked.
/// No output. Checked at module boundaries during interpretation.
/// Violations block compilation (errors, not warnings).
Invariant {
    /// The type this invariant constrains.
    target_type: TypeId,
    /// Human-readable invariant description for diagnostics.
    message: String,
},
```

These ops also need:
1. `is_contract()` method on `ComputeOp` and `ComputeNodeOp`
2. Input count rules in `typecheck/mod.rs` (Precondition expects 1, Postcondition expects 2, Invariant expects 2)
3. Serialization support (already handled by derive(Serialize, Deserialize) on ComputeOp)

### Property Test API Schema

```rust
// In lmlang-server/src/schema/contracts.rs

#[derive(Debug, Deserialize)]
pub struct PropertyTestRequest {
    /// Function to test.
    pub function_id: FunctionId,
    /// Agent-provided seed inputs (edge cases, interesting values).
    pub seeds: Vec<Vec<serde_json::Value>>,
    /// Number of randomized iterations to generate (agent-specified, required).
    pub iterations: u32,
    /// Random seed for reproducibility. Agent can reuse to replay failures.
    pub random_seed: Option<u64>,
    /// Whether to include execution traces for failures.
    #[serde(default)]
    pub trace_failures: bool,
}

#[derive(Debug, Serialize)]
pub struct PropertyTestResponse {
    /// Total number of test cases run (seeds + random).
    pub total_run: u32,
    /// Number of tests that passed all contracts.
    pub passed: u32,
    /// Number of tests that failed.
    pub failed: u32,
    /// The random seed used (for reproducibility).
    pub random_seed: u64,
    /// Details of each failure.
    pub failures: Vec<PropertyTestFailureView>,
}

#[derive(Debug, Serialize)]
pub struct PropertyTestFailureView {
    /// The inputs that caused the failure.
    pub inputs: Vec<serde_json::Value>,
    /// Which contract was violated.
    pub violation: ContractViolationView,
    /// Execution trace (if trace_failures was true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Vec<TraceEntryView>>,
}
```

### Dirty Query API Schema

```rust
// In lmlang-server/src/schema/compile.rs (extension)

#[derive(Debug, Serialize)]
pub struct DirtyStatusResponse {
    /// Functions that have changed since last compilation.
    pub dirty_functions: Vec<DirtyFunctionView>,
    /// Functions that need recompilation due to dependency changes.
    pub dirty_dependents: Vec<DirtyFunctionView>,
    /// Functions that can use cached object files.
    pub cached_functions: Vec<FunctionId>,
    /// Whether any recompilation is needed.
    pub needs_recompilation: bool,
}

#[derive(Debug, Serialize)]
pub struct DirtyFunctionView {
    pub function_id: FunctionId,
    pub function_name: String,
    /// Why this function is dirty: "changed" or "dependent_of:<function_name>"
    pub reason: String,
}
```

### Incremental Compilation Integration in compiler.rs

```rust
// Modified compile() in lmlang-codegen/src/compiler.rs

pub fn compile(
    graph: &ProgramGraph,
    options: &CompileOptions,
    incremental: Option<&mut IncrementalState>,
) -> Result<CompileResult, CodegenError> {
    // 1. Type check (same as before)
    let type_errors = typecheck::validate_graph(graph);
    if !type_errors.is_empty() {
        return Err(CodegenError::TypeCheckFailed(type_errors));
    }

    // 2. Check invariants (new: block compilation on invariant violations)
    let invariant_violations = check_invariants(graph);
    if !invariant_violations.is_empty() {
        return Err(CodegenError::InvariantViolations(invariant_violations));
    }

    // 3. Compute recompilation plan (incremental only)
    let plan = if let Some(ref incr) = incremental {
        let current_hashes = hash_all_functions_for_compilation(graph);
        let call_graph = build_call_graph(graph);
        Some(incr.compute_dirty(&current_hashes, &call_graph))
    } else {
        None
    };

    // 4. Initialize LLVM (same as before)
    // ...

    // 5. For incremental: compile only dirty functions, reuse cached .o files
    // For full: compile all functions (same as current behavior)
    if let Some(ref plan) = plan {
        // Compile dirty functions
        for func_id in plan.dirty.iter().chain(plan.dirty_dependents.iter()) {
            let func_def = graph.get_function(*func_id).unwrap();
            compile_function_to_object(&context, graph, *func_id, func_def, &cache_dir)?;
        }
        // Link all object files: fresh + cached
        let obj_files: Vec<PathBuf> = /* collect from cache_dir */;
        linker::link_objects(&obj_files, &output_path, options)?;
    } else {
        // Full compilation (existing path)
        // ...
    }

    // 6. Update incremental state with new hashes
    if let Some(incr) = incremental {
        incr.update_hashes(hash_all_functions_for_compilation(graph));
    }

    Ok(result)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Whole-program recompilation (Phase 5) | Incremental per-function recompilation (Phase 6) | This phase | Only changed functions recompile; cached object files reused |
| No contracts (Phase 3-5) | First-class contract nodes (Phase 6) | This phase | Functions gain pre/post-conditions and invariants as graph nodes |
| Manual testing only (interpret + compare) | Property-based testing with randomized inputs | This phase | Automated testing coverage via contract-aware test generation |

**Deprecated/outdated:**
- **Whole-program compilation** in `compiler.rs` becomes the fallback (first build or when cache is cleared). Subsequent builds use incremental path.
- No external libraries are being deprecated. This phase primarily extends existing crate functionality.

## Open Questions

1. **Invariant Association with Types**
   - What we know: Invariants constrain data structures (TypeIds). They need to be associated with specific types and checked when values of those types cross module boundaries.
   - What's unclear: Where to store the type-to-invariant mapping. Options: (a) Invariant nodes reference their target_type in their op data and are discovered by scanning all nodes, (b) TypeRegistry tracks which TypeIds have invariants via a separate map.
   - Recommendation: Store `target_type: TypeId` on the Invariant op variant (Pattern option a). Discovery is a scan of all Invariant nodes filtering by target_type. This is simpler and keeps invariants as regular graph nodes. The scan is O(n) over nodes but invariants are rare relative to total node count.

2. **Per-Function Object File Compilation**
   - What we know: The current compiler creates a single LLVM Module with all functions, emits one object file, and links it. Incremental compilation needs per-function object files.
   - What's unclear: Whether inkwell/LLVM supports having a function in Module A call a function in Module B without forward-declaring B's function in A. Cross-module references typically require forward declarations or LTO.
   - Recommendation: Each per-function Module should forward-declare all functions it calls (just the signature, not the body). This is the same pattern already used in `forward_declare_functions()`. The linker resolves the actual symbols at link time. This is standard separate compilation practice.

3. **ExecutionState Extension for Contract Violations**
   - What we know: The current `ExecutionState` enum has `Ready`, `Running`, `Paused`, `Completed`, and `Error` variants. Contract violations are a new kind of execution result.
   - What's unclear: Whether contract violations should be a new `ExecutionState` variant or a subtype of `Error`.
   - Recommendation: Add a new `ContractViolation { violation: ContractViolation }` variant to `ExecutionState`. This is distinct from runtime errors (which are unexpected failures) -- contract violations are expected feedback during development. The API response should clearly distinguish "your program crashed" from "your program violated a contract."

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `lmlang-core/src/ops.rs` -- Complete op set, ComputeOp/StructuredOp enum structure, serde derive patterns
- Codebase analysis: `lmlang-check/src/interpreter/state.rs` -- Interpreter execution model, CallFrame, ExecutionState lifecycle, function entry/return hooks
- Codebase analysis: `lmlang-check/src/interpreter/eval.rs` -- Per-op evaluation pattern, how ops process inputs and produce outputs
- Codebase analysis: `lmlang-storage/src/hash.rs` -- blake3 function hashing, Merkle composition, deterministic hash construction
- Codebase analysis: `lmlang-codegen/src/compiler.rs` -- Full compilation pipeline, function iteration, forward declarations, LLVM Module/Context lifecycle
- Codebase analysis: `lmlang-codegen/src/codegen.rs` -- Per-function codegen, topological sort, SSA value tracking, node dispatch
- Codebase analysis: `lmlang-server/src/service.rs` -- ProgramService coordinator, mutation flow, verification, simulation, compile integration
- Codebase analysis: `lmlang-server/src/schema/` -- API schema patterns for requests/responses, diagnostic types

### Secondary (MEDIUM confidence)
- [rand crate](https://crates.io/crates/rand) -- Standard Rust randomness library, ChaCha8Rng for deterministic reproducible sequences
- [rand_chacha crate](https://crates.io/crates/rand_chacha) -- ChaCha8Rng implementation, seedable PRNG
- LLVM separate compilation model -- Standard practice of forward-declaring functions across compilation units, linker resolves symbols

### Tertiary (LOW confidence)
- Per-function LLVM Module compilation performance -- Untested whether creating N separate LLVM Contexts/Modules (one per function) has significant overhead vs one Module with all functions. Needs benchmarking in implementation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- No new major dependencies; extending existing codebase with well-understood patterns. rand/rand_chacha are standard Rust ecosystem libraries.
- Architecture: HIGH -- Contract nodes as ComputeOp variants follow the exact pattern established in Phases 1-5. Incremental compilation builds directly on existing hash infrastructure. All patterns verified by reading the complete source code.
- Pitfalls: HIGH -- Contract circularity, cache invalidation, and ABI compatibility are well-known problems in compiler design. The specific codebase patterns (topological sort, work-list evaluation, function-scoped codegen) inform the specific pitfall manifestations.
- Property testing: MEDIUM -- The interpreter-based harness is a custom design since standard Rust property testing (proptest/quickcheck) operates on Rust types, not lmlang graph values. The approach is sound but the specific input generation strategy needs validation during implementation.

**Research date:** 2026-02-18
**Valid until:** 2026-03-18 (stable domain; no external dependency changes expected)
