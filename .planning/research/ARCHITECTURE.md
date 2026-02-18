# Architecture Research

**Domain:** AI-native graph-based programming system with LLVM compilation backend
**Researched:** 2026-02-17
**Confidence:** MEDIUM-HIGH

## Standard Architecture

### System Overview

```
                           External AI Agents (LLM tool-calling)
                                       |
                                       v
┌──────────────────────────────────────────────────────────────────────┐
│                         Agent Tool API Layer                         │
│  retrieve_subgraph | propose_edit | verify | simulate | optimize     │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────────────┐    ┌──────────────────────────────┐    │
│  │  Layer 1: Semantic KG    │<──>│  Layer 2: Computational Graph│    │
│  │                          │    │                              │    │
│  │  Modules, Functions,     │    │  Typed ops DAG + cycles:     │    │
│  │  Types, Specs, Tests,    │    │  add, mul, branch, loop,     │    │
│  │  Docs, Embeddings        │    │  call, load/store, alloc     │    │
│  │                          │    │                              │    │
│  │  Edges: calls, depends,  │    │  Edges: data deps +         │    │
│  │  implements, test_for    │    │  control flow                │    │
│  └────────────┬─────────────┘    └──────────────┬───────────────┘    │
│               │   Bidirectional Propagation      │                   │
│               └──────────────┬───────────────────┘                   │
│                              v                                       │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │                   Contract System                             │    │
│  │  Type checking | Pre/Post conditions | Invariants | PBT       │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                              │                                       │
├──────────────────────────────┼───────────────────────────────────────┤
│                              v                                       │
│           ┌─────────────────────────────────────┐                    │
│           │        Graph Interpreter             │                   │
│           │   (development execution)            │                   │
│           └─────────────────────────────────────┘                    │
│                              │                                       │
│           ┌─────────────────────────────────────┐                    │
│           │     LLVM Compilation Pipeline        │                   │
│           │                                     │                    │
│           │  Graph → IR Lowering → LLVM IR →    │                   │
│           │  Optimization → Object → Binary     │                    │
│           └─────────────────────────────────────┘                    │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│                       Storage Layer                                  │
│  ┌──────────────┐  ┌──────────────────┐  ┌───────────────────┐      │
│  │ Graph Store   │  │ Embedding Store  │  │ Compilation Cache │      │
│  │ (SQLite)      │  │ (vectors)        │  │ (dirty tracking)  │      │
│  └──────────────┘  └──────────────────┘  └───────────────────┘      │
└──────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| Agent Tool API | Structured interface for AI agents to query and mutate the program graph | JSON-RPC or HTTP API with typed request/response schemas; validates all mutations before applying |
| Semantic Knowledge Graph (Layer 1) | Stores high-level program intent: modules, functions, types, specs, relationships | petgraph `StableGraph<SemanticNode, SemanticEdge>` with typed node/edge variants |
| Computational Graph (Layer 2) | Stores executable operations as typed DAG + cycles (loops) | petgraph `StableGraph<OpNode, DataFlowEdge>` with SSA-like value numbering |
| Propagation Engine | Bidirectional sync between Layer 1 and Layer 2 | Event-driven with dirty tracking; queued propagation to avoid infinite loops |
| Contract System | Type checking, pre/post-conditions, invariants, property-based testing | Trait-based verifier pipeline; contracts stored as graph nodes linked to their subjects |
| Graph Interpreter | Development-time execution of computational graph without compilation | Topological-sort walker over the computational graph; value stack per execution frame |
| LLVM Codegen Pipeline | Translates computational graph to LLVM IR, optimizes, emits native binary | inkwell-based; topological walk of computational graph emitting LLVM instructions |
| Storage Layer | Persistent graph storage, embedding vectors, compilation artifacts | SQLite with graph schema (nodes table, edges table, embeddings table) via rusqlite |
| Dirty Tracker | Tracks which nodes changed for incremental recompilation and propagation | Red-green coloring algorithm on the dependency graph (inspired by rustc) |
| Visualization | Human-observable DAG view of the program graph | DOT export via petgraph's built-in Graphviz support; web viewer in later phases |
| NL Query Interface | Natural language queries over the program graph | Embedding similarity search + graph traversal; wraps graph store queries |

## Recommended Project Structure

```
lmlang/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── lmlang-core/           # Graph data structures, node/edge types, core traits
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── semantic/      # Layer 1 types: Module, Function, Type, Spec nodes
│   │   │   ├── compute/       # Layer 2 types: Op nodes, data flow edges
│   │   │   ├── graph.rs       # Unified dual-graph container
│   │   │   ├── types.rs       # Type system definitions
│   │   │   └── contracts.rs   # Contract types (pre/post/invariant)
│   │   └── Cargo.toml
│   │
│   ├── lmlang-store/          # Persistence layer
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── sqlite.rs      # SQLite-backed graph store
│   │   │   ├── memory.rs      # In-memory store (for tests/dev)
│   │   │   ├── embeddings.rs  # Vector storage for embeddings
│   │   │   └── traits.rs      # GraphStore trait (swappable backend)
│   │   └── Cargo.toml
│   │
│   ├── lmlang-propagation/    # Bidirectional layer synchronization
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── up.rs          # Layer 2 → Layer 1 (summary bubbling)
│   │   │   ├── down.rs        # Layer 1 → Layer 2 (expansion)
│   │   │   ├── dirty.rs       # Dirty tracking / change detection
│   │   │   └── queue.rs       # Propagation work queue
│   │   └── Cargo.toml
│   │
│   ├── lmlang-verify/         # Contract verification
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── typecheck.rs   # Type checking over the graph
│   │   │   ├── contracts.rs   # Pre/post-condition verification
│   │   │   ├── invariants.rs  # Structural invariant checking
│   │   │   └── pbt.rs         # Property-based testing harness
│   │   └── Cargo.toml
│   │
│   ├── lmlang-interp/         # Graph interpreter
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── eval.rs        # Op node evaluation
│   │   │   ├── frame.rs       # Execution frames / call stack
│   │   │   └── scheduler.rs   # Topological execution ordering
│   │   └── Cargo.toml
│   │
│   ├── lmlang-codegen/        # LLVM compilation pipeline
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── lower.rs       # Graph → LLVM IR lowering
│   │   │   ├── types.rs       # Graph types → LLVM types mapping
│   │   │   ├── builder.rs     # Wrapper around inkwell builder
│   │   │   ├── intrinsics.rs  # Built-in operations (math, I/O)
│   │   │   ├── optimize.rs    # LLVM optimization pass configuration
│   │   │   ├── target.rs      # Target machine configuration
│   │   │   └── incremental.rs # Dirty-node-aware recompilation
│   │   └── Cargo.toml
│   │
│   ├── lmlang-api/            # External tool API for AI agents
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── server.rs      # JSON-RPC / HTTP server
│   │   │   ├── handlers.rs    # Tool implementations
│   │   │   ├── schema.rs      # Request/response types
│   │   │   └── auth.rs        # Agent identity / session management
│   │   └── Cargo.toml
│   │
│   └── lmlang-viz/            # Visualization and NL query
│       ├── src/
│       │   ├── lib.rs
│       │   ├── dot.rs         # DOT/Graphviz export
│       │   ├── query.rs       # NL query interface
│       │   └── summary.rs     # Human-readable program summaries
│       └── Cargo.toml
│
├── src/
│   └── main.rs                # CLI entrypoint (drives API server, compilation, etc.)
│
└── tests/
    ├── integration/           # Cross-crate integration tests
    └── fixtures/              # Test graph fixtures
