# Phase 1: Core Graph Data Model - Research

**Researched:** 2026-02-18
**Domain:** Rust graph data structures, type systems, SSA-style IR design, LLVM-compatible op node sets
**Confidence:** HIGH

## Summary

Phase 1 delivers `lmlang-core` -- the foundational crate containing the type system, op node vocabulary, edge types, function/module boundaries, and dual StableGraph structure. Every subsequent phase depends on these types being correct and stable. The primary technical challenges are: (1) designing an op node set that is both agent-friendly (grouped, compact) and LLVM-lowerable (each op must map to specific LLVM instructions), (2) representing closures and captures in a flat graph structure, (3) implementing the dual-graph architecture with shared stable IDs, and (4) building a type system that supports enums/tagged unions with nominal identity while mapping cleanly to LLVM types.

The research confirms that petgraph 0.8's `StableGraph` is the correct foundation -- it preserves node/edge indices across removals, supports serde serialization that maintains index stability, and provides the graph algorithms needed for later phases (toposort, SCC, traversals). The op node design should use Rust enums with grouped variants (e.g., `BinaryArith { op: ArithOp, ty: ScalarType }`) rather than one variant per operation, which aligns with the user decision for CISC-like ops. Types should be inferred from input edges rather than carried as explicit parameters on every op, since the type system enforces edge typing anyway and this reduces redundancy. For control flow, the recommendation is to include BOTH high-level structured ops (Loop, IfElse, Match) AND low-level branch/jump ops, since structured ops are what agents will primarily work with, while branch-level ops are needed for LLVM lowering and optimization passes.

**Primary recommendation:** Design the type system and op node enums first, validate them against LLVM IR instruction categories, then build the graph container and edge types around them. Closures should be represented as functions with an explicit capture list stored on the function metadata node, using closure conversion (environment struct + function pointer) as the lowering strategy.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Op node vocabulary & tiers:**
- Richer / CISC-like op set (~20-25 Tier 1 ops) -- prioritize agent usability over minimalism. Fewer nodes per program means smaller graph representations that fit better in AI context windows.
- Include broader I/O operations: console I/O (print, readline) plus file operations (open, read, write, close). Programs should be able to interact with the filesystem from Phase 1.
- Op granularity: grouped with parameters (e.g., BinaryArith with operator and type fields), not one enum variant per operation. Claude's discretion on whether types are explicit parameters or inferred from input edges -- choose between grouped-with-params and grouped-type-inferred based on what works best for the graph model.

