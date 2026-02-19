---
phase: 04-ai-agent-tool-api
plan: 03
subsystem: api
tags: [axum, http, rest, handlers, router, cors, tracing]

# Dependency graph
requires:
  - phase: 04-ai-agent-tool-api
    plan: 01
    provides: "API schema types (request/response structs, diagnostics, verify scope)"
  - phase: 04-ai-agent-tool-api
    plan: 02
    provides: "ProgramService with all business logic, AppState, ApiError, undo system"
provides:
  - "HTTP handler functions for all 17 API routes"
  - "Axum router wiring handlers to endpoints with CORS and tracing"
  - "Server binary entrypoint with configurable port and database path"
affects: [04-04-integration-tests, phase-05, phase-06]

# Tech tracking
tech-stack:
  added: [axum-0.8-handlers, tower-http-cors, tower-http-trace, tracing-subscriber]
  patterns: [thin-handler-pattern, mutex-lock-then-drop-before-response, active-program-validation]

key-files:
  created:
    - crates/lmlang-server/src/handlers/mod.rs
    - crates/lmlang-server/src/handlers/programs.rs
    - crates/lmlang-server/src/handlers/mutations.rs
    - crates/lmlang-server/src/handlers/queries.rs
    - crates/lmlang-server/src/handlers/verify.rs
    - crates/lmlang-server/src/handlers/simulate.rs
    - crates/lmlang-server/src/handlers/history.rs
    - crates/lmlang-server/src/router.rs
    - crates/lmlang-server/src/main.rs
  modified:
    - crates/lmlang-server/src/lib.rs

key-decisions:
  - "Thin handler pattern: all handlers acquire mutex, call ProgramService, release lock -- no business logic in handlers"
  - "Active program validation in every handler: return 400 if path program_id != active program"
  - "Combined GET+POST routes for /programs and /programs/{id}/checkpoints using axum method chaining"
  - "Path parameter types: i64 for program_id (matching ProgramId(i64)), u32 for node_id/func_id"

patterns-established:
  - "Thin handler pattern: extract -> lock -> call -> respond, never hold Mutex across .await"
  - "Active program guard: every handler that touches graph state checks program_id matches"
  - "DetailQuery extraction: query parameter ?detail=summary|standard|full with Default trait"

requirements-completed: [TOOL-01, TOOL-02, TOOL-03, TOOL-04, TOOL-05, TOOL-06, STORE-03]

# Metrics
duration: 3min
completed: 2026-02-18
---

# Phase 4 Plan 3: HTTP Handlers and Router Summary

**Axum HTTP handlers as thin wrappers around ProgramService with 17-route router, CORS/tracing middleware, and configurable server binary**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-18T22:25:01Z
- **Completed:** 2026-02-18T22:28:28Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- All HTTP handlers implemented following thin-handler pattern (extract -> lock -> call -> respond)
- Router wires 17 routes covering CRUD, mutations, queries, verify, simulate, undo/redo, checkpoints
- Server binary compiles and starts on configurable port with LMLANG_DB_PATH and LMLANG_PORT env vars
- VerifyScope/VerifyResponse consistently imported from schema::verify across all layers (no redefinition)
- CORS permissive layer and TraceLayer for request logging enabled

## Task Commits

Each task was committed atomically:

1. **Task 1: HTTP handlers for mutations, verification, and program management** - `acdb868` (feat)
2. **Task 2: HTTP handlers for queries, simulation, history, and router/main** - `2847eef` (feat)

## Files Created/Modified
- `crates/lmlang-server/src/handlers/mod.rs` - Re-exports all handler sub-modules
- `crates/lmlang-server/src/handlers/programs.rs` - list, create, delete, load program handlers
- `crates/lmlang-server/src/handlers/mutations.rs` - propose_edit handler (thin wrapper)
- `crates/lmlang-server/src/handlers/queries.rs` - overview, get_node, get_function, neighborhood, search
- `crates/lmlang-server/src/handlers/verify.rs` - verify handler with scope parsing
- `crates/lmlang-server/src/handlers/simulate.rs` - simulate handler
- `crates/lmlang-server/src/handlers/history.rs` - list_history, undo, redo, checkpoints, diff
- `crates/lmlang-server/src/router.rs` - build_router with 17 routes, CORS + tracing layers
- `crates/lmlang-server/src/main.rs` - Binary entrypoint with env var config
- `crates/lmlang-server/src/lib.rs` - Added handlers and router modules

## Decisions Made
- Thin handler pattern: all handlers acquire mutex, call ProgramService, release lock -- no business logic in handlers
- Active program validation in every handler: return 400 if path program_id != active program ID
- Combined GET+POST routes for /programs and /programs/{id}/checkpoints using axum method chaining
- Path parameter types: i64 for program_id (matching ProgramId(i64)), u32 for node_id/func_id

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Complete HTTP API surface ready for integration testing (Plan 04)
- Server binary compiles and can start accepting requests
- All 7 requirements (TOOL-01 through TOOL-06, STORE-03) have HTTP endpoints wired

## Self-Check: PASSED

- All 10 files verified present on disk
- Commit acdb868 (Task 1) verified in git log
- Commit 2847eef (Task 2) verified in git log
- `cargo build -p lmlang-server` succeeds with no warnings from lmlang-server code

---
*Phase: 04-ai-agent-tool-api*
*Completed: 2026-02-18*