```

### Structure Rationale

- **`lmlang-core/`:** Zero-dependency foundation crate. Every other crate depends on it. Defines the graph data structures, node/edge type enums, the type system, and core traits. This must be stable early because everything else builds on it.
- **`lmlang-store/`:** Separated from core so the storage backend is swappable. Defines a `GraphStore` trait that SQLite, in-memory, and future graph DB backends implement. Core types never know about storage details.
- **`lmlang-propagation/`:** Isolated because bidirectional sync is the single hardest correctness problem. Dedicated crate means it can be exhaustively tested in isolation. Depends only on `core`.
- **`lmlang-verify/`:** Separate from propagation because verification is logically distinct from synchronization. Contracts verify invariants; propagation maintains consistency. Both operate on the graph but for different purposes.
- **`lmlang-interp/`:** Development-time execution. Completely independent of the LLVM pipeline. Shares only `core` types. This allows rapid iteration without LLVM build overhead.
- **`lmlang-codegen/`:** The LLVM dependency (inkwell) is heavy. Isolating it in its own crate means the rest of the system compiles fast. Only this crate links against LLVM.
- **`lmlang-api/`:** The external interface. Depends on all other crates to orchestrate operations. This is the "shell" -- everything else is a "core" library.
- **`lmlang-viz/`:** Optional visualization. Low priority, can be built last.

## Architectural Patterns

### Pattern 1: Dual-Graph with Cross-References

**What:** The two layers (Semantic KG and Computational Graph) are stored as separate `StableGraph` instances within a unified container, with explicit cross-reference edges connecting them. Each semantic function node points to its computational subgraph root; each computational op node points back to its owning semantic context.

**When to use:** Always -- this is the core data model.

**Trade-offs:**
- PRO: Each layer can be traversed independently with clean type safety
- PRO: `StableGraph` preserves node indices across mutations (critical for compiler passes)
- PRO: Cross-references are just typed edges, no magic
- CON: Must maintain cross-reference consistency manually
- CON: Slightly more complex than a single unified graph

**Example:**
```rust
use petgraph::stable_graph::StableGraph;

