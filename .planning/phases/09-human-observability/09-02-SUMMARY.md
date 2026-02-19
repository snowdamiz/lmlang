---
phase: 09-human-observability
plan: 02
subsystem: observability-ui-and-routing
tags: [observability-ui, dag, static-assets, routing, layer-filters]
requires:
  - "09-01"
provides:
  - "Dedicated observability handlers and router endpoints"
  - "Browser UI shell served directly by lmlang-server"
  - "Interactive SVG DAG rendering with semantic/compute separation"
  - "Layer toggles, presets, cross-layer filtering, and details panel"
  - "Route-level tests for static UI delivery"
requirements-completed: [VIZ-01, VIZ-02]
completed: 2026-02-19
---

# Phase 9 Plan 2 Summary

Delivered the human-facing observability surface by wiring static UI hosting and interactive graph exploration.

## What Was Built
- Added `crates/lmlang-server/src/handlers/observability.rs` with endpoints for:
  - graph payload retrieval
  - natural-language query
  - static UI/JS/CSS serving
- Updated route registration in `crates/lmlang-server/src/router.rs`:
  - `GET /programs/{id}/observability`
  - `GET /programs/{id}/observability/graph`
  - `POST /programs/{id}/observability/query`
  - static asset routes for `app.js` and `styles.css`
- Created no-build static UI assets:
  - `crates/lmlang-server/static/observability/index.html`
  - `crates/lmlang-server/static/observability/app.js`
  - `crates/lmlang-server/static/observability/styles.css`
- UI behavior includes:
  - clean-by-default graph with progressive detail
  - same-canvas semantic/compute layout regions
  - visible cross-layer styling and layer presets
  - right-side details panel and selection highlighting

## Verification
- `cargo test --package lmlang-server --test integration_test` passed (including static route checks)
