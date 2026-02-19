# Project Research Summary

**Project:** lmlang
**Domain:** AI-native programming system / graph-based compiler with LLVM backend
**Researched:** 2026-02-17
**Confidence:** MEDIUM-HIGH

## Executive Summary

lmlang is an AI-native programming system where programs are persistent dual-layer graphs manipulated by AI agents through a structured tool API, compiled to native binaries via LLVM. This is a novel domain with no direct precedent, but it draws heavily from three mature fields: compiler infrastructure (LLVM IR, MLIR, Sea of Nodes), knowledge graphs over code (Code Property Graphs, ProGraML), and AI agent tool-calling protocols (MCP, structured outputs). The recommended approach is to build it in Rust using petgraph for in-memory graph representation, rusqlite for persistence, inkwell for LLVM codegen, and axum for the agent-facing HTTP API. The stack choices are high-confidence -- petgraph, rusqlite, inkwell, and axum are all dominant in their respective niches with millions of downloads.

The critical architectural decision is the dual-layer graph: a semantic knowledge graph (intent, contracts, relationships) paired with an executable computational graph (typed ops, data/control flow). Research strongly recommends implementing these as two separate `StableGraph` instances with explicit cross-references, NOT a single heterogeneous graph. Bidirectional propagation between layers is the single hardest correctness problem in the system and must use a queue-based event-driven model with explicit flush, not immediate synchronization. The architecture research also warns against a pure "Sea of Nodes" approach for the computational graph -- V8 abandoned this after a decade -- and recommends a CFG-skeleton with floating pure nodes instead.

The top risks are: (1) bidirectional layer propagation becoming an infinite loop or inconsistency engine, (2) the graph-to-LLVM-IR lowering impedance mismatch requiring an intermediate scheduled linear IR, (3) inkwell's lifetime model forcing awkward architectural compromises if not handled with function-scoped Context patterns from the start, (4) the contract system becoming scope creep that blocks all other progress if not built incrementally (types first, simple pre/post later, full system much later), and (5) the op node set being either incomplete or incoherent, which cascades into every downstream system. The mitigation strategy across all five is the same: get the core graph data model and op node set right in Phase 1, defer complexity to later phases, and validate each layer with tests before adding the next.

## Key Findings

### Recommended Stack

The stack is Rust-native throughout, with no controversial choices. All core dependencies are battle-tested with millions of downloads. The key architectural constraint is that LLVM (via inkwell) should be feature-gated behind a cargo feature flag so that development builds do not require LLVM installation.

**Core technologies:**
- **Rust (stable 1.84+):** Implementation language -- memory safety without GC is critical for a compiler/runtime, ownership model fits graph lifecycle management
- **petgraph 0.8 (StableGraph):** Core in-memory graph representation -- dominant Rust graph library, preserves node indices across removals, built-in serde and DOT export
- **rusqlite 0.38 (bundled):** Persistent storage backend -- SQLite's single-file model maps perfectly to "one file per program," ACID transactions for graph mutation safety
- **inkwell 0.7 (LLVM 18):** Safe LLVM IR generation -- the only safe Rust wrapper over LLVM, strongly typed API catches errors at compile time
- **axum 0.8:** HTTP API framework for the AI agent tool interface -- Tokio ecosystem, clean API, tower middleware
- **tokio 1.43+:** Async runtime -- required by axum, provides async locks and channels for multi-agent coordination
- **thiserror 2.0 + miette 7:** Error handling -- structured errors for compiler internals (thiserror), rich diagnostic display for users/agents (miette)
- **serde + serde_json + bincode 2.0:** Serialization -- JSON for API payloads, bincode for efficient graph snapshot storage

**Critical version requirements:** LLVM 18 must be installed on the build machine for codegen. Pin inkwell to `llvm18-0` feature. Use `StableGraph` from petgraph, never `Graph`.

**Workspace structure:** 7 crates -- `lmlang-core` (graph types), `lmlang-store` (persistence), `lmlang-interp` (interpreter), `lmlang-verify` (contracts), `lmlang-propagation` (layer sync), `lmlang-codegen` (LLVM, feature-gated), `lmlang-api` (agent API). Clear dependency direction: core <- store/interp/verify/propagation <- codegen <- api <- cli.

