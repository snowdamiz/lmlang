# Pitfalls Research

**Domain:** AI-native programming system (graph-based IR, LLVM compilation, multi-agent editing)
**Researched:** 2026-02-17
**Confidence:** HIGH (LLVM/graph IR/CRDT pitfalls well-documented), MEDIUM (dual-layer sync, contract system)

---

## Critical Pitfalls

Mistakes that cause rewrites or major issues.

### Pitfall 1: Bidirectional Layer Propagation Becomes an Infinite Loop or Inconsistency Engine

**What goes wrong:**
The dual-layer architecture (semantic knowledge graph + executable computational graph) requires bidirectional synchronization: editing the semantic layer expands into executable subgraphs, and low-level optimizations bubble up summaries and contracts. In practice, this creates echo loops where a change to Layer 1 triggers a Layer 2 update, which triggers a Layer 1 update, ad infinitum. Even if you add cycle detection, the system can settle into inconsistent states where the two layers disagree about program semantics because the propagation logic is order-dependent.

**Why it happens:**
Bidirectional sync between two data models is a fundamentally hard distributed systems problem. The two layers have different granularity (one semantic concept may map to dozens of op nodes), and the mapping is not always bijective. A single high-level change may have multiple valid lowerings, and a single low-level optimization may have multiple valid semantic interpretations. When both directions are "live," conflict resolution becomes combinatorially complex.

**How to avoid:**
Designate one layer as the source of truth at any given moment. Use a "propose-validate-commit" protocol: changes are proposed to one layer, the system computes the derived state for the other layer, validates consistency, and commits atomically. Never allow simultaneous unsynchronized writes to both layers. Treat the semantic layer as the authoritative representation during normal editing, and only allow the executable layer to be authoritative during optimization passes (with explicit mode switching). Version both layers together with a single monotonic sequence number.

**Warning signs:**
- Tests pass when editing one layer but fail when both layers are active
- Non-deterministic test results depending on propagation order
- The team starts adding "stabilization loops" that run propagation multiple times until convergence
- Debugging requires tracing through multiple propagation rounds to understand a single edit

**Phase to address:**
Phase 1 (Core Graph). This is foundational. The propagation protocol must be designed before any features are built on top. Deferring this creates a rewrite when the naive bidirectional approach fails at scale.

---

### Pitfall 2: Graph-to-LLVM-IR Lowering Impedance Mismatch

**What goes wrong:**
The custom graph representation (DAG + cycles for loops, typed op nodes, data + control flow edges) does not map cleanly to LLVM IR's expectations. LLVM IR is SSA-form with basic blocks, phi nodes, explicit control flow graphs, and specific calling conventions. The graph representation is more like a "sea of nodes" where operations float freely constrained only by data/control dependencies. Converting this to LLVM IR requires an explicit scheduling phase (assigning operations to basic blocks and ordering within blocks) that is itself a compiler-quality optimization problem. Getting this wrong produces correct but catastrophically slow code, or worse, miscompilations.

**Why it happens:**
LLVM IR looks simple on the surface but encodes subtle invariants: SSA dominance, phi node placement, memory model semantics (the `undef`/`poison` value system is notoriously tricky), and ABI-specific details that LLVM does not abstract away. The "sea of nodes" to basic-block conversion requires Global Code Motion (assigning nodes to regions) followed by local scheduling. Most custom compiler projects underestimate this because tutorials show trivial examples where the mapping is obvious. Real programs with loops, conditionals, memory operations, and function calls expose all the corner cases at once.

Additionally, LLVM's own IR has known design issues (per Nikita Popov's "LLVM: The Bad Parts" analysis, Jan 2026): constraint encoding is split across poison flags, metadata, attributes, and assumes -- each with different loss/retention semantics. Custom compilers must navigate these correctly or suffer from misoptimizations in LLVM's passes.

