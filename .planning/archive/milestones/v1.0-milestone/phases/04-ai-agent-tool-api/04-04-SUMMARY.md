---
phase: 04-ai-agent-tool-api
plan: 04
subsystem: testing
tags: [axum, tower, integration-tests, sqlite, serde-json, tokio]

# Dependency graph
requires:
  - phase: 04-ai-agent-tool-api/04-01
    provides: HTTP router, handlers, schema types, ProgramService
  - phase: 04-ai-agent-tool-api/04-02
    provides: Type checker integration, verify endpoint, structured diagnostics
  - phase: 04-ai-agent-tool-api/04-03
    provides: Undo/redo, checkpoints, edit history, simulation endpoint
provides:
  - End-to-end integration test suite validating all 7 phase requirements
  - 13 HTTP-level tests covering mutations, queries, verification, simulation, undo/checkpoint
  - Proof that batch atomicity, dry_run, type mismatch detection, and structured diagnostics work correctly
affects: [phase-05, maintenance, regression-testing]

# Tech tracking
tech-stack:
  added: [tower::ServiceExt (oneshot testing), uuid (temp DB paths)]
  patterns: [router-level integration testing without network server, batch mutations for nodes requiring inputs, shared temp file for test DB isolation]

key-files:
  created:
    - crates/lmlang-server/tests/integration_test.rs
  modified:
    - crates/lmlang-server/src/service.rs

key-decisions:
  - "Used tower::ServiceExt::oneshot for direct router testing without starting a network server"
  - "Fixed ProgramService::in_memory() to use shared temp file instead of separate in-memory SQLite databases (FK constraint fix)"
  - "Batch mutations used for nodes that require inputs (BinaryArith needs edges atomically to pass validation)"
  - "Tests handle both batch-rejection and post-verify detection paths for type mismatches"

patterns-established:
  - "Integration test pattern: test_app() -> Router with unique temp DB, post_json/get_json helpers"
  - "Batch mutation pattern: nodes requiring inputs must be added atomically with their edges"
  - "Type mismatch testing: dual-path assertion handles both batch-time and verify-time detection"

requirements-completed: [TOOL-01, TOOL-02, TOOL-03, TOOL-04, TOOL-05, TOOL-06, STORE-03]

# Metrics
duration: 1min
completed: 2026-02-18
---

# Phase 4 Plan 4: Integration Tests Summary

**13 end-to-end HTTP integration tests proving all 7 phase requirements via tower::ServiceExt oneshot routing**

## Performance

- **Duration:** ~1 min (continuation from prior context that did the heavy lifting)
- **Started:** 2026-02-18T22:40:11Z
- **Completed:** 2026-02-18T22:41:01Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- 13 integration tests pass covering all 7 requirement IDs (TOOL-01 through TOOL-06, STORE-03)
- Full workspace test suite (244 tests) passes with zero regressions
- Fixed ProgramService::in_memory() shared database bug that caused FK constraint violations
- Tests exercise the complete stack: HTTP request -> axum router -> handler -> ProgramService -> graph/storage/checker/interpreter -> HTTP response

## Task Commits

Each task was committed atomically:

1. **Task 1: End-to-end integration tests for all phase requirements** - `14b68a3` (feat)

## Files Created/Modified
- `crates/lmlang-server/tests/integration_test.rs` - 13 end-to-end integration tests covering all 7 phase requirements (896 lines)
- `crates/lmlang-server/src/service.rs` - Fixed in_memory() to use shared temp file for test database isolation

## Decisions Made
- **tower::ServiceExt::oneshot** chosen over axum-test crate for zero extra dependencies; sends requests directly to the router
- **Shared temp file** for ProgramService::in_memory() instead of separate in-memory DBs; both conn and store must share the same SQLite database for FK constraints on edit_log/checkpoints tables
- **Batch mutations** for nodes requiring inputs (e.g., BinaryArith needs 2 input edges); single mutations trigger full validation which rejects incomplete nodes
- **Dual-path type mismatch assertion** handles both batch-rejection (type checker catches at mutation time) and post-verify detection (verify endpoint catches after commit)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed ProgramService::in_memory() separate database issue**
- **Found during:** Task 1 (integration test development)
- **Issue:** `ProgramService::in_memory()` created two separate in-memory SQLite databases -- one for `conn` (via `open_in_memory()`) and one for `store` (via `SqliteStore::in_memory()`). The `edit_log` table in `conn` had a foreign key to `programs(id)`, but the program was only created in `store`'s separate database, causing FOREIGN KEY constraint failures.
- **Fix:** Changed to use a shared temp file path (`std::env::temp_dir().join(format!("lmlang_test_{}.db", uuid::Uuid::new_v4()))`) so both `conn` and `store` connect to the same database.
- **Files modified:** `crates/lmlang-server/src/service.rs`
- **Verification:** All 13 integration tests pass; FK constraint errors eliminated
- **Committed in:** 14b68a3 (Task 1 commit)

**2. [Rule 3 - Blocking] Rewrote tests to use batch mutations for multi-node graphs**
- **Found during:** Task 1 (integration test development)
- **Issue:** BinaryArith(Add) nodes inserted as single mutations failed full-graph validation because they had 0 inputs (requires 2). Single mutations trigger immediate validation.
- **Fix:** Rewrote affected tests to use batch mutations, adding the BinaryArith node and its input edges atomically in one request. Used single mutations only for self-validating operations (Const, Parameter, AddFunction).
- **Files modified:** `crates/lmlang-server/tests/integration_test.rs`
- **Verification:** All 13 tests pass with batch mutation approach
- **Committed in:** 14b68a3 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both auto-fixes necessary for correctness. The in_memory() bug was a genuine defect. The batch mutation approach aligns with how the API is designed to be used. No scope creep.

## Issues Encountered
None beyond the deviations documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 4 is fully complete: all plans executed, all requirements proven by integration tests
- 244 total tests pass across the workspace
- The HTTP API is ready for AI agent consumption with proven: mutations, queries, verification, simulation, undo/checkpoint, and structured diagnostics

## Self-Check: PASSED

- FOUND: crates/lmlang-server/tests/integration_test.rs (895 lines, min_lines: 200 satisfied)
- FOUND: crates/lmlang-server/src/service.rs
- FOUND: .planning/phases/04-ai-agent-tool-api/04-04-SUMMARY.md
- FOUND: commit 14b68a3

---
*Phase: 04-ai-agent-tool-api, Plan: 04*
*Completed: 2026-02-18*
