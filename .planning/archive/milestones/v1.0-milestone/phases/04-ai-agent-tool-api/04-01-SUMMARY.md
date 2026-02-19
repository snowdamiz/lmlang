---
phase: 04-ai-agent-tool-api
plan: 01
subsystem: api
tags: [axum, serde, json, rest-api, schema-types, error-handling]

# Dependency graph
requires:
  - phase: 01-core-graph-model
    provides: "ID types (NodeId, EdgeId, FunctionId, ModuleId), TypeId, ComputeNodeOp, FlowEdge, Visibility"
  - phase: 02-persistence
    provides: "ProgramId, StorageError, GraphStore trait"
  - phase: 03-type-checking-interpreter
    provides: "TypeError, FixSuggestion, Value, TraceEntry, RuntimeError"
provides:
  - "Serde Serialize/Deserialize on all interpreter and checker types (Value, TraceEntry, RuntimeError, TypeError, FixSuggestion)"
  - "lmlang-server crate with axum 0.8, tokio, tower-http, serde, uuid, tracing, rusqlite"
  - "Complete API schema types: mutations, queries, diagnostics, simulate, history, programs, verify, common"
  - "ApiError enum with IntoResponse impl and From conversions for CoreError and StorageError"
  - "DiagnosticError with From<TypeError> covering all 6 TypeError variants"
  - "VerifyScope and VerifyResponse in schema/verify.rs for Plan 02 service layer"
affects: [04-02-service-layer, 04-03-handlers, 04-04-integration]

# Tech tracking
tech-stack:
  added: [axum 0.8, tokio 1, tower 0.5, tower-http 0.6, uuid 1, tracing 0.1, tracing-subscriber 0.3]
  patterns: [ApiResponse<T> generic wrapper, serde tag="type" for enums, skip_serializing_if for optional fields, IntoResponse for error mapping]

key-files:
  created:
    - crates/lmlang-server/Cargo.toml
    - crates/lmlang-server/src/lib.rs
    - crates/lmlang-server/src/error.rs
    - crates/lmlang-server/src/schema/mod.rs
    - crates/lmlang-server/src/schema/common.rs
    - crates/lmlang-server/src/schema/diagnostics.rs
    - crates/lmlang-server/src/schema/mutations.rs
    - crates/lmlang-server/src/schema/queries.rs
    - crates/lmlang-server/src/schema/simulate.rs
    - crates/lmlang-server/src/schema/history.rs
    - crates/lmlang-server/src/schema/programs.rs
    - crates/lmlang-server/src/schema/verify.rs
  modified:
    - crates/lmlang-check/Cargo.toml
    - crates/lmlang-check/src/interpreter/value.rs
    - crates/lmlang-check/src/interpreter/trace.rs
    - crates/lmlang-check/src/interpreter/error.rs
    - crates/lmlang-check/src/typecheck/diagnostics.rs

key-decisions:
  - "thiserror error format for ValidationFailed simplified to avoid ambiguous positional arg issue"
  - "Mutation enum uses serde tag=type for clean JSON discriminated union serialization"
  - "SimulateRequest inputs use serde_json::Value so agents send plain JSON, service layer converts to interpreter Value"
  - "DiagnosticError omits FixSuggestion per CONTEXT.md locked decision: errors describe problem only"

patterns-established:
  - "ApiResponse<T> wrapper: all successful responses use ApiResponse::ok(data) or ok_with_warnings(data, warnings)"
  - "Error to HTTP mapping: NotFound->404, BadRequest->400, ValidationFailed->422, InternalError->500, Conflict->409"
  - "DetailLevel enum: Summary/Standard/Full for agent-controlled response verbosity"
  - "skip_serializing_if on optional fields to keep JSON payloads clean"

requirements-completed: [TOOL-05, TOOL-06]

# Metrics
duration: 5min
completed: 2026-02-18
---

# Phase 4 Plan 1: Server Crate Foundation Summary

**lmlang-server crate with axum 0.8, full API schema types (mutations, queries, diagnostics, simulate, history, programs, verify), and ApiError->HTTP status code mapping**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-18T22:00:46Z
- **Completed:** 2026-02-18T22:06:34Z
- **Tasks:** 2
- **Files modified:** 17

