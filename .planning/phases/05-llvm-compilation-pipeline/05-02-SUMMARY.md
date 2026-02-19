---
phase: 05-llvm-compilation-pipeline
plan: 02
subsystem: codegen
tags: [llvm, inkwell, ir-emission, arithmetic-overflow, checked-arithmetic, topological-sort, petgraph]

# Dependency graph
requires:
  - phase: 05-01
    provides: "Type mapping (lm_type_to_llvm), runtime guards (overflow/div/bounds), linker, error types"
provides:
  - "Per-function code generation (compile_function) lowering ProgramGraph ops to LLVM IR"
  - "Topological sorting of compute nodes within function boundaries"
  - "Exhaustive LLVM IR emission for all 29 ComputeOp + 10 StructuredOp variants"
  - "Checked integer arithmetic via LLVM overflow intrinsics"
  - "Control flow emission (if/else, loop, match/switch, branch, jump, phi)"
  - "Memory ops (alloca/load/store/GEP)"
  - "Function call and closure emission"
affects: [05-03, 05-04, 05-05]

# Tech tracking
tech-stack:
  added: [petgraph (codegen crate dependency)]
  patterns: ["topological sort via Kahn's algorithm on data+control edges", "SSA value tracking in HashMap<NodeId, BasicValueEnum>", "aggregate_to_basic helper for inkwell AggregateValueEnum conversion"]

key-files:
  created:
    - crates/lmlang-codegen/src/codegen.rs
  modified:
    - crates/lmlang-codegen/src/lib.rs
    - crates/lmlang-codegen/Cargo.toml

key-decisions:
  - "Topological sort uses both data AND control edges for correct ordering of side-effect nodes"
  - "All integer add/sub/mul use LLVM overflow intrinsics (sadd/ssub/smul.with.overflow) for checked arithmetic"
  - "Division and remainder emit a div-by-zero guard before the sdiv/srem instruction"
  - "Closures use a {fn_ptr, env_ptr} struct pair with stack-allocated environment"
  - "AggregateValueEnum (from build_insert_value) is converted via explicit match rather than blanket Into"

patterns-established:
  - "compile_and_verify test helper: builds graph, compiles function, verifies LLVM module"
  - "get_input/get_input_type/get_output_type: edge-walking helpers for SSA value resolution"
  - "emit_checked_int_arith: reusable overflow intrinsic pattern for any integer width"

requirements-completed: [EXEC-02]

# Metrics
duration: 12min
completed: 2026-02-19
---

# Phase 05 Plan 02: Per-Function Code Generation Summary

**Exhaustive per-function LLVM IR emission for all 39 op variants with checked arithmetic, overflow intrinsics, and topological node ordering**

## Performance

- **Duration:** 12 min
- **Started:** 2026-02-19T00:02:42Z
- **Completed:** 2026-02-19T00:14:19Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Created 2516-line codegen.rs implementing compile_function with per-op LLVM IR emission for all 29 ComputeOp and 10 StructuredOp variants
- Topological sort via Kahn's algorithm ensures correct SSA value ordering using both data and control edges
- Checked integer arithmetic emits overflow intrinsics (sadd/ssub/smul.with.overflow) with runtime error on overflow
- 22 unit tests covering all major op categories: arithmetic, logic, shifts, comparison, memory, function calls, structs, arrays, casts, unary, and I/O
- All 58 lmlang-codegen tests and 302 workspace tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Per-function compilation framework with arithmetic/logic/comparison ops** - `28b1ceb` (feat)
2. **Task 2: Complete all op implementations with exhaustive match and comprehensive tests** - `c0dda0b` (feat)

## Files Created/Modified

- `crates/lmlang-codegen/src/codegen.rs` - Per-function code generation: compile_function, topological_sort, emit_node with exhaustive match on all 39 op variants
- `crates/lmlang-codegen/src/lib.rs` - Registered codegen module
- `crates/lmlang-codegen/Cargo.toml` - Added petgraph dependency for edge iteration
- `Cargo.lock` - Updated lockfile with petgraph for codegen crate

## Decisions Made

- **Topological sort includes control edges**: Initially only data edges were considered, but this caused side-effect nodes (Print) to be incorrectly ordered relative to Return. Changed to consider both data and control edges.
- **Overflow intrinsics for checked arithmetic**: Integer add/sub/mul use `llvm.{sadd,ssub,smul}.with.overflow.iN` intrinsics which return `{result, overflow_flag}` structs. The overflow flag branches to a runtime error block.
- **Division guard before sdiv/srem**: Zero-check emitted before every integer division/remainder via runtime::emit_div_guard.
- **Closure representation**: Uses `{fn_ptr, env_ptr}` struct pair where environment is stack-allocated. CaptureAccess uses GEP into the environment struct.
- **AggregateValueEnum conversion**: inkwell 0.8 returns AggregateValueEnum from build_insert_value (not BasicValueEnum), so an explicit `aggregate_to_basic` match helper was added.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added petgraph dependency to lmlang-codegen**
- **Found during:** Task 1
- **Issue:** codegen.rs needs petgraph::Direction and petgraph::visit::EdgeRef to iterate compute graph edges, but petgraph was only a dependency of lmlang-core
- **Fix:** Added `petgraph = { version = "0.8", features = ["serde-1"] }` to lmlang-codegen/Cargo.toml
- **Files modified:** crates/lmlang-codegen/Cargo.toml, Cargo.lock
- **Verification:** cargo check passes
- **Committed in:** 28b1ceb (Task 1 commit)

**2. [Rule 1 - Bug] Fixed topological sort to include control edges**
- **Found during:** Task 2 (test_print_op failure)
- **Issue:** Print node (side-effect, no data output) and Return node (void, no data input) had no data edge between them, so topo sort could place Return before Print, causing "block has no terminator" LLVM verification error
- **Fix:** Changed topological sort to consider all edge types (data + control), not just data edges
- **Files modified:** crates/lmlang-codegen/src/codegen.rs
- **Verification:** test_print_op passes, module verification succeeds
- **Committed in:** c0dda0b (Task 2 commit)

**3. [Rule 1 - Bug] Fixed AggregateValueEnum to BasicValueEnum conversion**
- **Found during:** Task 1
- **Issue:** inkwell 0.8 build_insert_value returns AggregateValueEnum which cannot be directly converted to BasicValueEnum via .into()
- **Fix:** Added aggregate_to_basic helper that matches on ArrayValue/StructValue variants and converts each individually
- **Files modified:** crates/lmlang-codegen/src/codegen.rs
- **Verification:** StructSet and ArraySet operations compile and produce valid IR
- **Committed in:** 28b1ceb (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (1 blocking dependency, 2 bugs)
**Impact on plan:** All auto-fixes were necessary for correctness. No scope creep.

## Issues Encountered

- inkwell 0.8 API differences from expected: `try_as_basic_value()` returns `ValueKind` (not `Either`), requiring `.basic()` instead of `.left()`. NodeIndex type inference needed explicit removal of turbofish. All resolved during initial implementation.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- compile_function is ready to be called by the pipeline orchestrator (Plan 03)
- All op emission is complete -- the pipeline just needs to iterate functions and call compile_function for each
- Runtime guards (overflow, division, bounds) are fully integrated
- 22 codegen tests provide regression coverage for all op categories

---
*Phase: 05-llvm-compilation-pipeline*
*Completed: 2026-02-19*
