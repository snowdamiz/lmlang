---
phase: 07-multi-agent-concurrency
plan: 01
subsystem: concurrency
tags: [dashmap, tokio, rwlock, uuid, blake3, multi-agent]

# Dependency graph
requires:
  - phase: 04-http-tool-api
    provides: "AppState, ProgramService, handler pattern, axum router"
  - phase: 02-persistence-hashing
    provides: "blake3 hash_function for conflict detection"
provides:
  - "LockManager: per-function RW locks with DashMap, TTL, batch acquire"
  - "AgentRegistry: UUID-based session management with sweep"
  - "Conflict detection: blake3 hash comparison with structured diffs"
  - "Schema types for lock and agent HTTP API"
  - "AppState with async Mutex, LockManager, AgentRegistry"
  - "Error types: LockDenied(423), LockRequired(428), AgentRequired(400)"
  - "extract_agent_id helper for X-Agent-Id header parsing"
affects: [07-02, 07-03, 08-bidirectional-propagation]

# Tech tracking
tech-stack:
  added: [dashmap 6]
  patterns: [async-mutex-state, function-level-locking, agent-session-registry]

key-files:
  created:
    - crates/lmlang-server/src/concurrency/mod.rs
    - crates/lmlang-server/src/concurrency/agent.rs
    - crates/lmlang-server/src/concurrency/lock_manager.rs
    - crates/lmlang-server/src/concurrency/conflict.rs
    - crates/lmlang-server/src/schema/agents.rs
    - crates/lmlang-server/src/schema/locks.rs
  modified:
    - crates/lmlang-server/Cargo.toml
    - crates/lmlang-server/src/lib.rs
    - crates/lmlang-server/src/state.rs
    - crates/lmlang-server/src/error.rs
    - crates/lmlang-server/src/concurrency/mod.rs
    - crates/lmlang-server/src/schema/mod.rs
    - crates/lmlang-server/src/handlers/mutations.rs
    - crates/lmlang-server/src/handlers/queries.rs
    - crates/lmlang-server/src/handlers/programs.rs
    - crates/lmlang-server/src/handlers/verify.rs
    - crates/lmlang-server/src/handlers/simulate.rs
    - crates/lmlang-server/src/handlers/history.rs
    - crates/lmlang-server/src/handlers/compile.rs
    - crates/lmlang-server/src/handlers/contracts.rs

key-decisions:
  - "tokio::sync::Mutex instead of RwLock because ProgramService contains rusqlite::Connection (!Sync)"
  - "Function-level concurrency via LockManager (DashMap) rather than ProgramService-level RwLock"
  - "Waiter tracking in LockDenial provides queue_position for agent planning"
  - "Lock expiry sweep runs every 60 seconds via tokio background task"

patterns-established:
  - "async-mutex-state: handlers use state.service.lock().await (non-blocking to tokio runtime)"
  - "agent-session-registry: UUID-based agent identification via X-Agent-Id header"
  - "function-level-locking: DashMap<FunctionId, FunctionLockState> with all-or-nothing batch acquire"

requirements-completed: [MAGENT-01, MAGENT-02]

# Metrics
duration: 8min
completed: 2026-02-19
---

# Phase 7 Plan 1: Core Concurrency Infrastructure Summary

**Per-function RW lock manager with DashMap, agent session registry, blake3 conflict detection, and async Mutex migration for all handlers**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-19T03:34:58Z
- **Completed:** 2026-02-19T03:43:05Z
- **Tasks:** 2
- **Files modified:** 20

## Accomplishments
- LockManager with per-function RW locks, batch acquire (all-or-nothing), TTL expiry, waiter tracking
- AgentRegistry for UUID-based session management with inactive sweep
- Conflict detection via blake3 hash comparison with structured FunctionDiff
- AppState migrated from std::sync::Mutex to tokio::sync::Mutex (async-aware, non-blocking)
- All 16 existing integration tests pass with zero behavioral changes

## Task Commits

Each task was committed atomically:

1. **Task 1: Create concurrency module** - `7b45a68` (feat)
2. **Task 2: Refactor AppState and error types** - `4dc5d69` (feat)

## Files Created/Modified
- `crates/lmlang-server/src/concurrency/mod.rs` - Module root with re-exports and extract_agent_id helper
- `crates/lmlang-server/src/concurrency/agent.rs` - AgentId, AgentSession, AgentRegistry (DashMap-backed)
- `crates/lmlang-server/src/concurrency/lock_manager.rs` - LockManager, FunctionLockState, LockGrant, LockDenial, LockError
- `crates/lmlang-server/src/concurrency/conflict.rs` - ConflictDetail, FunctionDiff, check_hashes, build_function_diff
- `crates/lmlang-server/src/schema/agents.rs` - RegisterAgentRequest/Response, AgentView, ListAgentsResponse
- `crates/lmlang-server/src/schema/locks.rs` - AcquireLockRequest/Response, ReleaseLockRequest/Response, LockStatusResponse
- `crates/lmlang-server/Cargo.toml` - Added dashmap = "6" dependency
- `crates/lmlang-server/src/lib.rs` - Added pub mod concurrency
- `crates/lmlang-server/src/state.rs` - AppState with tokio::sync::Mutex + LockManager + AgentRegistry
- `crates/lmlang-server/src/error.rs` - LockDenied, LockRequired, AgentRequired, TooManyRetries variants + From<LockError>
- `crates/lmlang-server/src/handlers/*.rs` - All 8 handler files migrated from .lock().unwrap() to .lock().await

## Decisions Made
- **tokio::sync::Mutex over RwLock:** ProgramService contains rusqlite::Connection which is !Sync, making RwLock impossible. tokio::sync::Mutex is async-aware (non-blocking to runtime), which is the key improvement over std::sync::Mutex.
- **Function-level concurrency via LockManager:** Concurrent multi-agent access is managed at the function level through DashMap-backed LockManager, not at the ProgramService level. This is the correct granularity per CONTEXT.md.
- **Waiter tracking for queue position:** When a lock is denied, the requesting agent is appended to a waiters vec. LockDenial includes the agent's 1-based queue_position so agents can plan around contention.
- **30-minute default TTL:** Locks auto-expire after 30 minutes to prevent orphaned locks from crashed agents. Sweep task runs every 60 seconds.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Used tokio::sync::Mutex instead of tokio::sync::RwLock**
- **Found during:** Task 2 (AppState refactor)
- **Issue:** ProgramService contains rusqlite::Connection which is !Sync. tokio::sync::RwLockReadGuard<T> requires T: Sync for Send (multiple readers access data simultaneously). Compiler error: Handler trait not satisfied.
- **Fix:** Used tokio::sync::Mutex instead. Still async-aware (handlers use .lock().await, non-blocking to runtime). Function-level concurrency handled by LockManager.
- **Files modified:** state.rs, all handler files
- **Verification:** cargo check passes, all 16 tests pass
- **Committed in:** 4dc5d69

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary due to rusqlite::Connection !Sync constraint. No functional impact -- concurrent access managed at function level by LockManager as designed.

## Issues Encountered
None beyond the RwLock deviation documented above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- LockManager, AgentRegistry, and conflict detection ready for handler integration (Plan 02)
- extract_agent_id helper ready for agent-aware lock/mutation handlers
- Schema types ready for lock and agent API routes
- All existing tests pass -- backward-compatible foundation

---
*Phase: 07-multi-agent-concurrency*
*Completed: 2026-02-19*
