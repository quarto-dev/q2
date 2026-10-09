---
title: 'Plan: Make `cargo xtask verify` match CI checks'
date: 2026-03-09
description: 'Makes `cargo xtask verify` run the same checks as CI, including `-D warnings` for builds and tests, custom lints and tree-sitter grammar tests, so failures are caught before pushing.'
braid:
  strand: bd-3flm
  priority: P1
---

## Overview

`cargo xtask verify` passes locally but CI fails because the two environments check different things. The goal is to make `cargo xtask verify` reproduce CI's checks so developers catch failures before pushing.

## CI vs xtask verify — Gap Analysis

| Check | CI (`test-suite.yml`) | `cargo xtask verify` | Gap |
|-------|----------------------|---------------------|-----|
| Custom lints | `cargo xtask lint` (step 14) | Not included | Missing |
| Rust build with `-D warnings` | `cargo build` with `RUSTFLAGS="-D warnings"` (step 20) | `cargo build --workspace` (no RUSTFLAGS) | **Root cause of the failure** |
| Tree-sitter grammar tests | `cd crates/tree-sitter-qmd/tree-sitter-markdown && tree-sitter test` (step 21) | Not included | Missing |
| Rust tests with `-D warnings` | `cargo nextest run` with `RUSTFLAGS="-D warnings"` (step 22) | `cargo nextest run --workspace` (no RUSTFLAGS) | Missing RUSTFLAGS |
| Hub-client build | Commented out in CI | Included in verify | verify is stricter (fine) |
| Hub-client tests | Commented out in CI | Included in verify | verify is stricter (fine) |

## Specific CI failure

```
error: feature `trim_prefix_suffix` is declared but not used
  = note: `-D unused-features` implied by `-D warnings`
```

The `pampa` crate declares a nightly feature that isn't needed. This is a warning locally (ignored) but an error in CI (`-D warnings`).

## Work Items

- [x] **Set `RUSTFLAGS="-D warnings"` for Rust build and test steps in `verify.rs`**
  - Modified `run_command` to accept optional `rustflags` parameter
  - Added `--no-deny-warnings` flag for developer iteration

- [x] **Add custom lint step to verify**
  - Refactored `lint::run` into `lint::run_check` (returns Result) + `lint::run` (CLI wrapper)
  - Verify calls `lint::run_check` directly as step 1

- [x] **Add tree-sitter grammar test step to verify**
  - Added as step 3, runs `tree-sitter test` in `crates/tree-sitter-qmd/tree-sitter-markdown`
  - Added `--skip-treesitter-tests` flag

- [x] **Update step numbering/messaging**
  - Updated to 6 steps with TOTAL_STEPS constant

- [x] **Fix the immediate CI failure** (separate commit)
  - Remove unused `trim_prefix_suffix` feature from `pampa/Cargo.toml` (or wherever it's declared)
  - This fixes the current breakage independently of the xtask changes

## Design Decisions

### RUSTFLAGS approach
Set `RUSTFLAGS="-D warnings"` as an environment variable on the `Command` object, not globally. This keeps the strictness scoped to the verify steps and matches how CI does it.

### Opt-out vs opt-in for strictness
Default to strict (matching CI). Provide `--no-deny-warnings` for developers who want to iterate on code with warnings present. This way `cargo xtask verify` is a reliable pre-push check by default.

### Lint integration
Call the lint module's `run()` function directly since we're in the same binary, rather than spawning `cargo xtask lint` as a subprocess.
