---
title: Quick task 2 - fix all issues/warnings, run tests, and format
created_at: 2026-02-19
must_haves:
  truths:
    - `cargo clippy --all-targets --all-features -- -D warnings` succeeds for the full workspace.
    - `cargo test --all-targets --all-features` succeeds for the full workspace.
    - Dashboard integration assertions track current dashboard shell/assets and remain green.
  artifacts:
    - crates/lmlang-core/src/graph.rs
    - crates/lmlang-core/src/types.rs
    - crates/lmlang-storage/src/dirty.rs
    - crates/lmlang-storage/src/memory.rs
    - crates/lmlang-storage/src/sqlite.rs
    - crates/lmlang-check/src/interpreter/eval.rs
    - crates/lmlang-check/src/interpreter/state.rs
    - crates/lmlang-codegen/src/codegen.rs
    - crates/lmlang-codegen/src/incremental.rs
    - crates/lmlang-server/tests/integration_test.rs
  key_links:
    - crates/lmlang-server/tests/integration_test.rs -> crates/lmlang-server/static/dashboard/index.html
    - crates/lmlang-storage/src/dirty.rs -> crates/lmlang-storage/src/hash.rs
    - crates/lmlang-check/src/contracts/check.rs -> crates/lmlang-check/src/interpreter/eval.rs
---

## Task 1
files: crates/lmlang-core/src/graph.rs, crates/lmlang-core/src/types.rs, crates/lmlang-storage/src/{dirty.rs,hash.rs,convert.rs,memory.rs,sqlite.rs}, crates/lmlang-check/src/{contracts/check.rs,contracts/property.rs,interpreter/eval.rs,interpreter/state.rs,typecheck/mod.rs,typecheck/rules.rs}
action: Apply clippy-driven simplifications and cleanup (unused imports/vars, `is_some_and`, `matches!`, `if let`, needless borrows/lifetimes, constants, and assertion cleanup).
verify: cargo clippy --all-targets --all-features -- -D warnings
done: completed

## Task 2
files: crates/lmlang-codegen/src/{codegen.rs,incremental.rs,lib.rs,types.rs}, crates/lmlang-server/src/{autonomous_runner.rs,autonomy_executor.rs}
action: Resolve remaining codegen/server clippy violations (map iteration helpers, control-flow simplifications, `io::Error::other`, `div_ceil`, derived default, and targeted lint allowances for intentionally large signatures/results).
verify: cargo clippy --all-targets --all-features -- -D warnings
done: completed

## Task 3
files: crates/lmlang-server/tests/integration_test.rs, crates/lmlang-server/static/dashboard/index.html, crates/lmlang-server/static/dashboard/styles.css
action: Update stale dashboard test assertions to stable current shell/CSS markers and rerun the full workspace tests; then apply cargo formatting.
verify: cargo test --all-targets --all-features && cargo fmt
done: completed
