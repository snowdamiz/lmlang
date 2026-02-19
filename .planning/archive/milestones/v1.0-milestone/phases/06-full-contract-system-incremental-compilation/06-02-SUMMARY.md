---
phase: 06-full-contract-system-incremental-compilation
plan: 02
subsystem: testing
tags: [property-testing, contracts, randomized-testing, prng, interpreter]

# Dependency graph
requires:
  - phase: 06-full-contract-system-incremental-compilation/01
    provides: "Contract nodes (Precondition, Postcondition, Invariant), ContractViolation type, check module"
provides:
  - "Property test harness with deterministic random input generation"
  - "PropertyTestConfig, PropertyTestResult, PropertyTestFailure types"
  - "POST /programs/{id}/property-test API endpoint"
  - "Interpreter contract violation detection during execution"
affects: ["06-full-contract-system-incremental-compilation"]

# Tech tracking
tech-stack:
  added: [rand 0.8, rand_chacha 0.3]
  patterns: [property-based-testing, deterministic-prng-seeding, boundary-value-weighting]

key-files:
  created:
    - crates/lmlang-check/src/contracts/property.rs
    - crates/lmlang-server/src/schema/contracts.rs
    - crates/lmlang-server/src/handlers/contracts.rs
  modified:
    - crates/lmlang-check/Cargo.toml
    - crates/lmlang-check/src/contracts/mod.rs
    - crates/lmlang-check/src/interpreter/state.rs
    - crates/lmlang-server/src/schema/mod.rs
    - crates/lmlang-server/src/handlers/mod.rs
    - crates/lmlang-server/src/service.rs
    - crates/lmlang-server/src/router.rs
    - crates/lmlang-server/tests/integration_test.rs

key-decisions:
  - "Added EvalResult::ContractViolated variant to interpreter -- contract nodes now check their condition input and halt execution with ContractViolation state"
  - "Boundary values weighted at 30% probability in random generation for better edge-case coverage"
  - "Control edge from Precondition to Return required in graphs to ensure contract checking before function completion"

patterns-established:
  - "Property test pattern: seeds first, then random variations, all through fresh interpreter instances"
  - "Deterministic PRNG: ChaCha8Rng seeded from u64 for reproducible test runs"
  - "Contract API pattern: handler -> service.property_test -> run_property_tests"

requirements-completed: [CNTR-05]

# Metrics
duration: 12min
completed: 2026-02-18
---

# Phase 6 Plan 02: Property-Based Contract Testing Summary

**Deterministic property test harness with ChaCha8 PRNG, boundary-weighted random input generation, and POST /programs/{id}/property-test API endpoint**

## Performance

- **Duration:** ~12 min
- **Started:** 2026-02-18
- **Completed:** 2026-02-18
- **Tasks:** 2/2
- **Files modified:** 13

## Accomplishments
- Property test engine that runs agent seeds then random variations through the interpreter, detecting contract violations
- Deterministic PRNG (ChaCha8Rng) ensures same random_seed produces identical test inputs for reproducibility
- Boundary value weighting (30% chance of MIN/MAX/0/1/-1) increases edge-case coverage
- Interpreter now detects contract violations inline during execution (EvalResult::ContractViolated)
- Full HTTP API endpoint with structured failure details including counterexample values and execution traces

## Task Commits

Each task was committed atomically:

1. **Task 1: Property test harness with random input generation** - `fb7bb36` (feat)
2. **Task 2: Property test API endpoint and server integration** - `dfafb89` (feat)

## Files Created/Modified
- `crates/lmlang-check/src/contracts/property.rs` - Property test harness: config types, random generation, test loop, failure collection
- `crates/lmlang-check/src/interpreter/state.rs` - Added EvalResult::ContractViolated and contract op evaluation in eval_node
- `crates/lmlang-server/src/schema/contracts.rs` - API types: PropertyTestRequest/Response, ContractViolationView
- `crates/lmlang-server/src/handlers/contracts.rs` - POST /programs/{id}/property-test handler
- `crates/lmlang-server/src/service.rs` - ProgramService::property_test method with JSON-to-Value conversion
- `crates/lmlang-server/src/router.rs` - Route wiring for property-test endpoint
- `crates/lmlang-server/tests/integration_test.rs` - Two integration tests: violation detection and all-pass scenario
- `crates/lmlang-check/Cargo.toml` - Added rand/rand_chacha dependencies

## Decisions Made
- **Interpreter contract enforcement:** The plan stated "The interpreter already halts with ContractViolation state from Plan 01" but contract nodes were evaluated as no-ops (returning `Ok(None)`). Added inline contract checking to `eval_node` so Precondition/Postcondition/Invariant nodes check their boolean condition input and transition to `ExecutionState::ContractViolation` when false. This is the correct design since the `ExecutionState::ContractViolation` variant already existed.
- **Boundary value weighting:** Random values have a 30% chance of being a boundary value (0, 1, -1, MIN, MAX for integers; 0.0, -0.0, 1.0, -1.0 for floats). This significantly increases the probability of finding edge-case violations.
- **Control edge ordering:** Graph construction for contract testing requires a control edge from contract nodes to Return to ensure contracts are checked before function completion. This is consistent with how real program graphs would be constructed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Interpreter contract enforcement not implemented**
- **Found during:** Task 1 (Property test harness)
- **Issue:** Contract ops (Precondition, Postcondition, Invariant) returned `Ok(None)` in eval dispatch, never checking conditions. Property tests could not detect violations.
- **Fix:** Added contract op handling to `eval_node` in state.rs. New `EvalResult::ContractViolated` variant. Contract nodes check port 0 boolean input and halt with `ExecutionState::ContractViolation` when false.
- **Files modified:** `crates/lmlang-check/src/interpreter/state.rs`
- **Verification:** Property tests correctly detect negative inputs violating `a >= 0` precondition
- **Committed in:** fb7bb36 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Essential fix for contract system to function correctly. The ExecutionState::ContractViolation variant existed but was never triggered. No scope creep.

## Issues Encountered
- Integration test initially failed because `serde_json::to_value(&Value)` serializes interpreter Values as enum objects (`{"I32": -1}`) rather than raw numbers. Test assertions updated to match the actual serialization format.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Property test infrastructure complete and tested
- Agents can now trigger randomized contract verification via HTTP API
- Ready for subsequent plans in Phase 6 (incremental compilation)

## Self-Check: PASSED

- FOUND: crates/lmlang-check/src/contracts/property.rs
- FOUND: crates/lmlang-server/src/schema/contracts.rs
- FOUND: crates/lmlang-server/src/handlers/contracts.rs
- FOUND: commit fb7bb36 (Task 1)
- FOUND: commit dfafb89 (Task 2)
- All workspace tests pass (368 total, 0 failures)

---
*Phase: 06-full-contract-system-incremental-compilation*
*Completed: 2026-02-18*
