# Phase 9: Human Observability - Research

**Researched:** 2026-02-19
**Domain:** Graph visualization UX, semantic+compute layer presentation, and natural-language observability queries
**Confidence:** HIGH

## Summary

The codebase already exposes most primitives needed for observability:
- compute graph and semantic graph state are both available through `ProgramGraph`
- semantic retrieval already returns rich metadata and optional embeddings
- HTTP routing and integration-test patterns are stable in `lmlang-server`

Phase 9 should avoid introducing a heavyweight frontend toolchain and instead ship a focused observability surface directly from the existing server:
- add observability-focused read APIs with explicit layer/function-boundary metadata
- add natural-language query orchestration that returns ranked matches, ambiguity prompts, and fallback nearest nodes
- ship a lightweight browser UI (served by `lmlang-server`) with a clean default DAG, layer toggles, and a right-side details panel
- keep graph selection and context tabs synchronized in both directions

This shape aligns with `09-CONTEXT.md` decisions:
- clean-by-default graph, details on demand
- same-canvas two-region layer layout with distinct cross-layer edge styling
- NL query UX that supports chips, ambiguity follow-up, and nearest-node fallback
- tabbed context outputs (Summary, Relationships, Contracts) synced with graph selection

## Requirements Mapping

| Requirement | Implication |
|-------------|-------------|
| VIZ-01 | Introduce graph-view payload/API and UI rendering for DAG nodes, typed edges, and function boundaries |
| VIZ-02 | Include explicit layer markers/layout hints and UI controls for layer filters and cross-layer edges |
| VIZ-03 | Add NL query endpoint using semantic embeddings + relationship traversal with ranking and ambiguity handling |
| VIZ-04 | Return contextual packets (summaries, relationships, contracts) for selected/ranked subgraphs |

## Architecture Notes

- Keep observability-specific schemas in a dedicated server schema module (for example `schema/observability.rs`) instead of overloading generic query DTOs.
- Keep ranking and ambiguity logic in `ProgramService` so handlers remain thin and deterministic.
- Use stable node/edge IDs across graph payloads and NL query results to enable bidirectional highlight/sync in UI.
- Prefer a no-build static UI (HTML/CSS/JS) served from the existing axum app to reduce phase risk and keep execution fast.

## Risks and Mitigations

- Risk: visual overload for large graphs.
  Mitigation: clean default labels, function-boundary grouping, layer toggles, and filtered render presets.
- Risk: ambiguous NL queries producing misleading results.
  Mitigation: ambiguity score threshold, explicit clarification prompts, and top-interpretation previews.
- Risk: context payloads becoming too large/noisy.
  Mitigation: bounded result count, compact summaries by default, and expandable detail tabs.
- Risk: graph/query desync in UI.
  Mitigation: shared stable IDs and explicit selection-state synchronization contract.

## Recommended Execution Order

1. Plan 09-01: observability graph/query backend contracts and ranking primitives.
2. Plan 09-02: interactive graph explorer UI, layer differentiation, and details panel.
3. Plan 09-03: NL query UX, contextual result tabs, sync behavior, and verification evidence.