**Program structure model:**
- Functions support nesting and closures -- inner functions can capture variables from enclosing scope. Environment/capture representation needed in the graph from day one.
- Hierarchical modules (like Rust's mod system) -- modules can contain sub-modules. Tree-structured organization.
- Public/private visibility on functions and types across module boundaries. Two-level visibility: public (visible outside module) or private (module-internal).
- Cross-module function calls use direct graph edges -- call nodes directly reference the target function's subgraph. The graph is one connected structure, not indirected through import/export declarations.

**Type system scope:**
- Include enums/tagged unions from day one -- enables Option/Result-like patterns for error handling and variant data.
- Concrete types only -- no generics/parametric polymorphism in Phase 1. All types are fully specified. Generics deferred to a later phase.
- Nominal typing -- types have names and identity. Two structs with the same fields but different names are different types.

**Dual-graph Phase 1 scope:**
- Basic semantic skeleton -- the semantic graph tracks module and function nodes with names and signatures, a lightweight structural mirror. No embeddings, summaries, or relationships yet.
- Shared stable IDs -- both graphs use the same ID space. A function node has the same ID in both the semantic and computational graphs.
- Dual-layer visible to agents -- when agents query the graph (Phase 4), they can query either layer explicitly.

### Claude's Discretion

- Control flow construct design -- whether to include high-level structured ops (Loop, If/Else, Match) alongside low-level Branch/Jump, or stick to low-level only. Decide based on LLVM lowering constraints and agent usability.
- Unit/Never type handling -- whether to include both Unit and Never types, or just Unit with diverging functions having no return edge.
- Semantic skeleton auto-sync behavior -- whether the semantic skeleton auto-updates when computational graph changes, or requires manual management until Phase 8's propagation engine.
- Op grouping detail -- whether op types carry explicit type parameters or infer types from input edges.

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope.

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| GRAPH-01 | ~30 primitive operations (arithmetic, comparison, logic, control flow, memory, call/return, I/O) | Op node vocabulary design with Tier 1 (~20-25 ops) and Tier 2 (~8-10 ops) mapped to LLVM IR instructions. Grouped enum design pattern. |
| GRAPH-02 | Data flow edges with typed connections (SSA-style) | SSA-style data flow edge design with typed connections. Single-assignment semantics via graph structure (each node produces a value, edges carry it to consumers). |
| GRAPH-03 | Control flow edges for side-effect ordering, conditionals, loops | Dual edge types (DataFlow + ControlFlow). Structured control flow ops (IfElse, Loop, Match) plus low-level Branch/Phi for lowering. |
| GRAPH-04 | Function subgraphs with typed interfaces and module boundaries | Function boundary representation via ownership metadata on nodes. Hierarchical module tree. Closure capture lists. |
| GRAPH-05 | Type system: scalars (i8-i64, f32/f64, bool), aggregates (arrays, structs), pointers/references, function signatures | Complete type enum with LLVM type mapping. Nominal typing with TypeId. Enums/tagged unions. |
| GRAPH-06 | Op tiers: Tier 1 core (~15-18 ops), Tier 2 structured (~8-10 ops) | Adjusted to ~20-25 Tier 1 (per user decision for richer set) and ~8-10 Tier 2. Tier mapping to LLVM provided. |
| DUAL-02 | Executable Computational Graph layer stores typed ops DAG with data + control flow edges | StableGraph<ComputeNode, FlowEdge> design with typed node/edge enums. |
| DUAL-03 | Two layers as separate StableGraph instances with explicit cross-references | Dual ProgramGraph container with semantic + compute StableGraphs sharing NodeIndex space. |

</phase_requirements>

## Standard Stack

### Core (Phase 1 Only)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| petgraph | 0.8 | Dual StableGraph instances for semantic + computational layers | Dominant Rust graph library (3.5M+ downloads). StableGraph preserves indices across removals -- critical for stable node IDs. Serde serialization preserves indices. Built-in toposort, SCC, traversal algorithms. |
| serde | 1.0 | Derive-based serialization for all core types | Universal Rust serialization. Every type in lmlang-core must be serializable for storage (Phase 2) and API (Phase 4). |
| serde_json | 1.0 | JSON serialization for graph export/debug | Needed for testing, debugging, and eventual API responses. |
| thiserror | 2.0 | Structured error types | Compiler internals need matchable error variants, not erased errors. |
| smallvec | 1.x | Stack-allocated small vectors for edge lists | Most nodes have <8 edges. Avoids heap allocation for the common case. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| indexmap | 2.x | Insertion-ordered maps for struct fields, enum variants | Preserve declaration order in type definitions. |

### Phase 1 Cargo.toml

```toml
[package]
name = "lmlang-core"
version = "0.1.0"
edition = "2021"

[dependencies]
petgraph = { version = "0.8", features = ["serde-1"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
smallvec = { version = "1", features = ["serde"] }
indexmap = { version = "2", features = ["serde"] }

[dev-dependencies]
proptest = "1.10"
insta = { version = "1", features = ["json"] }
```

## Architecture Patterns

### Recommended Project Structure (Phase 1)

```
lmlang/
  Cargo.toml              # Workspace root
  crates/
    lmlang-core/
      src/
        lib.rs             # Re-exports, ProgramGraph container
        types.rs           # Type system: LmType, ScalarType, etc.
        type_id.rs         # TypeId, TypeRegistry for nominal typing
        ops.rs             # Op node enums: Tier 1 + Tier 2
        node.rs            # ComputeNode, SemanticNode wrappers
        edge.rs            # FlowEdge (DataFlow + ControlFlow), SemanticEdge
        graph.rs           # ProgramGraph dual-graph container
        function.rs        # FunctionDef, parameter/return interfaces, captures
        module.rs          # ModuleDef, hierarchical module tree, visibility
        id.rs              # Stable ID types (NodeId, EdgeId, FunctionId, ModuleId)
        error.rs           # Core error types
      Cargo.toml
```

### Pattern 1: Dual StableGraph Container with Shared ID Space

**What:** Two separate `StableGraph` instances wrapped in a `ProgramGraph` struct, sharing a node ID space. A function node has the same `NodeIndex` value in both graphs. The computational graph stores the full op-level detail; the semantic graph stores only structural metadata (module/function names, signatures).

**When to use:** Always -- this is the core data model mandated by DUAL-02 and DUAL-03.

**Example:**
```rust
use petgraph::stable_graph::{StableGraph, NodeIndex};
use petgraph::Directed;

/// Index types for type safety
pub type ComputeIdx = NodeIndex<u32>;
pub type SemanticIdx = NodeIndex<u32>;

/// The unified program graph containing both layers
pub struct ProgramGraph {
    /// Layer 2: Executable Computational Graph (typed ops + data/control flow)
    pub compute: StableGraph<ComputeNode, FlowEdge, Directed>,
    /// Layer 1: Semantic skeleton (modules, functions with names/signatures)
    pub semantic: StableGraph<SemanticNode, SemanticEdge, Directed>,
    /// Type registry for nominal type identity
    pub types: TypeRegistry,
    /// Module tree root
    pub root_module: ModuleId,
}
```

**Key design choice -- shared ID space:** When a function is created, a node is added to BOTH graphs with the same index. This is achieved by adding nodes in lockstep. If the semantic graph needs a node without a compute counterpart (e.g., a module node), a placeholder is inserted in the compute graph. This avoids a separate mapping table and keeps cross-referencing O(1).

**Implementation note:** petgraph's `StableGraph::add_node` returns the next available `NodeIndex`. To keep indices synchronized, add nodes to both graphs in the same order. If one graph gets ahead (e.g., compute nodes within a function body), those nodes exist only in the compute graph with no semantic counterpart -- which is correct, since individual ops don't need semantic entries.

### Pattern 2: Grouped Op Nodes with Type Inference from Edges

**What:** Op nodes use grouped enum variants (e.g., `BinaryArith { op: ArithOp }`) rather than individual variants per operation. Types are NOT stored on the op node itself but inferred from the types of incoming data flow edges. The type system enforces edge typing, so type information flows through the graph naturally.

**When to use:** For all op node definitions. This is the user's locked decision (grouped with parameters) combined with the discretion recommendation (type-inferred).

**Why type-inferred over explicit type parameters:**
1. Reduces redundancy -- the type is already on the edges, duplicating it on nodes creates sync risk
2. Simpler node representation = fewer bytes per node = more compact graph for agents
3. Type inference from edges is how LLVM IR actually works (operations are typed by their operands)
4. Avoids the "which type annotation is authoritative?" problem (answer: always the edges)

**Example:**
```rust
/// Tier 1: Core operations (~20-25 ops, CISC-like grouped)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeOp {
    // -- Constants & Literals --
    Const { value: ConstValue },         // Produces a typed constant

    // -- Arithmetic (grouped) --
    BinaryArith { op: ArithOp },         // add, sub, mul, div, rem
    UnaryArith { op: UnaryArithOp },     // neg, abs

    // -- Comparison --
    Compare { op: CmpOp },              // eq, ne, lt, le, gt, ge

    // -- Logic --
    BinaryLogic { op: LogicOp },        // and, or, xor
    Not,                                 // logical/bitwise not

    // -- Bitwise --
    Shift { op: ShiftOp },              // shl, shr_logical, shr_arith

    // -- Control Flow (structured) --
    IfElse,       // High-level: condition input, then-branch, else-branch
    Loop,         // High-level: condition, body, produces value
    Match,        // High-level: discriminant input, N arms
    Branch,       // Low-level: conditional branch to one of two targets
    Jump,         // Low-level: unconditional jump
    Phi,          // SSA merge point

    // -- Memory --
    Alloc,        // Allocate memory (stack or heap)
    Load,         // Read from memory/reference
    Store,        // Write to memory/reference
    GetElementPtr, // Struct field / array element address

    // -- Functions --
    Call { target: FunctionId },         // Direct function call
    IndirectCall,                        // Call through function pointer
    Return,                              // Return from function
    Parameter { index: u32 },            // Function parameter (input node)

    // -- I/O (console) --
    Print,        // Output to stdout
    ReadLine,     // Read line from stdin

    // -- I/O (file) --
    FileOpen,     // Open file, produces handle
    FileRead,     // Read from file handle
    FileWrite,    // Write to file handle
    FileClose,    // Close file handle

    // -- Closures --
    MakeClosure { function: FunctionId }, // Create closure (captures environment)
    CaptureAccess { index: u32 },        // Access captured variable by index
}

/// Tier 2: Structured/aggregate operations (~8-10 ops)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StructuredOp {
    // -- Aggregate access --
    StructCreate { type_id: TypeId },    // Create struct from field values
    StructGet { field_index: u32 },      // Extract struct field
    StructSet { field_index: u32 },      // Produce new struct with field changed
    ArrayCreate { length: u32 },         // Create fixed-size array
    ArrayGet,                            // Index into array (index from edge)
    ArraySet,                            // Produce new array with element changed

    // -- Type operations --
    Cast { target_type: TypeId },        // Type cast/conversion
    EnumCreate { type_id: TypeId, variant_index: u32 }, // Create enum variant
    EnumDiscriminant,                    // Extract discriminant from enum value
    EnumPayload { variant_index: u32 },  // Extract payload from enum variant
}
```

### Pattern 3: SSA-Style Data Flow via Graph Edges

**What:** Each compute node produces at most one value. Consumers receive that value via typed data flow edges. This is natural SSA -- no variable names, no reassignment, just edges carrying typed values from producers to consumers. Phi nodes merge values at control flow join points.

**When to use:** For all data flow representation.

**Example:**
```rust
/// Edge types in the computational graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowEdge {
    /// Data dependency: source produces a value consumed by target
    Data {
        /// Which output port of the source (most nodes have port 0 only)
        source_port: u16,
        /// Which input port of the target
        target_port: u16,
        /// The type flowing through this edge
        value_type: TypeId,
    },
    /// Control dependency: target executes after source
    Control {
        /// For branches: which branch arm (0 = then, 1 = else, etc.)
        branch_index: Option<u16>,
    },
}
```

**Why separate Data and Control edges (not a single edge type):** LLVM IR distinguishes data flow (SSA values) from control flow (basic block terminators). Keeping them separate in the graph makes lowering cleaner and allows traversing either independently. Data flow forms a DAG; control flow may contain cycles (loops).

### Pattern 4: Function Boundaries via Ownership, Not Subgraphs

**What:** Functions are represented as metadata in the semantic graph, with ownership markers on compute nodes. Each compute node belongs to exactly one function. Function boundaries are NOT separate petgraph subgraphs -- they are logical groupings within the single compute StableGraph. A function's nodes can be collected by filtering for ownership.

**When to use:** For representing function decomposition in a flat graph.

**Why flat over nested graphs:** petgraph does not support nested/hierarchical graphs natively. Using separate StableGraph instances per function would break cross-function edges (direct calls). A flat graph with ownership metadata keeps the entire program as one connected structure (per user decision about direct graph edges for cross-module calls).

**Example:**
```rust
/// Wrapper around an op that includes ownership metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeNode {
    /// The operation this node performs
    pub op: ComputeNodeOp,
    /// Which function owns this node
    pub owner: FunctionId,
}

/// Either a core op or a structured op
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComputeNodeOp {
    Core(ComputeOp),
    Structured(StructuredOp),
}

/// Function definition in the semantic graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub module: ModuleId,
    pub visibility: Visibility,
    pub params: Vec<(String, TypeId)>,
    pub return_type: TypeId,
    /// Entry node in the compute graph (first node of function body)
    pub entry_node: ComputeIdx,
    /// Captured variables for closures (empty for non-closures)
    pub captures: Vec<Capture>,
    /// Whether this is a closure
    pub is_closure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub name: String,
    pub captured_type: TypeId,
    /// How the variable is captured
    pub mode: CaptureMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CaptureMode {
    ByValue,
    ByRef,
    ByMutRef,
}
```

### Pattern 5: Closure Representation via Capture Lists

**What:** Closures are functions with a non-empty capture list. The capture list is metadata on the FunctionDef. Inside the function body, captured variables are accessed via `CaptureAccess { index }` nodes that reference the capture list by position. When a closure is created at a call site, a `MakeClosure { function }` node takes the captured values as data flow inputs.

**When to use:** For all closure/nested function representation.

**Why this over lambda lifting:** Lambda lifting transforms closures into regular functions by adding parameters, which requires modifying all call sites. For an agent-facing graph, closures should remain first-class -- agents think in terms of "this function captures x" not "this function has an extra parameter that used to be a captured variable." Lambda lifting can be a lowering pass for LLVM codegen without changing the graph representation.

**Example of closure in the graph:**
```
// let x = 5;
// let add_x = |y| x + y;
// add_x(3)

Compute Graph:
  [Const(5)]  ----Data----> [MakeClosure(add_x_fn)]  ----Data----> [Call(add_x)]
  [Const(3)]  ----Data----> [Call(add_x)]

Inside add_x function:
  [Parameter(0)]     ----Data----> [BinaryArith(Add)]  ----Data----> [Return]
  [CaptureAccess(0)] ----Data----> [BinaryArith(Add)]
```

### Pattern 6: Hierarchical Module Tree

**What:** Modules form a tree structure in the semantic graph. Each module node has a parent (except root) and child edges. Functions and type definitions belong to a module. Visibility (pub/private) is stored on the function/type definition.

**Example:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDef {
    pub name: String,
    pub parent: Option<ModuleId>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
}

/// Semantic graph node types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SemanticNode {
    Module(ModuleDef),
    Function(FunctionDef),
    TypeDef(TypeDefNode),
}

