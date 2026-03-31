# Windows Lua Path Escaping Fix

**Date**: 2026-03-31
**Branch**: fix/lua-path-escaping
**Beads**: bd-3pe8 (follow-up: audit production Lua code)

## Goal

Fix Windows test failures where backslash file paths interpolated into Lua string literals cause syntax errors (`invalid escape sequence near '"C:\U'`), and consolidate 27 ad-hoc path escaping patterns into a single canonical utility.

## Background

Windows file paths use backslashes (`C:\Users\chris\...`). When interpolated into Lua string literals via `format!()`, Lua interprets `\U`, `\t`, etc. as escape sequences. Three test files in `pampa` had this problem:

- `filter_tests.rs`: 8 instances with **no escaping at all** (the bug — tests fail on Windows)
- `mediabag.rs`: 6 instances using `.replace('\\', "/")` (forward slash workaround)
- `system.rs`: 13 instances using `.replace('\\', "\\\\")` (backslash doubling workaround)

quarto-cli solves this with `pathWithForwardSlashes()` — converting backslashes to forward slashes before paths reach Lua. Windows APIs accept forward slashes, so this is universally safe.

## Work Items

- [x] Add `to_forward_slashes(&Path) -> String` utility to `crates/quarto-util/src/path.rs`
- [x] Register module and re-export in `crates/quarto-util/src/lib.rs`
- [x] Add `quarto-util` as dev-dependency of `pampa`
- [x] Fix `filter_tests.rs` — replace 8 unescaped `order_file.display()` calls
- [x] Migrate `mediabag.rs` — replace 6 ad-hoc `replace('\\', "/")` chains
- [x] Migrate `system.rs` — replace 13 ad-hoc `replace('\\', "\\\\")` chains
- [x] Verify full pampa test suite passes
- [x] Verify no remaining ad-hoc patterns in `crates/pampa/src/lua/`
- [ ] Full workspace verification (build + tests)

## Files Changed

| File | Change |
|------|--------|
| `crates/quarto-util/src/path.rs` | New — `to_forward_slashes` function + tests |
| `crates/quarto-util/src/lib.rs` | Add `pub mod path` and re-export |
| `crates/pampa/Cargo.toml` | Add `quarto-util` as dev-dependency |
| `crates/pampa/src/lua/filter_tests.rs` | Replace 8 `order_file.display()` with `to_forward_slashes` |
| `crates/pampa/src/lua/mediabag.rs` | Replace 6 `replace('\\', "/")` with `to_forward_slashes` |
| `crates/pampa/src/lua/system.rs` | Replace 13 `replace('\\', "\\\\")` with `to_forward_slashes` |

## Design Decisions

- **Utility in `quarto-util`, not `pampa`**: Although all current uses are in `pampa` test code, `to_forward_slashes` is a general-purpose path utility. Placing it in `quarto-util` makes it available workspace-wide for future use.
- **Dev-dependency only (for now)**: `pampa` production code currently passes paths to Lua via the C API (safe), not string interpolation. If the production audit (bd-3pe8) reveals production exposure, this should become a regular dependency.
- **Forward slashes over escaped backslashes**: Matches quarto-cli's `pathWithForwardSlashes()` convention. Simpler than doubling backslashes, and forward slashes work everywhere.
- **`#[cfg(windows)]` test gating**: The Windows-specific test uses `std::env::temp_dir()` (a real OS path) and is gated to only run on Windows where backslashes naturally appear.

## Follow-up

Beads issue bd-3pe8: Audit whether `pampa` production Lua code also needs path normalization. If quarto-cli needed `pathWithForwardSlashes()` in production, pampa likely does too — which would mean `to_forward_slashes` should be a regular dependency, not dev-only.
