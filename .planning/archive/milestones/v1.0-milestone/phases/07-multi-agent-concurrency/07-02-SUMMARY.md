---
phase: 07-multi-agent-concurrency
plan: 02
subsystem: server-api
tags: [agents, locks, conflict-detection, router]
requires:
  - "07-01"
provides:
  - "Agent lifecycle endpoints: register/list/deregister"
  - "Function lock endpoints: acquire/release/status"
  - "Agent-aware mutation path with lock verification"
  - "Hash conflict detection with structured conflict details"
  - "Global write lock for AddFunction/AddModule"
requirements-completed: [MAGENT-01, MAGENT-02, MAGENT-03]
completed: 2026-02-19
---

# Phase 7 Plan 2 Summary

Implemented agent and lock HTTP surface plus agent-aware mutation controls.

## What Was Built
- Added `POST /agents/register`, `GET /agents`, `DELETE /agents/{agent_id}` handlers and router wiring.
- Added `POST /programs/{id}/locks/acquire`, `POST /programs/{id}/locks/release`, `GET /programs/{id}/locks` handlers and router wiring.
- Extended mutation request schema with optional `expected_hashes`.
- Refactored mutation handler to support two modes:
  - non-agent mode: backward-compatible behavior
  - agent mode: lock verification + structure-write serialization + hash conflict check
- Added structured conflict error payload path via `ApiError::ConflictWithDetails`.
- Added `concurrency::affected_functions` helper used by mutation handler.

## Key Files
- `crates/lmlang-server/src/handlers/agents.rs`
- `crates/lmlang-server/src/handlers/locks.rs`
- `crates/lmlang-server/src/handlers/mutations.rs`
- `crates/lmlang-server/src/router.rs`
- `crates/lmlang-server/src/schema/mutations.rs`
- `crates/lmlang-server/src/concurrency/mod.rs`
- `crates/lmlang-server/src/error.rs`

## Verification
- `cargo check --package lmlang-server` passed.
- Full package tests later validated as part of Plan 03 execution.
