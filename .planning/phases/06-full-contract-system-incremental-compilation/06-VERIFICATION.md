---
phase: 06-full-contract-system-incremental-compilation
verified: 2026-02-19T02:04:09Z
status: gaps_found
score: 9/10 must-haves verified
gaps:
  - truth: "Invariant nodes are first-class ComputeOp variants associated with TypeIds and checked at module boundaries"
    status: partial
    reason: "Invariant is a valid ComputeOp variant and evaluates inline in the interpreter work-list, but check_invariants_for_value (the module-boundary trigger) is defined but never called from the interpreter. Module boundary crossing (caller.module != callee.module) is never checked. The REQUIREMENTS.md also still shows CNTR-04 as Pending and unchecked [ ]."
    artifacts:
      - path: "crates/lmlang-check/src/contracts/check.rs"
        issue: "check_invariants_for_value defined at line 221 but has zero call sites in the entire codebase"
      - path: "crates/lmlang-check/src/interpreter/state.rs"
        issue: "No module boundary detection. Call node handling (lines 620-343) never invokes check_invariants_for_value for cross-module calls."
    missing:
      - "Wire check_invariants_for_value into the interpreter's Call handling in state.rs: when a Call node targets a function in a different module (target_func.module != caller_func.module), call check_invariants_for_value for typed arguments"
      - "Update REQUIREMENTS.md to mark CNTR-02, CNTR-03, CNTR-04, CNTR-05 as complete (checkboxes and Traceability table)"
  - truth: "REQUIREMENTS.md reflects phase completion for CNTR-02, CNTR-03, CNTR-04, CNTR-05"
    status: failed
    reason: "REQUIREMENTS.md still shows CNTR-02, CNTR-03, CNTR-04 as unchecked [ ] with status 'Pending' in the Traceability table. The implementation is complete in the codebase but the tracking document was not updated."
    artifacts:
      - path: ".planning/REQUIREMENTS.md"
        issue: "Lines 39-41 show unchecked boxes for CNTR-02/03/04. Lines 128-130 show 'Pending' for all three. CNTR-05 shows Complete but its checkbox on line 43 shows [x] correctly."
    missing:
      - "Mark CNTR-02, CNTR-03, CNTR-04 as [x] complete in REQUIREMENTS.md"
      - "Update Traceability table: change CNTR-02, CNTR-03, CNTR-04 from 'Pending' to 'Complete'"
human_verification:
  - test: "Property test endpoint end-to-end"
    expected: "POST /programs/{id}/property-test with seeds=[[-1],[5]] and iterations=50 returns a response with at least 1 failure (the -1 seed violating a >= 0 precondition), random_seed in response, and total_run=52"
    why_human: "Requires a running server with a live program graph containing a precondition. Integration tests cover this but human confirmation of real HTTP round-trip is valuable."
  - test: "Contract node zero overhead in compiled binary"
    expected: "A function with 5 Precondition nodes compiles to a binary with identical behavior and approximately identical size to the same function without contracts"
    why_human: "Binary inspection and timing require human tooling (objdump, size comparison)."
---

# Phase 6: Full Contract System & Incremental Compilation Verification Report

**Phase Goal:** Programs have rich behavioral contracts (pre/post-conditions, invariants, property-based tests) and only changed functions recompile
**Verified:** 2026-02-19T02:04:09Z
**Status:** gaps_found
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Precondition nodes are first-class ComputeOp variants checked at function entry | VERIFIED | `ComputeOp::Precondition` in `ops.rs` lines 250-253. Inline eval in `state.rs` lines 934-983: checks port 0 Bool, halts with `EvalResult::ContractViolated` if false. |
| 2 | Postcondition nodes are first-class ComputeOp variants checked at function return | VERIFIED | `ComputeOp::Postcondition` in `ops.rs` lines 258-262. Handled in same match arm as Precondition in `state.rs` lines 934-983. |
| 3 | Invariant nodes are first-class ComputeOp variants associated with TypeIds and checked at module boundaries | PARTIAL | `ComputeOp::Invariant { target_type, message }` exists in `ops.rs` lines 268-273. Inline evaluation in `state.rs` lines 985-1022. BUT `check_invariants_for_value` in `check.rs` has zero call sites -- module boundary detection is NOT wired. |
| 4 | Contract violations produce structured diagnostics with counterexample values, node IDs, and inputs | VERIFIED | `ContractViolation` struct in `contracts/mod.rs`: `kind`, `contract_node`, `function_id`, `message`, `inputs`, `actual_return`, `counterexample`. Populated correctly in `state.rs` violation path. |
| 5 | Compiled binaries contain zero contract overhead -- contract nodes stripped during codegen | VERIFIED | `codegen.rs` line 95: `!node.op.is_contract()` filter before topological sort. Integration tests in `lmlang-codegen` confirm matching binary behavior. |
| 6 | Contract changes do not mark functions dirty for recompilation | VERIFIED | `hash_function_for_compilation` in `hash.rs` lines 169-232: filters nodes where `op.is_contract()`, also skips edges to contract nodes. Dedicated test at line 480 confirms hash stability after adding Precondition node. |
| 7 | Agent can provide seed inputs, system generates randomized variations to test contracts (property tests) | VERIFIED | `run_property_tests` in `property.rs` lines 154+. Seeds run first, then `config.iterations` random inputs via `ChaCha8Rng`. `PropertyTestConfig`, `PropertyTestResult`, `PropertyTestFailure` types exist and are substantive. |
| 8 | Given the same random seed, same test inputs are generated (reproducibility) | VERIFIED | `ChaCha8Rng::seed_from_u64(config.random_seed)` in `property.rs` line 166. Random seed echoed in `PropertyTestResult.random_seed`. |
| 9 | After editing a single function, only that function and its dependents recompile | VERIFIED | `IncrementalState::compute_dirty` in `incremental.rs` lines 87-155: Phase 1 finds directly changed functions, Phase 2 BFS through reverse call graph for transitive callers, Phase 3 marks rest as cached. `compile_incremental` in `compiler.rs` line 228 uses this. |
| 10 | REQUIREMENTS.md reflects phase completion for CNTR-02/03/04/05 | FAILED | Lines 39-41 of `REQUIREMENTS.md` show unchecked `[ ]` for CNTR-02/03/04. Traceability table lines 128-130 show "Pending" for all three. Document not updated post-implementation. |

