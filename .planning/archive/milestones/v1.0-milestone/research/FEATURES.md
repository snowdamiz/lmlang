# Feature Research

**Domain:** AI-native programming system / graph-based program representation with LLVM compilation
**Researched:** 2026-02-17
**Confidence:** MEDIUM -- novel domain with strong precedent in adjacent fields (compiler IR, knowledge graphs, AI coding agents, formal verification), but no direct competitor ships this exact combination

## Feature Landscape

### Table Stakes (System Does Not Work Without These)

Features that are foundational. If any of these are missing, the system cannot demonstrate its core value proposition (AI agents building verified programs on a graph representation).

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Typed computational graph nodes** (~30+ ops: arithmetic, control flow, memory, I/O) | Without a complete set of computational primitives, the graph cannot represent general-purpose programs. Every graph-based IR (LLVM IR, MLIR dialects, Sea of Nodes) provides this. | HIGH | Must cover: integer/float arithmetic, comparison, bitwise, load/store, branch/phi, loop, function call/return, allocate/free, basic I/O. The set must be extensible. |
| **Data flow edges with typed connections** | Data dependencies are how graph-based IRs represent computation. Without typed edges, you cannot verify that op outputs match op inputs. ProGraML, LLVM IR SSA, and Sea of Nodes all enforce this. | MEDIUM | SSA-style: each value produced exactly once, consumed by typed edges. |
| **Control flow edges** | Pure dataflow is insufficient for side effects, sequencing, and I/O ordering. LLVM IR, Sea of Nodes, and MLIR all maintain explicit control dependencies. | MEDIUM | Must handle: sequential ordering of side-effecting ops, branch/merge for conditionals, loop back-edges. |
| **Function/module boundaries** | Programs decompose into callable units. Every compiler IR has this. Without it, you cannot build programs larger than a single expression. | MEDIUM | Graph-level: function subgraph boundaries with typed interface (params + return). Module = collection of functions + types. |
| **Type system** (scalars, aggregates, pointers/references, function types) | Cannot compile to LLVM IR without a type system. Cannot type-check edges without types. Every compilation target requires this. | HIGH | Must map cleanly to LLVM types. Support: i8-i64, f32/f64, bool, arrays, structs, pointers, function signatures. |
| **Graph persistence and serialization** | The graph IS the program. If it cannot be saved and loaded, the program is ephemeral. Unison proved content-addressable storage works; SQLite is the right embedded choice. | MEDIUM | SQLite-backed. Must support: save/load full graph, atomic writes, schema migration. Swappable to graph DB later per PROJECT.md constraints. |
| **AI tool API: propose_structured_edit** | The core interaction primitive. Without structured edits, AI agents cannot modify the graph safely. This is what makes the system AI-native vs. just "a graph data structure." MCP and structured outputs (JSON Schema) provide the protocol pattern. | HIGH | Must accept: node insertions, edge additions/removals, node modifications, subgraph replacements. Must validate edits before applying (type-check, connectivity check). |
| **AI tool API: retrieve_subgraph** | AI agents have finite context windows. Subgraph retrieval is the mechanism that gives them focused, relevant context. This is the graph equivalent of "semantic code search" tools like grepai, Sourcegraph Cody, Kilo Code. | HIGH | Must support: retrieve by node ID, by function boundary, by N-hop neighborhood, by type/relationship filter. Return as structured data (JSON). |
| **AI tool API: verify_and_propagate** | After an edit, the system must check that contracts still hold and propagate changes between layers. Without verification, multi-agent editing has no safety net. Analogous to rustc's red-green dirty tracking combined with DbC checking. | HIGH | Must: type-check affected subgraph, run contract checks (pre/post/invariants), mark dirty nodes, propagate changes to other layer. Return pass/fail with diagnostics. |
| **Graph interpreter** | During development, the graph must be executable without a full LLVM compilation cycle. Provides fast feedback loop. Every modern language has a REPL or interpreter mode alongside compilation. | HIGH | Walk the graph, execute ops, handle control flow. Does not need to be fast -- correctness over performance. |
| **LLVM IR generation** | The system's output is native binaries. LLVM IR is the target per PROJECT.md. Without this, the system is a fancy data structure with no executable output. | HIGH | Map each op node to LLVM instructions. Handle: SSA form (natural fit since graph is already SSA-like), function boundaries, type mapping, memory model. Use inkwell crate. |
| **Basic contract system: type checking** | Type checking is the minimum contract. Without it, edges can connect incompatible ops, and LLVM codegen will produce nonsense. Every typed IR enforces this. | MEDIUM | Static analysis pass over graph. Check: edge source type matches edge sink expected type. Run on every edit. |

