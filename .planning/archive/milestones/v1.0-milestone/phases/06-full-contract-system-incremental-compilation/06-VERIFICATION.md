---
phase: 06-full-contract-system-incremental-compilation
verified: 2026-02-19T02:38:26Z
status: passed
score: 10/10 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 9/10
  gaps_closed:
    - "Invariant nodes checked at module boundaries -- check_invariants_for_value now called from interpreter Call handler when caller.module != callee.module"
    - "REQUIREMENTS.md updated -- CNTR-02, CNTR-03, CNTR-04 now show [x] and Complete in both checkboxes and Traceability table"
  gaps_remaining: []
  regressions: []
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
**Verified:** 2026-02-19T02:38:26Z
**Status:** passed
**Re-verification:** Yes -- after gap closure (06-04-PLAN.md closed 2 gaps from initial verification)

## Gap Closure Summary

Both gaps from initial verification are now closed:

**Gap 1 closed: CNTR-04 module boundary enforcement wired**

`check_invariants_for_value` is now called from the interpreter's Call handler in `crates/lmlang-check/src/interpreter/state.rs` at line 353. The call is guarded by `caller_func.module != target_func.module` (line 349), ensuring same-module calls bypass invariant checking with zero overhead. The implementation uses a mini-subgraph evaluation approach via `evaluate_invariant_for_value` (in `check.rs` line 292), which avoids reliance on pre-existing frame `node_values` -- the fundamental evaluation bug from the initial attempt is fixed. A dedicated integration test `test_cross_module_invariant_violation` (state.rs line 1689) verifies a cross-module call with a violating argument halts with `ContractViolation { kind: Invariant }`.

**Gap 2 closed: REQUIREMENTS.md updated to reflect completion**

CNTR-02, CNTR-03, and CNTR-04 now show `[x]` in the requirement checklist (lines 39-41) and `Complete` in the Traceability table (lines 128-130). CNTR-05 and STORE-05 were already Complete and remain so.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Precondition nodes are first-class ComputeOp variants checked at function entry | VERIFIED | `ComputeOp::Precondition` in `ops.rs` lines 250-253. Inline eval in `state.rs` lines 934-983: checks port 0 Bool, halts with `EvalResult::ContractViolated` if false. `is_contract()` returns true (ops.rs line 311). |
| 2 | Postcondition nodes are first-class ComputeOp variants checked at function return | VERIFIED | `ComputeOp::Postcondition` in `ops.rs` lines 258-262. Handled in same match arm as Precondition in `state.rs`. `is_contract()` returns true. |
| 3 | Invariant nodes are first-class ComputeOp variants associated with TypeIds and checked at module boundaries | VERIFIED | `ComputeOp::Invariant { target_type, message }` in `ops.rs` lines 268-273. `check_invariants_for_value` called from state.rs line 353 inside `caller_func.module != target_func.module` guard (line 349). Mini-subgraph evaluation via `evaluate_invariant_for_value` runs the invariant condition without relying on frame state. Test at state.rs line 1689 confirms cross-module violation detection. |
| 4 | Contract violations produce structured diagnostics with counterexample values, node IDs, and inputs | VERIFIED | `ContractViolation` struct in `contracts/mod.rs`: `kind`, `contract_node`, `function_id`, `message`, `inputs`, `actual_return`, `counterexample`. Populated correctly in `state.rs` violation path and `check.rs` `check_invariants_for_value` lines 253-261. |
| 5 | Compiled binaries contain zero contract overhead -- contract nodes stripped during codegen | VERIFIED | `codegen.rs` line 90-95: `!node.op.is_contract()` filter before topological sort. `unreachable!` at line 815 guards the codegen match arm. No regression detected. |
| 6 | Contract changes do not mark functions dirty for recompilation | VERIFIED | `hash_function_for_compilation` in `hash.rs` line 169: filters `op.is_contract()` at lines 175 and 205. Dedicated test at line 501 confirms hash stability after adding Precondition node. |
| 7 | Agent can provide seed inputs, system generates randomized variations to test contracts (property tests) | VERIFIED | `run_property_tests` in `property.rs` line 154. Seeds run first, then `config.iterations` random inputs via `ChaCha8Rng`. `PropertyTestConfig`, `PropertyTestResult`, `PropertyTestFailure` all substantive. |
| 8 | Given the same random seed, same test inputs are generated (reproducibility) | VERIFIED | `ChaCha8Rng::seed_from_u64(config.random_seed)` in `property.rs` line 166. Random seed echoed in `PropertyTestResult.random_seed`. |
| 9 | After editing a single function, only that function and its dependents recompile | VERIFIED | `IncrementalState::compute_dirty` in `incremental.rs` lines 87-155: Phase 1 finds directly changed functions by hash diff, Phase 2 BFS through reverse call graph for transitive callers, Phase 3 marks rest as cached. `compile_incremental` in `compiler.rs` line 228 uses this. |
| 10 | REQUIREMENTS.md reflects phase completion for CNTR-02/03/04/05 and STORE-05 | VERIFIED | Lines 39-42 show `[x]` for all four contract requirements and line 25 for STORE-05. Traceability table lines 120, 128-131 all show `Complete`. |