**Score:** 9/10 truths verified (1 partial, 1 failed)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/lmlang-core/src/ops.rs` | Precondition, Postcondition, Invariant variants + is_contract() | VERIFIED | All three variants exist (lines 250-273). `is_contract()` on `ComputeOp` (lines 308-315) and `ComputeNodeOp` (lines 430-432). Serde roundtrip tests confirmed. |
| `crates/lmlang-check/src/contracts/mod.rs` | ContractViolation, ContractKind types | VERIFIED | Both types defined with all required fields: `kind`, `contract_node`, `function_id`, `message`, `inputs`, `actual_return`, `counterexample`. |
| `crates/lmlang-check/src/contracts/check.rs` | check_preconditions, check_postconditions, check_invariants_for_value | VERIFIED (artifact) / ORPHANED (key link) | All three functions defined with correct signatures and substantive implementations. 6 tests in the file. However, `check_invariants_for_value` has no call sites. |
| `crates/lmlang-storage/src/hash.rs` | hash_function_for_compilation excluding contract nodes | VERIFIED | Function at line 169, also `hash_all_functions_for_compilation` at line 238. Both filter `op.is_contract()`. Tests present including "excludes contract nodes" test. |
| `crates/lmlang-check/src/contracts/property.rs` | PropertyTestConfig, PropertyTestResult, PropertyTestFailure, run_property_tests | VERIFIED | All types defined and substantive. `run_property_tests` runs seeds then random variations. ChaCha8Rng used for determinism. |
| `crates/lmlang-server/src/schema/contracts.rs` | PropertyTestRequest, PropertyTestResponse, ContractViolationView | VERIFIED | All types present and substantive with correct fields. |
| `crates/lmlang-server/src/handlers/contracts.rs` | property_test handler | VERIFIED | Handler present, delegates to service.property_test(). |
| `crates/lmlang-codegen/src/incremental.rs` | IncrementalState, RecompilationPlan, build_call_graph | VERIFIED | All types and functions present. BFS dirty detection, reverse call graph, cached object paths, save/load. |
| `crates/lmlang-server/src/schema/compile.rs` | DirtyStatusResponse, DirtyFunctionView | VERIFIED | `DirtyStatusResponse` at line 55, `DirtyFunctionView` at line 68, `CachedFunctionView` at line 79. All fields present. |
| `crates/lmlang-codegen/src/compiler.rs` | IncrementalState used in compile() path | VERIFIED | `compile_incremental` function at line 228 uses `IncrementalState`. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `interpreter/state.rs` | `contracts/check.rs` | Contract checking at function entry/return | PARTIAL | Precondition/Postcondition inline evaluation is wired (lines 934-983). `check_invariants_for_value` NOT called from interpreter. |
| `codegen/codegen.rs` | `ops.rs` | is_contract() filter before topological sort | VERIFIED | Line 95: `!node.op.is_contract()` confirmed. |
| `storage/hash.rs` | `ops.rs` | is_contract() filter for compilation hash | VERIFIED | `hash_function_for_compilation` calls `op.is_contract()` at lines 175, 205. |
| `handlers/contracts.rs` | `contracts/property.rs` | Handler calls run_property_tests through ProgramService | VERIFIED | Handler -> service.property_test -> run_property_tests chain confirmed. |
| `contracts/property.rs` | `interpreter/state.rs` | Each test iteration runs the interpreter | VERIFIED | `run_single_test` creates fresh `Interpreter::new` instance per test. |
| `codegen/compiler.rs` | `codegen/incremental.rs` | compile() checks dirty state and routes to incremental path | VERIFIED | `compile_incremental` calls `state.compute_dirty()` and `state.update_hashes()`. |
| `codegen/incremental.rs` | `storage/hash.rs` | Uses hash_function_for_compilation for dirty detection | VERIFIED | `hash_all_functions_for_compilation` imported and used at lines 261, 356, 392, 475, 522, 542 in incremental.rs. |
| `handlers/compile.rs` | `service.rs` | Dirty query endpoint calls service method | VERIFIED | `dirty_status` handler confirmed, `service.dirty_status()` call confirmed. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| CNTR-02 | 06-01 | Functions support pre-conditions as contract nodes checked at function entry | SATISFIED | `ComputeOp::Precondition` exists; interpreter checks it inline in `state.rs` eval_node; `check_preconditions` available for external use. Tests confirm violation detection. REQUIREMENTS.md incorrectly shows Pending. |
| CNTR-03 | 06-01 | Functions support post-conditions as contract nodes checked at function return | SATISFIED | `ComputeOp::Postcondition` exists; interpreter checks it inline. `check_postconditions` available. REQUIREMENTS.md incorrectly shows Pending. |
| CNTR-04 | 06-01 | Data structures support invariants checked at module boundaries | PARTIALLY SATISFIED | `ComputeOp::Invariant { target_type }` exists; inline eval in interpreter checks condition. Module boundary trigger (`check_invariants_for_value` call at cross-module Call nodes) is NOT wired. Plan deferred this explicitly if FunctionDef lacked module info, but `FunctionDef.module: ModuleId` exists. REQUIREMENTS.md shows Pending (correctly for the module boundary gap). |
| CNTR-05 | 06-02 | Property-based tests auto-generated from contracts to verify graph behavior across input ranges | SATISFIED | Full property test harness with ChaCha8Rng, boundary-weighted random generation, seed-first execution, failure collection with traces. POST endpoint wired and tested. REQUIREMENTS.md shows Complete. |
| STORE-05 | 06-03 | Incremental recompilation via red-green dirty node tracking -- only recompile functions whose subgraphs actually changed | SATISFIED | IncrementalState with per-function blake3 hashes, BFS dirty propagation, per-function .o caching, contract-aware hashing (changes to contracts do not dirty functions). GET /dirty endpoint. REQUIREMENTS.md shows Complete. |

### Anti-Patterns Found

| File | Pattern | Severity | Impact |
|------|---------|----------|--------|
| `crates/lmlang-check/src/contracts/check.rs` | `check_invariants_for_value` defined but never called (0 call sites) | Warning | CNTR-04 partial gap: module boundary invariant checking exists as infrastructure but is not triggered automatically |
| `.planning/REQUIREMENTS.md` | CNTR-02/03/04 show unchecked `[ ]` and "Pending" despite implementation being complete in codebase | Warning | Tracking inaccuracy; CNTR-02 and CNTR-03 are fully implemented, CNTR-04 is partially implemented |

### Human Verification Required

#### 1. Property Test HTTP Endpoint

**Test:** Start the server, create a program with a function containing a precondition `a >= 0`, then POST to `/programs/{id}/property-test` with body `{"function_id": N, "seeds": [[-1], [5]], "iterations": 50}`
**Expected:** Response shows `total_run: 52`, at least 1 failure for the -1 seed input, `random_seed` present in response, `failed >= 1`
**Why human:** Requires live server and graph with real function/contract node construction.

#### 2. Contract Zero Overhead in Compiled Binary

**Test:** Compile a function with and without Precondition nodes, compare binary size and output behavior
**Expected:** Identical program behavior, negligible binary size difference (no contract code in output)
**Why human:** Requires objdump inspection or binary instrumentation to verify no contract IR is emitted.

### Gaps Summary

**Gap 1: CNTR-04 module boundary enforcement not wired (partial implementation)**

The `check_invariants_for_value` function is fully implemented in `crates/lmlang-check/src/contracts/check.rs` but has no call sites. The interpreter evaluates Invariant nodes when they appear in the function's work-list (as inline eval-node dispatch), but does not trigger invariant checks when a Call node crosses module boundaries. The plan explicitly allowed deferring this if `FunctionDef` lacked module info -- however `FunctionDef.module: ModuleId` does exist (confirmed in `crates/lmlang-core/src/function.rs` line 52). The module-boundary trigger logic simply was not written. This is the gap between "Invariant as a graph node" (done) and "Invariant checked at module boundaries" (the second half of CNTR-04).

**Gap 2: REQUIREMENTS.md not updated post-implementation**

CNTR-02 and CNTR-03 are fully implemented (precondition and postcondition checking are fully wired through all layers with tests). The REQUIREMENTS.md document still shows them as `[ ]` Pending. CNTR-04 is partially implemented (the unchecked box is partially correct for the module boundary gap, but misleading). CNTR-05 correctly shows Complete. This is a documentation gap, not a code gap, but it creates an inaccurate project status view.

---

_Verified: 2026-02-19T02:04:09Z_
_Verifier: Claude (gsd-verifier)_
