# Phase 1: Core Graph Data Model - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Define the foundational data model for representing programs as typed computational graphs. Includes op nodes (Tier 1 + Tier 2), data flow and control flow edges, function/module boundaries, the type system, and the dual StableGraph structure with cross-references. This phase delivers `lmlang-core` — the types and structures everything else builds on.

Requirements: GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, DUAL-02, DUAL-03

</domain>

<decisions>
## Implementation Decisions

### Op node vocabulary & tiers
- Richer / CISC-like op set (~20-25 Tier 1 ops) — prioritize agent usability over minimalism. Fewer nodes per program means smaller graph representations that fit better in AI context windows.
- Include broader I/O operations: console I/O (print, readline) plus file operations (open, read, write, close). Programs should be able to interact with the filesystem from Phase 1.
- Op granularity: grouped with parameters (e.g., BinaryArith with operator and type fields), not one enum variant per operation. Claude's discretion on whether types are explicit parameters or inferred from input edges — choose between grouped-with-params and grouped-type-inferred based on what works best for the graph model.

### Program structure model
- Functions support nesting and closures — inner functions can capture variables from enclosing scope. Environment/capture representation needed in the graph from day one.
- Hierarchical modules (like Rust's mod system) — modules can contain sub-modules. Tree-structured organization.
- Public/private visibility on functions and types across module boundaries. Two-level visibility: public (visible outside module) or private (module-internal).
- Cross-module function calls use direct graph edges — call nodes directly reference the target function's subgraph. The graph is one connected structure, not indirected through import/export declarations.

### Type system scope
- Include enums/tagged unions from day one — enables Option/Result-like patterns for error handling and variant data.
- Concrete types only — no generics/parametric polymorphism in Phase 1. All types are fully specified. Generics deferred to a later phase.
- Nominal typing — types have names and identity. Two structs with the same fields but different names are different types.

### Dual-graph Phase 1 scope
- Basic semantic skeleton — the semantic graph tracks module and function nodes with names and signatures, a lightweight structural mirror. No embeddings, summaries, or relationships yet.
- Shared stable IDs — both graphs use the same ID space. A function node has the same ID in both the semantic and computational graphs.
- Dual-layer visible to agents — when agents query the graph (Phase 4), they can query either layer explicitly. Semantic context ("what does this function do?") and computational detail ("what ops does it contain?") are separately addressable.

### Claude's Discretion
- Control flow construct design — whether to include high-level structured ops (Loop, If/Else, Match) alongside low-level Branch/Jump, or stick to low-level only. Decide based on LLVM lowering constraints and agent usability.
- Unit/Never type handling — whether to include both Unit and Never types, or just Unit with diverging functions having no return edge.
- Semantic skeleton auto-sync behavior — whether the semantic skeleton auto-updates when computational graph changes (simple one-directional sync for structural changes), or requires manual management until Phase 8's propagation engine.
- Op grouping detail — whether op types carry explicit type parameters or infer types from input edges.

</decisions>

<specifics>
## Specific Ideas

- The richer CISC-like op set is motivated by agent context window efficiency — fewer nodes per program = more programs fit in an agent's context.
- Closures from day one because they're fundamental to expressive programming; retrofitting capture semantics later would be disruptive.
- Nominal typing chosen to prevent accidental type confusion — aligns with systems language expectations (Rust, C).
- Shared stable IDs between dual graphs for simplicity — same ID, same entity, no mapping table to maintain.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 01-core-graph-data-model*
*Context gathered: 2026-02-18*
