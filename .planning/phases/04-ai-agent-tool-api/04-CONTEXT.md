# Phase 4: AI Agent Tool API - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

HTTP/JSON interface for AI agents to build, query, verify, simulate, and undo changes to program graphs. Agents interact with the graph exclusively through this API. Multi-agent concurrency is a separate phase (Phase 7).

</domain>

<decisions>
## Implementation Decisions

### Mutation workflow
- Support both single-operation and batch mutation modes -- agent chooses based on task complexity
- `dry_run` flag on each mutation call: dry_run=true previews validation results, dry_run=false validates and commits atomically
- Batch mutations are all-or-nothing: if any mutation in the batch fails validation, the entire batch is rejected with no partial commits

### Query & context shape
- No default query scope -- every query must explicitly specify what it wants (function, neighborhood, node ID, filter, etc.)
- Both a program overview endpoint (module tree, function signatures, high-level structure) and search/filter endpoints for targeted queries
- Agent-controlled response verbosity via a detail parameter (summary/standard/full) controlling how much info per node/edge
- Query responses can include derived/computed information (inferred types, dependency chains, dataflow paths) when requested, not just raw graph data

### Error & diagnostics
- Layered diagnostic format: top-level summary (error code + message) with an optional details field containing full context (failing nodes, types, surrounding graph)
- Errors describe the problem only -- no fix suggestions. Agent determines the resolution.
- Two severity levels: errors (block commit) and warnings (informational, agent can commit with warnings -- e.g., unreachable node, unused parameter)
- Verification scope is agent-controlled: 'local' checks affected subgraph and immediate dependents, 'full' re-verifies the entire program

### Undo & versioning
- Both linear undo stack (step-by-step reversal) and named checkpoints (agent-created save points)
- Undo history is persistent -- survives across API sessions, stored in the database
- Unlimited history depth -- full mutation history kept forever
- History is inspectable: agent can list past mutations, view checkpoint metadata, and diff between versions before rollback

### Claude's Discretion
- Whether to include high-level compound operations (add_function, replace_subgraph) alongside primitives, or start with primitives only
- HTTP framework and routing design
- Request/response serialization details
- Internal architecture for the undo storage

</decisions>

<specifics>
## Specific Ideas

No specific requirements -- open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 04-ai-agent-tool-api*
*Context gathered: 2026-02-18*
