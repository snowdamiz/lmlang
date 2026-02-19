# Phase 7: Multi-Agent Concurrency - Research

**Researched:** 2026-02-18
**Domain:** Concurrent multi-agent graph editing with region locking, optimistic concurrency control, conflict detection, and post-merge verification
**Confidence:** HIGH

## Summary

Phase 7 transforms the single-agent lmlang server into a multi-agent concurrent system. Currently, `AppState` wraps `ProgramService` in `Arc<Mutex<ProgramService>>`, giving the entire service a single coarse-grained lock. Every handler (mutations, queries, verify, simulate) acquires this global mutex for the duration of the request. This means agents are fully serialized today -- one agent's mutation blocks all other agents from even reading.

The transformation requires three layers: (1) a **lock manager** that tracks per-function read-write locks with TTL-based auto-expiry, supporting atomic batch acquisition; (2) a **conflict detector** built on the existing blake3 per-function hashing infrastructure, which detects when a function's content changed between an agent's read and write; and (3) an **incremental verification** integration that runs type-checking and contract validation on modified functions plus their transitive dependents before releasing locks, leveraging Phase 6's dirty tracking and the existing `IncrementalState::compute_dirty` mechanism.

The existing codebase provides strong foundations: `hash_function()` already computes deterministic per-function blake3 hashes (Phase 2), `compute_dirty_set()` and `IncrementalState::compute_dirty()` already identify dirty functions and their transitive dependents via BFS on the reverse call graph (Phase 6), the `ProgramGraph` is `Clone` (used by batch mutations for clone-and-swap), and `ProgramGraph::function_nodes()` returns all nodes owned by a function. The main new work is replacing the global `Mutex<ProgramService>` with fine-grained per-function `RwLock`s, adding the lock manager and conflict detection as new modules, and extending the HTTP API with lock-related endpoints.

**Primary recommendation:** Keep `ProgramService` methods synchronous but restructure `AppState` to use `tokio::sync::RwLock` for the graph and a separate `LockManager` struct backed by `DashMap` for per-function lock tracking. Handlers acquire function locks from the `LockManager` before delegating to `ProgramService`. Use reject-and-retry (not auto-merge) for conflict resolution, session-based agent IDs, and auto-rollback on verification failure.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Region boundaries:**
- Function-level locking granularity -- each function subgraph is the lock unit
- Batch lock acquisition: agents can request multiple function locks atomically (all-or-nothing) for cross-function edits
- Module structure changes (add/remove functions, rename modules) are single-writer serialized globally -- not lockable per-agent
- Read-write locks: multiple concurrent readers OR one exclusive writer per function

**Conflict resolution:**
- Content hashes (blake3 from Phase 2) detect conflicts -- conflict if function hash changed since agent's last read
- Conflict responses include a structured diff showing what the other agent changed, so the rejected agent can retry intelligently
- Claude's Discretion: whether to attempt auto-merge for non-overlapping node edits or always reject on hash mismatch
- Claude's Discretion: retry limit strategy (configurable cap vs unlimited)

**Agent experience:**
- Lock denial responses include: holder identity, what they're doing (if available), and the requesting agent's queue position
- Lock status endpoint: dedicated endpoint showing all current locks, holders, and queues -- agents can plan around contention proactively
- Locks have a long TTL (no heartbeat required) -- auto-expire to prevent orphaned locks from crashed agents
- Claude's Discretion: agent identification mechanism (session-based vs token-based)

**Verification scope:**
- Incremental verification: only modified functions + transitive dependents are checked (leverages Phase 6 dirty tracking)
- Synchronous verification: agent holds lock until verification passes -- graph is always in a verified state
- Global verification endpoint: any agent can trigger full-graph verification on demand as a safety net
- Claude's Discretion: behavior on verification failure (auto-rollback vs mark-dirty)

