---
phase: 03-type-checking-graph-interpreter
plan: 02
subsystem: interpreter
tags: [graph-interpreter, work-list, state-machine, checked-arithmetic, control-flow, recursion]

# Dependency graph
requires:
  - phase: 03-type-checking-graph-interpreter/01
    provides: type-checking rules, coercion, validation framework
  - phase: 02-data-model-graph-layer
    provides: ProgramGraph, ComputeNodeOp, FlowEdge, FunctionDef, TypeRegistry
provides:
  - Graph interpreter module with step/run/pause state machine
  - Value enum for runtime value representation (all scalar and compound types)
  - RuntimeError enum with trap semantics (overflow, div-by-zero, OOB, recursion limit)
  - Per-op evaluation with checked arithmetic for all ComputeOp and StructuredOp variants
  - Call stack with CallFrame for function calls and recursion
  - Control flow via Branch/Phi with branch-aware node scheduling
  - Memory model with Alloc/Store/Load
  - Execution tracing (optional per InterpreterConfig)
  - 36 interpreter tests including 27 integration tests
affects: [03-type-checking-graph-interpreter, 04-parser-frontend]

# Tech tracking
tech-stack:
  added: [thiserror]
  patterns: [work-list evaluation, control-gated scheduling, state-machine execution]

key-files:
  created:
    - crates/lmlang-check/src/interpreter/mod.rs
    - crates/lmlang-check/src/interpreter/value.rs
    - crates/lmlang-check/src/interpreter/error.rs
    - crates/lmlang-check/src/interpreter/state.rs
    - crates/lmlang-check/src/interpreter/eval.rs
    - crates/lmlang-check/src/interpreter/trace.rs
  modified:
    - crates/lmlang-check/src/lib.rs

key-decisions:
  - "Work-list algorithm with control-gated scheduling: nodes behind control edges wait for control predecessor"
  - "Phi selects data port based on Branch decision (true->port 0, false->port 1)"
  - "Only Parameter, Const, CaptureAccess, Alloc, ReadLine are seedable nodes (prevents evaluating incomplete nodes)"
  - "Store produces NoValue; Store->Load sequencing via control edges with control_ready tracking"
  - "Bool coerced to I8 for arithmetic (consistent with type checker)"

patterns-established:
  - "Control-gated scheduling: CallFrame tracks control_gated and control_ready sets; nodes with incoming control edges must be explicitly unblocked"
  - "EvalResult enum: Value | NoValue | Return | Call for clean separation of node evaluation outcomes"
  - "try_schedule_node: unified scheduling that checks both data-readiness and control-readiness"

requirements-completed: [EXEC-01]

# Metrics
duration: 15min
completed: 2026-02-18
---

# Phase 3 Plan 2: Graph Interpreter Summary

**Work-list graph interpreter with checked arithmetic, control flow branching via Branch/Phi, recursive function calls, memory ops, and 36 tests across all ComputeOp/StructuredOp variants**

## Performance

- **Duration:** 15 min
- **Started:** 2026-02-18T20:15:00Z
- **Completed:** 2026-02-18T20:30:00Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Complete graph interpreter with state machine execution (Ready -> Running -> Paused | Completed | Error)
- Per-op evaluation covering all 40+ ComputeOp and StructuredOp variants with checked arithmetic and trap semantics
- Control flow via Branch/Phi with control-gated node scheduling -- nodes behind control edges wait for explicit unblocking
- 36 tests: 9 unit tests for core state machine + 27 integration tests covering arithmetic, traps, conditionals, recursion, memory, arrays, structs, enums, casts, I/O

## Task Commits

Each task was committed atomically:

1. **Task 1: Interpreter core** - `a1d5985` (feat)
2. **Task 2: Integration tests + control flow fixes** - `1605dbb` (feat)

