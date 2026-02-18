---
phase: 03-type-checking-graph-interpreter
plan: 01
subsystem: type-checking
tags: [type-checker, coercion, petgraph, static-analysis, graph-validation]

# Dependency graph
requires:
  - phase: 02-persistence-serialization
    provides: ProgramGraph with types, ops, edges, nodes, functions
provides:
  - lmlang-check crate with typecheck module
  - Per-op type rule resolution for all ~34 ComputeNodeOp variants
  - Coercion rules (bool->int, integer widening, float widening, &mut T -> &T)
  - TypeError diagnostics with rich context and fix suggestions
  - validate_data_edge for eager per-edit checking
  - validate_graph for full graph validation (report all errors)
affects: [03-02-interpreter, 04-api]

# Tech tracking
tech-stack:
  added: [lmlang-check]
  patterns: [per-op-type-rules, eager-validation, collect-all-errors, nominal-typing]

key-files:
  created:
    - crates/lmlang-check/Cargo.toml
    - crates/lmlang-check/src/lib.rs
    - crates/lmlang-check/src/typecheck/mod.rs
    - crates/lmlang-check/src/typecheck/diagnostics.rs
    - crates/lmlang-check/src/typecheck/coercion.rs
    - crates/lmlang-check/src/typecheck/rules.rs
  modified:
    - Cargo.lock

key-decisions:
  - "Safe implicit widening: i8->i16->i32->i64, f32->f64, bool->integer, &mut T -> &T; no cross-family (int<->float)"
  - "Bool coercion resolves to I8 for arithmetic (not preserving Bool type in numeric contexts)"
  - "Nominal struct typing: TypeId equality only, structural similarity irrelevant"
  - "InsertCast fix suggestion generated when both types are numeric but incompatible"
  - "Input count validation for fixed-arity ops (BinaryArith=2, Not=1, Branch=1, etc.)"

patterns-established:
  - "Per-op type rule resolution via exhaustive match (no wildcards) returning OpTypeRule"
  - "Validation functions as standalone API (not wrapping ProgramGraph) for architectural independence"
  - "Rich diagnostics with source/target node IDs, ports, function context, and actionable fix suggestions"

requirements-completed: [CNTR-01]

# Metrics
duration: 9min
completed: 2026-02-18
---

# Phase 3 Plan 1: Static Type Checker Summary

**Per-op type rule resolution for all ~34 ops with coercion rules, eager edge validation, and full graph validation reporting all errors with rich diagnostics**

## Performance

- **Duration:** 9 min
- **Started:** 2026-02-18T17:03:22Z
- **Completed:** 2026-02-18T17:12:29Z
- **Tasks:** 2/2
- **Files modified:** 7

## Accomplishments
- Created lmlang-check crate with full type checker module (diagnostics, coercion, rules, validation API)
- Exhaustive per-op type rules for all ComputeNodeOp variants including arithmetic, logic, comparison, control flow, memory, functions, closures, I/O, and structured ops
- Coercion system supporting bool-to-integer, safe integer widening, float widening, and mutable-to-immutable reference coercion
- validate_data_edge for eager per-edit checking and validate_graph for full graph validation that reports ALL errors at once
- 76 unit tests covering every op category, coercion path, error case, and graph validation scenario

## Task Commits

Each task was committed atomically:

1. **Task 1: Create lmlang-check crate with diagnostics, coercion, and per-op type rules** - `d7967e7` (feat)
2. **Task 2: Implement validate_data_edge and validate_graph public API** - `252fad6` (feat)

## Files Created/Modified
- `crates/lmlang-check/Cargo.toml` - New crate with lmlang-core, petgraph, thiserror deps
- `crates/lmlang-check/src/lib.rs` - Crate root re-exporting typecheck module
- `crates/lmlang-check/src/typecheck/mod.rs` - Public API: validate_data_edge, validate_graph, helper functions
- `crates/lmlang-check/src/typecheck/diagnostics.rs` - TypeError enum with TypeMismatch, MissingInput, WrongInputCount, NonNumericArithmetic, NonBooleanCondition; FixSuggestion enum
- `crates/lmlang-check/src/typecheck/coercion.rs` - can_coerce, common_numeric_type, is_numeric/integer/float helpers
- `crates/lmlang-check/src/typecheck/rules.rs` - resolve_type_rule with exhaustive matching on all op variants, OpTypeRule struct
- `Cargo.lock` - Updated with new crate

## Decisions Made
- Safe implicit widening along lossless chains (i8->i16->i32->i64, f32->f64) with NO cross-family conversions (int<->float requires explicit Cast)
- Bool-to-integer coercion resolves to I8 as the base numeric type (Bool+Bool arithmetic produces I8, not Bool)
- Strict nominal typing for structs: TypeId equality only, regardless of structural similarity
- Validation functions as standalone API (Approach A from research) rather than wrapper type, keeping lmlang-check independent of core's mutation API
- InsertCast fix suggestions generated when both types are numeric but coercion fails, giving AI agents actionable remediation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed common_numeric_type Bool+Bool resolution**
- **Found during:** Task 1 (coercion rules)
- **Issue:** `common_numeric_type(Bool, Bool)` short-circuited at `a == b` and returned Bool instead of I8
- **Fix:** Moved Bool-to-I8 resolution before the same-type check so Bool arithmetic always resolves to I8
- **Files modified:** crates/lmlang-check/src/typecheck/coercion.rs
- **Verification:** Test `common_type_bool_resolves_to_i8` passes
- **Committed in:** d7967e7 (Task 1 commit)

**2. [Rule 3 - Blocking] Fixed TypeRegistry import path**
- **Found during:** Task 1 (initial compilation)
- **Issue:** TypeRegistry is in `type_id` module, not `types` module; petgraph EdgeRef trait not in scope
- **Fix:** Corrected import paths, added `use petgraph::visit::EdgeRef`
- **Files modified:** crates/lmlang-check/src/typecheck/coercion.rs, crates/lmlang-check/src/typecheck/mod.rs
- **Verification:** Clean compilation, all tests pass
- **Committed in:** d7967e7 (Task 1), 252fad6 (Task 2)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness and compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed issues above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Type checker complete and tested, ready for interpreter implementation (Plan 02)
- The `resolve_type_rule` and validation functions provide the foundation for the interpreter to verify types before execution
- All coercion rules established for runtime value conversion logic in the interpreter

---
## Self-Check: PASSED

All 7 created files verified on disk. Both task commits (d7967e7, 252fad6) verified in git history.

---
*Phase: 03-type-checking-graph-interpreter*
*Completed: 2026-02-18*