### Claude's Discretion
- Conflict resolution strategy: auto-merge vs reject-and-retry
- Retry limit defaults
- Agent identification mechanism (session vs token)
- Verification failure handling (rollback vs mark-dirty)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MAGENT-01 | Multiple AI agents can read and edit the graph concurrently through the tool API | Replace `Arc<Mutex<ProgramService>>` with `Arc<RwLock<ProgramService>>` + per-function `LockManager`. Agents identify via session ID in request headers. Concurrent reads proceed with `RwLock::read()`, writes require function-level exclusive lock from `LockManager` + graph-level write lock for application. |
| MAGENT-02 | Region-level locking prevents conflicting edits to the same subgraph | `LockManager` uses `DashMap<FunctionId, FunctionLock>` tracking lock state (readers count, exclusive writer, queue, TTL). Batch acquisition sorts function IDs and acquires all-or-nothing. Lock denial returns holder identity, description, and queue position. Lock status endpoint exposes full lock state. |
| MAGENT-03 | Optimistic concurrency with conflict detection and rollback for overlapping edits | On mutation submit, agent includes `expected_hashes: HashMap<FunctionId, String>` (blake3 hex from last read). Server computes current hash via `hash_function()`, rejects if mismatch. Rejection response includes structured diff (added/removed/modified nodes). Clone-and-swap pattern already supports rollback. |
| MAGENT-04 | Verification runs on merge to ensure global invariants hold after concurrent modifications | After applying mutations, run `typecheck::validate_graph()` on modified subgraph + use `IncrementalState::compute_dirty()` to identify transitive dependents. Agent holds lock until verification passes. On verification failure, auto-rollback the mutations (revert clone). Global verify endpoint triggers full `validate_graph()`. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x (already in workspace) | Async runtime, `tokio::sync::RwLock`, `tokio::time` for TTL | Already used; provides async-aware RwLock needed for holding locks across await points |
| dashmap | 6.x | Concurrent HashMap for lock manager internal state | Sharded locking, 173M+ downloads, de facto standard for concurrent maps in Rust |
| blake3 | (already in lmlang-storage) | Per-function content hashing for conflict detection | Already used in Phase 2; `hash_function()` and `hash_all_functions()` exist |
| uuid | 1.x (already in workspace) | Agent session ID generation | Already a dependency; provides unique session identifiers |
| axum | 0.8 (already in workspace) | HTTP routing, extractors for lock endpoints | Already the framework; add new routes for lock management |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde/serde_json | (already in workspace) | Lock state serialization, diff responses | Already used throughout for API responses |
| tracing | 0.1 (already in workspace) | Lock acquisition/release logging, contention monitoring | Already used; add span context for agent operations |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| DashMap | `tokio::sync::RwLock<HashMap>` | DashMap has lower contention via sharding; RwLock<HashMap> is simpler but creates a single bottleneck for lock manager itself |
| tokio::sync::RwLock (graph) | std::sync::RwLock | std::sync is fine for short critical sections but tokio::sync is needed because verification across await points may be required and write-preferring fairness prevents reader starvation of writers |
| Session-based agent IDs | Token-based (API keys) | Sessions are simpler, no auth infrastructure needed, sufficient for the "no auth/authz" scope constraint |

**Installation:**
```bash
cargo add dashmap@6 --package lmlang-server
```
(All other dependencies already exist in the workspace.)

## Architecture Patterns

### Current Architecture (Pre-Phase 7)
```
AppState
  └── service: Arc<Mutex<ProgramService>>    # global lock, fully serialized

ProgramService
  ├── graph: ProgramGraph                     # the in-memory graph
  ├── store: SqliteStore                      # persistence
  ├── conn: Connection                        # edit log queries
  ├── program_id: ProgramId                   # active program
  └── incremental_state: Option<IncrementalState>  # dirty tracking
```

Every handler does `state.service.lock().unwrap()` to get exclusive access. One agent at a time.

### Target Architecture (Phase 7)
```
AppState
  ├── service: Arc<tokio::sync::RwLock<ProgramService>>   # graph access
  ├── lock_manager: Arc<LockManager>                       # per-function locks
  └── agent_registry: Arc<AgentRegistry>                   # session tracking

LockManager
  ├── function_locks: DashMap<FunctionId, FunctionLockState>
  ├── global_write_lock: tokio::sync::RwLock<()>  # for module structure changes
  └── default_ttl: Duration

FunctionLockState
  ├── state: LockState                    # Unlocked | ReadLocked(readers) | WriteLocked(holder)
  ├── queue: VecDeque<LockRequest>        # waiting agents
  ├── holder_info: Option<LockHolderInfo> # who holds, what they're doing
  └── expires_at: Option<Instant>         # TTL auto-expiry

AgentRegistry
  └── sessions: DashMap<AgentId, AgentSession>  # active agent sessions
```