### Table Stakes (Experience -- Expected by Users/Agents)

These are not strictly required for the system to function, but their absence makes the system feel broken or unusable for the target audience (AI agents + human observers).

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Error messages with graph location context** | When verification fails, agents need to know WHERE in the graph the problem is. Every compiler provides source location in errors. Since there is no source file, errors must reference node IDs, subgraph paths, and edge connections. | MEDIUM | Return: failing node ID, failing edge, expected vs actual type, contract that failed, path from function entry to failure point. |
| **AI tool API: simulate** | Agents need to test behavior before committing edits. Simulation = "run this subgraph with these inputs, show me the output." Analogous to REPL evaluation, but scoped to a subgraph. | MEDIUM | Execute subgraph in interpreter with provided inputs. Return output values. Must handle: pure subgraphs easily, side-effecting subgraphs with mocking/sandboxing. |
| **Graph visualization (DAG viewer)** | Humans observe the program through visualization, not text. Without it, human oversight is impossible. D3-dag, Graphviz, and DAGViz are mature libraries for this. The system's own PROJECT.md lists this as a requirement. | MEDIUM | Render graph as interactive DAG. Show: nodes with op types, edges with data types, function boundaries, semantic layer annotations. Use web-based renderer (d3-dag or similar). |
| **Undo/rollback for edits** | AI agents make mistakes. Without undo, a bad edit requires reconstructing the previous state manually. Version control for graph state. | MEDIUM | Store graph snapshots or edit log. Support: undo last edit, rollback to named checkpoint. Critical for multi-agent safety. |
| **Deterministic graph hashing** | Content-addressable identity for graph nodes and subgraphs. Unison proved this is transformative: enables caching, deduplication, incremental recompilation. The graph's structure lends itself naturally to Merkle-tree-style hashing. | MEDIUM | Hash each node by: op type + input types + output types + constant values + child hashes. Ignore names (like Unison). |

### Differentiators (Competitive Advantage)