/// The unified program graph containing both layers
pub struct ProgramGraph {
    /// Layer 1: Semantic Knowledge Graph
    pub semantic: StableGraph<SemanticNode, SemanticEdge>,
    /// Layer 2: Executable Computational Graph
    pub compute: StableGraph<OpNode, FlowEdge>,
    /// Cross-references: semantic node index -> compute node indices
    pub sem_to_compute: HashMap<SemanticIdx, Vec<ComputeIdx>>,
    /// Reverse: compute node index -> owning semantic node
    pub compute_to_sem: HashMap<ComputeIdx, SemanticIdx>,
}

/// Semantic node types (Layer 1)
pub enum SemanticNode {
    Module { name: String, summary: String },
    Function { name: String, signature: FnSignature, embedding: Option<Vec<f32>> },
    DataType { name: String, definition: TypeDef },
    Spec { description: String, contracts: Vec<ContractId> },
    Test { name: String, target: SemanticIdx },
}

/// Computational op nodes (Layer 2)
pub enum OpNode {
    Const { value: TypedValue },
    Add { typ: NumericType },
    Mul { typ: NumericType },
    Load { typ: ValueType, address: MemoryRef },
    Store { typ: ValueType, address: MemoryRef },
    Branch { condition_input: PortId },
    Loop { body_entry: ComputeIdx },
    Call { target: SemanticIdx },  // cross-layer reference
    Alloc { typ: ValueType, size: usize },
    Return { typ: ValueType },
    Phi { typ: ValueType },  // SSA merge point
}
```

**Why `StableGraph` over `Graph`:** During optimization passes and propagation, nodes are frequently removed (dead code elimination), replaced (constant folding), or rewired (edge redirects). `StableGraph` preserves `NodeIndex` values after removals, which is essential because indices are stored in cross-references, the dirty tracker, and the compilation cache. With `Graph`, every removal invalidates downstream indices -- a correctness nightmare.

### Pattern 2: Event-Driven Propagation with Dirty Queues

**What:** Changes to either layer enqueue dirty notifications rather than propagating immediately. A propagation engine processes the queue in batches, computing the transitive closure of affected nodes, then applying updates. This avoids infinite loops from bidirectional feedback and enables batched verification.

**When to use:** Every mutation that crosses layer boundaries.

**Trade-offs:**
- PRO: No infinite recursion from bidirectional propagation
- PRO: Can batch multiple edits before propagating (important for AI agent workflows that make several related changes)
- PRO: Propagation is inspectable and debuggable (you can see the queue)
- CON: Eventual consistency within a mutation batch (not immediate)
- CON: Queue management adds complexity

**Example:**
```rust
pub enum DirtyEvent {
    /// A semantic node was modified (Layer 1 change)
    SemanticChanged { node: SemanticIdx, change: ChangeKind },
    /// A compute node was modified (Layer 2 change)
    ComputeChanged { node: ComputeIdx, change: ChangeKind },
}

