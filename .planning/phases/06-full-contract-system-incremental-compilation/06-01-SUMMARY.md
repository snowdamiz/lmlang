---
phase: 06-full-contract-system-incremental-compilation
plan: 01
subsystem: contracts
tags: [contracts, preconditions, postconditions, invariants, incremental-compilation, dirty-detection, blake3]

# Dependency graph
requires:
  - phase: 05-closures-advanced-codegen
    provides: closure support, MakeClosure/CaptureAccess ops, interpreter eval dispatch
provides:
  - Precondition, Postcondition, Invariant op variants in ComputeOp
  - is_contract() predicates on ComputeOp and ComputeNodeOp
  - ContractViolation structured diagnostic type with counterexample collection
  - Contract checking functions (check_preconditions, check_postconditions, check_invariants_for_value)
  - Contract-aware compilation hashing (hash_function_for_compilation)
  - DirtySet incremental compilation dirty detection
  - Codegen contract node filtering (zero overhead in compiled binaries)
  - ExecutionState::ContractViolation interpreter state variant
affects: [06-02, 06-03, 06-04, 06-05]

# Tech tracking
tech-stack:
  added: []
  patterns: [contract-nodes-as-graph-ops, dev-only-strip-at-compile, contract-aware-hashing]

key-files:
  created:
    - crates/lmlang-check/src/contracts/mod.rs
    - crates/lmlang-check/src/contracts/check.rs
    - crates/lmlang-storage/src/dirty.rs
  modified:
    - crates/lmlang-core/src/ops.rs
    - crates/lmlang-check/src/lib.rs
    - crates/lmlang-check/src/typecheck/mod.rs
    - crates/lmlang-check/src/typecheck/rules.rs
    - crates/lmlang-check/src/interpreter/eval.rs
    - crates/lmlang-check/src/interpreter/state.rs
    - crates/lmlang-check/src/interpreter/value.rs
    - crates/lmlang-codegen/src/codegen.rs
    - crates/lmlang-codegen/tests/integration_tests.rs
    - crates/lmlang-storage/src/hash.rs
    - crates/lmlang-storage/src/lib.rs

key-decisions:
  - "Contract ops as ComputeOp variants (not a separate tier) for minimal type system disruption"
  - "Contracts filtered before topological sort in codegen, not at emit_node, for clean separation"
  - "Contract-aware hashing excludes contract nodes entirely so contract edits never trigger recompilation"
  - "ContractViolation includes counterexample values from the failing subgraph for agent debugging"
  - "Added PartialEq to Value for test assertions (floating-point PartialEq is sufficient)"

patterns-established:
  - "Contract-as-graph-node: contracts are first-class compute nodes checked at dev-time, stripped at compile-time"
  - "Compilation hash exclusion: hash_function_for_compilation filters is_contract() nodes for dirty detection"
  - "Structured violations: ContractViolation carries kind, message, inputs, return value, and counterexample"

requirements-completed: []

# Metrics
duration: 12min
completed: 2026-02-19
---

# Phase 6 Plan 01: Contract Op Foundation Summary

**Precondition/postcondition/invariant ops with codegen stripping, contract-aware incremental hashing, and DirtySet dirty detection**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-02-19T01:30:00Z
- **Completed:** 2026-02-19T01:43:24Z
- **Tasks:** 4/4
- **Files modified:** 14

## Accomplishments
- Three new contract op variants (Precondition, Postcondition, Invariant) wired through all 4 crates
- Contract nodes filtered at codegen boundary with zero runtime overhead, verified by integration test (exit code matches)
- Contract checking functions with counterexample collection for agent-friendly diagnostics
- DirtySet incremental compilation module that ignores contract-only changes

## Task Commits

Each task was committed atomically:

1. **Task 1: Contract op types, codegen filtering, contract-aware hashing** - `95181a0` (feat)
2. **Task 2: Interpreter contract checking** - `216c460` (feat)
3. **Task 3: Dirty detection module** - `d4cee8a` (feat)
4. **Task 4: Serde roundtrip and cross-crate integration tests** - `e12c3ae` (test)

## Files Created/Modified

### Created
- `crates/lmlang-check/src/contracts/mod.rs` - ContractKind, ContractViolation types
- `crates/lmlang-check/src/contracts/check.rs` - Contract checking logic (find, evaluate, collect counterexamples)
- `crates/lmlang-storage/src/dirty.rs` - DirtySet type and compute_dirty_set function

### Modified
- `crates/lmlang-core/src/ops.rs` - Added Precondition/Postcondition/Invariant to ComputeOp with is_contract()
- `crates/lmlang-check/src/lib.rs` - Added contracts module
- `crates/lmlang-check/src/typecheck/mod.rs` - Added contract ops to input count checker
- `crates/lmlang-check/src/typecheck/rules.rs` - Type rules for contract ops (Bool condition ports)
- `crates/lmlang-check/src/interpreter/eval.rs` - Contract ops return Ok(None) in eval dispatch
- `crates/lmlang-check/src/interpreter/state.rs` - Added ExecutionState::ContractViolation variant
- `crates/lmlang-check/src/interpreter/value.rs` - Added PartialEq derive to Value
- `crates/lmlang-codegen/src/codegen.rs` - Filter contract nodes before topo sort, unreachable arms
- `crates/lmlang-codegen/tests/integration_tests.rs` - 2 integration tests for contract stripping
- `crates/lmlang-storage/src/hash.rs` - hash_function_for_compilation, 3 new tests
- `crates/lmlang-storage/src/lib.rs` - Added dirty module, re-exports

## Decisions Made
- Contract ops added as ComputeOp variants rather than a new ComputeNodeOp tier to minimize type system disruption (all existing match arms in 4 crates would need modification for a new tier)
- Contract node filtering placed before topological sort (not at emit_node) so contract subgraph edges don't confuse the sort
- hash_function_for_compilation also skips edges TO contract nodes, not just the nodes themselves, ensuring full isolation
- ContractViolation includes counterexample node values sorted by NodeId for deterministic diagnostics

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added PartialEq derive to Value**
- **Found during:** Task 2 (contract checking tests)
- **Issue:** Test assertions using assert_eq! on Vec<Value> required PartialEq
- **Fix:** Added PartialEq to #[derive] on Value enum (f32/f64 PartialEq is fine for test use)
- **Files modified:** crates/lmlang-check/src/interpreter/value.rs
- **Committed in:** 216c460 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minimal - standard derive addition for test support. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Contract op types are fully wired through type checking, interpretation, codegen, and hashing
- Ready for Plan 02 (incremental compilation pipeline) which will use DirtySet for selective recompilation
- Ready for Plan 03+ which will add contract subgraph evaluation during interpretation

---
*Phase: 06-full-contract-system-incremental-compilation*
*Completed: 2026-02-19*