/// Semantic graph edge types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SemanticEdge {
    /// Module contains child (module, function, or type)
    Contains,
    /// Function calls another function
    Calls,
    /// Function/type references a type
    UsesType,
}
```

### Anti-Patterns to Avoid

- **Single unified heterogeneous graph:** Do NOT put semantic and compute nodes in the same StableGraph. Type safety is lost, traversals require constant filtering, and algorithms meant for one layer pollute the other.
- **Separate StableGraph per function:** Breaks cross-function edges. Use flat graph with ownership metadata instead.
- **Individual enum variant per arithmetic operation:** Creates massive match statements, wastes discriminant space, and makes the graph harder for agents to read. Use grouped variants with operator parameters.
- **Storing LLVM types on graph nodes:** LLVM types are a codegen concern. The graph has its own type system (LmType). Type mapping happens during lowering, not in the core data model.
- **Using petgraph's `Graph` instead of `StableGraph`:** Node removal invalidates all downstream indices. This is catastrophic for a persistent program graph where IDs must be stable.
- **Storing types as strings:** Types must be interned with TypeId for O(1) comparison and compact representation. String-based types are slow and error-prone.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| In-memory graph with stable indices | Custom adjacency list | petgraph `StableGraph` | Handles tombstones, iteration, algorithms. Years of edge-case hardening. |
| Graph serialization preserving indices | Custom serialize/deserialize for graph | petgraph `serde-1` feature | Handles holes (tombstones from removed nodes), edge ordering, index stability. |
| Ordered maps for struct fields | `HashMap` (loses declaration order) | `IndexMap` from indexmap crate | Struct field order matters for layout and LLVM lowering. |
| Small inline collections for edges | `Vec` (always heap allocates) | `SmallVec<[T; 4]>` | Most nodes have 1-4 edges. Avoids heap for the common case. |
| Type interning / identity | String comparison for types | TypeId newtype + TypeRegistry | Nominal typing requires O(1) identity checks. Strings are O(n). |
| Graph algorithms (toposort, SCC) | Custom implementations | petgraph `algo` module | `toposort()`, `tarjan_scc()`, `has_path_connecting()` are correct and tested. |

**Key insight:** petgraph's StableGraph handles the hard problems (index stability, hole management, serialization with holes) that would take weeks to hand-roll correctly. The effort should go into the TYPE SYSTEM and OP NODE DESIGN, not the graph infrastructure.

## Common Pitfalls

### Pitfall 1: Op Node Set That Cannot Be Lowered to LLVM

**What goes wrong:** Op nodes are designed for agent ergonomics (high-level, grouped) but cannot be cleanly lowered to LLVM IR because the mapping is ambiguous or requires runtime information not available at compile time.

**Why it happens:** The tension between "CISC-like" ops (user decision) and LLVM's RISC-like instruction set. A high-level `Match` op with N arms has no single LLVM instruction -- it must lower to a chain of branches or a switch.

**How to avoid:** Every op MUST have a documented lowering path to LLVM IR instructions BEFORE implementation. Create a mapping table during design. High-level ops (IfElse, Loop, Match) lower to basic blocks + branches + phi nodes. Grouped arithmetic ops lower to their corresponding LLVM instructions (add/fadd, sub/fsub, etc.) selected by the inferred type.

**Warning signs:** An op that requires "figuring out the lowering later." If you can't write the LLVM lowering pseudocode, the op design is wrong.

### Pitfall 2: Type System That Doesn't Map to LLVM Types

**What goes wrong:** Custom types (enums/tagged unions, closures) have no clean representation in LLVM's type system, causing the codegen phase to require complex, ad-hoc transformations.

**Why it happens:** LLVM has no built-in sum type / tagged union. Enums must be represented as a struct containing a discriminant integer + a union of payloads. Closures must be represented as a struct (environment) + function pointer.

**How to avoid:** Define the LLVM lowering representation for every type at design time:
- Enum -> `{ i8 discriminant, [max_payload_size x i8] payload }` struct
- Closure -> `{ ptr function_ptr, ptr environment_ptr }` struct
- Array -> LLVM array type `[N x T]`
- Struct -> LLVM struct type `{ T1, T2, ... }`

### Pitfall 3: Shared ID Space Divergence

**What goes wrong:** The semantic and compute graphs get out of sync -- a function exists in the semantic graph but its compute nodes are missing, or compute nodes reference a function ID that doesn't exist in the semantic graph.

**Why it happens:** Operations that modify one graph forget to update the other. No invariant enforcement.

**How to avoid:** All graph mutations go through ProgramGraph methods that maintain both graphs atomically. Never expose raw mutable access to individual StableGraph instances. Add debug assertions that verify cross-graph consistency after every mutation.

### Pitfall 4: Overly Complex Phase 1 Semantic Graph

**What goes wrong:** The semantic graph is designed with full Phase 8 features (embeddings, summaries, relationships, propagation) from day one, creating massive complexity before the computational graph is even working.

**Why it happens:** The dual-graph architecture is exciting and it's tempting to build it all at once.

**How to avoid:** Phase 1 semantic graph is a SKELETON only. Module nodes, function nodes with names/signatures, type definition nodes. No embeddings, no summaries, no semantic relationships beyond structural containment. The semantic graph should be <100 lines of code in Phase 1.

### Pitfall 5: Function Boundary Representation That Breaks Cross-Function Edges

**What goes wrong:** Functions are represented as separate, disconnected subgraphs. Cross-function calls cannot be represented as direct edges because the target function is in a different graph instance.

**Why it happens:** Natural instinct is to model "function = subgraph" literally.

**How to avoid:** Use a FLAT graph with ownership metadata. All compute nodes live in one StableGraph. A function's nodes are identified by their `owner: FunctionId` field. Call edges cross function boundaries within the same graph. This is what the user explicitly decided: "The graph is one connected structure."

## Code Examples

### Complete Type System Definition

```rust
use serde::{Serialize, Deserialize};