### Expected Features

**Must have (table stakes -- v1 demo):**
- Type system (scalars, aggregates, pointers, function types) -- foundation for everything else
- Typed computational graph nodes (~30 ops in tiers: 15-18 core, 8-10 structured)
- Data flow + control flow edges with typed connections
- Function/module boundaries for program decomposition
- Graph persistence in SQLite -- programs must survive process restart
- AI tool API: `propose_structured_edit`, `retrieve_subgraph`, `verify_and_propagate`
- Basic contract system: type checking only (fast, well-understood)
- Graph interpreter for development execution without LLVM
- LLVM IR generation for native binary output
- Error messages with graph location context (node IDs, edge paths, contract failures)
- Undo/rollback for edits (edit log or snapshots)

**Should have (add after core loop is validated -- v1.x):**
- Full contract system (pre/post-conditions, invariants) -- layer on top of type checking
- AI tool API: `simulate` (test behavior before committing) and `optimize`
- Deterministic graph hashing + incremental recompilation via dirty node tracking
- Semantic embeddings on graph nodes for AI-driven navigation
- Compact node summaries for context-window efficiency
- Graph visualization (DAG viewer) for human observability
- Multi-agent region locking for concurrent editing

**Defer to v2+:**
- Dual-layer bidirectional propagation (the hardest feature -- defer until executable layer is rock-solid)
- Natural language query interface (requires embeddings infrastructure)
- Property-based testing from contracts (requires full contract system + interpreter maturity)
- Content-addressable deduplication (optimization for large graphs)
- AI-guided optimization (evolutionary/MCTS -- research-grade)
- Full graph CRDT/OT for concurrent editing (research frontier, premature for v1)

**Anti-features (deliberately NOT building):**
- Human-readable text syntax (undermines the core thesis)
- Built-in AI agent loop (tight coupling to specific LLM providers)
- IDE/text editor integration (no text to edit)
- General-purpose graph query language like SPARQL/Cypher (agents call structured APIs, not query languages)
- Plugin/extension system (premature abstraction)

### Architecture Approach

The system is a layered architecture with a dual-graph core, an event-driven propagation engine, a contract verification pipeline, two execution backends (interpreter + LLVM codegen), and an agent-facing HTTP API that orchestrates everything. The two graph layers are separate `StableGraph` instances with typed cross-references. All mutations flow through a propose-validate-commit protocol. The LLVM compilation pipeline uses topological walk with SCC decomposition for loops, emitting through an intermediate scheduled linear IR (not directly from graph traversal). Incremental recompilation follows rustc's red-green algorithm.

**Major components:**
1. **ProgramGraph** (lmlang-core) -- Dual `StableGraph` container with semantic + computational layers and cross-reference maps
2. **GraphStore** (lmlang-store) -- Trait-based persistence abstraction with SQLite and in-memory implementations
3. **PropagationEngine** (lmlang-propagation) -- Queue-based dirty event processing for bidirectional layer sync with cycle breaking
4. **ContractVerifier** (lmlang-verify) -- Type checking pipeline, extensible to pre/post-conditions and invariants
5. **GraphInterpreter** (lmlang-interp) -- Topological-sort walker for development-time execution
6. **LlvmCodegen** (lmlang-codegen) -- Graph-to-LLVM-IR lowering via scheduled linear IR, with incremental recompilation
7. **AgentToolAPI** (lmlang-api) -- axum HTTP server exposing structured tool endpoints for AI agents

**Key patterns:**
- Dual-Graph with Cross-References (not a single heterogeneous graph)
- Event-Driven Propagation with Dirty Queues (not immediate bidirectional sync)
- CFG-Skeleton with Floating Pure Nodes (not pure Sea of Nodes)
- Red-Green Incremental Recompilation (rustc-inspired)
- Function-Scoped LLVM Context (avoids inkwell lifetime contamination)

