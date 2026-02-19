# Phase 3: Type Checking & Graph Interpreter - Context

**Gathered:** 2026-02-18
**Status:** Ready for planning

<domain>
## Phase Boundary

Static type verification on every graph edit and development-time execution of computational graphs via interpretation. Programs can be type-checked for edge compatibility and executed with provided inputs to produce correct outputs for arithmetic, logic, control flow, memory operations, and function calls. No LLVM dependency. Contract system is Phase 6; agent API is Phase 4.

</domain>

<decisions>
## Implementation Decisions

### Type error diagnostics
- Rich context in diagnostics: nodes involved, surrounding edges, function boundary, and fix suggestions
- Report all type errors in the graph at once (no stop-at-first)
- Include actionable fix suggestions when the fix is obvious (e.g., "insert a cast node from i32 to i64 here")
- Type checking happens eagerly on every graph edit (add_edge, modify_node) — errors caught immediately

### Type coercion rules
- Bool-to-integer implicit conversion allowed (true=1, false=0)
- Claude's discretion: implicit widening (i32→i64, f32→f64) vs strict-only matching
- Claude's discretion: pointer/reference mutability enforcement (whether &mut can flow to & but not reverse)
- Claude's discretion: nominal vs structural struct type compatibility

### Interpreter execution model
- Step-by-step execution supported — can pause after each node, inspect intermediate values, then continue
- Optional execution trace — tracing off by default, enabled via flag to log every node evaluation and its result
- Proper call stack with frames — each function call pushes a frame, return pops it (supports recursion)
- Configurable recursion depth limit — default limit with option to increase, error on exceed

### Runtime error handling
- Integer overflow: trap (stop execution with overflow error, like Rust debug mode)
- Divide-by-zero: trap (stop execution with error, include the node that caused it)
- Out-of-bounds array access: trap (stop execution with bounds-check error including index and array size)
- Claude's discretion: whether runtime errors return partial results (values computed before the error) alongside the error

### Claude's Discretion
- Implicit widening conversion rules (strict vs safe widening)
- Pointer/reference mutability checking scope
- Nominal vs structural struct typing
- Whether runtime errors include partial results for debugging
- Exact recursion depth default value

</decisions>

<specifics>
## Specific Ideas

- Error philosophy is "trap everything" in the interpreter — overflow, div-by-zero, bounds checks all halt execution with clear diagnostics. This is a development-time tool, not a production runtime.
- Type checking is eager (on every edit), not lazy — the graph should never be in a type-invalid state.
- Step-by-step execution implies the interpreter needs an explicit state machine (not just a recursive walk), since execution can be paused and resumed.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 03-type-checking-graph-interpreter*
*Context gathered: 2026-02-18*
