# Roadmap: lmlang

## Overview

lmlang delivers an AI-native programming system where programs are persistent dual-layer graphs manipulated by AI agents through a structured tool API, compiled to native binaries via LLVM. The roadmap builds outward from the core graph data model: first making programs representable and storable, then verifiable and executable, then agent-accessible, then compilable to native code, then robust (contracts, incremental compilation), then concurrent (multi-agent), then fully dual-layer (semantic + executable with bidirectional propagation), and finally human-observable (visualization and natural language queries). Each phase delivers a coherent, testable capability that builds on the previous ones.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Core Graph Data Model** - Type system, op nodes, data/control flow edges, function boundaries, and dual-graph structure (completed 2026-02-18)
- [x] **Phase 2: Storage & Persistence** - SQLite backend, trait abstraction, content hashing so programs survive restarts (completed 2026-02-18)
- [x] **Phase 3: Type Checking & Graph Interpreter** - Static type verification and development-time execution without LLVM (completed 2026-02-18)
- [ ] **Phase 4: AI Agent Tool API** - HTTP endpoints for structured graph manipulation, retrieval, verification, simulation, and undo
- [ ] **Phase 5: LLVM Compilation Pipeline** - Graph to LLVM IR to native binary with function-scoped codegen
- [ ] **Phase 6: Full Contract System & Incremental Compilation** - Pre/post-conditions, invariants, property-based testing, and red-green dirty tracking
- [ ] **Phase 7: Multi-Agent Concurrency** - Concurrent graph editing with region locking and optimistic concurrency control
- [ ] **Phase 8: Dual-Layer Semantic Architecture** - Semantic knowledge graph, bidirectional propagation, and vector embeddings
- [ ] **Phase 9: Human Observability** - Graph visualization, layer differentiation, natural language queries, and contextual results

## Phase Details

### Phase 1: Core Graph Data Model
**Goal**: Programs can be represented as typed computational graphs with operations, data flow, control flow, and function/module boundaries using dual StableGraph instances
**Depends on**: Nothing (first phase)
**Requirements**: GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, DUAL-02, DUAL-03
**Success Criteria** (what must be TRUE):
  1. A program can be constructed as a graph containing typed op nodes (arithmetic, comparison, logic, control flow, memory, call/return, I/O) organized in Tier 1 and Tier 2 sets
  2. Data flow between operations is represented by typed edges where each value is produced once and consumed via typed connections (SSA-style)
  3. Control flow for conditionals, loops, and side-effect ordering is represented by separate control edges with branch/merge and back-edge semantics
  4. Programs decompose into function subgraphs with typed parameter/return interfaces and module boundaries
  5. The type system supports scalars (i8-i64, f32/f64, bool), aggregates (arrays, structs), pointers/references, and function signatures
**Plans**: 4 plans

Plans:
- [ ] 01-01-PLAN.md — Cargo workspace setup + type system, type registry, IDs, error types
- [ ] 01-02-PLAN.md — Op node enums (Tier 1 + Tier 2), edge types, node wrappers
- [ ] 01-03-PLAN.md — Function definitions with closures, hierarchical module tree
- [ ] 01-04-PLAN.md — Dual ProgramGraph container, builder API, integration test

### Phase 2: Storage & Persistence
**Goal**: Programs persist across process restarts in SQLite with a swappable backend and content-addressable identity
**Depends on**: Phase 1
**Requirements**: STORE-01, STORE-02, STORE-04
**Success Criteria** (what must be TRUE):
  1. A graph program saved to SQLite can be loaded back with all nodes, edges, types, and structure intact across process restarts
  2. Storage operations go through a GraphStore trait that can be swapped to an alternative backend (e.g., in-memory) without changing core logic
  3. Every graph node has a deterministic content hash that changes when and only when the node's content changes
**Plans**: 3 plans