### Critical Pitfalls

1. **Bidirectional layer propagation loops** -- Designate one layer as source of truth at any given moment. Use propose-validate-commit protocol. Never allow simultaneous unsynchronized writes to both layers. Queue-based propagation with explicit flush, not immediate sync. Design this protocol in Phase 1 before building anything on top.

2. **Graph-to-LLVM-IR lowering impedance mismatch** -- Do NOT emit LLVM IR directly from graph traversal. Build an explicit intermediate step: graph -> scheduled linear IR -> LLVM IR. Use "alloca everything, let mem2reg optimize" pattern for memory operations. Verify generated IR with `module.verify()` on every test case. The op node set must be designed to facilitate clean lowering.

3. **Inkwell lifetime contamination** -- Design compilation as a function, not a struct. Create `Context` at function scope, pass `&'ctx Context` down, serialize output to object files immediately, drop all LLVM state. No LLVM types escape the compilation function boundary. Inkwell does not support multithreading -- each compilation unit gets its own context.

4. **Contract system scope creep** -- Build in layers: types only (Phase 1), simple boolean pre/post-conditions (Phase 2), invariants (Phase 4+), property-based testing (future/background). Never make the full contract system a gate for compilation. Contracts should be advisory before they become mandatory.

5. **Op node set incoherence** -- Define ops in tiers: Tier 1 (core: ~15-18 ops mapping directly to LLVM instructions), Tier 2 (structured: ~8-10 ops with well-defined lowerings), Tier 3 (extension: subgraph templates, not new primitives). Every op must have exactly one LLVM lowering. Validate with property-based testing from day one.

6. **Multi-agent concurrent editing without structural invariants** -- Do NOT use CRDTs for the graph. Use centralized graph with subgraph-level optimistic concurrency control (MVCC-style). Agents get working copies, propose edits, system validates against full graph invariants, commits or rejects. Design locking granularity in Phase 1 even though multi-agent is Phase 4.

## Implications for Roadmap

Based on the combined research, the project naturally decomposes into 6 phases following the crate dependency graph and feature dependency tree. The critical insight is that the type system and op node set are foundational -- nearly everything depends on them -- and that LLVM codegen should come after the interpreter proves the graph model is correct.

### Phase 1: Core Graph Foundation
**Rationale:** Everything depends on the graph data model, type system, and op node set. Getting these wrong forces rewrites of all downstream systems. Research unanimously identifies this as the highest-risk, highest-impact work.
**Delivers:** `lmlang-core` crate with dual `StableGraph` types, type system, op node enum (Tier 1 + 2), data/control flow edges, function/module boundaries, cross-reference maps, and a `GraphStore` trait with in-memory implementation.
**Features addressed:** Type system, typed computational graph nodes, data flow + control flow edges, function/module boundaries.
**Pitfalls to avoid:** Op node set incoherence (define tiers, validate each op has exactly one LLVM lowering target), petgraph index instability (use `StableGraph` from day one, add stable ID layer), propagation protocol design (define propose-validate-commit even if not implementing propagation yet), storage trait abstraction (define `GraphStore` trait with graph-semantic operations, not SQL-semantic).
**Research flag:** Needs deeper research on the op node set -- map each proposed op to its LLVM IR instruction(s) before committing to the set.

### Phase 2: Storage + Interpreter + Basic Verification
**Rationale:** These three subsystems are independent of each other but all depend on core. They can be built in parallel. The interpreter provides a fast feedback loop for validating the graph model without LLVM. Storage enables persistence. Type checking is the minimum contract that makes the system useful.
**Delivers:** `lmlang-store` (SQLite backend), `lmlang-interp` (graph interpreter), `lmlang-verify` (type checking). Programs can be saved, loaded, type-checked, and executed via interpretation.
**Features addressed:** Graph persistence (SQLite), graph interpreter, basic contract system (type checking), error messages with graph context.
**Pitfalls to avoid:** SQLite schema must use graph-semantic trait operations (not raw SQL), full graph validation on every edit (implement incremental subgraph validation from the start), interpreter/codegen coupling (share OpNode types but implement execution independently).
**Research flag:** Standard patterns -- SQLite graph schema, tree-walking interpreters, and type checkers are well-documented. Skip phase research.