/// Unique identifier for a named type (nominal typing)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeId(pub u32);

/// The lmlang type system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LmType {
    /// Scalar types mapping directly to LLVM primitives
    Scalar(ScalarType),
    /// Fixed-size array: [T; N]
    Array { element: TypeId, length: u32 },
    /// Named struct with ordered fields (nominal)
    Struct(StructDef),
    /// Named enum / tagged union (nominal)
    Enum(EnumDef),
    /// Pointer/reference to another type
    Pointer { pointee: TypeId, mutable: bool },
    /// Function signature
    Function { params: Vec<TypeId>, return_type: TypeId },
    /// Unit type (zero-size, like Rust's ())
    Unit,
    /// Never type (function never returns, like Rust's !)
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarType {
    Bool,
    I8, I16, I32, I64,
    F32, F64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub type_id: TypeId,
    pub fields: indexmap::IndexMap<String, TypeId>,
    pub module: ModuleId,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub type_id: TypeId,
    pub variants: indexmap::IndexMap<String, EnumVariant>,
    pub module: ModuleId,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    /// Index of this variant (used as discriminant)
    pub index: u32,
    /// Payload type, if any (None = unit variant)
    pub payload: Option<TypeId>,
}
```

**LLVM type mapping (for reference, implemented in Phase 5):**

| LmType | LLVM Type (inkwell) |
|--------|---------------------|
| `Scalar(Bool)` | `context.bool_type()` (i1) |
| `Scalar(I8)` | `context.i8_type()` |
| `Scalar(I16)` | `context.i16_type()` |
| `Scalar(I32)` | `context.i32_type()` |
| `Scalar(I64)` | `context.i64_type()` |
| `Scalar(F32)` | `context.f32_type()` |
| `Scalar(F64)` | `context.f64_type()` |
| `Array { element, length }` | `element_llvm_type.array_type(length)` |
| `Struct { fields }` | `context.struct_type(&field_types, false)` |
| `Enum { variants }` | `context.struct_type(&[i8_type, max_payload_array], false)` |
| `Pointer { .. }` | `context.ptr_type(AddressSpace::default())` (opaque ptr) |
| `Function { params, ret }` | `ret_type.fn_type(&param_types, false)` |
| `Unit` | `context.void_type()` (or zero-size struct) |
| `Never` | No LLVM type; functions returning Never use `unreachable` terminator |

### Op Node to LLVM IR Mapping (Design Reference)

| Op | LLVM IR Instruction(s) | Notes |
|----|------------------------|-------|
| `BinaryArith(Add)` on int | `add` / `add nsw` | Signedness from type context |
| `BinaryArith(Add)` on float | `fadd` | |
| `BinaryArith(Sub)` on int | `sub` | |
| `BinaryArith(Mul)` on int | `mul` | |
| `BinaryArith(Div)` on signed int | `sdiv` | |
| `BinaryArith(Div)` on unsigned int | `udiv` | |
| `BinaryArith(Div)` on float | `fdiv` | |
| `BinaryArith(Rem)` on signed int | `srem` | |
| `Compare(Eq)` on int | `icmp eq` | |
| `Compare(Lt)` on signed int | `icmp slt` | |
| `Compare(Lt)` on float | `fcmp olt` | |
| `BinaryLogic(And)` | `and` | |
| `Not` | `xor %val, -1` | Bitwise NOT via XOR with all-ones |
| `Shift(Shl)` | `shl` | |
| `Shift(ShrLogical)` | `lshr` | |
| `Shift(ShrArith)` | `ashr` | |
| `Const { value }` | Constant literal | |
| `Alloc` | `alloca` | Stack allocation |
| `Load` | `load` | |
| `Store` | `store` | |
| `GetElementPtr` | `getelementptr` (inbounds) | |
| `Call { target }` | `call` | |
| `Return` | `ret` | |
| `Parameter { index }` | Function argument | |
| `Branch` | `br i1 %cond, label %t, label %f` | |
| `Jump` | `br label %target` | |
| `Phi` | `phi` | |
| `IfElse` | Lowers to: `br` + then_bb + else_bb + merge_bb + `phi` | |
| `Loop` | Lowers to: loop_header_bb + `phi` + body_bb + `br` back-edge | |
| `Match` | Lowers to: `switch` or chain of `br` instructions | |
| `Print` | `call @printf` or runtime function | External C function |
| `ReadLine` | `call @readline` or runtime function | External C function |
| `FileOpen/Read/Write/Close` | `call @fopen/@fread/@fwrite/@fclose` | libc functions |
| `Cast { target_type }` | `trunc`/`zext`/`sext`/`fptrunc`/`fpext`/`fptosi`/`sitofp`/etc. | Selected by source+target types |
| `MakeClosure` | Creates struct `{ fn_ptr, env_ptr }` | Environment allocated separately |
| `CaptureAccess { index }` | `getelementptr` on environment struct | |

## Discretion Recommendations

### Control Flow: Include BOTH High-Level and Low-Level Ops

**Recommendation:** Include high-level structured ops (IfElse, Loop, Match) AND low-level ops (Branch, Jump, Phi).

**Rationale:**
1. **Agent usability:** Agents should construct programs using IfElse/Loop/Match -- these are the concepts they reason about. Asking agents to manually construct branch chains with phi nodes is error-prone and produces larger graphs.
2. **LLVM lowering:** High-level ops have well-defined lowering patterns (IfElse -> branch + basic blocks + phi; Loop -> header + body + back-edge + phi; Match -> switch or branch chain). These lowering patterns are deterministic and can be automated.
3. **Optimization passes:** Some optimizations (loop unrolling, branch elimination) are easier to express on high-level ops. Others (dead code elimination, constant propagation) work naturally on low-level ops. Having both levels available supports both.
4. **Precedent:** Cranelift uses a similar approach -- high-level structured control flow in its front-end IR, lowered to basic blocks for optimization and code generation.

### Unit and Never Types: Include Both

**Recommendation:** Include both `Unit` and `Never` types.

**Rationale:**
1. **Unit** represents functions that complete but return no useful value (like Rust's `()` or C's `void`). Needed for side-effecting functions (Print, Store, FileWrite).
2. **Never** represents functions that diverge -- they never return (infinite loops, panics, process exit). Having `Never` as a type allows the type checker to reason about unreachable code. A function returning `Never` is a valid argument to any call site that doesn't use the return value.
3. **LLVM mapping:** Unit maps to `void` return type. Never maps to functions with `unreachable` terminator and `noreturn` attribute.
4. **Cost:** Minimal -- two additional variants in the type enum.

### Semantic Skeleton Auto-Sync: Simple One-Directional Sync

**Recommendation:** The semantic skeleton auto-updates when computational graph structure changes (function added/removed, signature changed), using simple one-directional sync.

**Rationale:**
1. **Phase 1 semantic graph is trivial** -- it only tracks modules and functions with names/signatures. There's very little to sync.
2. **Manual management is error-prone** -- if adding a function to the compute graph requires a separate API call to add it to the semantic graph, they WILL get out of sync during development and testing.
3. **Simple != Phase 8 propagation** -- auto-sync for structural changes (add function, remove function, change signature) is NOT the same as bidirectional propagation. It's just "when you add a function, also create the semantic node." This is ~20 lines of code, not a propagation engine.
4. **Opt-out available:** If the caller needs to suppress auto-sync for batch operations, provide a `batch_mode` flag that defers sync until explicitly flushed.

### Op Grouping: Type-Inferred (Not Explicit Parameters)

**Recommendation:** Op types infer their value types from input edges, not from explicit type parameters on the node.

**Rationale:**
1. **No redundancy:** The edge already carries `value_type: TypeId`. Storing the type again on the node duplicates information.
2. **Single source of truth:** If the type is on the edge AND the node, which is authoritative? Keeping it on edges only eliminates this question.
3. **Smaller nodes:** Without a `TypeId` field, nodes are ~4-8 bytes smaller. Across thousands of nodes, this matters for agent context window efficiency.
4. **How type inference works:** A `BinaryArith(Add)` node has two incoming Data edges, each with a `value_type`. The type checker verifies both are the same numeric type. The output type equals the input type. No type annotation needed on the node itself.
5. **Exception:** Some ops DO need type parameters because they cannot be inferred from inputs: `Cast { target_type }`, `StructCreate { type_id }`, `EnumCreate { type_id, variant_index }`, `Alloc` (via output edge type). These carry the minimum necessary type information.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| LLVM typed pointers | Opaque pointers (mandatory since LLVM 17) | LLVM 15-17 (2022-2024) | Use `context.ptr_type(addr_space)` not `type.ptr_type()`. All pointers are `ptr`, not `i32*`. |
| Sea of Nodes IR | CFG-skeleton with floating pure nodes | V8 abandoned Sea of Nodes (2024-2025) | Don't use pure sea-of-nodes. Keep explicit control flow structure. |
| Individual opcodes per type (`IAdd`, `FAdd`) | Grouped opcodes with type inference | Modern compiler IR trend | Reduces enum variant explosion. Type comes from context. |
| petgraph `Graph` for mutable graphs | `StableGraph` for index-stable graphs | petgraph 0.4+ | Always use `StableGraph` when indices must be stable across mutations. |
| `thiserror` 1.x | `thiserror` 2.0 | 2024 | Use `thiserror = "2.0"` -- improved derive ergonomics. |

## Open Questions

1. **Signed vs. unsigned integer distinction**
   - What we know: LLVM distinguishes signed/unsigned at the operation level (sdiv vs udiv, sext vs zext), not the type level (both are just `iN`).
   - What's unclear: Should lmlang's type system include `U8`-`U64` as separate types, or follow LLVM's approach of just `I8`-`I64` with signed/unsigned semantics on the operations?
   - Recommendation: Follow LLVM -- use `I8`-`I64` for all integers. Add signed/unsigned variants to comparison and division operations (e.g., `Div` vs `UDiv`, `Shr` vs `UShr`). This avoids type proliferation and matches the compilation target. If agents need unsigned types for clarity, add it as metadata, not a distinct type.

2. **String type in Phase 1**
   - What we know: I/O ops (Print, ReadLine, FileOpen) need strings. LLVM has no native string type.
   - What's unclear: Should Phase 1 include a string type, or just use `Pointer { pointee: I8 }` (C-style strings)?
   - Recommendation: Include a basic `String` type alias or wrapper in the type system that lowers to `{ ptr, i64 }` (pointer + length) for LLVM. This keeps the agent-facing API clean while having a clear lowering path. Alternatively, defer strings and have I/O ops take byte arrays.

3. **Exact op count validation**
   - What we know: The user wants ~20-25 Tier 1 and ~8-10 Tier 2.
   - What's unclear: The proposed set above has approximately 24 Tier 1 and 10 Tier 2 ops. Need to validate during implementation that this covers common programs without gaps.
   - Recommendation: Implement the proposed set, then write test programs (fibonacci, linked list, file reader) and verify all can be expressed. Add ops only when a real program requires them.

## Sources

### Primary (HIGH confidence)
- [petgraph StableGraph docs](https://docs.rs/petgraph/latest/petgraph/stable_graph/struct.StableGraph.html) -- API, index stability semantics, serde serialization format
- [petgraph serde serialization source](https://github.com/petgraph/petgraph/blob/master/src/graph_impl/serialization.rs) -- serialization preserves indices, handles holes
- [petgraph GitHub](https://github.com/petgraph/petgraph) -- version 0.8.2-0.8.3, features, algorithm support
- [LLVM Language Reference Manual](https://llvm.org/docs/LangRef.html) -- complete instruction set, type system, SSA form requirements
- [LLVM Instruction.def](https://github.com/llvm-mirror/llvm/blob/master/include/llvm/IR/Instruction.def) -- canonical opcode list
- [inkwell types module](https://thedan64.github.io/inkwell/inkwell/types/index.html) -- LLVM type mapping to Rust types
- [inkwell Kaleidoscope example](https://github.com/TheDan64/inkwell/blob/master/examples/kaleidoscope/main.rs) -- phi nodes, branches, basic blocks in Rust
- [Mapping High Level Constructs to LLVM IR](https://mapping-high-level-constructs-to-llvm-ir.readthedocs.io/) -- if-then-else, SSA/phi patterns

### Secondary (MEDIUM confidence)
- [Closure Conversion (Thunderseethe)](https://thunderseethe.dev/posts/closure-convert-base/) -- closure representation in graph IRs
- [Compiling Lambda Calculus (compiler.club)](https://compiler.club/compiling-lambda-calculus/) -- closure conversion + LLVM lowering
- [Matt Might: Closure Conversion](https://matt.might.net/articles/closure-conversion/) -- environment struct + function pointer pattern
- [Lambda Lifting (Wikipedia)](https://en.wikipedia.org/wiki/Lambda_lifting) -- alternative to closure conversion
- [Rust MIR Design (RFC 1211)](https://rust-lang.github.io/rfcs/1211-mir.html) -- SSA-like IR design decisions, why allocas over phi
- [SSA Form (Wikipedia)](https://en.wikipedia.org/wiki/Static_single-assignment_form) -- SSA fundamentals
- [A Gentle Introduction to LLVM IR (mcyoung)](https://mcyoung.xyz/2023/08/01/llvm-ir/) -- practical LLVM IR guide
- [V8: Leaving the Sea of Nodes](https://v8.dev/blog/leaving-the-sea-of-nodes) -- why not pure sea of nodes

### Tertiary (LOW confidence)
- [Graph-Based Higher-Order IR (Thorin)](https://compilers.cs.uni-saarland.de/papers/lkh15_cgo.pdf) -- academic, closure elimination in graph IR
- [Scrapscript IR design](https://bernsteinbear.com/blog/scrapscript-ir/) -- practical closure conversion IR decisions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- petgraph StableGraph is the uncontested choice, serde integration verified
- Type system design: HIGH -- LLVM type system is well-documented, mapping path is clear
- Op node design: MEDIUM-HIGH -- grouped approach is sound but exact op set needs validation with test programs
- Architecture (dual graph): HIGH -- two StableGraph instances with cross-references is well-supported by project research
- Closure representation: MEDIUM -- closure conversion is well-established in compiler literature but novel in graph-first context
- Pitfalls: HIGH -- informed by extensive project-level pitfalls research

**Research date:** 2026-02-18
**Valid until:** 2026-03-18 (stable domain, 30 days)
