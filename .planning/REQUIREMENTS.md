# Requirements: lmlang

**Defined:** 2026-02-17
**Core Value:** AI agents can build, modify, and verify programs of arbitrary size with perfect local and global awareness

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Core Graph Representation

- [x] **GRAPH-01**: System represents programs as typed computational graph nodes with ~30 primitive operations (arithmetic, comparison, logic, control flow, memory, function call/return, I/O)
- [x] **GRAPH-02**: Graph edges encode data flow with typed connections (SSA-style: each value produced once, consumed via typed edges)
- [x] **GRAPH-03**: Graph edges encode control flow for side-effect ordering, conditionals (branch/merge), and loop back-edges
- [ ] **GRAPH-04**: Programs decompose into function subgraphs with typed interfaces (parameters + return type) and module boundaries
- [x] **GRAPH-05**: Type system supports scalars (i8-i64, f32/f64, bool), aggregates (arrays, structs), pointers/references, and function signatures
- [x] **GRAPH-06**: Op node set organized in tiers: Tier 1 core (~15-18 ops mapping directly to LLVM IR), Tier 2 structured (~8-10 ops for struct access, array index, cast, phi/merge)

### Storage & Persistence

- [ ] **STORE-01**: Programs persist in SQLite with atomic writes and schema migration support
- [ ] **STORE-02**: Graph storage uses a trait abstraction (`GraphStore`) swappable to alternative backends without core changes
- [ ] **STORE-03**: User can undo/rollback edits via edit log or graph snapshots
- [ ] **STORE-04**: Each graph node has a deterministic content hash for identity and change detection
- [ ] **STORE-05**: Incremental recompilation via red-green dirty node tracking -- only recompile functions whose subgraphs actually changed

### AI Agent Tool Interface

- [ ] **TOOL-01**: `propose_structured_edit` accepts structured graph mutations (node insert, edge add/remove, node modify, subgraph replace) with validation before commit
- [ ] **TOOL-02**: `retrieve_subgraph` returns focused graph context by node ID, function boundary, N-hop neighborhood, or type/relationship filter as structured JSON
- [ ] **TOOL-03**: `verify_and_propagate` type-checks affected subgraph, runs contract checks, marks dirty nodes, propagates changes between layers, returns pass/fail with diagnostics
- [ ] **TOOL-04**: `simulate` executes a subgraph with provided inputs in the interpreter, returns output values and execution trace
- [ ] **TOOL-05**: Tool API exposed as HTTP/JSON endpoints via axum, callable by any LLM with structured output support
- [ ] **TOOL-06**: Error responses include structured diagnostics with graph location context (node IDs, edge paths, failing contract, expected vs actual types)

### Contract System

- [ ] **CNTR-01**: Static type checking verifies that edge source types match edge sink expected types on every edit
- [ ] **CNTR-02**: Functions support pre-conditions as contract nodes checked at function entry
- [ ] **CNTR-03**: Functions support post-conditions as contract nodes checked at function return
- [ ] **CNTR-04**: Data structures support invariants checked at module boundaries
- [ ] **CNTR-05**: Property-based tests auto-generated from contracts to verify graph behavior across input ranges

### Execution & Compilation

- [ ] **EXEC-01**: Graph interpreter walks the computational graph and executes op nodes for development-time feedback without LLVM
- [ ] **EXEC-02**: LLVM compilation pipeline maps each op node to LLVM IR instructions via inkwell, handles SSA form, function boundaries, type mapping
- [ ] **EXEC-03**: Compilation produces native binaries (x86_64/ARM) through LLVM optimization passes and system linker
- [ ] **EXEC-04**: LLVM codegen uses function-scoped Context pattern (create, compile, serialize, drop) to avoid lifetime contamination

### Dual-Layer Architecture

- [ ] **DUAL-01**: Semantic Knowledge Graph layer stores modules, functions, types, specs, tests, docs with embeddings and summaries
- [ ] **DUAL-02**: Executable Computational Graph layer stores typed ops DAG with data + control flow edges
- [ ] **DUAL-03**: Two layers implemented as separate StableGraph instances with explicit cross-references (not one heterogeneous graph)
- [ ] **DUAL-04**: Bidirectional propagation syncs layers via queue-based event model with explicit flush (not immediate sync)
- [ ] **DUAL-05**: Downward propagation: semantic changes (new function, signature change, contract addition) expand into computational subgraph modifications
- [ ] **DUAL-06**: Upward propagation: computational changes update semantic summaries, embeddings, relationship edges, and complexity metrics
- [ ] **DUAL-07**: Each semantic node carries vector embeddings for semantic similarity search and AI-driven navigation

