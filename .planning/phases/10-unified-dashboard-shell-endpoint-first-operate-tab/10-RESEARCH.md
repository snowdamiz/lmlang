# Phase 10: Unified Dashboard Shell and Endpoint-First Operate Tab - Research

**Researched:** 2026-02-19
**Domain:** Unified operator dashboard UX, endpoint-first orchestration controls, and observability reuse
**Confidence:** HIGH

## Summary

The codebase already contains almost everything needed for Phase 10:
- `lmlang-server` already serves a complete observability UI at `/programs/{id}/observability`.
- The API surface already includes the core orchestration primitives required for `Operate`:
  `/agents/register`, `/agents`, `/agents/{agent_id}`,
  `/programs/{id}/locks`, `/programs/{id}/locks/acquire`, `/programs/{id}/locks/release`,
  `/programs/{id}/mutations`, `/programs/{id}/verify`, `/programs/{id}/simulate`,
  `/programs/{id}/compile`, `/programs/{id}/history`.
- Integration-test helpers and route test patterns are stable and can be extended directly.

Phase 10 should focus on UI composition, not backend expansion:
- add a unified dashboard route at `/programs/{id}/dashboard`
- serve a dedicated dashboard shell (static HTML/CSS/JS)
- keep `Observe` endpoint-first by embedding or mounting existing observability UX without duplicating backend logic
- build `Operate` against existing endpoints only, with explicit `X-Agent-Id` handling for lock/mutation workflows

## Requirements Mapping

| Requirement | Implication |
|-------------|-------------|
| UI-01 | `Operate` must show registered agents and an actionable status-oriented session panel (`idle`, `running`, `blocked`, `error`) from existing APIs and derived UI state |
| UI-02 | `Operate` must provide run setup controls (program + workflow template + task prompt) and dispatch workflow actions through existing endpoints without adding new Phase 10 APIs |

## Existing Contracts to Reuse

### Observability Reuse Surface
- `GET /programs/{id}/observability` serves full existing Observe UI shell.
- `GET /programs/{id}/observability/graph` and `POST /programs/{id}/observability/query` power graph + query behavior.
- Existing static assets are already wired via handlers and router routes.

### Operate Endpoint Matrix
- Agent lifecycle:
  - `POST /agents/register`
  - `GET /agents`
  - `DELETE /agents/{agent_id}`
- Locking:
  - `POST /programs/{id}/locks/acquire` (requires `X-Agent-Id`)
  - `POST /programs/{id}/locks/release` (requires `X-Agent-Id`)
  - `GET /programs/{id}/locks`
- Graph mutations:
  - `POST /programs/{id}/mutations` (`dry_run` supported; optional `X-Agent-Id`)
- Verification and execution checks:
  - `POST /programs/{id}/verify`
  - `POST /programs/{id}/simulate`
  - `POST /programs/{id}/compile`
  - `GET /programs/{id}/history`

## Architectural Guidance

- Introduce a dedicated dashboard handler module and static asset set under `static/dashboard/`.
- Keep Observe reuse low-risk in Phase 10 by mounting existing observability view in the dashboard `Observe` tab (for example, iframe or shared container strategy) while preserving existing observability routes.
- Implement a thin client-side API adapter in dashboard JS that:
  - centralizes JSON request handling
  - applies `X-Agent-Id` only where required
  - normalizes errors into user-facing status panels
- Represent run/session status in UI as derived state from existing endpoint responses plus operation outcomes:
  - `idle`: registered agent with no active lock/action
  - `running`: active operation in progress
  - `blocked`: lock conflict / conflict response / lock-required response
  - `error`: failed API action

## Risks and Mitigations

- Risk: Program mismatch errors because handlers require active program alignment.
  Mitigation: dashboard boot flow should validate loaded program and provide explicit remediation UX.
- Risk: Operate interactions fail when `X-Agent-Id` is missing.
  Mitigation: require selected/registered agent before lock or mutation actions.
- Risk: Re-implementing Observe duplicates existing UI behavior.
  Mitigation: reuse existing observability surface directly in Phase 10.
- Risk: API overload in one screen reduces usability.
  Mitigation: split Operate into clear sections: Agents, Run Setup, Locks, Mutations, Verify/Simulate/Compile, History.

## Recommended Execution Order

1. Plan 10-01: Dashboard routing and shell scaffolding with tab structure and Observe reuse mount.
2. Plan 10-02: Endpoint-first Operate implementation using existing APIs only.
3. Plan 10-03: Observe integration hardening, UX polish, and requirement verification evidence.