**How to avoid:**
Do NOT attempt to emit LLVM IR directly from the graph. Build an explicit intermediate step: graph -> scheduled linear IR -> LLVM IR. The scheduled linear IR should be a simple basic-block + instruction list format that maps 1:1 to LLVM IR constructs. Use inkwell's builder API to emit from this linear IR, not from graph traversal. For memory operations, use the "alloca everything, let mem2reg optimize" pattern (proven in the Move-on-LLVM project and Rust's own codegen). This avoids manually constructing SSA form with phi nodes. Verify generated IR with `module.verify()` on every test case. Build a comprehensive IR test suite from day one that checks both correctness and round-trip stability.

**Warning signs:**
- Simple programs compile but anything with nested loops or conditionals crashes LLVM's verifier
- Generated code is 10-100x slower than equivalent C because scheduling is naive
- The team starts special-casing LLVM IR generation for different graph patterns instead of using a systematic lowering
- Phi node placement bugs that only manifest with specific optimization levels

**Phase to address:**
Phase 3 (LLVM Compilation). But the graph IR design in Phase 1 must anticipate this -- the op node set and edge types should be designed to facilitate clean lowering. If the graph representation cannot be linearly scheduled, the LLVM phase will require a rewrite of the graph layer.

---

### Pitfall 3: Inkwell Lifetime Contamination and LLVM Context Entanglement

**What goes wrong:**
Inkwell ties all LLVM objects (`Module`, `Builder`, `BasicBlock`, `Value`) to a `Context` lifetime `'ctx`. Attempting to store these alongside the `Context` in the same struct creates a self-referential type, which Rust forbids. This forces awkward architectural decisions that ripple through the entire compilation pipeline. Teams commonly try to work around this with `unsafe`, `Pin`, or `Rc<RefCell<>>` hacks that create subtle use-after-free bugs or make the code unmaintainable.

**Why it happens:**
Inkwell is a safe Rust wrapper around LLVM's inherently unsafe C API. LLVM's ownership model (Context owns everything) does not map to Rust's ownership model. The `'ctx` lifetime parameter is Inkwell's solution, but it means every type that touches LLVM becomes generic over `'ctx`, and you cannot have a self-contained "compiler" struct that owns both the context and the things derived from it. This is the single most commonly reported problem in Inkwell's issue tracker and community forums.

**How to avoid:**
Design the compilation pipeline as a function, not a struct. Create the `Context` at the top of compilation, then pass `&'ctx Context` into all compilation functions. Structure the compiler as `compile(graph) -> Result<CompiledModule>` where the Context is created and dropped within that function scope. If you need persistent state across compilations (e.g., for incremental compilation), store the *inputs and outputs* of compilation, not the LLVM objects themselves. Serialize compiled modules to bitcode or object files immediately, then drop all LLVM state.

Additionally, inkwell does not support multithreading. If multi-agent concurrent compilation is a goal, each agent must get its own single-threaded compilation pipeline, and results are merged at the object-file level (linking), not the LLVM-IR level.

**Warning signs:**
- Type signatures accumulate lifetime parameters that propagate through unrelated code
- `unsafe` blocks appear in the compilation pipeline "just to make lifetimes work"
- Segfaults during compilation that only appear under certain drop orders
- Architecture discussions about "how to store the LLVM context" consume disproportionate time

**Phase to address:**
Phase 3 (LLVM Compilation). Design the compilation function signature first, before implementing any codegen. The function boundary is the critical architectural decision.

---

### Pitfall 4: Multi-Agent Concurrent Graph Editing Without Structural Invariant Preservation

**What goes wrong:**
Multiple AI agents concurrently editing the program graph violate structural invariants that the graph must maintain: type consistency across connected nodes, acyclicity in data-flow subgraphs, referential integrity of edges, and contract satisfaction. Naive optimistic concurrency (edit freely, merge later) cannot maintain these invariants because graph structural properties are non-local -- a cycle introduced by merging two independently valid edits cannot be detected without global analysis.