## Accomplishments
- Added Serialize/Deserialize derives to all lmlang-check interpreter and type checker types without breaking any of the 115 existing tests
- Created lmlang-server crate with full dependency chain (axum 0.8, tokio, tower-http, serde, uuid, tracing, rusqlite)
- Defined complete API schema covering all 8 endpoint domains with 30+ request/response types
- Implemented ApiError with IntoResponse producing structured JSON with correct HTTP status codes
- From<TypeError> for DiagnosticError converts all 6 TypeError variants with distinct error codes

## Task Commits

Each task was committed atomically:

1. **Task 1: Add serde derives to lmlang-check types and create lmlang-server crate** - `0790456` (feat)
2. **Task 2: Define API schema types and error response system** - `d91c642` (feat)

## Files Created/Modified
- `crates/lmlang-check/Cargo.toml` - Added serde and serde_json dependencies
- `crates/lmlang-check/src/interpreter/value.rs` - Added Serialize/Deserialize to Value enum
- `crates/lmlang-check/src/interpreter/trace.rs` - Added Serialize/Deserialize to TraceEntry
- `crates/lmlang-check/src/interpreter/error.rs` - Added Clone/Serialize/Deserialize to RuntimeError
- `crates/lmlang-check/src/typecheck/diagnostics.rs` - Added Clone/Serialize/Deserialize to TypeError and FixSuggestion
- `crates/lmlang-server/Cargo.toml` - New crate manifest with all dependencies
- `crates/lmlang-server/src/lib.rs` - Module declarations for error and schema
- `crates/lmlang-server/src/error.rs` - ApiError enum, ApiErrorDetail, IntoResponse impl, From conversions
- `crates/lmlang-server/src/schema/mod.rs` - Re-exports all 8 schema sub-modules
- `crates/lmlang-server/src/schema/common.rs` - ApiResponse<T> generic wrapper
- `crates/lmlang-server/src/schema/diagnostics.rs` - DiagnosticError, DiagnosticWarning, DiagnosticDetails, From<TypeError>
- `crates/lmlang-server/src/schema/mutations.rs` - ProposeEditRequest, Mutation enum (8 variants), ProposeEditResponse, CreatedEntity
- `crates/lmlang-server/src/schema/queries.rs` - DetailLevel, NodeView, EdgeView, FunctionView, neighborhood/search types
- `crates/lmlang-server/src/schema/simulate.rs` - SimulateRequest, SimulateResponse, TraceEntryView
- `crates/lmlang-server/src/schema/history.rs` - HistoryEntry, undo/redo responses, checkpoint CRUD, DiffRequest/Response
- `crates/lmlang-server/src/schema/programs.rs` - CreateProgramRequest/Response, ProgramListResponse
- `crates/lmlang-server/src/schema/verify.rs` - VerifyScope, VerifyResponse

## Decisions Made
- Used simplified thiserror format for ValidationFailed variant to avoid ambiguous positional argument issue with tuple variants
- Mutation enum uses `#[serde(tag = "type")]` for JSON discriminated union serialization (e.g., `{"type": "InsertNode", "op": ...}`)
- SimulateRequest inputs use `serde_json::Value` since agents send plain JSON; service layer will convert to interpreter Value
- DiagnosticError intentionally omits FixSuggestion per CONTEXT.md locked decision: errors describe the problem only, agent determines resolution
- WrongInputCount diagnostic maps expected/actual counts as unused in DiagnosticDetails since the struct uses type-level fields, not count fields; the information is in the message string

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed thiserror format string for ValidationFailed**
- **Found during:** Task 2 (error.rs compilation)
- **Issue:** `#[error("validation failed: {0} errors", .0.len())]` produced "ambiguous reference to positional arguments by number in a tuple variant" error
- **Fix:** Simplified to `#[error("validation failed")]` since the detailed error count appears in the JSON response body, not the Display impl
- **Files modified:** crates/lmlang-server/src/error.rs
- **Verification:** `cargo check -p lmlang-server` succeeds
- **Committed in:** d91c642 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor syntactic fix. No scope change.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All schema types compiled and ready for Plan 02 (service layer with ProgramService, undo system)
- VerifyScope and VerifyResponse available in schema/verify.rs for service.rs imports
- ApiError conversions ready for handler error propagation in Plan 03
- rusqlite direct dependency in lmlang-server enables Plan 02 to use Connection in function signatures

## Self-Check: PASSED

All 13 files verified present. Both task commits (0790456, d91c642) verified in git log.

---
*Phase: 04-ai-agent-tool-api*
*Completed: 2026-02-18*