pub enum ChangeKind {
    Added,
    Modified,
    Removed,
    EdgeAdded { target: NodeRef },
    EdgeRemoved { target: NodeRef },
}

pub struct PropagationEngine {
    queue: VecDeque<DirtyEvent>,
    /// Tracks which nodes are already queued to avoid duplicates
    in_queue: HashSet<NodeRef>,
    /// Cycle breaker: nodes currently being propagated
    propagating: HashSet<NodeRef>,
}

impl PropagationEngine {
    /// Enqueue a change notification
    pub fn notify(&mut self, event: DirtyEvent) {
        let node_ref = event.node_ref();
        if !self.in_queue.contains(&node_ref) && !self.propagating.contains(&node_ref) {
            self.in_queue.insert(node_ref);
            self.queue.push_back(event);
        }
    }

    /// Process all queued events, propagating changes across layers
    pub fn flush(&mut self, graph: &mut ProgramGraph) -> PropagationResult {
        while let Some(event) = self.queue.pop_front() {
            let node_ref = event.node_ref();
            self.in_queue.remove(&node_ref);
            self.propagating.insert(node_ref);

            match event {
                DirtyEvent::SemanticChanged { node, change } => {
                    // Layer 1 -> Layer 2: expand semantic changes into compute graph
                    self.propagate_down(graph, node, change);
                }
                DirtyEvent::ComputeChanged { node, change } => {
                    // Layer 2 -> Layer 1: bubble up summaries, update signatures
                    self.propagate_up(graph, node, change);
                }
            }

            self.propagating.remove(&node_ref);
        }
        PropagationResult::Ok
    }
}
```

### Pattern 3: Topological Walk for Code Generation

**What:** LLVM IR emission traverses the computational graph in topological order (data dependencies resolved before dependents). For acyclic subgraphs, this is a standard toposort. For loops, the graph is decomposed into Strongly Connected Components (SCCs) first, and each SCC is emitted as LLVM basic blocks with phi nodes at loop headers.

**When to use:** During compilation (graph -> LLVM IR).

**Trade-offs:**
- PRO: Correct by construction -- data dependencies are always emitted before their uses
- PRO: Matches LLVM's SSA form naturally
- PRO: petgraph provides `toposort()` and `tarjan_scc()` out of the box
- CON: Loop handling requires special SCC decomposition
- CON: Must handle unreachable nodes (dead code) during traversal

**Example:**
```rust
use petgraph::algo::{toposort, tarjan_scc};

pub struct LlvmCodegen<'ctx> {
    context: &'ctx inkwell::context::Context,
    module: inkwell::module::Module<'ctx>,
    builder: inkwell::builder::Builder<'ctx>,
    /// Maps compute graph node indices to LLVM values
    values: HashMap<ComputeIdx, inkwell::values::BasicValueEnum<'ctx>>,
}

impl<'ctx> LlvmCodegen<'ctx> {
    pub fn compile_function(
        &mut self,
        graph: &ProgramGraph,
        func_node: SemanticIdx,
    ) -> Result<inkwell::values::FunctionValue<'ctx>> {
        // 1. Get the computational subgraph for this function
        let subgraph = graph.subgraph_for_function(func_node);

        // 2. Decompose into SCCs to identify loops
        let sccs = tarjan_scc(&subgraph);