Plans:
- [x] 02-01-PLAN.md — GraphStore trait, StorageError, ProgramId, decompose/recompose, InMemoryStore
- [x] 02-02-PLAN.md — SQLite schema, migrations, SqliteStore implementation, save/load roundtrip tests
- [ ] 02-03-PLAN.md — blake3 content hashing, Merkle-tree composition, per-function root hashes

### Phase 3: Type Checking & Graph Interpreter
**Goal**: Programs can be statically type-checked and executed via interpretation for development-time feedback without requiring LLVM
**Depends on**: Phase 1
**Requirements**: CNTR-01, EXEC-01
**Success Criteria** (what must be TRUE):
  1. On every graph edit, edge source types are verified to match edge sink expected types, with diagnostic errors identifying the mismatched nodes and types
  2. A computational graph can be executed by the interpreter with provided inputs, producing correct output values for arithmetic, logic, control flow, memory operations, and function calls
  3. Interpreter execution of a multi-function program with conditionals and loops produces the same results as hand-computed expected outputs
**Plans**: 3 plans

Plans:
- [x] 03-01-PLAN.md — lmlang-check crate with static type checker: per-op type rules, coercion, diagnostics, validate_data_edge/validate_graph
- [x] 03-02-PLAN.md — Graph interpreter: Value enum, state machine execution, per-op evaluation, control flow, memory, function calls, traps, traces
- [ ] 03-03-PLAN.md — Gap closure: real Loop op integration test with back-edge iteration, EXEC-01 tracking update

### Phase 4: AI Agent Tool API
**Goal**: An AI agent can build, query, verify, test, and undo changes to programs through a structured HTTP/JSON interface
**Depends on**: Phase 2, Phase 3
**Requirements**: TOOL-01, TOOL-02, TOOL-03, TOOL-04, TOOL-05, TOOL-06, STORE-03
**Success Criteria** (what must be TRUE):
  1. An AI agent can propose structured graph mutations (node insert, edge add/remove, node modify, subgraph replace) via HTTP and receive validation feedback before commit
  2. An AI agent can retrieve focused graph context by node ID, function boundary, N-hop neighborhood, or type/relationship filter as structured JSON
  3. An AI agent can trigger verification that type-checks the affected subgraph, runs contract checks, marks dirty nodes, and returns pass/fail with structured diagnostics including node IDs and edge paths
  4. An AI agent can simulate a subgraph with provided inputs via the interpreter and receive output values and execution trace
  5. An AI agent can undo/rollback edits to restore a previous graph state
**Plans**: TBD

Plans:
- [ ] 04-01: TBD
- [ ] 04-02: TBD
- [ ] 04-03: TBD

### Phase 5: LLVM Compilation Pipeline
**Goal**: Programs represented as computational graphs compile to native binaries through LLVM with correct output matching the interpreter
**Depends on**: Phase 3
**Requirements**: EXEC-02, EXEC-03, EXEC-04
**Success Criteria** (what must be TRUE):
  1. Each op node in the computational graph maps to LLVM IR instructions via inkwell, handling SSA form, function boundaries, and type mapping correctly
  2. The compilation pipeline produces working native binaries (x86_64 or ARM) through LLVM optimization passes and the system linker
  3. LLVM codegen uses function-scoped Context (create, compile, serialize, drop) with no LLVM types escaping the compilation boundary
  4. For any program, the native binary produces the same outputs as the graph interpreter given the same inputs
**Plans**: TBD

Plans:
- [ ] 05-01: TBD
- [ ] 05-02: TBD
- [ ] 05-03: TBD

### Phase 6: Full Contract System & Incremental Compilation
**Goal**: Programs have rich behavioral contracts (pre/post-conditions, invariants, property-based tests) and only changed functions recompile
**Depends on**: Phase 3, Phase 5
**Requirements**: CNTR-02, CNTR-03, CNTR-04, CNTR-05, STORE-05
**Success Criteria** (what must be TRUE):
  1. Functions can have pre-condition contract nodes that are checked at function entry, rejecting invalid inputs with diagnostic messages
  2. Functions can have post-condition contract nodes that are checked at function return, catching violated promises with diagnostics
  3. Data structures can have invariant contracts checked at module boundaries
  4. Property-based tests are auto-generated from contracts and verify graph behavior across randomized input ranges
  5. After editing a single function in a multi-function program, only that function and its dependents recompile (not the entire program)