### Recommended Module Structure
```
lmlang-server/src/
├── lib.rs                     # (existing)
├── main.rs                    # (existing)
├── error.rs                   # (existing, extend with lock errors)
├── state.rs                   # (refactor: Arc<RwLock<ProgramService>> + LockManager + AgentRegistry)
├── service.rs                 # (existing, minor: add hash snapshot methods)
├── router.rs                  # (extend: add lock and agent routes)
├── handlers/
│   ├── mod.rs                 # (extend: add locks module)
│   ├── mutations.rs           # (refactor: acquire function locks before calling service)
│   ├── queries.rs             # (refactor: use read lock)
│   ├── locks.rs               # NEW: acquire, release, status, batch-acquire
│   ├── agents.rs              # NEW: register, heartbeat (optional), list
│   └── ...                    # (existing handlers unchanged)
├── concurrency/
│   ├── mod.rs                 # NEW: module root
│   ├── lock_manager.rs        # NEW: FunctionLockState, LockManager, batch acquisition
│   ├── conflict.rs            # NEW: hash-based conflict detection, diff generation
│   ├── agent.rs               # NEW: AgentId, AgentSession, AgentRegistry
│   └── verify.rs              # NEW: incremental verification on commit
└── schema/
    ├── mod.rs                 # (extend)
    ├── locks.rs               # NEW: lock request/response types
    ├── agents.rs              # NEW: agent registration types
    └── ...                    # (existing schemas unchanged)
```

### Pattern 1: Handler Lock Flow (Write Path)
**What:** Agent acquires function lock(s), submits mutations, server validates hashes, applies, verifies, releases.
**When to use:** All mutation requests in multi-agent mode.
**Example:**
```rust
// Handler: POST /programs/{id}/mutations
pub async fn propose_edit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(program_id): Path<i64>,
    Json(req): Json<ConcurrentEditRequest>,
) -> Result<Json<ConcurrentEditResponse>, ApiError> {
    let agent_id = extract_agent_id(&headers)?;

    // 1. Determine which functions are affected by these mutations
    let affected_functions = req.affected_function_ids();

    // 2. Verify agent holds write locks for all affected functions
    state.lock_manager.verify_write_locks(&agent_id, &affected_functions)?;

    // 3. Validate expected hashes (optimistic concurrency check)
    {
        let service = state.service.read().await;
        for (func_id, expected_hash) in &req.expected_hashes {
            let current_hash = hash_function(service.graph(), *func_id);
            if current_hash.to_hex().as_str() != expected_hash {
                // Hash mismatch: build structured diff and reject
                let diff = build_function_diff(service.graph(), *func_id, expected_hash);
                return Err(ApiError::Conflict(/* structured conflict response */));
            }
        }
    }

    // 4. Apply mutations (requires write access to service)
    let response = {
        let mut service = state.service.write().await;
        let result = service.propose_edit(req.into_inner())?;

        // 5. Verify affected region + transitive dependents
        if result.committed {
            let verification = run_incremental_verification(&service, &affected_functions)?;
            if !verification.valid {
                // Auto-rollback: undo the edit
                service.undo()?;
                return Err(ApiError::ValidationFailed(verification.errors));
            }
        }

        result
    };

    Ok(Json(response.into()))
}
```

### Pattern 2: Batch Lock Acquisition (Deadlock-Free)
**What:** Acquire multiple function locks atomically, sorted by FunctionId to prevent deadlocks.
**When to use:** Cross-function edits requiring locks on multiple functions.
**Example:**
```rust
impl LockManager {
    /// Acquires write locks on multiple functions atomically (all-or-nothing).
    /// Functions are sorted by ID to prevent deadlock from circular wait.
    pub fn batch_acquire_write(
        &self,
        agent_id: &AgentId,
        function_ids: &[FunctionId],
        description: Option<String>,
    ) -> Result<Vec<LockGrant>, LockError> {
        // Sort to prevent deadlock (consistent ordering)
        let mut sorted_ids = function_ids.to_vec();
        sorted_ids.sort_by_key(|f| f.0);
        sorted_ids.dedup();

        // Try to acquire all locks; if any fails, release those already acquired
        let mut acquired = Vec::new();
        for &func_id in &sorted_ids {
            match self.try_acquire_write(agent_id, func_id, description.clone()) {
                Ok(grant) => acquired.push(grant),
                Err(e) => {
                    // Rollback: release all locks acquired so far
                    for grant in acquired {
                        self.release(agent_id, grant.function_id);
                    }
                    return Err(e);
                }
            }
        }
        Ok(acquired)
    }
}
```