        // 3. Create LLVM function and entry block
        let fn_type = self.map_signature(graph.function_signature(func_node));
        let function = self.module.add_function(
            &graph.function_name(func_node),
            fn_type,
            None,
        );
        let entry_block = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry_block);

        // 4. Emit each SCC (single nodes = straight-line, multi-node = loop)
        for scc in sccs.iter().rev() {  // reverse for dependency order
            if scc.len() == 1 {
                self.emit_node(&subgraph, scc[0]);
            } else {
                self.emit_loop(&subgraph, function, scc);
            }
        }

        Ok(function)
    }

    fn emit_node(&mut self, graph: &ComputeSubgraph, idx: ComputeIdx) {
        let node = &graph[idx];
        let llvm_val = match node {
            OpNode::Add { typ } => {
                let (lhs, rhs) = self.get_binary_inputs(graph, idx);
                self.builder.build_int_add(lhs, rhs, "add").unwrap()
            }
            OpNode::Const { value } => {
                self.map_constant(value)
            }
            // ... other node types
            _ => todo!(),
        };
        self.values.insert(idx, llvm_val.into());
    }
}
```

### Pattern 4: Red-Green Incremental Recompilation

**What:** Inspired by rustc's incremental compilation, each node in the computational graph has a content hash. When a node changes, it is marked "red". Downstream dependents are checked: if their inputs are all "green" (unchanged), they skip recompilation. If any input is "red", re-evaluate and compare the output hash to decide the new color. Only truly changed nodes trigger LLVM IR re-emission.

**When to use:** During recompilation after graph mutations.

**Trade-offs:**
- PRO: Avoids recompiling the entire program after small changes
- PRO: Hash comparison is cheap -- most nodes stay green
- PRO: Naturally integrates with the dirty tracking from propagation
- CON: Must maintain a persistent content hash per node
- CON: LLVM module-level linking means granularity is limited (recompile per-function, not per-node)

**Example:**
```rust
pub enum NodeColor {
    /// Node result is unchanged from previous compilation
    Green,
    /// Node result has changed, dependents must re-evaluate
    Red,
    /// Not yet evaluated in this compilation cycle
    Unknown,
}

pub struct IncrementalTracker {
    /// Content hash of each node from the previous compilation
    prev_hashes: HashMap<ComputeIdx, u64>,
    /// Current cycle's node colors
    colors: HashMap<ComputeIdx, NodeColor>,
}

impl IncrementalTracker {
    /// Determine if a node needs recompilation
    pub fn try_mark_green(
        &mut self,
        graph: &ProgramGraph,
        idx: ComputeIdx,
    ) -> NodeColor {
        if let Some(color) = self.colors.get(&idx) {
            return *color;
        }

        // Check all input dependencies
        let all_inputs_green = graph.compute_inputs(idx)
            .all(|input| self.try_mark_green(graph, input) == NodeColor::Green);

        let color = if all_inputs_green {
            // All inputs unchanged -> this node is green without re-evaluation
            NodeColor::Green
        } else {
            // Some input changed -> recompute hash and compare
            let new_hash = graph.content_hash(idx);
            if self.prev_hashes.get(&idx) == Some(&new_hash) {
                NodeColor::Green  // Hash matches despite input changes
            } else {
                NodeColor::Red
            }
        };

        self.colors.insert(idx, color);
        color
    }
}
```

## Data Flow

### Primary Data Flows

```
AI Agent Request (e.g., "add function that sorts a list")
    |
    v
[Agent Tool API] ──validates──> [Schema Validator]
    |
    v
[propose_structured_edit]
    |
    ├──> [Semantic KG] : Create Function node, Spec node, Type nodes
    |         |
    |         v
    |    [Propagation Engine] : Queue Layer1->Layer2 dirty events
    |         |
    |         v
    |    [Computational Graph] : Expand function into op-node subgraph
    |         |
    |         v
    |    [Propagation Engine] : Queue Layer2->Layer1 (update summary)
    |         |
    |         v
    |    [Semantic KG] : Update function summary, embedding
    |
    ├──> [Contract Verifier] : Type-check the new subgraph
    |         |
    |         ├── Pass ──> [Dirty Tracker] : Mark affected nodes red
    |         |                  |
    |         |                  v
    |         |            [Incremental Codegen] : Recompile red functions
    |         |                  |
    |         |                  v
    |         |            [LLVM Pipeline] : Emit IR, optimize, link
    |         |
    |         └── Fail ──> [Agent Tool API] : Return verification errors
    |
    v
[Agent Tool API] : Return success + subgraph summary to AI agent
```

### Compilation Pipeline Detail

```
[Computational Graph (per-function subgraph)]
    |
    v
[SCC Decomposition]  ── identifies loops
    |
    v
