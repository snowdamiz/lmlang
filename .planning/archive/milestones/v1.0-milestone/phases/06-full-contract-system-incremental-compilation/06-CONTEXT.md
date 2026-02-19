# Phase 6: Full Contract System & Incremental Compilation - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Programs have rich behavioral contracts (pre/post-conditions, invariants, property-based tests) that are checked at development time, and only changed functions recompile. Contracts are development-time constructs (interpreter/simulation only) — compiled binaries have zero contract overhead. Incremental compilation tracks dirty functions and their dependents to avoid full recompilation.

</domain>

<decisions>
## Implementation Decisions

### Contract model
- Contracts are special graph nodes (Precondition, Postcondition, Invariant) — first-class op types like any other node
- Contract nodes can contain full graph expressions — any logic expressible in the main graph can appear in a contract subgraph
- Agents build contract subgraphs using existing node/edge mutation API — no separate contract-specific endpoints needed

### Violation feedback
- Contract violations produce structured diagnostics with counterexample values embedded — agent sees node IDs, the contract that failed, actual vs expected values, and the specific inputs that triggered the failure
- Consistent with Phase 4 decision: errors describe the problem only, agent determines the fix (no fix suggestions)
- Invariant violations on data structures block compilation — they are errors, not warnings
- Contracts are development-time only — checked during interpretation/simulation, stripped from compiled binaries

### Property testing strategy
- Agent-seeded: agent provides seed inputs and interesting edge cases, system generates randomized variations to test contracts
- Agent-controlled iteration count — no default, agent always specifies how many iterations per test run
- Test results include detailed execution trace for each failure showing the path through the graph
- Property tests run through the graph interpreter only — no compiled execution for testing

### Incremental recompilation
- Function-level dirty tracking — if any node in a function changes, the whole function recompiles (matches existing function-scoped LLVM codegen)
- Dirty status visible to agent — agent can query which functions are dirty, see what will recompile, and get a recompilation plan before triggering
- Contract changes do NOT mark functions dirty for recompilation — since contracts are dev-only, they don't affect compiled output

### Claude's Discretion
- Whether contract API uses dedicated endpoints for common patterns or purely existing mutations (leaning toward existing mutations given the "full graph expressions" decision)
- Whether contracts are fully separable from function logic or integrated — pick based on graph architecture
- Dependent identification strategy for incremental recompilation — call graph analysis vs content hash comparison vs hybrid

</decisions>

<specifics>
## Specific Ideas

- Contract subgraphs should be as expressive as regular function bodies — the same ops and edges
- Property testing is interpreter-based, consistent with contracts being a development-time concern
- Agent has full visibility into the incremental compilation state — can make informed decisions about when to recompile

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 06-full-contract-system-incremental-compilation*
*Context gathered: 2026-02-18*