### Pattern 3: Global Write Lock for Module Structure Changes
**What:** Module-level changes (add/remove functions, add/remove modules) serialize globally.
**When to use:** Mutations that change the function/module structure, not just function body content.
**Example:**
```rust
// For module structure changes, acquire the global write lock
// This blocks ALL other operations (reads and writes) until complete
let _global_guard = state.lock_manager.global_write_lock.write().await;
let mut service = state.service.write().await;
// Apply AddFunction / AddModule / RemoveFunction mutations
// Release global lock when _global_guard drops
```

### Pattern 4: TTL-Based Lock Expiry
**What:** Locks auto-expire after a configurable TTL to prevent orphaned locks from crashed agents.
**When to use:** All locks have a TTL. A background task periodically sweeps expired locks.
**Example:**
```rust
impl LockManager {
    /// Spawns a background task that periodically sweeps expired locks.
    pub fn start_expiry_sweep(self: &Arc<Self>, interval: Duration) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                manager.sweep_expired_locks();
            }
        });
    }

    fn sweep_expired_locks(&self) {
        let now = Instant::now();
        self.function_locks.retain(|_func_id, lock_state| {
            if let Some(expires_at) = lock_state.expires_at {
                if now > expires_at {
                    tracing::warn!(
                        func_id = ?_func_id,
                        holder = ?lock_state.holder_info,
                        "Lock expired, auto-releasing"
                    );
                    // Promote next in queue or release
                    return false; // remove expired entry
                }
            }
            true
        });
    }
}
```

### Anti-Patterns to Avoid
- **Holding the global service RwLock across long operations:** Read-heavy queries should acquire a read lock, extract needed data quickly, and drop the lock before serializing the response. Never hold the service lock while doing I/O (except the brief write-lock for mutation application).
- **Per-node locking:** Locking at node granularity would add massive overhead and deadlock risk. Function-level is the right granularity for this system.
- **Lock acquisition without consistent ordering:** Always sort function IDs before batch acquisition. Without consistent ordering, agents requesting {A, B} and {B, A} can deadlock.
- **Auto-merge with structural changes:** Attempting to auto-merge when functions have been restructured (nodes added/removed by another agent) is extremely error-prone. Reject-and-retry is safer.

## Discretion Recommendations

### 1. Conflict Resolution: Reject-and-Retry (RECOMMENDED)

**Recommendation:** Always reject on hash mismatch. Do NOT attempt auto-merge.

**Rationale:**
- The graph's integrity depends on edge connectivity between nodes. If agent A adds node X and wires it to node Y, and agent B independently removes node Y, an auto-merge would produce a dangling edge. Detecting which edits are "non-overlapping" in a graph (vs. in a text document) requires understanding the full topology -- edges connect nodes across the function, and what looks independent at the node level may conflict at the edge level.
- Reject-and-retry is the standard pattern for database optimistic concurrency control. It is well-understood, deterministic, and avoids the complexity of merge semantics.
- The structured diff in the rejection response gives the rejected agent full visibility into what changed, enabling intelligent retry. This is sufficient for AI agents who can re-plan their edits.
- Auto-merge can be added later as an optimization if contention patterns warrant it, without changing the fundamental API.

**Confidence:** HIGH -- reject-and-retry is industry standard for OCC and avoids graph-specific merge hazards.

### 2. Retry Limit: Configurable with Sensible Default (RECOMMENDED)

**Recommendation:** Default retry limit of 5 per transaction, configurable via server config. No client-side enforcement -- the server tracks retry count per agent per operation and returns 429 (Too Many Requests) after the limit.