[Topological Sort]  ── establishes emission order
    |
    v
[Type Mapping]  ── graph types -> LLVM types
    |                (i32 -> IntType, f64 -> FloatType, struct -> StructType)
    |
    v
[Node Emission]  ── for each node in topo order:
    |                OpNode::Add -> builder.build_int_add()
    |                OpNode::Branch -> builder.build_conditional_branch()
    |                OpNode::Loop -> emit loop header BB + phi + body BBs
    |                OpNode::Call -> builder.build_call()
    |                OpNode::Alloc -> builder.build_alloca()
    |
    v
[Contract Injection]  ── pre-conditions become entry-block assertions
    |                     post-conditions become return-block assertions
    |                     invariants become loop-header/exit assertions
    |                     (controlled by build profile: debug=on, release=off)
    |
    v
[LLVM Module]
    |
    v
[LLVM Optimization Passes]  ── OptimizationLevel::Default or Aggressive
    |
    v
[Target Machine]  ── native or cross-compile target triple
    |
    v
[Object File]  ── .o file
    |
    v
[Linker]  ── system linker (ld/lld) produces final binary
```

### How Dual-Layer Synchronization Works

The bidirectional propagation between Layer 1 (Semantic) and Layer 2 (Computational) follows a **causal consistency** model, not strict immediate consistency. This is a deliberate design choice because:

1. AI agents make multiple related edits in a batch (e.g., "add function + its spec + its test")
2. Immediate propagation after each edit would trigger redundant intermediate states
3. The agent should control when propagation happens (explicit `verify_and_propagate` call)

**Downward propagation (Semantic -> Computational):**
- Adding a new function node with a signature creates a skeleton computational subgraph (entry/return nodes, parameter loads)
- Modifying a function's signature propagates type changes to the computational graph's input/output nodes
- Adding a spec or contract creates assertion nodes in the computational graph
- Deleting a semantic node removes its entire computational subgraph

**Upward propagation (Computational -> Semantic):**
- Modifying op nodes updates the parent function's summary text and embedding
- Adding/removing edges in the computational graph updates the semantic `calls` and `data_flows_to` edges
- Optimization passes that change the computational graph update semantic-level complexity metrics
- Type inference results from the computational graph flow back to semantic type annotations

**Cycle breaking:** The propagation engine uses a "currently propagating" set. If propagation would cause a node that is already being propagated to re-enter the queue, it is skipped. After the current flush completes, a second pass can run if needed, but in practice one pass suffices because the two layers contain non-overlapping information (intent vs. implementation).

## Scaling Considerations

| Scale | Architecture Adjustments |
|-------|--------------------------|
| Small programs (<1K nodes) | Single in-memory `ProgramGraph`. No incremental compilation needed. Interpret everything. |
| Medium programs (1K-100K nodes) | SQLite-backed persistence becomes important. Incremental compilation per-function. Subgraph loading (don't hold full graph in memory). |
| Large programs (100K+ nodes) | Swap to a graph database (Neo4j/Memgraph) for the storage layer. Parallel LLVM codegen units (one per module). Lazy embedding computation. |

### Scaling Priorities

1. **First bottleneck: Full graph in memory.** For programs beyond ~50K nodes, the in-memory `StableGraph` will strain memory. Mitigation: implement a "virtual graph" that loads subgraphs from SQLite on demand, using LRU eviction. The `GraphStore` trait abstraction makes this non-breaking.

2. **Second bottleneck: LLVM compilation time.** LLVM is not fast. Even with incremental compilation, recompiling large functions is slow. Mitigation: codegen unit parallelism (already native to LLVM), function-level caching, and the graph interpreter for development-time execution bypassing LLVM entirely.

3. **Third bottleneck: Propagation fan-out.** A change to a widely-depended-upon type could dirty half the graph. Mitigation: propagation depth limits with explicit agent approval for cascading changes, and coarse-grained change detection (hash-based, not diff-based) to detect when propagation stabilizes early.

## Anti-Patterns

### Anti-Pattern 1: Single Unified Graph for Both Layers

**What people do:** Represent semantic and computational nodes in one giant heterogeneous graph with edge types distinguishing the layers.
**Why it's wrong:** Traversal algorithms must constantly filter by layer, type safety is lost (can accidentally create a `calls` edge from an `Add` op to a `Module`), and optimization passes that should only touch one layer become complex.
**Do this instead:** Two separate `StableGraph` instances with explicit cross-references. Each graph is homogeneous within its layer and can be traversed with clean type-safe algorithms.

### Anti-Pattern 2: Immediate Bidirectional Propagation

**What people do:** Every mutation instantly triggers propagation to the other layer, which instantly triggers propagation back.
**Why it's wrong:** Creates infinite recursion or requires complex re-entrancy guards. Also generates many intermediate states during batch edits that are immediately obsoleted.
**Do this instead:** Queue-based propagation with explicit flush. AI agent makes all edits, then calls `verify_and_propagate` to trigger one propagation + verification pass.

### Anti-Pattern 3: Sea of Nodes IR for the Computational Graph

**What people do:** Use a pure sea-of-nodes representation where control flow and data flow are unified, with scheduling deferred to code generation.
**Why it's wrong:** V8's decade of experience with Sea of Nodes (Turbofan) ended with them abandoning it for Turboshaft (a CFG-based IR). The problems: difficult to debug, 3x more cache misses, compile times 2x slower, and effectful operations (memory loads/stores) cannot actually float freely. The theoretical optimization benefits rarely materialize for languages with abundant side effects.
**Do this instead:** Use a **CFG-skeleton with floating pure nodes** approach (similar to Cranelift's aegraph). The computational graph has explicit basic blocks and control flow edges, but pure arithmetic/logic nodes can be reordered freely within their dominator region. This gets 80% of the optimization benefit with 20% of the complexity.

### Anti-Pattern 4: Storing LLVM IR in the Graph

**What people do:** Persist generated LLVM IR text alongside the graph nodes, treating it as part of the program representation.
**Why it's wrong:** LLVM IR is an output artifact, not a source of truth. It becomes stale the moment the graph changes. Storing it couples the graph format to a specific LLVM version.
**Do this instead:** Treat LLVM IR as a derived, cacheable artifact. Store only content hashes for incremental compilation. Regenerate IR from the computational graph whenever needed.

### Anti-Pattern 5: Tight Coupling Between Interpreter and Codegen

**What people do:** Share execution logic between the interpreter and the LLVM codegen, trying to avoid duplication.
**Why it's wrong:** The interpreter executes nodes dynamically (evaluating values at runtime), while codegen emits static instructions (building an LLVM function). Their control flow is fundamentally different. Coupling them makes both harder to change.
**Do this instead:** Both the interpreter and codegen share the same `OpNode` type definitions from `lmlang-core`, but implement execution/emission independently. The op node type enum is the shared contract; the execution strategies diverge.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| LLVM (via inkwell) | Compile-time dependency; `lmlang-codegen` is the only crate that links it | Pin to a specific LLVM version via inkwell feature flag (e.g., `llvm18-0`). LLVM version bumps are a coordinated event. |
| SQLite (via rusqlite) | `lmlang-store` dependency; behind `GraphStore` trait | Use WAL mode for concurrent reads. Schema migrations via embedded SQL. |
| System linker (ld/lld) | Invoked as subprocess after LLVM emits object files | Detect available linker at build time. Prefer `lld` for speed. |
| AI agents (external) | Connect via Agent Tool API (HTTP/JSON-RPC) | Stateless request/response. Agent sessions tracked server-side. |
| Embedding model (external) | Called during propagation to compute node embeddings | Optional; system works without embeddings. API adapter pattern. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| core <-> store | `GraphStore` trait with `load/save/query` methods | Core types are serializable (serde). Store handles persistence format. |
| core <-> propagation | Propagation reads/writes `ProgramGraph` directly | Propagation engine takes `&mut ProgramGraph`. No message passing -- direct mutation for performance. |
| core <-> verify | Verifier takes `&ProgramGraph` (read-only) | Returns `Vec<VerificationError>`. Never mutates the graph. |
| core <-> codegen | Codegen takes `&ProgramGraph` (read-only) + `IncrementalTracker` | Returns compiled object files. Never mutates the graph. |
| core <-> interp | Interpreter takes `&ProgramGraph` + runtime inputs | Returns execution result. Never mutates the graph. |
| api <-> everything | API layer orchestrates: validate -> mutate -> propagate -> verify -> respond | This is the only crate with mutable access coordination. Uses a `RwLock<ProgramGraph>` or similar. |

## Build Order (Critical Path)

The crate dependency structure dictates the build order for the project:

```
Phase 1: Foundation
    lmlang-core           (graph types, node/edge enums, type system)
         |