### Phase 3: AI Agent Tool API
**Rationale:** The AI tool API is the primary user interface. Once the graph can be stored, interpreted, and type-checked, agents need a way to interact with it. This phase validates the core interaction loop: agent proposes edit -> system validates -> system responds.
**Delivers:** `lmlang-api` crate with axum HTTP server exposing `propose_structured_edit`, `retrieve_subgraph`, `verify_and_propagate`, `simulate`, and undo/rollback. Single-agent only.
**Features addressed:** AI tool API (propose, retrieve, verify), undo/rollback, simulate (via interpreter).
**Pitfalls to avoid:** Agent UX pitfalls (return semantic identifiers not raw UUIDs, structured error messages with suggestions, add `validate_edit` dry-run mode, implement `get_valid_operations` helper), retrieve_subgraph returning too much data (default to 2-hop neighborhood, support pagination).
**Research flag:** Needs research on agent tool API design patterns. Anthropic's "Writing Effective Tools for AI Agents" guidance should inform schema design. Test with a real LLM to measure tool call success rates.

### Phase 4: LLVM Compilation Pipeline
**Rationale:** With the interpreter proving the graph model correct, LLVM codegen can use interpreter results as a reference oracle. The intermediate scheduled linear IR must be built -- do NOT emit LLVM IR directly from graph traversal. This is the phase where the op node set design from Phase 1 pays off or demands revision.
**Delivers:** `lmlang-codegen` crate with graph -> scheduled linear IR -> LLVM IR pipeline, function-level compilation, basic optimization passes, native binary output.
**Features addressed:** LLVM IR generation, native binary compilation.
**Pitfalls to avoid:** Inkwell lifetime contamination (function-scoped Context, no LLVM types escape), graph-to-LLVM impedance mismatch (scheduled linear IR intermediate, "alloca everything + mem2reg" pattern), LLVM target assumptions (always specify target triple, test on x86_64 + aarch64).
**Research flag:** Needs research on the scheduled linear IR format and SCC-based loop emission. The Move-on-LLVM experience report and Cranelift's aegraph design are key references.

### Phase 5: Incremental Compilation + Contracts + Multi-Agent
**Rationale:** These features add robustness and scalability to the working system. Incremental compilation requires deterministic hashing (which requires the graph model to be stable). Richer contracts require type checking to be solid. Multi-agent editing requires the validation pipeline and undo system to be battle-tested.
**Delivers:** Deterministic graph hashing, red-green incremental recompilation, pre/post-condition contracts, multi-agent region locking with optimistic concurrency control.
**Features addressed:** Deterministic graph hashing, incremental recompilation, full contract system (pre/post), multi-agent region locking.
**Pitfalls to avoid:** Contract system becoming a bottleneck (keep contracts advisory, enforce complexity budgets: <1ms for types, <10ms for pre/post), multi-agent invariant violations (MVCC-style concurrency, NOT CRDTs), over-invalidation in dirty tracking (use red-green algorithm, verify <5 function recompilations for single-function change in 100-function program).
**Research flag:** Multi-agent concurrency needs deeper research. DAG CRDT literature (ACM PaPoC 2024) confirms CRDTs are wrong for this. OCC/MVCC patterns on graphs are less documented. Incremental compilation follows rustc's well-documented red-green pattern -- standard.

### Phase 6: Semantic Layer + Visualization + Advanced Features
**Rationale:** The dual-layer bidirectional propagation is the hardest feature and should only be attempted once the executable layer is rock-solid. Semantic embeddings, NL query, and visualization are valuable but not essential for the core agent-builds-programs loop.
**Delivers:** `lmlang-propagation` (bidirectional layer sync), `lmlang-viz` (DOT export, web viewer), semantic embeddings on nodes, compact node summaries, NL query interface, graph optimization API.
**Features addressed:** Dual-layer bidirectional propagation, semantic embeddings, NL query interface, graph visualization, compact node summaries, AI tool API: optimize.
**Pitfalls to avoid:** Bidirectional propagation infinite loops (queue-based with explicit flush, "currently propagating" cycle breaker set, one layer authoritative at a time), propagation fan-out (depth limits, hash-based early stabilization detection).
**Research flag:** Needs significant research. Bidirectional model synchronization is a formal research topic. The propagation protocol must be designed carefully -- no standard pattern exists for this specific dual-layer architecture. Embedding model selection and update strategy also need investigation.