**Rationale:**
- Unlimited retries risk livelock when two agents repeatedly conflict on the same function.
- A default of 5 retries is generous enough for transient conflicts but prevents infinite loops.
- Making it configurable allows tuning for specific workloads (e.g., higher limits for large teams, lower for latency-sensitive scenarios).
- Server-side enforcement is more reliable than trusting agents to self-limit.

**Confidence:** MEDIUM -- the specific number (5) is a reasonable default but may need tuning in practice.

### 3. Agent Identification: Session-Based (RECOMMENDED)

**Recommendation:** Use session-based agent identification. Agent registers via `POST /agents/register` providing an optional display name, receives an `AgentId` (UUID v4). The agent includes `X-Agent-Id: <uuid>` header in all subsequent requests. Sessions have a configurable inactivity timeout (default: 1 hour) after which locks are released and the session is cleaned up.

**Rationale:**
- Auth/authz is explicitly out of scope, so token-based (API key) identification adds unnecessary complexity.
- Session UUIDs are simple, unique, and don't require key management infrastructure.
- The registration endpoint allows agents to set a display name (e.g., "Agent building auth module") which is shown in lock denial responses, giving other agents useful context.
- Session inactivity timeout doubles as the lock TTL safety net -- if an agent disappears, its session eventually expires and all its locks are released.

**Confidence:** HIGH -- sessions are the simplest mechanism that meets the requirements without auth infrastructure.

### 4. Verification Failure: Auto-Rollback (RECOMMENDED)

**Recommendation:** On verification failure after a mutation, auto-rollback the mutation and return verification errors to the agent. The graph always remains in a verified state.

**Rationale:**
- The user explicitly decided "synchronous verification: agent holds lock until verification passes -- graph is always in a verified state." Auto-rollback is the only way to maintain this invariant.
- Mark-dirty would leave the graph in a potentially invalid state, violating the "always verified" guarantee. Other agents reading the graph would see invalid state.
- The existing `propose_edit` method already supports rollback: single mutations use inverse-revert, batch mutations use clone-and-swap (the clone is discarded on failure). This infrastructure is already in place.
- The undo system from Phase 4 provides an additional rollback mechanism if needed.

**Confidence:** HIGH -- auto-rollback is the only strategy consistent with the "always verified" invariant.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Concurrent hash map for lock state | Custom sharded map | DashMap | Proven sharded locking, 173M+ downloads, handles concurrent access correctly |
| Content hashing for conflict detection | New hash implementation | Existing `hash_function()` from lmlang-storage | Already deterministic, already blake3, already per-function -- exactly what's needed |
| Dirty tracking for verification scope | New change tracking | Existing `IncrementalState::compute_dirty()` + `build_call_graph()` | Already computes dirty functions + transitive dependents via BFS on reverse call graph |
| Async RwLock with fairness | Custom async lock | `tokio::sync::RwLock` | Write-preferring fairness prevents reader starvation, well-tested, async-aware |
| Graph diff for conflict responses | Custom diff algorithm | Compare `function_nodes()` + `hash_node_content()` before/after | Existing node hashing + function_nodes enumeration provides the primitives |

**Key insight:** The existing codebase has nearly all the building blocks. Phase 2 provides deterministic per-function blake3 hashing. Phase 6 provides dirty tracking with BFS propagation. Phase 4's clone-and-swap mutation pattern provides natural rollback. The main new work is the lock manager and API wiring, not the underlying conflict detection or verification.

## Common Pitfalls

### Pitfall 1: Deadlock from Inconsistent Lock Ordering
**What goes wrong:** Agent A requests locks {func_1, func_2}, Agent B requests {func_2, func_1}. Both acquire their first lock, then block waiting for the second.
**Why it happens:** Acquiring multiple locks without a consistent global ordering.
**How to avoid:** Always sort function IDs before acquisition. The `batch_acquire_write` method must sort by `FunctionId.0` before attempting acquisition. This is a classic deadlock prevention technique (resource ordering).
**Warning signs:** Tests where two agents requesting overlapping function sets both hang indefinitely.