Phase 2: Storage + Interpreter
    lmlang-store          (persistence, depends on core)
    lmlang-interp         (interpreter, depends on core) -- can run in parallel with store
         |
Phase 3: Verification + Propagation
    lmlang-verify         (contract checking, depends on core)
    lmlang-propagation    (layer sync, depends on core)
         |
Phase 4: Compilation
    lmlang-codegen        (LLVM pipeline, depends on core + verify + propagation)
         |
Phase 5: External Interface
    lmlang-api            (agent API, depends on all above)
    lmlang-viz            (visualization, depends on core + store)
```

**Rationale for this order:**
1. **Core first** because everything depends on it. Get the data model right.
2. **Store + Interpreter in parallel** because they are independent. The interpreter enables testing graph execution without LLVM. The store enables persistence.
3. **Verify + Propagation** depend on having a stable graph model. Propagation is the hardest correctness problem and benefits from the interpreter being available for testing.
4. **Codegen last** among the core crates because it has the heaviest dependency (LLVM) and benefits from all other components being stable. The interpreter provides a reference implementation to test codegen correctness against.
5. **API and Viz** are the shell -- they compose everything else. Building them last means all internal APIs are settled.

## Sources

- [petgraph GitHub repository](https://github.com/petgraph/petgraph) -- graph data structures for Rust (HIGH confidence)
- [petgraph `StableGraph` documentation](https://docs.rs/petgraph/latest/petgraph/stable_graph/struct.StableGraph.html) -- index stability semantics (HIGH confidence)
- [inkwell GitHub repository](https://github.com/TheDan64/inkwell) -- Rust LLVM bindings (HIGH confidence)
- [inkwell API documentation](https://thedan64.github.io/inkwell/) -- Context/Module/Builder API (HIGH confidence)
- [Rust compiler code generation guide](https://rustc-dev-guide.rust-lang.org/backend/codegen.html) -- MIR->LLVM IR pipeline (HIGH confidence)
- [Rust incremental compilation guide](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html) -- red-green algorithm (HIGH confidence)
- [V8 blog: Leaving the Sea of Nodes](https://v8.dev/blog/leaving-the-sea-of-nodes) -- cautionary tale for graph IR design (HIGH confidence)
- [Glow: Graph Lowering Compiler Techniques](https://arxiv.org/abs/1805.00907) -- two-level graph IR architecture (HIGH confidence)
- [MLIR introduction by Stephen Diehl](https://www.stephendiehl.com/posts/mlir_introduction/) -- multi-level IR patterns (MEDIUM confidence)
- [ProGraML: Graph-based Program Representation](https://arxiv.org/abs/2003.10536) -- multi-relation program graphs (MEDIUM confidence)
- [SeaOfNodes/Simple tutorial](https://github.com/SeaOfNodes/Simple) -- sea of nodes implementation patterns (MEDIUM confidence)
- [Cranelift architecture](https://cranelift.dev/) -- CFG-based IR alternative to LLVM, e-graph optimization (MEDIUM confidence)
- [Create Your Own Programming Language with Rust](https://createlang.rs/01_calculator/basic_llvm.html) -- inkwell usage patterns (MEDIUM confidence)
- [Neo4j codebase knowledge graph](https://neo4j.com/blog/developer/codebase-knowledge-graph/) -- code knowledge graph architecture (LOW confidence)
- [Bidirectional model synchronization taxonomy](https://www.sciencedirect.com/science/article/abs/pii/S016412121500120X) -- formal bidirectional sync patterns (LOW confidence)

---
*Architecture research for: lmlang -- AI-native graph-based programming system*
*Researched: 2026-02-17*
