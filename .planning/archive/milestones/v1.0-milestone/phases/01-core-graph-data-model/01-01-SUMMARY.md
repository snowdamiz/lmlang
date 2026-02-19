---
phase: 01-core-graph-data-model
plan: 01
subsystem: core
tags: [rust, cargo-workspace, type-system, nominal-typing, petgraph, serde, indexmap, thiserror]

# Dependency graph
requires: []
provides:
  - "Cargo workspace with resolver 2 and crates/* layout"
  - "lmlang-core crate with petgraph, serde, thiserror, indexmap, smallvec dependencies"
  - "LmType enum: Scalar, Array, Struct, Enum, Pointer, Function, Unit, Never"
  - "ScalarType enum: Bool, I8-I64, F32, F64"
  - "TypeId with O(1) nominal identity comparison"
  - "TypeRegistry with 9 pre-registered builtins and named type lookup"
  - "NodeId, EdgeId, FunctionId, ModuleId distinct newtype wrappers"
  - "CoreError enum for all anticipated failure modes"
  - "ConstValue enum for constant literal values"
affects: [01-02, 01-03, 01-04, 02-ops-and-edges, 03-functions-modules]

# Tech tracking
tech-stack:
  added: [petgraph 0.8, serde 1.0, serde_json 1.0, thiserror 2.0, smallvec 1, indexmap 2, proptest 1.10, insta 1]
  patterns: [newtype-id-pattern, nominal-typing-via-registry, insertion-ordered-maps]

key-files:
  created:
    - Cargo.toml
    - crates/lmlang-core/Cargo.toml
    - crates/lmlang-core/src/lib.rs
    - crates/lmlang-core/src/types.rs
    - crates/lmlang-core/src/type_id.rs
    - crates/lmlang-core/src/id.rs
    - crates/lmlang-core/src/error.rs
  modified: []

key-decisions:
  - "No unsigned integer types -- follows LLVM approach where signedness is at the operation level"
  - "F32 ConstValue stores as f64 internally to avoid float comparison issues in enum derives"
  - "TypeId constants (BOOL, I8, ..., UNIT, NEVER) as associated consts for ergonomic access"
  - "Default impl on TypeRegistry delegates to new() for idiomatic Rust usage"

patterns-established:
  - "Newtype ID pattern: all graph entity IDs are distinct u32 newtypes with serde support"
  - "TypeRegistry pre-registration: built-in types always occupy TypeId(0)..TypeId(8)"
  - "IndexMap for ordered collections: struct fields and enum variants preserve declaration order"
  - "Re-export pattern: lib.rs re-exports all key types for ergonomic use"

requirements-completed: [GRAPH-05]

# Metrics
duration: 4min
completed: 2026-02-18
---

# Phase 01 Plan 01: Workspace and Type System Summary

**Cargo workspace with lmlang-core crate implementing complete nominal type system: 8-variant LmType enum, TypeRegistry with 9 pre-registered builtins, 5 distinct ID newtypes, and CoreError -- all with serde support and 26 passing tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-18T05:39:54Z
- **Completed:** 2026-02-18T05:43:32Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Cargo workspace established with resolver 2 and all dependencies resolving cleanly
- Complete type system: LmType covers scalars (Bool, I8-I64, F32, F64), arrays, structs, enums/tagged unions, pointers, function signatures, Unit, and Never
- TypeRegistry provides nominal typing with O(1) lookup, 9 pre-registered built-in types, and named type registration with duplicate detection
- All 5 ID newtypes (NodeId, EdgeId, FunctionId, ModuleId, TypeId) are distinct, with NodeId bridging to petgraph's NodeIndex
- 26 unit tests covering serde roundtrip, registry operations, ordering preservation, and type construction

## Task Commits

Each task was committed atomically:

1. **Task 1: Create Cargo workspace and lmlang-core crate** - `d02fa17` (feat)
2. **Task 2: Implement type system, type registry, IDs, and error types** - `f8a5dfb` (feat)

## Files Created/Modified
- `Cargo.toml` - Workspace root with resolver 2
- `Cargo.lock` - Locked dependency versions
- `crates/lmlang-core/Cargo.toml` - Crate dependencies: petgraph, serde, thiserror, indexmap, smallvec
- `crates/lmlang-core/src/lib.rs` - Module declarations and re-exports of all key types
- `crates/lmlang-core/src/types.rs` - LmType, ScalarType, StructDef, EnumDef, EnumVariant, Visibility, ConstValue
- `crates/lmlang-core/src/type_id.rs` - TypeId, TypeRegistry with pre-registered builtins and named lookup
- `crates/lmlang-core/src/id.rs` - NodeId, EdgeId, FunctionId, ModuleId newtypes with petgraph bridge
- `crates/lmlang-core/src/error.rs` - CoreError enum with 7 error variants

## Decisions Made
- No unsigned integer types -- follows LLVM approach where signedness is determined at the operation level (sdiv vs udiv), not the type level. Avoids type proliferation.
- F32 ConstValue stores as f64 internally -- prevents issues with float comparison in Rust enum derives while preserving value precision until LLVM lowering.
- Added TypeId associated constants (BOOL, I8, ..., UNIT, NEVER) for ergonomic access to well-known type IDs without registry lookup.
- Implemented Default for TypeRegistry via new() for idiomatic Rust usage patterns.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Type system is stable and ready for Wave 2 plans to build on
- Plan 01-02 (op nodes) can define ComputeOp/StructuredOp using TypeId, FunctionId
- Plan 01-03 (graph container) can use all ID types and TypeRegistry
- Plan 01-04 (functions/modules) can use ModuleId, FunctionId, Visibility, StructDef, EnumDef

## Self-Check: PASSED

All 8 created files verified on disk. Both task commits (d02fa17, f8a5dfb) verified in git log.

---
*Phase: 01-core-graph-data-model*
*Completed: 2026-02-18*