### Pitfall 2: Lock-Service Lock Ordering
**What goes wrong:** Agent acquires function lock, then tries to acquire service write lock. Meanwhile, another operation holds service read lock and tries to acquire the same function lock.
**Why it happens:** Two different locking layers (LockManager function locks and service RwLock) without consistent ordering.
**How to avoid:** Establish a strict ordering: always acquire function locks (from LockManager) FIRST, then acquire the service RwLock. Never reverse this order. Handlers should: (1) verify function locks, (2) acquire service lock, (3) operate, (4) release service lock, (5) release function locks (or let TTL handle it).
**Warning signs:** Intermittent hangs under concurrent load testing.

### Pitfall 3: Stale Hash Reads Leading to False Conflicts
**What goes wrong:** Agent reads function hash, another agent modifies a different function (not the one being edited), but the global state pointer changes, causing the hash comparison to fail erroneously.
**Why it happens:** Hash comparison happening at the wrong granularity (e.g., comparing whole-graph hash instead of per-function hash).
**How to avoid:** Always compare per-function hashes using `hash_function(graph, func_id)`, never whole-graph hashes. Each function's hash is independent of other functions (verified by existing tests: `test_function_hash_independent_across_functions`).
**Warning signs:** Conflict rejections when agents are editing completely different functions.

### Pitfall 4: Orphaned Locks After Agent Crash
**What goes wrong:** Agent acquires write lock, crashes, lock is never released. Other agents permanently blocked from that function.
**Why it happens:** No TTL or no background expiry sweep.
**How to avoid:** All locks have a TTL (default: 30 minutes). Background sweep task runs every 60 seconds, releasing expired locks. The sweep promotes the next queued agent if any.
**Warning signs:** Functions becoming permanently "locked" during development testing when agents are killed mid-operation.

### Pitfall 5: Race Between Hash Check and Mutation Application
**What goes wrong:** Agent checks hash (matches), but between the check and the actual mutation application, another agent modifies the same function.
**Why it happens:** TOCTOU (time-of-check to time-of-use) race if the hash check and mutation are not atomic.
**How to avoid:** The write lock on the function (from LockManager) prevents this. If Agent A holds the write lock for function F, no other agent can modify F between A's hash check and A's mutation. The sequence must be: acquire write lock -> check hash -> apply mutation -> verify -> release lock (or rollback and release).
**Warning signs:** Data corruption in stress tests despite hash checking being implemented.

### Pitfall 6: Read Starvation from Write-Preferring Lock
**What goes wrong:** `tokio::sync::RwLock` is write-preferring (FIFO queue). If agents frequently request write locks, read requests queue behind them and experience high latency.
**Why it happens:** Write-preferring is fair but can starve reads under heavy write load.
**How to avoid:** Keep write critical sections as short as possible. Read the graph state quickly, drop the service read lock, then do serialization/response building outside the lock. For the lock manager itself, DashMap's sharded design avoids this problem.
**Warning signs:** Read latency spikes under concurrent write testing.

## Code Examples

### Existing Infrastructure: Per-Function Hashing (from lmlang-storage/src/hash.rs)
```rust
// Source: /Users/sn0w/Documents/dev/lmlang/crates/lmlang-storage/src/hash.rs
pub fn hash_function(graph: &ProgramGraph, func_id: FunctionId) -> blake3::Hash {
    let mut func_nodes = graph.function_nodes(func_id);
    func_nodes.sort_by_key(|n| n.0);
    // ... two-pass: content hashes, then composite hashes with edges
    // Returns deterministic blake3 hash of entire function subgraph
}

pub fn hash_all_functions(graph: &ProgramGraph) -> HashMap<FunctionId, blake3::Hash> {
    // Computes hashes for all functions, sorted by FunctionId
}
```

### Existing Infrastructure: Current Handler Pattern (from lmlang-server/src/handlers/mutations.rs)
```rust
// Source: /Users/sn0w/Documents/dev/lmlang/crates/lmlang-server/src/handlers/mutations.rs
pub async fn propose_edit(
    State(state): State<AppState>,
    Path(program_id): Path<i64>,
    Json(req): Json<ProposeEditRequest>,
) -> Result<Json<ProposeEditResponse>, ApiError> {
    let mut service = state.service.lock().unwrap();  // <-- global lock, blocks everything
    let active_id = service.program_id();
    if active_id.0 != program_id {
        return Err(ApiError::BadRequest(/* ... */));
    }
    let response = service.propose_edit(req)?;
    Ok(Json(response))
}
```

