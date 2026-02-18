# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-17)

**Core value:** AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness
**Current focus:** Phase 4: AI Agent Tool API

## Current Position

Phase: 4 of 9 (AI Agent Tool API)
Plan: 3 of 4 in current phase
Status: Executing Phase 4
Last activity: 2026-02-18 — Completed 04-03-PLAN.md

Progress: [█████░░░░░] 46%

## Performance Metrics

**Velocity:**
- Total plans completed: 13
- Average duration: 7min
- Total execution time: 1.51 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 4/4 | 15min | 4min |
| 02 | 3/3 | 18min | 6min |
| 03 | 3/3 | 35min | 12min |
| 04 | 3/4 | 23min | 8min |

**Recent Trend:**
- Last 5 plans: 03-02 (15min), 03-03 (11min), 04-01 (5min), 04-02 (15min), 04-03 (3min)
- Trend: handler+router plan fast due to thin-wrapper pattern -- all logic in ProgramService

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [Roadmap]: 9 phases derived from 41 requirements following crate dependency graph and feature dependency tree
- [Roadmap]: Phases 2 and 3 both depend only on Phase 1 (could parallelize); Phase 4 depends on both 2 and 3
- [Roadmap]: Bidirectional dual-layer propagation deferred to Phase 8 (hardest correctness problem, per research)
- [Roadmap]: Incremental compilation grouped with full contract system (Phase 6) since both harden the working system
- [Phase 01]: No unsigned integer types -- follows LLVM approach (signedness at operation level)
- [Phase 01]: TypeId constants (BOOL through NEVER) as associated consts for ergonomic builtin access
- [Phase 01]: F32 ConstValue stored as f64 internally for float comparison safety in enum derives
- [Phase 01]: Types inferred from edges, not stored on ops -- follows LLVM model, eliminates redundancy
- [Phase 01]: Both high-level (IfElse, Loop, Match) and low-level (Branch, Jump, Phi) control flow ops included
- [Phase 01]: ModuleDef stub in node.rs with TODO(plan-03) for later migration to module.rs
- [Phase 01]: FunctionSummary (not full FunctionDef) in SemanticNode -- full def goes in separate lookup table
- [Phase 01]: Closures are FunctionDefs with is_closure=true and non-empty captures -- no separate closure type
- [Phase 01]: ModuleDef gains id field (ModuleId) in canonical definition vs the Plan 02 stub
- [Phase 01]: ModuleTree serde roundtrip uses structural comparison due to HashMap non-deterministic key ordering
- [Phase 01]: Compute and semantic graphs are private -- all mutations go through ProgramGraph methods for consistency
- [Phase 01]: Module and function semantic node indices tracked in HashMaps for O(1) Contains edge creation
- [Phase 01]: Debug-only assert_consistency verifies FunctionId-to-SemanticNode mapping integrity
- [Phase 02]: Sync GraphStore trait (not async) matching current single-threaded design
- [Phase 02]: from_parts constructors on ProgramGraph, TypeRegistry, ModuleTree for storage reconstruction
- [Phase 02]: StableGraph gap-filling with dummy nodes for index preservation during recompose
- [Phase 02]: find_nodes_by_type via edge value_type filtering (types inferred from edges, not stored on nodes)
- [Phase 02]: rusqlite 0.32 (not 0.38 from research) to match rusqlite_migration 1.3 compatibility
- [Phase 02]: Explicit child-table DELETE ordering before program deletion (not relying on CASCADE alone)
- [Phase 02]: ModuleTree rebuilt from stored modules + functions during load (not serialized as blob)
- [Phase 02]: Semantic index maps derived from semantic node content on load (no extra table)
- [Phase 02]: serde_json::to_vec for canonical op serialization in hashing (safe because ComputeNodeOp uses no HashMap)
- [Phase 02]: Two-pass hash_function: content hashes first, then composite hashes with edges (avoids topological ordering)
- [Phase 02]: Cross-function edge targets use content-only hash (not composite) for function boundary isolation
- [Phase 03]: Safe implicit widening: i8->i16->i32->i64, f32->f64, bool->integer, &mut T -> &T; no cross-family (int<->float)
- [Phase 03]: Nominal struct typing: TypeId equality only, structural similarity irrelevant
- [Phase 03]: Validation functions as standalone API (not wrapping ProgramGraph) for architectural independence
- [Phase 03]: Bool-to-integer coercion resolves to I8 for arithmetic
- [Phase 03]: InsertCast fix suggestion generated when both types are numeric but coercion fails
- [Phase 03]: Work-list interpreter with control-gated scheduling: nodes behind control edges wait for control predecessor
- [Phase 03]: Phi selects data port based on Branch decision (true->port 0, false->port 1)
- [Phase 03]: Only Parameter, Const, CaptureAccess, Alloc, ReadLine are seedable in work-list
- [Phase 03]: Bool coerced to I8 for arithmetic at runtime (matches type checker coercion)
- [Phase 03]: Memory-based loop variables (Alloc/Store/Load) for loop-carried values -- avoids Phi<->Loop circular dependency in work-list
- [Phase 03]: BFS loop body discovery + external readiness pre-credit for Loop back-edge re-evaluation
- [Phase 03]: Control back-edge pattern: store -> control -> load triggers condition re-evaluation for Loop iteration

- [Phase 04]: Mutation enum uses serde tag="type" for JSON discriminated union serialization
- [Phase 04]: SimulateRequest inputs use serde_json::Value -- agents send plain JSON, service converts to interpreter Value
- [Phase 04]: DiagnosticError omits FixSuggestion per CONTEXT.md: errors describe problem only, agent determines fix
- [Phase 04]: ApiError maps NotFound->404, BadRequest->400, ValidationFailed->422, InternalError->500, Conflict->409
- [Phase 04]: ProgramService owns graph, store, connection, and program_id as single coordinator
- [Phase 04]: Batch mutations use clone-and-swap: clone graph, apply all, validate, swap on success
- [Phase 04]: Single mutations apply to real graph with inverse-revert on validation failure
- [Phase 04]: EditCommand inverse() computes inverse for each variant (LIFO for batches)
- [Phase 04]: Checkpoint stores full ProgramGraph JSON with edit_log position reference
- [Phase 04]: New mutations clear redo stack (invalidate undone entries)
- [Phase 04]: Thin handler pattern: extract -> lock -> call -> respond, no business logic in handlers
- [Phase 04]: Active program guard in every handler: 400 if path program_id != active program
- [Phase 04]: Combined GET+POST routes for /programs and /programs/{id}/checkpoints via axum method chaining
- [Phase 04]: Path param types: i64 for program_id (matching ProgramId(i64)), u32 for node_id/func_id

### Pending Todos

None yet.

### Blockers/Concerns

- [Research]: Op node set needs careful mapping to LLVM IR targets before Phase 1 implementation
- [Research]: Agent tool API schema design is novel -- test with real LLMs early in Phase 4
- [Research]: Bidirectional propagation (Phase 8) has no production precedent -- needs formal specification before implementation

## Session Continuity

Last session: 2026-02-18
Stopped at: Completed 04-03-PLAN.md
Resume file: .planning/phases/04-ai-agent-tool-api/04-03-SUMMARY.md