Features that distinguish lmlang from existing tools and create unique value. Not expected (since nothing like this exists in production), but these are what make the system worth building.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Dual-layer graph (semantic + executable) with bidirectional propagation** | This is THE core differentiator. No existing system maintains synchronized semantic and executable representations with automatic bidirectional propagation. MLIR has multi-level IRs but they lower in one direction. Knowledge graphs over code (KG4Py, Code Property Graphs) are read-only analytical artifacts, not live synchronized representations. | VERY HIGH | Semantic layer: modules, functions, specs, relationships, embeddings, summaries. Executable layer: typed op DAGs. Propagation: semantic intent change -> expand to executable subgraph; executable optimization -> update semantic summaries/contracts. This is research-grade complexity. |
| **Full contract system: pre/post-conditions + invariants + property-based testing** | Goes beyond type checking to Eiffel/Dafny-style contracts, but integrated into the graph structure itself. Contracts on graph nodes are machine-checkable by construction. Combined with the "vericoding" trend, this positions lmlang as a formally verifiable AI programming system. Dafny is being explored as an intermediate verification language for LLM code gen (POPL 2025). | HIGH | Pre/post-conditions as contract nodes attached to function boundaries. Invariants as annotations on data structure nodes. Property-based test generation from contracts (like GUMBOX framework). Contracts propagate across layers. |
| **Multi-agent concurrent graph manipulation with consistency guarantees** | No existing AI coding tool provides concurrent graph-level editing with formal consistency. OpenAI Codex uses git worktrees (file-level isolation). CrewAI/MetaGPT coordinate at task level, not data structure level. True concurrent graph editing requires CRDT-like or OT-like mechanisms, or transactional isolation. | VERY HIGH | Options: (1) optimistic concurrency with conflict detection + rollback, (2) graph-region locking (simpler, start here), (3) graph CRDT (research frontier, per Kleppmann). Recommend starting with region-level locking + verification on merge. |
| **Semantic embeddings on graph nodes** | Each node carries vector embeddings of its semantic meaning. Enables: natural language queries over the graph, semantic similarity search, AI-driven navigation. Goes beyond what Code Property Graphs or ProGraML offer (they compute embeddings externally, not as first-class node attributes). | MEDIUM | Generate embeddings via external model (e.g., CodeBERT, or any embedding API). Store as node metadata. Update on edit. Enable cosine-similarity search over nodes. |
| **Natural language query interface** | "Show me all functions that handle authentication" works by querying semantic embeddings + relationship edges. This is the human-facing killer feature. Existing tools (Sourcegraph Cody, grepai, ZeroEntropy) do this over text files; doing it over a structured semantic graph should be strictly more powerful. | MEDIUM | Embed query -> vector search over node embeddings -> filter by graph relationships -> return relevant subgraphs. Requires embeddings to be populated and indexed. |
| **AI tool API: optimize** | The AI agent can request optimization of a subgraph. The system applies graph transformations (constant folding, dead code elimination, common subexpression elimination) and returns the optimized version. Draws from equality saturation (TENSAT), STOKE, and LLVM's own optimization passes. | HIGH | Phase 1: classical graph optimizations (constant folding, DCE, CSE) implemented as graph rewrite rules. Phase 2: LLVM optimization passes on generated IR. Phase 3: AI-guided optimization via evolutionary/MCTS mutation (Compiler.next vision). |
| **Incremental recompilation via dirty node tracking** | When a subgraph changes, only recompile affected nodes. Rustc's red-green algorithm is the gold standard. The graph structure is IDEAL for this -- dependencies are explicit edges, dirty propagation is graph traversal. | HIGH | Mark edited nodes dirty. Propagate dirty flag along dependency edges. Recompile only dirty subgraphs. Cache LLVM IR per function. Hash-based invalidation (pairs with deterministic hashing). |
| **Compact node summaries for context-window efficiency** | Each function/module node carries an auto-generated natural language summary. Agents can read summaries instead of full subgraphs, dramatically reducing context window usage. This directly addresses the key limitation of LLM coding agents (context window limits). | MEDIUM | Auto-generate summaries from: function signature, contract, semantic relationships. Update on edit. Return summaries in retrieve_subgraph responses. LLM or template-based generation. |

### Anti-Features (Deliberately NOT Building)