## Files Created/Modified
- `crates/lmlang-check/src/interpreter/mod.rs` - Module root with re-exports and 27 integration tests
- `crates/lmlang-check/src/interpreter/value.rs` - Value enum (Bool, I8-I64, F32, F64, Unit, Array, Struct, Enum, Pointer, FunctionRef, Closure)
- `crates/lmlang-check/src/interpreter/error.rs` - RuntimeError with IntegerOverflow, DivideByZero, OutOfBoundsAccess, RecursionLimitExceeded, TypeMismatch, MissingValue
- `crates/lmlang-check/src/interpreter/state.rs` - Interpreter struct with step/run/pause/resume, CallFrame, ExecutionState, control-gated scheduling
- `crates/lmlang-check/src/interpreter/eval.rs` - Per-op evaluation: arithmetic (checked), comparisons, logic, shifts, casts, structs, arrays, enums
- `crates/lmlang-check/src/interpreter/trace.rs` - TraceEntry for optional execution recording
- `crates/lmlang-check/src/lib.rs` - Added `pub mod interpreter`

## Decisions Made
- **Work-list with control gating:** Nodes are evaluated when all data inputs are ready AND (no control inputs OR at least one control predecessor completed). This prevents Load from executing before Store when they share a pointer via data edge but are sequenced via control edge.
- **Phi port selection by Branch value:** Phi receives both true-path and false-path values on ports 0/1. It inspects the incoming Branch node's stored Bool value to select the correct port. This avoids needing separate Phi implementations per control flow pattern.
- **Seed node restriction:** Only explicitly seedable ops (Parameter, Const, CaptureAccess, Alloc, ReadLine) are added to the initial work list. Other nodes with 0 incoming edges (e.g., unconnected BinaryArith) are NOT seeded, preventing spurious MissingValue errors.
- **Bool-to-I8 coercion in arithmetic:** When a Bool flows into a BinaryArith node, it's coerced to I8(0/1) before evaluation. This matches the type checker's coercion behavior from Plan 01.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Control-gated scheduling for Store->Load sequencing**
- **Found during:** Task 2 (integration_memory_alloc_store_load)
- **Issue:** Load was scheduled before Store completed because both received the pointer from Alloc via data edges, and Load had no way to know Store needed to run first
- **Fix:** Added control_gated/control_ready tracking in CallFrame. Nodes with incoming control edges are blocked until at least one control predecessor completes.
- **Files modified:** crates/lmlang-check/src/interpreter/state.rs
- **Verification:** integration_memory_alloc_store_load passes -- Load reads 42 after Store writes it

**2. [Rule 1 - Bug] Phi node selecting wrong branch value**
- **Found during:** Task 2 (integration_conditional_false_branch)
- **Issue:** Phi was taking the first available input regardless of branch decision, always returning the true-path value
- **Fix:** Phi now inspects incoming control edges' source Branch node, reads its stored Bool decision, and maps true->port 0, false->port 1
- **Files modified:** crates/lmlang-check/src/interpreter/state.rs
- **Verification:** Both integration_conditional_true_branch and integration_conditional_false_branch pass

**3. [Rule 1 - Bug] Unconnected nodes incorrectly seeded**
- **Found during:** Task 2 (integration_partial_results_on_error)
- **Issue:** BinaryArith nodes with 0 incoming edges were seeded as ready, then failed with MissingValue when evaluated
- **Fix:** Restricted seed node list to only: Parameter, Const, CaptureAccess, Alloc, ReadLine
- **Files modified:** crates/lmlang-check/src/interpreter/state.rs
- **Verification:** integration_partial_results_on_error correctly reports DivideByZero with partial results

---

**Total deviations:** 3 auto-fixed (3 bugs discovered during integration testing)
**Impact on plan:** All fixes essential for correct interpreter behavior. No scope creep.

## Issues Encountered
None beyond the auto-fixed bugs above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Interpreter module ready for use in REPL and test harness integration
- All ComputeOp and StructuredOp variants handled (with placeholder I/O ops returning I64(0))
- Execution tracing available for debugging future development
- 112 total tests across typecheck + interpreter provide regression safety

## Self-Check: PASSED
- All 7 created files verified present
- Both task commits (a1d5985, 1605dbb) verified in git log
- 112 tests pass (0 failures)

---
*Phase: 03-type-checking-graph-interpreter*
*Completed: 2026-02-18*
