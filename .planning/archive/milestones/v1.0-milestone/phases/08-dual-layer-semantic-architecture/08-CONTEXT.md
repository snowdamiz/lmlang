# Phase 8: Dual-Layer Semantic Architecture - Context

**Gathered:** 2026-02-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Programs gain a first-class semantic graph layer that remains synchronized with the executable compute graph. The phase delivers bidirectional propagation (semantic -> compute and compute -> semantic), deterministic conflict handling, and semantic embeddings for retrieval/navigation.

</domain>

<decisions>
## Implementation Decisions

### Semantic node model
- Use an extended semantic schema, not a minimal one.
- Required entities include module/function/type/spec/test/doc plus ownership and provenance metadata.
- Semantic nodes should be rich enough to support both generation (downward propagation) and explanation/navigation (upward propagation + query).

### Propagation trigger model
- Use a hybrid trigger model.
- Local semantic/compute edits may enqueue immediate local propagation.
- Cross-cutting reconciliation is explicitly flushed through a queue-based propagation cycle.
- Queue processing must be loop-safe and deterministic.

### Conflict and loop policy
- Use rule-priority conflict resolution instead of last-writer-wins.
- Predefined rule classes determine which layer's update applies for each event kind.
- Conflicts that cannot be resolved by rule should be surfaced as structured diagnostics (not silently dropped).

### Embeddings strategy
- Store embeddings at both semantic node level and semantic subgraph-summary level.
- Subgraph embeddings are required for larger intent-level retrieval, while node embeddings support precise lookup.
- Embedding regeneration should be scoped to changed semantic regions and their derived summaries.

### Claude's Discretion
- Exact queue/event schema and batching thresholds.
- Exact precedence table details (as long as behavior remains deterministic and testable).
- Embedding model/provider plumbing and storage format.

</decisions>

<specifics>
## Specific Ideas

- Downward propagation examples to support: semantic function creation, signature change, and contract addition.
- Upward propagation examples to support: compute-graph edits updating summaries, relationship edges, and complexity metadata.
- System should preserve explicit "flush" semantics for predictable multi-step agent workflows.

</specifics>

<deferred>
## Deferred Ideas

None - discussion stayed within phase scope.

</deferred>

---

*Phase: 08-dual-layer-semantic-architecture*
*Context gathered: 2026-02-19*