### Phase Ordering Rationale

- **Core first** because literally every other component depends on the graph types, type system, and op node set. The feature dependency tree shows Type System as the root of nearly all dependency chains.
- **Storage + Interpreter + Verification in parallel** because they are independent leaf crates. The interpreter is prioritized because it validates the graph model without requiring LLVM (which is the heaviest dependency). This follows the STACK.md recommendation of "interpreter-first prototype."
- **Agent API before LLVM** because the primary user is an AI agent, not a human running a compiler. Validating the agent interaction loop early (with interpreter-based execution) proves the system's value proposition before investing in native compilation.
- **LLVM after interpreter** because the interpreter serves as a correctness oracle for codegen. Every LLVM output can be compared against interpreter results.
- **Multi-agent and contracts after single-agent works** because the PITFALLS research explicitly warns that making the full contract system a Phase 1 requirement is a critical mistake, and that concurrent editing requires all safety nets (validation, undo, contracts) to exist first.
- **Bidirectional propagation last** because it is the single hardest correctness problem, it requires both layers to be individually solid, and the FEATURES research rates it VERY HIGH complexity. The v1 demo can succeed with manual layer management.

### Research Flags

**Phases likely needing deeper research during planning:**
- **Phase 1 (Core Graph):** Op node set design needs careful mapping to LLVM IR targets. Recommend creating a bidirectional op-to-LLVM mapping document before implementation.
- **Phase 3 (Agent API):** Agent tool API schema design is novel. Test with real LLMs early. Anthropic's agent tool design guidelines should be incorporated.
- **Phase 4 (LLVM Codegen):** Scheduled linear IR format, SCC-based loop emission, and phi node placement need investigation. Move-on-LLVM and Cranelift are key references.
- **Phase 5 (Multi-Agent):** Graph-level MVCC/OCC is not widely documented. Need to design locking granularity and conflict resolution.
- **Phase 6 (Propagation):** No standard pattern exists for bidirectional propagation between a semantic KG and an executable graph. This is research-grade work.

**Phases with standard patterns (skip phase research):**
- **Phase 2 (Storage + Interpreter + Verification):** SQLite graph schemas, tree-walking interpreters, and Hindley-Milner-style type checkers are thoroughly documented. Well-established patterns.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All core crates verified on crates.io/docs.rs with high download counts. Version compatibility confirmed. No risky or experimental choices. |
| Features | MEDIUM | Novel domain -- feature set is inferred from adjacent fields (compiler IR, knowledge graphs, AI agent tools). No direct competitor to validate against. Feature dependencies and MVP definition are well-reasoned. |
| Architecture | MEDIUM-HIGH | Dual-graph pattern is well-supported by graph IR literature. Propagation and codegen patterns draw from proven systems (rustc, MLIR, Cranelift). The novel aspect -- bidirectional dual-layer sync -- has academic foundations but no production precedent. |
| Pitfalls | HIGH | LLVM integration pitfalls are thoroughly documented by multiple authoritative sources. Graph CRDT limitations confirmed by recent academic research. Contract system risks well-known from Eiffel/Dafny community. Agent UX pitfalls informed by Anthropic's own guidance. |

**Overall confidence:** MEDIUM-HIGH

### Gaps to Address

