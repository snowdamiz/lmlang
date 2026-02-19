---
phase: 09-human-observability
plan: 01
subsystem: observability-backend-contract
tags: [observability, dual-layer, graph-projection, semantic-query, ranking]
requires: []
provides:
  - "Dedicated observability DTOs for graph projection and NL query contracts"
  - "Deterministic dual-layer graph projection with function-boundary grouping"
  - "Cross-layer edge markers for semantic->compute navigation"
  - "Natural-language ranking with ambiguity signaling and nearest fallback"
  - "Backend integration coverage for observability graph/query behavior"
requirements-completed: [VIZ-01, VIZ-02, VIZ-03]
completed: 2026-02-19
---

# Phase 9 Plan 1 Summary

Implemented the backend observability data plane for both graph rendering and natural-language query interpretation.

## What Was Built
- Added `crates/lmlang-server/src/schema/observability.rs` for observability-specific request/response contracts:
  - dual-layer graph payload DTOs
  - function-boundary grouping metadata
  - NL query response shape with ambiguity and contextual tabs
- Extended `ProgramGraph` in `crates/lmlang-core/src/graph.rs` with deterministic traversal helpers:
  - `function_nodes_sorted`
  - `sorted_function_ids`
  - `semantic_node_id_for_function`
- Added `ProgramService` observability methods in `crates/lmlang-server/src/service.rs`:
  - `observability_graph` for deterministic semantic+compute projection
  - `observability_query` for ranking, disambiguation candidates, and low-confidence nearest fallback
  - contextual tab synthesis (summary, relationships, contracts)
- Added integration coverage in `crates/lmlang-server/tests/integration_test.rs` for:
  - dual-layer payload shape + cross-layer links
  - deterministic repeated projection output
  - query ambiguity and fallback branches

## Verification
- `cargo test --package lmlang-core` passed
- `cargo test --package lmlang-server --test integration_test` passed
