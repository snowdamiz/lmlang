# Phase 8: Dual-Layer Semantic Architecture - Research

**Researched:** 2026-02-19
**Domain:** Semantic graph enrichment, embedding infrastructure, and deterministic bidirectional synchronization between semantic and compute layers
**Confidence:** HIGH

## Summary

The codebase already has the hard prerequisite for Phase 8: `ProgramGraph` maintains separate compute and semantic `StableGraph` instances with explicit cross-reference maps (`module_semantic_nodes`, `function_semantic_nodes`). Current semantic coverage is intentionally thin (`SemanticNode::{Module, Function, TypeDef}` and `SemanticEdge::{Contains, Calls, UsesType}`), and synchronization is one-way for specific builder paths (for example, adding a function creates a semantic summary node).

Phase 8 needs to evolve that baseline into a full semantic knowledge layer with deterministic two-way propagation and embedding-backed retrieval.

The safest implementation shape is:
- Extend semantic schema first (entities, metadata, richer relationships, embedding fields, summary payloads).
- Introduce a queue-backed propagation engine with explicit flush boundaries and idempotent event processing.
- Add rule-priority conflict resolution and diagnostics, then validate full bidirectional behavior with integration tests.

This sequencing matches user constraints in `08-CONTEXT.md`:
- Hybrid trigger model (local enqueue + explicit flush).
- Rule-priority conflict policy (not last-writer-wins).
- Embeddings at node and subgraph-summary levels.
- Loop-safe, deterministic processing.

## Requirements Mapping

| Requirement | Implication |
|-------------|-------------|
| DUAL-01 | Add rich semantic entities (module/function/type/spec/test/doc) with provenance/ownership metadata and summary payloads |
| DUAL-04 | Add queue-based propagation model with explicit flush and loop/idempotency guards |
| DUAL-05 | Implement semantic -> compute transforms (new function/signature/contract changes) |
| DUAL-06 | Implement compute -> semantic transforms (summary, relationship, complexity updates) |
| DUAL-07 | Persist/query embeddings for semantic nodes and semantic subgraph summaries |

## Architecture Notes

- `lmlang-core` should remain the canonical location for semantic model types, propagation event types, and deterministic ordering rules.
- `lmlang-storage` should persist any newly added semantic payloads and embedding vectors through existing save/load flows.
- `lmlang-server` should expose explicit propagation flush and semantic query endpoints; existing mutation endpoints should enqueue propagation events.
- Conflict handling should emit structured diagnostics through existing server error schema patterns.

## Risks and Mitigations

- Risk: propagation loops from mutual triggers.
  Mitigation: event IDs, lineage markers, and bounded replay in a single flush transaction.
- Risk: nondeterministic outcomes from queue ordering.
  Mitigation: stable event ordering key (priority class, timestamp, sequence).
- Risk: embedding churn on broad edits.
  Mitigation: dirty-region scoping and subgraph summary recompute only for impacted semantic regions.
- Risk: cross-layer conflict ambiguity.
  Mitigation: explicit precedence table and diagnostics for unresolved classes.

## Recommended Execution Order

1. Plan 08-01: semantic schema + embedding plumbing.
2. Plan 08-02: queue-based hybrid propagation engine.
3. Plan 08-03: conflict policy + end-to-end deterministic sync validation.