### Existing Infrastructure: Current AppState (from lmlang-server/src/state.rs)
```rust
// Source: /Users/sn0w/Documents/dev/lmlang/crates/lmlang-server/src/state.rs
pub struct AppState {
    pub service: Arc<Mutex<ProgramService>>,  // <-- will change to RwLock
}
```

### Existing Infrastructure: Dirty Tracking (from lmlang-codegen/src/incremental.rs)
```rust
// Source: /Users/sn0w/Documents/dev/lmlang/crates/lmlang-codegen/src/incremental.rs
impl IncrementalState {
    pub fn compute_dirty(
        &self,
        current_hashes: &HashMap<FunctionId, [u8; 32]>,
        call_graph: &HashMap<FunctionId, Vec<FunctionId>>,
    ) -> RecompilationPlan {
        // Phase 1: Find directly dirty functions (hash mismatch)
        // Phase 2: BFS through reverse call graph for transitive dependents
        // Phase 3: Everything else is cached
    }
}
```

### New: Lock Manager State Types
```rust
/// Unique agent identifier (UUID v4, assigned at session registration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

/// Describes who holds a lock and what they're doing.
#[derive(Debug, Clone, Serialize)]
pub struct LockHolderInfo {
    pub agent_id: AgentId,
    pub agent_name: Option<String>,
    pub description: Option<String>,
    pub acquired_at: Instant,
    pub expires_at: Instant,
}

/// The state of a per-function lock.
#[derive(Debug)]
pub enum FunctionLockState {
    /// No one holds this lock.
    Unlocked,
    /// One or more readers hold the lock.
    ReadLocked {
        readers: HashSet<AgentId>,
        expires_at: Instant,
    },
    /// Exactly one writer holds the lock.
    WriteLocked {
        holder: LockHolderInfo,
    },
}

/// A request from an agent waiting in the lock queue.
#[derive(Debug)]
pub struct LockRequest {
    pub agent_id: AgentId,
    pub mode: LockMode,  // Read or Write
    pub requested_at: Instant,
    pub waker: Option<tokio::sync::oneshot::Sender<LockGrant>>,
}

/// Result of a successful lock acquisition.
#[derive(Debug, Clone, Serialize)]
pub struct LockGrant {
    pub function_id: FunctionId,
    pub mode: LockMode,
    pub expires_at: String,  // ISO 8601
}

/// Result of a failed lock acquisition.
#[derive(Debug, Clone, Serialize)]
pub struct LockDenial {
    pub function_id: FunctionId,
    pub holder: LockHolderInfo,
    pub queue_position: usize,
}
```

### New: Conflict Detection Response
```rust
/// Structured diff for conflict responses.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictDetail {
    pub function_id: FunctionId,
    pub expected_hash: String,
    pub current_hash: String,
    pub changes: FunctionDiff,
}

/// What changed in a function since the agent last read it.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDiff {
    pub added_nodes: Vec<NodeId>,
    pub removed_nodes: Vec<NodeId>,
    pub modified_nodes: Vec<NodeId>,
    pub added_edges: Vec<EdgeId>,
    pub removed_edges: Vec<EdgeId>,
}
```