- **Op node set validation:** The specific set of ~30 ops needs to be mapped to LLVM IR instructions before committing. Prototype Tier 1 ops with round-trip LLVM compilation tests before expanding.
- **Bidirectional propagation protocol:** No production precedent exists for syncing a semantic KG with an executable graph. This needs a formal specification before implementation. Consider starting with one-directional propagation (semantic -> computational only) and adding the reverse later.
- **Agent tool API effectiveness:** No data on how well AI agents perform with graph-based tool APIs vs. text-based ones. Plan to run agent integration tests in Phase 3 and iterate on the schema based on real LLM performance.
- **MCP protocol evolution:** The MCP ecosystem is rapidly evolving (LOW confidence on rmcp crate). Start with plain axum REST API; add MCP as a protocol layer later when the ecosystem stabilizes.
- **Performance at scale:** No data on petgraph `StableGraph` performance beyond ~50K nodes. The virtual graph / lazy loading strategy from the architecture research needs validation if programs grow large.
- **LLVM version lifecycle:** Pinning to LLVM 18 is correct now, but LLVM 19+ will eventually be needed. The inkwell feature flag abstraction handles this, but CI must test the upgrade path.

## Sources

### Primary (HIGH confidence)
- [petgraph on crates.io/docs.rs](https://docs.rs/crate/petgraph/latest) -- graph data structure, StableGraph semantics, serde serialization
- [inkwell GitHub](https://github.com/TheDan64/inkwell) -- LLVM bindings, lifetime model, version support matrix
- [rusqlite on crates.io](https://crates.io/crates/rusqlite) -- SQLite bindings, bundled feature
- [axum 0.8 announcement](https://tokio.rs/blog/2025-01-01-announcing-axum-0-8-0) -- web framework
- [Rust incremental compilation guide](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html) -- red-green algorithm
- [V8: Leaving the Sea of Nodes](https://v8.dev/blog/leaving-the-sea-of-nodes) -- cautionary tale for graph IR design
- [LLVM: The Bad Parts (Nikita Popov, 2026)](https://www.npopov.com/2026/01/11/LLVM-The-bad-parts.html) -- LLVM IR design issues
- [Writing an LLVM Backend for Move in Rust](https://brson.github.io/2023/03/12/move-on-llvm/) -- alloca+mem2reg pattern
- [DAG CRDTs (ACM PaPoC 2024)](https://dl.acm.org/doi/10.1145/3721473.3722141) -- CRDT limitations for DAGs
- [CRDTs: The Hard Parts (Kleppmann)](https://martin.kleppmann.com/2020/07/06/crdt-hard-parts-hydra.html) -- concurrent editing challenges
- [Writing Effective Tools for AI Agents (Anthropic, 2025)](https://www.anthropic.com/engineering/writing-tools-for-agents) -- agent UX design
- [Dafny as Verification-Aware IL (POPL 2025)](https://popl25.sigplan.org/details/dafny-2025-papers/11/) -- formal verification for code gen
- [Agentic Coding Trends Report 2026 (Anthropic)](https://resources.anthropic.com/hubfs/2026%20Agentic%20Coding%20Trends%20Report.pdf) -- AI coding agent patterns

### Secondary (MEDIUM confidence)
- [MLIR Introduction (Stephen Diehl)](https://www.stephendiehl.com/posts/mlir_introduction/) -- multi-level IR patterns
- [ProGraML](https://arxiv.org/abs/2003.10536) -- graph-based program representation
- [Glow: Graph Lowering Compiler Techniques](https://arxiv.org/abs/1805.00907) -- two-level graph IR
- [Cranelift architecture](https://cranelift.dev/) -- CFG-based IR, e-graph optimization
- [Unison: The Big Idea](https://www.unison-lang.org/docs/the-big-idea/) -- content-addressable code
- [Runtime Verification Overhead (Cornell, ISSTA 2024)](https://www.cs.cornell.edu/~legunsen/pubs/GuanAndLegunsenRVOverheadStudyISSTA24.pdf) -- contract checking costs

### Tertiary (LOW confidence)
- [MCP Rust SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk) -- rapidly evolving, verify at implementation time
- [CozoDB](https://github.com/cozodb/cozo) -- future optional graph DB backend, small community

---
*Research completed: 2026-02-17*
*Ready for roadmap: yes*