Features that are commonly requested or seem appealing but would actively harm the system's design or dilute its value proposition.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Human-readable text syntax / source language** | Developers instinctively want to "read the code." Feels incomplete without it. | Undermines the core thesis. If there is text syntax, the graph becomes a compilation artifact (like an AST), not the source of truth. Text introduces parsing ambiguity, formatting debates, and a representation gap. The system explicitly stores programs as graphs, not text. | Graph visualization + NL query interface + node summaries. Humans observe and query; they do not write text. |
| **Built-in AI agent loop / orchestrator** | Seems natural to include the AI that drives the system. | Creates tight coupling to specific LLM providers. The system should be a general-purpose programmable graph store + compiler, not an AI product. External agents (Claude, GPT, open-source models) call the tool API. The system is model-agnostic by design. | Expose a clean, well-documented tool API (MCP-compatible). Let any LLM framework orchestrate agents externally. |
| **IDE / text editor integration** | Developers expect VS Code extensions, LSP support. | There is no text to edit. IDE integration for a non-textual representation would be a bespoke visualization tool, not a text editor plugin. The effort is better spent on the graph visualization and API. | Web-based graph visualization + API playground. Consider VS Code webview extension only after core is solid. |
| **Full graph CRDT for concurrent editing** | Academic elegance. "Real" distributed collaboration. | Graph CRDTs are an active research frontier (Kleppmann's tree CRDT is state of the art). Building a custom graph CRDT for a novel dual-layer representation is a multi-year research project. Premature for a v1 prototype. | Region-level locking with optimistic concurrency. Simpler, proven pattern. Upgrade to CRDT only if multi-datacenter collaboration becomes a real requirement. |
| **General-purpose graph query language (SPARQL, Cypher)** | Powerful, flexible querying. | Massive implementation effort. Users are AI agents, not humans writing queries. Agents call structured tool APIs, not query languages. A query language adds complexity without serving the primary user. | Structured API: `retrieve_subgraph` with typed filters. Expose graph traversal primitives through the tool API. Add NL query for human observers. |
| **Cloud/distributed deployment** | Scale! Multi-user! | Per PROJECT.md: single-machine prototype first. Distributed graph storage introduces consensus, partition tolerance, and replication complexity that dwarfs the core system. | SQLite embedded storage. Design storage interface to be swappable (per PROJECT.md constraints). Defer distribution. |
| **Real-time streaming compilation** | "Compile as you type" (edit). | The graph changes in discrete edits, not continuous streams. Incremental recompilation on edit is the right granularity. Streaming compilation implies continuous partial compilation which is not meaningful for a graph representation. | Incremental recompilation triggered by `verify_and_propagate`. Fast enough for interactive use without streaming complexity. |
| **Plugin/extension system** | Customizability for different domains. | Premature abstraction. Building a plugin API before the core is stable creates maintenance burden and design constraints. The system's abstractions will change as the design evolves. | Hard-code everything in v1. Extract extension points only when concrete extension needs emerge. The op node set can be extended by adding Rust code. |

## Feature Dependencies

```
[Type System]
    |
    +--requires--> [Typed Computational Graph Nodes]
    |                  |
    |                  +--requires--> [Data Flow Edges]
    |                  +--requires--> [Control Flow Edges]
    |
    +--requires--> [Basic Contract System: Type Checking]
                       |
                       +--enables--> [Full Contract System: Pre/Post/Invariants]
                       |                 |
                       |                 +--enables--> [Property-Based Testing from Contracts]
                       |                 +--enables--> [Multi-Agent Consistency Guarantees]
                       |
                       +--enables--> [AI Tool: verify_and_propagate]
                                        |
                                        +--enables--> [Bidirectional Layer Propagation]
                                        +--enables--> [Incremental Recompilation]

[Function/Module Boundaries]
    |
    +--enables--> [AI Tool: retrieve_subgraph]
    |                 |
    |                 +--enables--> [Semantic Embeddings on Nodes]
    |                 |                 |
    |                 |                 +--enables--> [NL Query Interface]
    |                 |
    |                 +--enables--> [Compact Node Summaries]
    |
    +--enables--> [LLVM IR Generation]
                      |
                      +--enables--> [Incremental Recompilation]

[Graph Persistence]
    |
    +--enables--> [Undo/Rollback]
    +--enables--> [Deterministic Graph Hashing]
                      |
                      +--enables--> [Incremental Recompilation]
                      +--enables--> [Content-Addressable Deduplication]

[Graph Interpreter]
    |
    +--enables--> [AI Tool: simulate]
    +--enables--> [Property-Based Testing from Contracts]

[AI Tool: propose_structured_edit]
    |
    +--requires--> [Type System]
    +--requires--> [Graph Persistence]
    +--enables--> [Multi-Agent Concurrent Editing]

[Dual-Layer Graph (Semantic + Executable)]
    +--requires--> [Type System]
    +--requires--> [Function/Module Boundaries]
    +--requires--> [Semantic Embeddings on Nodes]
    +--enables--> [Bidirectional Propagation]
    +--enables--> [NL Query Interface]
```

### Dependency Notes

- **Type System is foundational:** Nearly everything depends on it. Build first.
- **Full contracts require basic type checking:** You cannot check pre/post-conditions without first being able to type-check the graph.
- **Bidirectional propagation requires verify_and_propagate:** You cannot sync layers without a verification mechanism.
- **NL query requires embeddings:** Semantic search only works once embeddings are populated on nodes.
- **Incremental recompilation requires hashing + dirty tracking:** Both content-addressable identity and change detection are prerequisites.
- **Multi-agent concurrency requires contracts + undo:** Safety nets must exist before allowing concurrent modification.
- **Dual-layer graph is an overlay:** It requires the executable graph + semantic metadata to both exist before synchronization can work.

## MVP Definition

### Launch With (v1 Demo)

The v1 target is: "multiple AI agents collaborate to build a non-trivial program from a spec."

- [ ] **Type system** (scalars, aggregates, pointers, function types) -- foundation for everything
- [ ] **Typed computational graph nodes** (30+ ops) -- the executable representation
- [ ] **Data flow + control flow edges** -- the graph structure
- [ ] **Function/module boundaries** -- program decomposition
- [ ] **Graph persistence** (SQLite) -- programs survive process restart
- [ ] **AI tool API: propose_structured_edit** -- agents can modify the graph
- [ ] **AI tool API: retrieve_subgraph** -- agents can read the graph with focused context
- [ ] **AI tool API: verify_and_propagate** -- edits are validated
- [ ] **Basic contract system: type checking** -- minimum correctness guarantee
- [ ] **Graph interpreter** -- execute programs without LLVM compile cycle
- [ ] **LLVM IR generation** -- produce native binaries from the graph
- [ ] **Error messages with graph context** -- agents can diagnose failures
- [ ] **Undo/rollback** -- recover from bad edits

### Add After Validation (v1.x)

Features to add once the core loop (agent edits graph -> verifies -> compiles) is working.

- [ ] **Full contract system** (pre/post-conditions, invariants) -- when type checking alone is insufficient for complex programs
- [ ] **AI tool API: simulate** -- when agents need to test behavior before committing
- [ ] **AI tool API: optimize** -- when compiled output needs performance tuning
- [ ] **Deterministic graph hashing** -- when recompilation becomes a bottleneck
- [ ] **Incremental recompilation** -- when full recompilation is too slow for iteration
- [ ] **Semantic embeddings on nodes** -- when agents need semantic navigation beyond structural queries
- [ ] **Compact node summaries** -- when agent context windows become the bottleneck
- [ ] **Graph visualization (DAG viewer)** -- when humans need to observe agent activity
- [ ] **Multi-agent region locking** -- when single-agent demo is validated and concurrent agents are needed

### Future Consideration (v2+)

Features to defer until the core system is proven and the demo is compelling.

- [ ] **Dual-layer bidirectional propagation** -- the hardest feature; defer until executable layer is rock-solid
- [ ] **NL query interface** -- requires embeddings infrastructure; defer until semantic layer exists
- [ ] **Property-based testing from contracts** -- requires full contract system + interpreter maturity
- [ ] **Content-addressable deduplication** -- optimization; defer until graph sizes warrant it
- [ ] **AI-guided optimization** (evolutionary/MCTS) -- research-grade; defer until classical optimizations are complete
- [ ] **Multi-agent CRDT/OT** -- defer until region locking proves insufficient

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Type system | HIGH | HIGH | P1 |
| Typed computational graph nodes (30+ ops) | HIGH | HIGH | P1 |
| Data flow + control flow edges | HIGH | MEDIUM | P1 |
| Function/module boundaries | HIGH | MEDIUM | P1 |
| Graph persistence (SQLite) | HIGH | MEDIUM | P1 |
| AI tool: propose_structured_edit | HIGH | HIGH | P1 |
| AI tool: retrieve_subgraph | HIGH | MEDIUM | P1 |
| AI tool: verify_and_propagate | HIGH | HIGH | P1 |
| Basic type checking | HIGH | MEDIUM | P1 |
| Graph interpreter | HIGH | HIGH | P1 |
| LLVM IR generation | HIGH | HIGH | P1 |
| Error messages with context | MEDIUM | LOW | P1 |
| Undo/rollback | MEDIUM | MEDIUM | P1 |
| Full contract system | HIGH | HIGH | P2 |
| AI tool: simulate | MEDIUM | MEDIUM | P2 |
| AI tool: optimize | MEDIUM | HIGH | P2 |
| Deterministic hashing | MEDIUM | MEDIUM | P2 |
| Incremental recompilation | MEDIUM | HIGH | P2 |
| Semantic embeddings | MEDIUM | MEDIUM | P2 |
| Compact node summaries | MEDIUM | LOW | P2 |
| Graph visualization | MEDIUM | MEDIUM | P2 |
| Multi-agent region locking | HIGH | HIGH | P2 |
| Dual-layer bidirectional propagation | HIGH | VERY HIGH | P3 |
| NL query interface | MEDIUM | MEDIUM | P3 |
| Property-based testing | MEDIUM | HIGH | P3 |
| Content-addressable dedup | LOW | MEDIUM | P3 |
| AI-guided optimization (MCTS/evolutionary) | LOW | VERY HIGH | P3 |

**Priority key:**
- P1: Must have for v1 demo (agents build a program from spec)
- P2: Should have, add once core loop works
- P3: Future consideration, deferred to v2+

## Competitor/Adjacent Feature Analysis

| Feature | LLVM/MLIR | Unison | Dafny | AI Coding Agents (Cursor/Codex) | lmlang Approach |
|---------|-----------|--------|-------|-------------------------------|-----------------|
| Graph-based IR | Yes (SSA CFG + dataflow) | No (content-addressed AST) | No (text-based) | No (text files) | Yes, dual-layer graph is the program |
| Content-addressable code | No | Yes (SHA3 hash of AST) | No | No | Yes (hash of graph nodes) |
| Formal contracts | No (types only) | No | Yes (pre/post/invariants, SMT-verified) | No | Yes (graph-integrated contracts) |
| AI-native manipulation | No | No | Emerging (LLM-assisted spec gen) | Yes (text-based edit/generate) | Yes (structured graph API, not text) |
| Multi-level IR | Yes (MLIR dialects) | No | No | No | Yes (semantic + executable layers) |
| Incremental compilation | No (LLVM recompiles modules) | Yes (hash-based, perfect) | No | N/A | Yes (dirty node tracking) |
| Multi-agent concurrent editing | No | No | No | Yes (git worktrees in Codex) | Yes (region locking + verification) |
| NL query over program | No | No | No | Partial (semantic code search) | Yes (embeddings + graph relationships) |
| Visualization | No (external tools) | No (text UCM) | No | No | Yes (built-in DAG viewer) |
| Native compilation | Yes (LLVM backend) | Yes (native runtime) | Yes (compiles to C#/Go/Java/JS/Py) | N/A | Yes (graph -> LLVM IR -> native) |

## Sources

### Compiler Infrastructure & Graph IRs
- [ProGraML: Graph-based Program Representation](https://github.com/ChrisCummins/ProGraML) -- MEDIUM confidence, academic research
- [MLIR Introduction (Stephen Diehl)](https://www.stephendiehl.com/posts/mlir_introduction/) -- HIGH confidence, well-sourced technical overview
- [Sea of Nodes (Wikipedia)](https://en.wikipedia.org/wiki/Sea_of_nodes) -- HIGH confidence
- [V8 Leaving Sea of Nodes](https://v8.dev/blog/leaving-the-sea-of-nodes) -- HIGH confidence, official V8 blog
- [Cliff Click's Sea of Nodes IR](https://github.com/SeaOfNodes/Simple) -- HIGH confidence, original author

### AI Coding Agents
- [Agentic Coding Trends Report 2026 (Anthropic)](https://resources.anthropic.com/hubfs/2026%20Agentic%20Coding%20Trends%20Report.pdf) -- HIGH confidence, primary source
- [Multi-agent AI Workflows (InfoWorld)](https://www.infoworld.com/article/4035926/multi-agent-ai-workflows-the-next-evolution-of-ai-coding.html) -- MEDIUM confidence
- [Coding Agent Teams (DevOps.com)](https://devops.com/coding-agent-teams-the-next-frontier-in-ai-assisted-software-development/) -- MEDIUM confidence

### Formal Verification & Contracts
- [Dafny as Verification-Aware IL for Code Gen (POPL 2025)](https://popl25.sigplan.org/details/dafny-2025-papers/11/Dafny-as-Verification-Aware-Intermediate-Language-for-Code-Generation) -- HIGH confidence, academic venue
- [Vericoding Benchmark (POPL 2026)](https://popl26.sigplan.org/details/dafny-2026-papers/13/A-benchmark-for-vericoding-formally-verified-program-synthesis) -- HIGH confidence, academic venue
- [AI Will Make Formal Verification Mainstream (Kleppmann)](https://martin.kleppmann.com/2025/12/08/ai-formal-verification.html) -- MEDIUM confidence, expert opinion
- [Design by Contract (Wikipedia)](https://en.wikipedia.org/wiki/Design_by_contract) -- HIGH confidence

### Content-Addressable Code
- [Unison: The Big Idea](https://www.unison-lang.org/docs/the-big-idea/) -- HIGH confidence, official docs

### Concurrent Editing
- [CRDT Dictionary (Ian Duncan, 2025)](https://www.iankduncan.com/engineering/2025-11-27-crdt-dictionary/) -- MEDIUM confidence
- [Tree CRDT Move Operation (Kleppmann)](https://martin.kleppmann.com/papers/move-op.pdf) -- HIGH confidence, academic paper

### Incremental Compilation
- [Rust Incremental Compilation (rustc dev guide)](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html) -- HIGH confidence, official docs
- [Kotlin Incremental Compilation on Buck2 (Meta)](https://engineering.fb.com/2025/08/26/open-source/enabling-kotlin-incremental-compilation-on-buck2/) -- HIGH confidence, primary source

### AI Tool APIs & MCP
- [Model Context Protocol (Wikipedia)](https://en.wikipedia.org/wiki/Model_Context_Protocol) -- HIGH confidence
- [Structured Outputs (OpenAI)](https://developers.openai.com/api/docs/guides/structured-outputs/) -- HIGH confidence, official docs
- [Structured Outputs (Claude)](https://platform.claude.com/docs/en/build-with-claude/structured-outputs) -- HIGH confidence, official docs

### Optimization
- [Equality Saturation for Tensor Graph Superoptimization (TENSAT)](https://arxiv.org/abs/2101.01332) -- HIGH confidence, academic paper
- [MCTS + Equality Saturation (PACT 2024)](https://arxiv.org/abs/2410.05534) -- HIGH confidence, academic venue

### Semantic Code Search
- [Greptile: Codebases Are Hard to Search Semantically](https://www.greptile.com/blog/semantic-codebase-search) -- MEDIUM confidence
- [Code Context MCP Server](https://www.pulsemcp.com/servers/code-context) -- MEDIUM confidence

---
*Feature research for: AI-native programming system / graph-based program representation*
*Researched: 2026-02-17*