**Why it happens:**
CRDTs, the standard approach for conflict-free concurrent editing, work well for sequences and sets but struggle with graphs that must maintain structural invariants. Recent research (ACM PaPoC 2024) on Directed Acyclic Graph CRDTs specifically highlights that maintaining acyclicity under concurrent modifications is an open research problem. The naive approach of using an OR-Set for vertices and a grow-only set for edges does not prevent cycles or dangling edges. Additionally, CRDT implementations suffer from tombstone accumulation (deleted nodes remain in memory), metadata bloat, and the fundamental impossibility of concurrent move operations that preserve global invariants (per Martin Kleppmann's "CRDTs: The Hard Parts").

**How to avoid:**
Do not use CRDTs for the graph. Use a centralized graph with fine-grained locking and optimistic concurrency control (OCC) at the subgraph level. The protocol should be:
1. Agent requests a "working copy" (snapshot of a subgraph)
2. Agent proposes edits to the working copy
3. System validates edits against full graph invariants (type checking, acyclicity, contracts)
4. System attempts to commit, checking for conflicts with other concurrent commits
5. If conflict, agent receives the updated state and must re-propose

This is essentially database-style MVCC applied to graphs. It avoids the CRDT invariant problem entirely by centralizing validation. The trade-off is that conflicting edits require retry, but for an AI agent this is acceptable -- agents can re-plan cheaply.

**Warning signs:**
- The team researches CRDTs for months without finding one that preserves all required invariants
- "Merge conflict" resolution logic grows to handle more and more special cases
- Tests pass with single-agent editing but fail with concurrent agents
- Graph corruption that only manifests as type errors or runtime crashes much later

**Phase to address:**
Phase 4 (Multi-Agent). But the graph's locking granularity and snapshot mechanism must be designed in Phase 1. The storage layer needs MVCC-like capabilities from the start.

---

### Pitfall 5: Contract System Becomes the Bottleneck That Blocks All Progress

**What goes wrong:**
The full contract system (types + pre/post-conditions + invariants + property-based testing) is specified as a v1 requirement because "contracts are the mechanism that makes multi-agent concurrent editing safe." In practice, implementing a full contract system is equivalent to building a theorem prover or SMT solver integration. The contract checking becomes the most complex subsystem, delaying everything else. Worse, contracts that are too strict reject valid programs, and contracts that are too loose provide false confidence. Property-based testing adds computational cost that makes the edit-verify cycle too slow for interactive AI agent use.

**Why it happens:**
Design by Contract sounds simple (preconditions, postconditions, invariants) but the implementation complexity is exponential in the expressiveness of the contract language. If contracts can reference arbitrary program state, checking them requires executing or symbolically analyzing arbitrary code. Path explosion in symbolic execution means verification time grows exponentially with program complexity. Runtime verification (actually running contracts) introduces overhead that research shows is hard to keep below 10% -- and property-based testing can be orders of magnitude more expensive, generating thousands of test cases per verification.

The Eiffel community (which invented DbC) found that "designing and implementing contracts can be time-consuming and complex, especially for large and dynamic systems" and that empirical studies at Karlstad University could not find statistically significant benefits from DbC in practice.

**How to avoid:**
Build contracts in layers, not all at once:
1. **Layer 0 (Phase 1):** Type checking only. This is fast, well-understood, and sufficient for basic consistency. Types are the most valuable contracts.
2. **Layer 1 (Phase 2):** Simple pre/post-conditions that are purely boolean expressions over function inputs/outputs. No quantifiers, no references to global state. These can be checked at runtime with <1% overhead.
3. **Layer 2 (Phase 4+):** Invariants over data structures. These are checked at module boundaries, not on every operation.
4. **Layer 3 (Future):** Property-based testing as an offline verification tool, not an inline check. Runs in background, reports violations asynchronously.

Never make the full contract system a gate for the compilation pipeline. Contracts should be advisory (warnings) before they become mandatory (errors). This lets the system be useful before the contract system is complete.

**Warning signs:**
- Contract checking takes longer than compilation
- The contract language requires its own parser, type checker, and evaluator
- Agents spend more time satisfying contracts than writing program logic
- The team debates contract semantics for weeks without writing graph code

**Phase to address:**
Phase 1: types only. Phase 2: simple pre/post. Phase 4+: invariants and property-based testing. The critical mistake is making the full system a Phase 1 requirement.

---

### Pitfall 6: The ~30+ Op Node Set Is Either Incomplete or Incoherent

**What goes wrong:**
Designing the set of primitive operation nodes for a general-purpose computational graph is equivalent to designing an instruction set architecture (ISA). Get it wrong and you face one of two failure modes: (a) the op set is incomplete, forcing awkward encodings of common operations that make the graph unreadable and unoptimizable, or (b) the op set has overlapping/redundant operations that create canonicalization nightmares where the same computation has multiple graph representations. Both failure modes cascade into the LLVM lowering phase, the contract verification system, and the AI agent's ability to reason about programs.

**Why it happens:**
ISA design is a decades-old discipline with hard-won lessons. The temptation is to start with a "minimal" set (add, mul, branch, call) and add operations as needed, but this leads to an incoherent set where some operations are atomic and others are composite patterns. The alternative -- starting with a comprehensive set -- risks premature commitment to operations that turn out to be wrong. The Sea of Nodes research (Click & Cooper) specifically warns about spreading semantics across switch statements when opcode sets grow organically.

LLVM's own IR went through painful migrations (typed pointers to opaque pointers, GEP to ptradd) because early design decisions in the op set proved wrong. These migrations took years across millions of lines of code.

**How to avoid:**
Define the op set in tiers:
- **Tier 1 (Core):** Arithmetic (add, sub, mul, div, mod), comparison (eq, lt, gt), logic (and, or, not), control flow (branch, loop, return), memory (load, store, alloc), function (call, param). ~15-18 ops. These map directly to LLVM IR instructions.
- **Tier 2 (Structured):** Struct access, array index, cast/convert, phi/merge. These have well-defined LLVM lowerings. ~8-10 ops.
- **Tier 3 (Extension):** I/O, string operations, higher-level patterns. These should be defined as subgraph templates (compositions of Tier 1-2 ops) rather than new primitive ops.

Critically: every op must have exactly one LLVM lowering, and every LLVM IR instruction used in lowering must come from exactly one op (or a well-defined composition). Maintain a bidirectional mapping document. Use property-based testing to verify that `lower(op) |> verify_llvm == ok` for all ops with all valid input types.

**Warning signs:**
- The same computation has 3+ different graph representations and the system produces different results depending on which one the AI agent chose
- Adding a new feature requires adding new op nodes because existing ones cannot express it
- The LLVM lowering has per-op special cases that keep growing
- Graph optimizations (constant folding, dead code elimination) must be written per-op rather than generically

**Phase to address:**
Phase 1 (Core Graph). The op set is the most consequential design decision in the entire project. Prototype with Tier 1 only, validate with LLVM round-trips, then add Tier 2. Never add a Tier 3 op when a Tier 1-2 composition suffices.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Storing graph in memory only (skip persistence) | Faster prototyping, no serialization code | Cannot test incremental compilation, crash = total loss, no multi-session agent work | Phase 1 prototyping only, must add persistence before Phase 2 |
| Using `petgraph::Graph` instead of `StableGraph` | Simpler API, no tombstone overhead | Node removal reindexes everything, breaking external references (agent handles, LLVM mappings, contract bindings) | Never -- use `StableGraph` from day one |
| Hardcoding LLVM version in inkwell feature flag | Avoids multi-version testing complexity | Locks to one LLVM version, cannot benefit from fixes/optimizations in newer LLVM, painful upgrade when forced | Acceptable during prototyping, must abstract before release |
| Skipping the scheduled linear IR (graph -> LLVM directly) | Less code, faster initial implementation | Cannot add graph-level optimizations, scheduling is ad-hoc and fragile, impossible to debug generated IR | Never -- the intermediate linear IR is essential architecture |
| Making all graph edits synchronous | Simple concurrency model | Blocks agents waiting for validation, creates bottleneck at the graph lock, terrible latency for multi-agent scenarios | Phase 1-3 (single agent), must redesign for Phase 4 |
| SQLite with ad-hoc schema for graph storage | Fast to implement, flexible | Query patterns baked into SQL strings, impossible to swap to graph DB without rewriting all queries, no query optimization | Early prototyping only -- define a storage trait interface from Phase 1 |

---

## Integration Gotchas

Common mistakes when connecting to external services and libraries.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Inkwell / LLVM | Trying to store `Context`, `Module`, `Builder` in the same struct | Create `Context` at function scope, pass as `&'ctx Context`, drop after compilation. Serialize output to bitcode/object immediately. |
| Inkwell / LLVM | Generating IR with deprecated patterns (typed pointers, constant expressions, multi-index GEPs) | Use opaque pointers exclusively (mandatory since LLVM 17). Use `i8`-based GEPs or `ptradd` when available. Follow Nikita Popov's LLVM 2025 migration guide. |
| Inkwell / LLVM | Assuming LLVM IR is target-agnostic | LLVM IR is NOT fully target-independent. ABI, calling conventions, data layout all leak through. Always specify a target triple and data layout. Test on at least two targets (x86_64 + aarch64). |
| petgraph | Using `NodeIndex` as a stable identifier | `NodeIndex` changes on removal in `Graph`. Use `StableGraph` and treat `NodeIndex` as unstable even there (store your own stable IDs mapped to graph indices). |
| SQLite (embedded storage) | Putting graph traversal logic in SQL queries (recursive CTEs) | Use SQLite for persistence (node/edge tables with JSON properties) but perform all graph traversal in Rust with petgraph. SQLite is a storage backend, not a query engine for graph operations. |
| SQLite -> Graph DB migration | Assuming the abstraction layer will be clean | Define a trait with operations like `get_node`, `get_edges`, `get_subgraph`, `put_node`, `put_edge` -- NOT `execute_sql`. The trait must be graph-semantic, not storage-semantic. Test the trait with both an in-memory mock and SQLite from day one. |
| serde (serialization) | Serializing petgraph's internal structure directly | Define your own serialization format for nodes and edges. petgraph's internal representation is not stable across versions and includes implementation details (free lists, etc.) that should not be persisted. |

---

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Full graph validation on every edit | Edits feel snappy with <100 nodes, acceptable with <1000 | Implement incremental validation: only re-validate the subgraph affected by the edit + its immediate dependencies | >1,000 nodes (seconds per edit), >10,000 nodes (unusable) |
| Naive dirty node tracking (mark everything downstream as dirty) | Incremental recompilation works but recompiles too much | Use red/green algorithm (as in rustc): propagate dirty marks, but stop when a re-evaluated node produces the same result as before | >500 nodes, where false-positive invalidation causes 80%+ recompilation |
| LLVM compilation on every edit (no caching) | Compilation latency grows linearly with program size | Cache compiled object files per-function. Only recompile functions whose subgraph actually changed. Link incrementally. | >50 functions, where full recompilation takes >5 seconds |
| Property-based testing in the edit loop | Contract verification appears instant for trivial properties | Move property-based testing to background/async. Use type checking + simple pre/post in the hot path. Report property violations asynchronously. | Any non-trivial property with >10 test cases or >100ms per case |
| Storing full graph snapshots for undo/history | Undo works great, memory usage is manageable | Use structural sharing (persistent data structures) or store edit operations (event sourcing) instead of full snapshots | >100 edits, where each snapshot copies the entire graph |
| Single-threaded LLVM compilation | Compilation of one module is fast enough | Partition the graph into independently compilable units (functions/modules). Compile units in parallel using separate LLVM contexts (one per thread). Inkwell does not support shared contexts across threads. | >20 functions, where sequential compilation becomes the bottleneck |

---

## Security Mistakes

Domain-specific security issues for an AI-agent-driven programming system.

| Mistake | Risk | Prevention |
|---------|------|------------|
| AI agent tool API allows arbitrary memory read/write through graph manipulation | Agent could construct a program that exploits the host system when compiled and executed | Sandbox compiled program execution. Never execute JIT'd code in the compiler's address space. Use separate process + seccomp/sandbox for test execution. |
| No rate limiting or resource bounds on agent API calls | A malfunctioning or adversarial agent could create millions of nodes, exhausting memory | Enforce resource quotas per agent session: max nodes, max edges, max compilation time, max execution time. Return clear errors when limits are hit. |
| Trusting agent-provided type annotations without verification | Agent claims a node has type `int` when it actually receives a pointer, causing memory corruption in compiled code | Always re-derive types from the graph structure. Agent-provided types are hints, not truth. The type checker is the authority. |
| Storing compiled native code without integrity verification | Cached object files could be tampered with or corrupted | Hash all cached compilation artifacts. Verify hash before linking. Consider signing if the system is exposed to untrusted environments. |
| Exposing LLVM error messages directly to agents | LLVM error messages can leak internal paths, memory addresses, and system information | Translate LLVM errors into structured, sanitized error responses. Log raw LLVM errors internally but return only semantic errors to agents. |

---

## Agent-Specific UX Pitfalls

Common mistakes in the AI agent tool interface that cause agents to fail.

| Pitfall | Agent Impact | Better Approach |
|---------|-------------|-----------------|
| Tool API returns raw graph node IDs (UUIDs) without context | Agent cannot reason about what node "a1b2c3d4" represents, leading to hallucinated references | Return semantic identifiers: `function:fibonacci/body/loop_1/add_op` not `node:a1b2c3d4`. Per Anthropic's guidance: "eschew low-level technical identifiers." |
| `retrieve_subgraph` returns the entire graph for large programs | Agent's context window fills with irrelevant nodes, degrading reasoning quality | Implement pagination, depth-limited traversal, and relevance filtering. Default to 2-hop neighborhood of the target node. Let agents request more if needed. |
| `propose_structured_edit` has no dry-run / preview mode | Agent commits changes that violate invariants, then must undo and retry (costly round-trip) | Add a `validate_edit` tool that checks an edit without committing. Return specific constraint violations so the agent can fix them before committing. |
| Error messages say "invalid edit" without explaining why | Agent retries the same invalid edit or makes random changes hoping to fix it | Return structured errors: `{ "violation": "type_mismatch", "node": "add_op_3", "expected": "i64", "got": "f32", "suggestion": "insert cast node" }` |
| No way for agent to query "what can I do here?" | Agent guesses valid operations, wastes attempts on impossible edits | Provide a `get_valid_operations(node_id)` tool that returns the set of legal edits for a given context. Dramatically reduces wasted agent actions. |

---

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Graph storage:** Often missing edge-case handling for concurrent node deletion + edge creation (edge references deleted node) -- verify with concurrent stress tests
- [ ] **LLVM codegen:** Often missing handling for `unreachable` code paths -- verify that all control flow paths terminate with `ret`, `br`, or `unreachable`
- [ ] **Type system:** Often missing proper handling of recursive types (struct containing pointer to itself) -- verify with self-referential data structures
- [ ] **Contract checking:** Often missing timeout/resource limits on contract evaluation -- verify that a malicious/buggy contract cannot hang the system
- [ ] **Incremental compilation:** Often missing invalidation of transitive dependencies -- verify that changing a function signature recompiles all callers
- [ ] **Agent API:** Often missing idempotency guarantees -- verify that replaying the same edit twice produces the same result (not a duplicate node)
- [ ] **Graph serialization:** Often missing handling of graph cycles in serialization -- verify that loop-containing graphs round-trip correctly
- [ ] **Op node coverage:** Often missing edge cases in arithmetic (integer overflow, division by zero, NaN propagation) -- verify with boundary value inputs for every op

---

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Bidirectional sync inconsistency | HIGH | Freeze one layer as source of truth, regenerate the other from scratch. Add consistency check tool that detects divergence. |
| LLVM IR generation bugs | MEDIUM | Add LLVM IR verification (`module.verify()`) as a hard gate. Diff generated IR against known-good baselines. Use `opt -verify` standalone. |
| Inkwell lifetime contamination | HIGH | Refactor compilation to function-scoped Context pattern. This is an architectural change that affects all codegen code. Do it early. |
| Graph invariant violations from concurrent edits | HIGH | Implement a graph integrity checker that validates all invariants (type consistency, acyclicity, referential integrity). Run it after every suspicious failure. Add a "repair" mode that removes invalid edges. |
| Contract system blocking progress | LOW | Make contracts advisory (warnings not errors). Ship without mandatory contract enforcement. Add it back incrementally as the system matures. |
| Op node set incoherence | HIGH | Define canonical forms and write a canonicalization pass that normalizes equivalent representations. Costly but necessary once the problem is detected. |
| petgraph index instability | MEDIUM | Migrate from `Graph` to `StableGraph`. Introduce a stable ID layer on top. This is a find-and-replace refactor if caught early, a rewrite if caught late. |
| Storage layer too tightly coupled to SQLite | MEDIUM | Extract a storage trait, implement it for SQLite, and add an in-memory implementation for testing. Requires touching all persistence code but not the graph logic itself. |

---

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Bidirectional layer sync loops | Phase 1 (Core Graph) | Automated test: edit Layer 1, verify Layer 2 converges in 1 pass. Edit Layer 2, verify Layer 1 converges in 1 pass. No oscillation. |
| Graph-to-LLVM lowering mismatch | Phase 1 (design) + Phase 3 (implementation) | Every op node has a lowering test: `graph_op -> linear_ir -> llvm_ir -> verify -> execute -> check_result`. Full matrix of ops x types. |
| Inkwell lifetime contamination | Phase 3 (LLVM Compilation) | Compilation function takes `&Graph` and returns `Result<Vec<u8>>` (object bytes). No LLVM types escape the function boundary. Enforced by API design. |
| Concurrent editing invariant violations | Phase 1 (design) + Phase 4 (implementation) | Concurrent stress test: N agents making random valid edits for M seconds. Graph integrity checker passes after every test run. Zero invariant violations. |
| Contract system blocking progress | Phase 1 (types only) + Phase 2 (simple contracts) + Phase 4+ (full system) | Each phase has a "contract complexity budget": Phase 1 contracts must check in <1ms, Phase 2 in <10ms. Budget is enforced in CI. |
| Op node set incoherence | Phase 1 (Core Graph) | Canonicalization property test: for any two graphs G1 and G2, if `execute(G1) == execute(G2)` then `canonicalize(G1) == canonicalize(G2)`. |
| petgraph index instability | Phase 1 (Core Graph) | All external interfaces use stable IDs (not `NodeIndex`). Integration test: create graph, delete random nodes, verify all external references still valid. |
| Storage trait abstraction | Phase 1 (Core Graph) | Storage trait has both SQLite and in-memory implementations from day one. All tests run against both backends. CI enforces this. |
| Incremental compilation over-invalidation | Phase 3 (LLVM Compilation) | Benchmark: change one function body in a 100-function program. Measure recompilation. Must recompile <5 functions. |
| Agent API usability | Phase 2 (AI Tool API) | Agent integration test: give a real LLM the tool API and a task. Measure: attempts to complete, failed tool calls, total tokens used. Compare against baseline. |

---

## Sources

### LLVM Integration
- [LLVM: The Bad Parts (Nikita Popov, Jan 2026)](https://www.npopov.com/2026/01/11/LLVM-The-bad-parts.html) -- HIGH confidence, authoritative first-party analysis
- [Design Issues in LLVM IR (Nikita Popov, 2021)](https://www.npopov.com/2021/06/02/Design-issues-in-LLVM-IR.html) -- HIGH confidence
- [This Year in LLVM 2025 (Nikita Popov, Jan 2026)](https://www.npopov.com/2026/01/31/This-year-in-LLVM-2025.html) -- HIGH confidence, documents migration status
- [Writing an LLVM Backend for Move in Rust (Brian Anderson, 2023)](https://brson.github.io/2023/03/12/move-on-llvm/) -- HIGH confidence, first-party experience report
- [LWN.net: "LLVM is a mess"](https://lwn.net/Articles/965699/) -- MEDIUM confidence, community perspective
- [Inkwell GitHub: Lifetime issues](https://users.rust-lang.org/t/problem-with-lifetime-with-inkwell-module/40482) -- HIGH confidence, primary source

### Graph IR Design
- [A Simple Graph-Based Intermediate Representation (Click & Cooper)](https://www.oracle.com/technetwork/java/javase/tech/c2-ir95-150110.pdf) -- HIGH confidence, foundational paper
- [Semantic Reasoning about the Sea of Nodes (INRIA)](https://inria.hal.science/hal-01723236v1/document) -- HIGH confidence, academic
- [Glow: Graph Lowering Compiler Techniques](https://arxiv.org/abs/1805.00907) -- MEDIUM confidence, ML-specific but lowering approach is relevant
- [petgraph Performance Comparison (Feb 2025)](https://arxiv.org/html/2502.13862v1) -- MEDIUM confidence, academic benchmark

### Concurrent Editing
- [DAG CRDTs (ACM PaPoC 2024)](https://dl.acm.org/doi/10.1145/3721473.3722141) -- HIGH confidence, directly addresses DAG invariant preservation
- [CRDTs: The Hard Parts (Martin Kleppmann)](https://martin.kleppmann.com/2020/07/06/crdt-hard-parts-hydra.html) -- HIGH confidence, authoritative
- [Bidirectional Sync Engineering Challenges](https://www.stacksync.com/blog/the-engineering-challenges-of-bi-directional-sync-why-two-one-way-pipelines-fail) -- MEDIUM confidence

### Contract Systems
- [Design by Contract (Wikipedia)](https://en.wikipedia.org/wiki/Design_by_contract) -- MEDIUM confidence, general reference
- [Runtime Verification Overhead Study (Cornell, ISSTA 2024)](https://www.cs.cornell.edu/~legunsen/pubs/GuanAndLegunsenRVOverheadStudyISSTA24.pdf) -- HIGH confidence, empirical overhead data

### Incremental Compilation
- [Rust Incremental Compilation in Detail](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html) -- HIGH confidence, official documentation
- [Red/Green Dependency Tracking (rust-lang/rust #42293)](https://github.com/rust-lang/rust/issues/42293) -- HIGH confidence, primary source

### AI Agent Tool Design
- [Writing Effective Tools for AI Agents (Anthropic, 2025)](https://www.anthropic.com/engineering/writing-tools-for-agents) -- HIGH confidence, first-party guidance
- [Problems in Agentic Coding (Tim Sylvester)](https://medium.com/@TimSylvester/problems-in-agentic-coding-2866ca449ff0) -- MEDIUM confidence, practitioner experience

---
*Pitfalls research for: lmlang -- AI-native programming system with dual-layer graph representation and LLVM compilation*
*Researched: 2026-02-17*