### New: API Endpoints
```
# Agent management
POST   /agents/register              # Register agent, get AgentId
DELETE /agents/{agent_id}            # Deregister agent, release all locks
GET    /agents                        # List active agents

# Lock management
POST   /programs/{id}/locks/acquire   # Acquire lock(s) on function(s)
POST   /programs/{id}/locks/release   # Release lock(s)
GET    /programs/{id}/locks           # Lock status: all current locks, holders, queues
POST   /programs/{id}/locks/batch     # Batch acquire multiple function locks atomically

# Modified existing endpoints (now require agent identity)
POST   /programs/{id}/mutations       # Extended with expected_hashes, agent_id header
POST   /programs/{id}/verify          # Extended: global verify endpoint
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Global Mutex for entire service | Per-resource RwLock with lock manager | Phase 7 | Enables concurrent reads, concurrent edits to different functions |
| No conflict detection | Blake3 hash-based optimistic concurrency | Phase 7 | Prevents silent data corruption from concurrent edits |
| No agent identity | Session-based agent registration | Phase 7 | Enables lock ownership tracking and contention visibility |

## Open Questions

1. **Cross-function edge handling during concurrent edits**
   - What we know: Edges can connect nodes in different functions (cross-function call edges). An agent editing function A might add a Call node targeting function B.
   - What's unclear: If agent A holds a write lock on function A and adds a Call to function B, should agent A need a read lock on function B to validate the call target exists? Or is it sufficient to validate at commit time?
   - Recommendation: Require a read lock on any function referenced by a new Call node. This prevents the target function from being deleted while the call is being wired up. The batch acquire endpoint makes this ergonomic.

2. **Undo/redo interaction with multi-agent concurrency**
   - What we know: The existing undo system records per-program edit history. In single-agent mode, undo reverses the last edit.
   - What's unclear: In multi-agent mode, whose edits does "undo" affect? Should each agent have an independent undo stack, or should undo be globally serialized?
   - Recommendation: Per-agent undo stacks. Each agent can only undo their own edits. Global undo (undoing another agent's work) should require explicit confirmation or a different API. This is a follow-up design question that may warrant a separate discussion, but for Phase 7 the simplest approach is to disable global undo/redo when multiple agents are active, or scope undo to the requesting agent's edit history.

3. **Lock renewal for long-running operations**
   - What we know: Locks have a long TTL (no heartbeat required per the user decision).
   - What's unclear: What if an agent needs more time than the TTL? Should there be a renewal endpoint?
   - Recommendation: Add a `POST /programs/{id}/locks/renew` endpoint that extends the TTL. The agent calls it proactively if they anticipate needing more time. Default TTL should be 30 minutes (long enough that renewal is rarely needed).

## Sources

### Primary (HIGH confidence)
- **Existing codebase** - `lmlang-server/src/service.rs`, `state.rs`, `handlers/mutations.rs` (current architecture)
- **Existing codebase** - `lmlang-storage/src/hash.rs` (blake3 per-function hashing, `hash_function()`, `hash_all_functions()`)
- **Existing codebase** - `lmlang-storage/src/dirty.rs` (dirty set computation)
- **Existing codebase** - `lmlang-codegen/src/incremental.rs` (`IncrementalState::compute_dirty()`, `build_call_graph()`)
- **Existing codebase** - `lmlang-core/src/graph.rs` (`ProgramGraph`, `function_nodes()`, Clone impl)
- [tokio::sync::RwLock official docs](https://docs.rs/tokio/latest/tokio/sync/struct.RwLock.html) - fairness, async awareness, owned guards
- [DashMap GitHub](https://github.com/xacrimon/dashmap) - sharded concurrent HashMap, v6.x stable
- [axum shared state discussions](https://github.com/tokio-rs/axum/discussions/1758) - fine-grained locking patterns in axum

### Secondary (MEDIUM confidence)
- [Optimistic Concurrency Control - Wikipedia](https://en.wikipedia.org/wiki/Optimistic_concurrency_control) - OCC phases (begin, modify, validate, commit/rollback)
- [Distributed Locking: A Practical Guide](https://www.architecture-weekly.com/p/distributed-locking-a-practical-guide) - TTL best practices, lock expiry patterns
- [Shadecoder: Optimistic Concurrency Control Practical Guide 2025](https://www.shadecoder.com/topics/optimistic-concurrency-control-a-practical-guide-for-2025) - reject/retry patterns, version stamping

### Tertiary (LOW confidence)
- None -- all findings verified against official docs or existing codebase.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in the workspace except DashMap (which is the most-used concurrent map in Rust, 173M+ downloads)
- Architecture: HIGH -- builds directly on existing patterns (AppState, ProgramService, handler delegation), well-understood concurrency primitives
- Pitfalls: HIGH -- deadlock prevention via ordering, TOCTOU prevention via lock-then-check, TTL expiry are all established patterns with existing infrastructure support
- Discretion recommendations: HIGH -- all four recommendations align with the user's explicit requirements and industry standard practices

**Research date:** 2026-02-18
**Valid until:** 2026-03-20 (30 days -- stable domain, no rapidly changing dependencies)
