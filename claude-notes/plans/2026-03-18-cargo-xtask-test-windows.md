# cargo xtask test — Platform-Aware Test Runner

## Overview

Add a `cargo xtask test` subcommand that automatically excludes v8-dependent crates
on Windows, so contributors don't need to remember 12 `--exclude` flags.

## Context

- v8 crate doesn't produce rlib on Windows, causing test compilation to fail for
  12 crates that transitively depend on it via `quarto-system-runtime`
- nextest's `default-filter` only controls which tests run, not which compile
- `cargo nextest run` (default-members) also fails — `crates/*` glob still matches v8 crates
- Full investigation: `memory://main/tasks/q2-windows-fix-v8-rlib-test-compilation`
- v8 cascade docs: `memory://main/docs/v8-rlib-unavailability-on-windows`

## Design

Extend `crates/xtask/src/main.rs` with a new `Test` subcommand alongside existing
`Lint` and `Verify`. Implementation in a new `crates/xtask/src/test.rs`.

### Behavior

1. Detect platform at compile time via `cfg(target_os = "windows")`
2. On Windows: auto-add `--exclude` for each v8-dependent crate
3. On other platforms: pass through to `cargo nextest run --workspace` unchanged
4. Forward all extra arguments to nextest (filters, `-p`, `--no-fail-fast`, etc.)

### Excluded crates (Windows)

```
quarto-system-runtime, pampa, quarto-core, quarto-sass, quarto-test,
quarto, quarto-project-create, qmd-syntax-helper, comrak-to-pandoc,
quarto-lsp, quarto-lsp-core, reconcile-viewer
```

### Usage

```bash
# Daily driver on Windows (auto-excludes v8 crates)
cargo xtask test

# Pass args through to nextest
cargo xtask test -- -p quarto-doctemplate
cargo xtask test -- --no-fail-fast

# On macOS/Linux — identical to cargo nextest run --workspace
cargo xtask test
```

### Also update `cargo xtask verify`

Step 5 in `verify.rs` currently runs `cargo nextest run --workspace` unconditionally.
Extract the platform-aware logic into a shared function so both `test` and `verify`
benefit.

## Work Items

- [x] Add `Test` variant to `Command` enum in `main.rs`
- [x] Create `test.rs` with platform-aware nextest invocation
- [x] Extract shared `nextest_base_args()` function for Windows excludes
- [x] Update `verify.rs` step 5 to use shared function
- [x] Test on Windows: `cargo xtask test` runs 2087 tests with 8 known CRLF failures + 1 lockfile mismatch
- [ ] Test on WSL/Linux: `cargo xtask test` runs full suite with no excludes (cannot test from Windows)
- [x] Update `claude-notes/instructions/windows-dev.md` to reference `cargo xtask test`
- [x] Update CLAUDE.local.md
- [x] Update CONTRIBUTING.md to use `cargo xtask test` instead of manual `--exclude` flags