**Score:** 10/10 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/lmlang-core/src/ops.rs` | Precondition, Postcondition, Invariant variants + is_contract() | VERIFIED | All three variants (lines 250-273). `is_contract()` on `ComputeOp` (lines 308-315) and `ComputeNodeOp` (lines 430-432). Tests confirm correct returns. |
| `crates/lmlang-check/src/contracts/mod.rs` | ContractViolation, ContractKind types | VERIFIED | Both types with all required fields: `kind`, `contract_node`, `function_id`, `message`, `inputs`, `actual_return`, `counterexample`. |
| `crates/lmlang-check/src/contracts/check.rs` | check_preconditions, check_postconditions, check_invariants_for_value, evaluate_invariant_for_value | VERIFIED + WIRED | All four functions defined. `check_invariants_for_value` called from state.rs line 353. `evaluate_invariant_for_value` called from within `check_invariants_for_value` at line 248. 3 dedicated unit tests for invariant evaluation at lines 772, 780, 788. |
| `crates/lmlang-storage/src/hash.rs` | hash_function_for_compilation excluding contract nodes | VERIFIED | Function at line 169. Filters `op.is_contract()` at lines 175 and 205. `hash_all_functions_for_compilation` at line 244. Tests including "excludes contract nodes" test at line 501. |
| `crates/lmlang-check/src/contracts/property.rs` | PropertyTestConfig, PropertyTestResult, PropertyTestFailure, run_property_tests | VERIFIED | All types defined and substantive. `run_property_tests` runs seeds then random variations. ChaCha8Rng for determinism. Tests at lines 359, 393, 425. |
| `crates/lmlang-server/src/schema/contracts.rs` | PropertyTestRequest, PropertyTestResponse, ContractViolationView | VERIFIED | All types present with correct fields. |
| `crates/lmlang-server/src/handlers/contracts.rs` | property_test handler | VERIFIED | Handler present, delegates to service.property_test(). |
| `crates/lmlang-codegen/src/incremental.rs` | IncrementalState, RecompilationPlan, compute_dirty | VERIFIED | All types and functions present. BFS dirty detection, reverse call graph, cached object paths, save/load. Tests at lines 351, 388, 410, 481, 528, 568, 580, 604. |
| `crates/lmlang-server/src/schema/compile.rs` | DirtyStatusResponse, DirtyFunctionView | VERIFIED | `DirtyStatusResponse` at line 55, `DirtyFunctionView` at line 68, `CachedFunctionView` at line 79. All fields present. |
| `crates/lmlang-codegen/src/compiler.rs` | IncrementalState used in compile() path | VERIFIED | `compile_incremental` function at line 228 uses `IncrementalState`. |
| `.planning/REQUIREMENTS.md` | CNTR-02/03/04/05 and STORE-05 marked Complete | VERIFIED | Lines 39-42 show `[x]`. Traceability table lines 128-131 show `Complete`. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `interpreter/state.rs` | `contracts/check.rs` | `check_invariants_for_value` called at Call node, inside `module !=` guard | WIRED | state.rs line 353 calls `crate::contracts::check::check_invariants_for_value`. Guard at line 349: `caller_func.module != target_func.module`. |
| `contracts/check.rs` | mini-subgraph evaluator | `evaluate_invariant_for_value` for on-the-fly condition evaluation | WIRED | `check_invariants_for_value` calls `evaluate_invariant_for_value` at line 248. Mini-eval walks condition subgraph with a local value map, no frame dependency. |
| `codegen/codegen.rs` | `ops.rs` | `is_contract()` filter before topological sort | WIRED | Line 95: `!node.op.is_contract()` confirmed. No regression. |
| `storage/hash.rs` | `ops.rs` | `is_contract()` filter for compilation hash | WIRED | `hash_function_for_compilation` calls `op.is_contract()` at lines 175, 205. |
| `handlers/contracts.rs` | `contracts/property.rs` | Handler calls `run_property_tests` through ProgramService | WIRED | Handler -> service.property_test -> run_property_tests chain confirmed. |
| `contracts/property.rs` | `interpreter/state.rs` | Each test iteration runs the interpreter | WIRED | `run_single_test` creates fresh `Interpreter::new` instance per test. |
| `codegen/compiler.rs` | `codegen/incremental.rs` | `compile()` checks dirty state and routes to incremental path | WIRED | `compile_incremental` calls `state.compute_dirty()` and `state.update_hashes()`. |
| `codegen/incremental.rs` | `storage/hash.rs` | Uses `hash_function_for_compilation` for dirty detection | WIRED | `hash_all_functions_for_compilation` imported and used at multiple call sites in incremental.rs. |
| `handlers/compile.rs` | `service.rs` | Dirty query endpoint calls service method | WIRED | `dirty_status` handler delegates to `service.dirty_status()`. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| CNTR-02 | 06-01, 06-04 | Functions support pre-conditions as contract nodes checked at function entry | SATISFIED | `ComputeOp::Precondition` exists; interpreter checks inline in `state.rs`; `check_preconditions` available. REQUIREMENTS.md shows `[x]` and `Complete`. |
| CNTR-03 | 06-01, 06-04 | Functions support post-conditions as contract nodes checked at function return | SATISFIED | `ComputeOp::Postcondition` exists; interpreter checks inline. `check_postconditions` available. REQUIREMENTS.md shows `[x]` and `Complete`. |
| CNTR-04 | 06-01, 06-04 | Data structures support invariants checked at module boundaries | SATISFIED | `ComputeOp::Invariant { target_type }` exists; `check_invariants_for_value` is called from interpreter at cross-module Call boundaries (state.rs line 353, guarded by module inequality at line 349); mini-subgraph evaluation via `evaluate_invariant_for_value` works without frame pre-population; test at state.rs line 1689 verifies end-to-end cross-module violation detection. REQUIREMENTS.md shows `[x]` and `Complete`. |
| CNTR-05 | 06-02 | Property-based tests auto-generated from contracts to verify graph behavior across input ranges | SATISFIED | Full property test harness with ChaCha8Rng, boundary-weighted random generation, seed-first execution, failure collection with traces. POST endpoint wired and tested. REQUIREMENTS.md shows `[x]` and `Complete`. |
| STORE-05 | 06-03 | Incremental recompilation via red-green dirty node tracking -- only recompile functions whose subgraphs actually changed | SATISFIED | IncrementalState with per-function blake3 hashes, BFS dirty propagation, per-function .o caching, contract-aware hashing. GET /dirty endpoint. REQUIREMENTS.md shows `[x]` and `Complete`. |

### Anti-Patterns Found

None. Scan of `crates/lmlang-check/src/interpreter/state.rs`, `crates/lmlang-check/src/contracts/check.rs`, and `.planning/REQUIREMENTS.md` produced no TODO/FIXME/PLACEHOLDER patterns, no empty return stubs, and no console.log-only handlers.

### Human Verification Required

#### 1. Property Test HTTP Endpoint

**Test:** Start the server, create a program with a function containing a precondition `a >= 0`, then POST to `/programs/{id}/property-test` with body `{"function_id": N, "seeds": [[-1], [5]], "iterations": 50}`
**Expected:** Response shows `total_run: 52`, at least 1 failure for the -1 seed input, `random_seed` present in response, `failed >= 1`
**Why human:** Requires live server and graph with real function/contract node construction.

#### 2. Contract Zero Overhead in Compiled Binary

**Test:** Compile a function with and without Precondition nodes, compare binary size and output behavior
**Expected:** Identical program behavior, negligible binary size difference (no contract code in output)
**Why human:** Requires objdump inspection or binary instrumentation to verify no contract IR is emitted.

### Re-verification Regression Check

All previously-verified truths (1, 2, 4-9) passed quick regression checks:

- `ComputeOp::Precondition/Postcondition/Invariant` variants and `is_contract()` present in `ops.rs` (lines 250-315): no changes
- `ContractViolation` struct still fully populated in violation paths: no changes
- `codegen.rs` contract filter at line 95 (`!node.op.is_contract()`): no changes
- `hash_function_for_compilation` contract exclusion at lines 175, 205: no changes
- `run_property_tests` / `PropertyTestConfig` / `ChaCha8Rng` in `property.rs`: no changes
- `IncrementalState::compute_dirty` BFS logic in `incremental.rs`: no changes

No regressions detected.

---

_Verified: 2026-02-19T02:38:26Z_
_Verifier: Claude (gsd-verifier)_