**Plans**: TBD

Plans:
- [ ] 06-01: TBD
- [ ] 06-02: TBD
- [ ] 06-03: TBD

### Phase 7: Multi-Agent Concurrency
**Goal**: Multiple AI agents can simultaneously read and edit the program graph with consistency guarantees preventing corruption
**Depends on**: Phase 4
**Requirements**: MAGENT-01, MAGENT-02, MAGENT-03, MAGENT-04
**Success Criteria** (what must be TRUE):
  1. Two or more AI agents can read and propose edits to the graph concurrently through the tool API without data corruption
  2. Region-level locking prevents two agents from simultaneously modifying the same subgraph, with clear feedback when a lock is held
  3. When agents edit overlapping regions, optimistic concurrency detects conflicts and rolls back the later commit with an explanation
  4. After concurrent modifications are merged, a verification pass confirms all global invariants (type correctness, contract satisfaction) still hold
**Plans**: TBD

Plans:
- [ ] 07-01: TBD
- [ ] 07-02: TBD

### Phase 8: Dual-Layer Semantic Architecture
**Goal**: Programs have a semantic knowledge graph layer (intent, contracts, relationships, embeddings) that stays synchronized with the executable layer through bidirectional propagation
**Depends on**: Phase 1
**Requirements**: DUAL-01, DUAL-04, DUAL-05, DUAL-06, DUAL-07
**Success Criteria** (what must be TRUE):
  1. The semantic layer stores modules, functions, types, specs, tests, and docs with embeddings and summaries as a separate graph from the computational layer
  2. Changes to the semantic layer (new function, signature change, contract addition) automatically propagate downward into computational subgraph modifications
  3. Changes to the computational layer automatically propagate upward to update semantic summaries, embeddings, relationship edges, and complexity metrics
  4. Bidirectional propagation uses a queue-based event model with explicit flush, preventing infinite loops and maintaining consistency
  5. Each semantic node carries vector embeddings enabling semantic similarity search and AI-driven navigation
**Plans**: TBD

Plans:
- [ ] 08-01: TBD
- [ ] 08-02: TBD
- [ ] 08-03: TBD

### Phase 9: Human Observability
**Goal**: Humans can visually explore the program graph and ask natural language questions to understand what the program does
**Depends on**: Phase 1, Phase 8
**Requirements**: VIZ-01, VIZ-02, VIZ-03, VIZ-04
**Success Criteria** (what must be TRUE):
  1. The program renders as an interactive DAG showing nodes with op types, edges with data types, and function boundaries
  2. Visualization visually distinguishes between semantic and computational layers (e.g., different colors, shapes, or layout regions)
  3. A human can ask a natural language question about the program and receive relevant subgraph results found via semantic embeddings and relationship edges
  4. Query results include contextual information: summaries, relationships, and contracts for the returned subgraph nodes
**Plans**: TBD

Plans:
- [ ] 09-01: TBD
- [ ] 09-02: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Core Graph Data Model | 4/4 | Complete    | 2026-02-18 |
| 2. Storage & Persistence | 3/3 | Complete   | 2026-02-18 |
| 3. Type Checking & Graph Interpreter | 2/3 | Gap Closure | 2026-02-18 |
| 4. AI Agent Tool API | 0/? | Not started | - |
| 5. LLVM Compilation Pipeline | 0/? | Not started | - |
| 6. Full Contract System & Incremental Compilation | 0/? | Not started | - |
| 7. Multi-Agent Concurrency | 0/? | Not started | - |
| 8. Dual-Layer Semantic Architecture | 0/? | Not started | - |
| 9. Human Observability | 0/? | Not started | - |