### Multi-Agent Concurrency

- [ ] **MAGENT-01**: Multiple AI agents can read and edit the graph concurrently through the tool API
- [ ] **MAGENT-02**: Region-level locking prevents conflicting edits to the same subgraph
- [ ] **MAGENT-03**: Optimistic concurrency with conflict detection and rollback for overlapping edits
- [ ] **MAGENT-04**: Verification runs on merge to ensure global invariants hold after concurrent modifications

### Human Observability

- [ ] **VIZ-01**: Graph visualization renders the program as an interactive DAG showing nodes with op types, edges with data types, and function boundaries
- [ ] **VIZ-02**: Visualization distinguishes between semantic and computational layers with visual differentiation
- [ ] **VIZ-03**: Natural language query interface maps questions to graph queries via semantic embeddings + relationship edges
- [ ] **VIZ-04**: Query results return relevant subgraphs with context (summaries, relationships, contracts)

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Advanced AI Tools

- **TOOL-07**: `optimize` applies graph transformations (constant folding, DCE, CSE) and returns optimized subgraph
- **TOOL-08**: Compact node summaries auto-generated for context-window efficiency

### Advanced Optimization

- **OPT-01**: AI-guided optimization via evolutionary/MCTS mutation of subgraphs
- **OPT-02**: Content-addressable deduplication of common subgraph patterns

### Advanced Concurrency

- **MAGENT-05**: MVCC-style transaction isolation for agent sessions
- **MAGENT-06**: Graph CRDT for distributed concurrent editing (research frontier)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Human-readable text syntax / source language | Graph is the program, not a transpilation target -- undermines core thesis |
| Built-in AI agent loop / orchestrator | System is model-agnostic; external LLMs drive it via tool API |
| IDE / text editor integration | No text to edit; humans observe via visualization |
| Cloud/distributed deployment | Single-machine prototype first |
| Plugin/extension system | Premature abstraction; hard-code in v1 |
| General-purpose graph query language (Cypher/SPARQL) | AI agents call structured APIs, not query languages |
| Real-time streaming compilation | Discrete edits + incremental recompilation is the right granularity |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| GRAPH-01 | Phase 1 | Complete |
| GRAPH-02 | Phase 1 | Complete |
| GRAPH-03 | Phase 1 | Complete |
| GRAPH-04 | Phase 1 | Pending |
| GRAPH-05 | Phase 1 | Complete |
| GRAPH-06 | Phase 1 | Complete |
| STORE-01 | Phase 2 | Pending |
| STORE-02 | Phase 2 | Pending |
| STORE-03 | Phase 4 | Pending |
| STORE-04 | Phase 2 | Pending |
| STORE-05 | Phase 6 | Pending |
| TOOL-01 | Phase 4 | Pending |
| TOOL-02 | Phase 4 | Pending |
| TOOL-03 | Phase 4 | Pending |
| TOOL-04 | Phase 4 | Pending |
| TOOL-05 | Phase 4 | Pending |
| TOOL-06 | Phase 4 | Pending |
| CNTR-01 | Phase 3 | Pending |
| CNTR-02 | Phase 6 | Pending |
| CNTR-03 | Phase 6 | Pending |
| CNTR-04 | Phase 6 | Pending |
| CNTR-05 | Phase 6 | Pending |
| EXEC-01 | Phase 3 | Pending |
| EXEC-02 | Phase 5 | Pending |
| EXEC-03 | Phase 5 | Pending |
| EXEC-04 | Phase 5 | Pending |
| DUAL-01 | Phase 8 | Pending |
| DUAL-02 | Phase 1 | Pending |
| DUAL-03 | Phase 1 | Pending |
| DUAL-04 | Phase 8 | Pending |
| DUAL-05 | Phase 8 | Pending |
| DUAL-06 | Phase 8 | Pending |
| DUAL-07 | Phase 8 | Pending |
| MAGENT-01 | Phase 7 | Pending |
| MAGENT-02 | Phase 7 | Pending |
| MAGENT-03 | Phase 7 | Pending |
| MAGENT-04 | Phase 7 | Pending |
| VIZ-01 | Phase 9 | Pending |
| VIZ-02 | Phase 9 | Pending |
| VIZ-03 | Phase 9 | Pending |
| VIZ-04 | Phase 9 | Pending |

**Coverage:**
- v1 requirements: 41 total
- Mapped to phases: 41
- Unmapped: 0

---
*Requirements defined: 2026-02-17*
*Last updated: 2026-02-17 after roadmap creation*
