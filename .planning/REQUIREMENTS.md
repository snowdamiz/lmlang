# Requirements: lmlang

**Defined:** 2026-02-19
**Milestone:** v1.1 milestone-2
**Core Value:** AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness

## v1.1 Requirements (Draft)

Scope is intentionally draft until explicitly approved.

### Concurrency Hardening

- [ ] **MAGENT-03**: Optimistic concurrency detects overlapping edits and rolls back the later commit with conflict diagnostics
- [ ] **MAGENT-04**: Verification runs on merge and confirms global invariants hold after concurrent modifications

### Release Readiness

- [ ] **REL-01**: Milestone produces a release checklist and reproducible validation run for server, CLI, and core crates

## Future Requirements

- All additional capabilities not explicitly selected into v1.1

## Out of Scope (v1.1)

- New major architectural subsystems beyond concurrency hardening and release readiness

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| MAGENT-03 | TBD | Planned |
| MAGENT-04 | TBD | Planned |
| REL-01 | TBD | Planned |

**Coverage:**
- v1.1 requirements: 3 total
- Mapped to phases: 0
- Unmapped: 3

---
*Requirements defined: 2026-02-19*
