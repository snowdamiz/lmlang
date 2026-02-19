---
phase: 01-core-graph-data-model
plan: 02
subsystem: core
tags: [rust, ops, edges, nodes, ssa, control-flow, serde, llvm-lowering]

# Dependency graph
requires:
  - phase: 01-core-graph-data-model
    provides: "TypeId, FunctionId, ModuleId, ConstValue, Visibility from Plan 01"
provides:
  - "ComputeOp enum with ~24 Tier 1 ops (arithmetic, comparison, logic, shifts, control flow, memory, functions, I/O, closures)"
  - "StructuredOp enum with 10 Tier 2 ops (struct/array/enum operations, type cast)"
  - "Sub-enums: ArithOp, UnaryArithOp, CmpOp, LogicOp, ShiftOp"
  - "ComputeNodeOp wrapper unifying Tier 1 and Tier 2"
  - "FlowEdge: Data (typed SSA) + Control (branch index) for computational graph"
  - "SemanticEdge: Contains, Calls, UsesType for semantic graph"
  - "ComputeNode with FunctionId owner for flat-graph function boundaries"
  - "SemanticNode: Module, Function, TypeDef variants for semantic skeleton"
  - "FunctionSummary, FunctionSignature, TypeDefNode, ModuleDef (stub)"
  - "Helper methods: is_control_flow, is_terminator, is_io, tier"
affects: [01-03, 01-04, 02-functions-modules, 03-graph-container]

# Tech tracking
tech-stack:
  added: []
  patterns: [grouped-op-enums-with-type-inference, ssa-data-flow-edges, flat-graph-ownership, dual-layer-edge-types]

key-files:
  created:
    - crates/lmlang-core/src/ops.rs
    - crates/lmlang-core/src/edge.rs
    - crates/lmlang-core/src/node.rs
  modified:
    - crates/lmlang-core/src/lib.rs

key-decisions:
  - "Types inferred from edges, not stored on ops -- follows LLVM model, eliminates redundancy"
  - "Both high-level (IfElse, Loop, Match) and low-level (Branch, Jump, Phi) control flow ops included"
  - "ModuleDef defined as stub in node.rs with TODO(plan-03) marker for later migration"
  - "FunctionSummary (not full FunctionDef) in SemanticNode -- full def in separate lookup table"

patterns-established:
  - "Grouped op enums with sub-enum parameters: BinaryArith { op: ArithOp } not individual Add/Sub/Mul variants"
  - "Type inference from edges: no TypeId on arithmetic/logic/comparison ops"
  - "LLVM lowering path documented on every op variant via doc comments"
  - "Flat graph ownership: ComputeNode.owner for function boundaries"
  - "Dual edge types: FlowEdge (Data + Control) separate from SemanticEdge"
  - "Helper method classification: is_control_flow, is_terminator, is_io for op categorization"

requirements-completed: [GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-06]

# Metrics
duration: 4min
completed: 2026-02-18
---

# Phase 01 Plan 02: Ops, Edges, and Nodes Summary

**Complete op vocabulary (~24 Tier 1 + 10 Tier 2 ops) with LLVM lowering docs, SSA-style typed data/control flow edges, and compute/semantic node wrappers with flat-graph ownership -- 39 new tests all passing**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-18T05:47:32Z
- **Completed:** 2026-02-18T05:51:29Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Complete Tier 1 op vocabulary: constants, arithmetic (5 ops), unary (2), comparison (6), logic (3), shifts (3), control flow (6 ops -- both high-level IfElse/Loop/Match and low-level Branch/Jump/Phi), memory (4), functions (4), console I/O (2), file I/O (4), closures (2)
- Complete Tier 2 op vocabulary: struct create/get/set, array create/get/set, type cast, enum create/discriminant/payload (10 ops total)
- FlowEdge with typed SSA data flow (source_port, target_port, value_type) and control flow (optional branch_index) -- data DAG + control cycles (loops) as separate edge types per LLVM IR conventions
- ComputeNode with FunctionId ownership for flat-graph function boundaries, plus SemanticNode for semantic skeleton (Module, Function, TypeDef)
- Every op has documented LLVM IR lowering path in doc comments
- 39 new tests (65 total): classification helpers, serde roundtrips, constructor verification

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement op node enums (Tier 1 + Tier 2)** - `7945922` (feat)
2. **Task 2: Implement edge types and node wrappers** - `493f416` (feat)

## Files Created/Modified
- `crates/lmlang-core/src/ops.rs` - ComputeOp (24 variants), StructuredOp (10 variants), sub-enums, ComputeNodeOp, helper methods
- `crates/lmlang-core/src/edge.rs` - FlowEdge (Data + Control), SemanticEdge (Contains, Calls, UsesType)
- `crates/lmlang-core/src/node.rs` - ComputeNode, SemanticNode, FunctionSummary, FunctionSignature, TypeDefNode, ModuleDef stub
- `crates/lmlang-core/src/lib.rs` - Added module declarations and re-exports for ops, edge, node

## Decisions Made
- Types are inferred from input edges, not stored on ops -- follows LLVM IR model where operations are typed by their operands. Exception: Cast, StructCreate, EnumCreate carry TypeId because target type cannot be inferred.
- Both high-level structured ops (IfElse, Loop, Match) and low-level ops (Branch, Jump, Phi) included per research discretion recommendation: agents construct with high-level ops, lowering produces low-level ops.
- ModuleDef placed as stub in node.rs (not module.rs) since Plan 03 creates the full module system. Marked with TODO(plan-03) for migration.
- SemanticNode uses FunctionSummary (name, id, module, visibility, signature) not full FunctionDef -- full def with entry nodes and captures goes in a separate lookup table.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- All op enums, edge types, and node wrappers are stable and ready for Plan 03 (functions/modules) and Plan 04 (graph container)
- Plan 03 will define full FunctionDef and ModuleDef, migrating the stub from node.rs
- Plan 04 will use ComputeNode/FlowEdge as StableGraph type parameters and SemanticNode/SemanticEdge for the semantic layer
- 65 total tests provide regression safety for downstream changes

## Self-Check: PASSED

All 4 created/modified files verified on disk. Both task commits (7945922, 493f416) verified in git log.

---
*Phase: 01-core-graph-data-model*
*Completed: 2026-02-18*
