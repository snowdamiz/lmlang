# Phase 7: Multi-Agent Concurrency - Context

**Gathered:** 2026-02-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Multiple AI agents can simultaneously read and edit the program graph with consistency guarantees preventing corruption. Includes region locking, optimistic concurrency control, conflict detection/resolution, and post-merge invariant verification. Agent authentication, authorization, and role-based access are NOT in scope.

</domain>

<decisions>
## Implementation Decisions

### Region boundaries
- Function-level locking granularity — each function subgraph is the lock unit
- Batch lock acquisition: agents can request multiple function locks atomically (all-or-nothing) for cross-function edits
- Module structure changes (add/remove functions, rename modules) are single-writer serialized globally — not lockable per-agent
- Read-write locks: multiple concurrent readers OR one exclusive writer per function

### Conflict resolution
- Content hashes (blake3 from Phase 2) detect conflicts — conflict if function hash changed since agent's last read
- Conflict responses include a structured diff showing what the other agent changed, so the rejected agent can retry intelligently
- Claude's Discretion: whether to attempt auto-merge for non-overlapping node edits or always reject on hash mismatch
- Claude's Discretion: retry limit strategy (configurable cap vs unlimited)

### Agent experience
- Lock denial responses include: holder identity, what they're doing (if available), and the requesting agent's queue position
- Lock status endpoint: dedicated endpoint showing all current locks, holders, and queues — agents can plan around contention proactively
- Locks have a long TTL (no heartbeat required) — auto-expire to prevent orphaned locks from crashed agents
- Claude's Discretion: agent identification mechanism (session-based vs token-based)

### Verification scope
- Incremental verification: only modified functions + transitive dependents are checked (leverages Phase 6 dirty tracking)
- Synchronous verification: agent holds lock until verification passes — graph is always in a verified state
- Global verification endpoint: any agent can trigger full-graph verification on demand as a safety net
- Claude's Discretion: behavior on verification failure (auto-rollback vs mark-dirty)

### Claude's Discretion
- Conflict resolution strategy: auto-merge vs reject-and-retry
- Retry limit defaults
- Agent identification mechanism (session vs token)
- Verification failure handling (rollback vs mark-dirty)

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches. Key constraints are:
- Must build on the existing HTTP/JSON tool API from Phase 4
- Must leverage blake3 content hashing from Phase 2 for conflict detection
- Must use Phase 6 dirty tracking for incremental verification
- Lock semantics should be familiar to agents (acquire, release, query)

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 07-multi-agent-concurrency*
*Context gathered: 2026-02-19*
