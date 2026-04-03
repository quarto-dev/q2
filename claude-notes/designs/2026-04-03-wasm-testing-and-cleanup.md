# WASM Testing and Cleanup Design

Date: 2026-04-03
Beads: bd-itj9

## Problem

`filter.rs` and `shortcode.rs` use `#[cfg(any(target_arch = "wasm32", test))]` to force
native tests through the WASM-restricted Lua stdlib + synthetic io/os modules. This was
a proxy for WASM testing: native tests would catch WASM-incompatible Lua code without
needing real WASM test infrastructure.

The proxy causes 8 filter traversal tests to fail on Windows because the synthetic
`io.open` only handles POSIX VFS paths (`/project/...`), not Windows paths (`C:\...`).

Additionally, `wasm-qmd-parser` is a stale crate fully superseded by
`wasm-quarto-hub-client`. Its CI workflow (`build-wasm.yml`) and wasm-pack dependency
are orphaned artifacts.

## Success criteria

- All 8 filter traversal tests pass on Windows (currently fail with os error 123)
- WASM smoke tests pass in CI on wasm32-unknown-unknown target
- No active build/test/runtime references to wasm-qmd-parser or wasm-pack remain
  (historical references in claude-notes/ plans are acceptable)
- hub-client WASM build (`npm run build:all`) unaffected

## Solution

Replace the cfg proxy with real WASM tests, clean up stale artifacts, and document the
WASM testing convention.

**Phase ordering:** WASM tests (Phase 3) must be added before or alongside the cfg proxy
removal (Phase 2) to avoid any validation gap. In practice, Phase 3 setup and Phase 2
cfg changes should land in the same changeset.

## Phase 1: Clean up stale WASM artifacts

### Remove

- `crates/wasm-qmd-parser/` — entire crate (superseded by wasm-quarto-hub-client)
- `.github/workflows/build-wasm.yml` — only builds wasm-qmd-parser, manual dispatch
- wasm-pack from `cargo xtask dev-setup` install list
- `Cargo.toml` root: remove wasm-qmd-parser from `exclude` list, remove
  `[workspace.dependencies.wasm-qmd-parser]` entry (line 86-87), remove wasm-pack comments

### Check and update

- `.github/workflows/hub-client-e2e.yml` — remove stale `cargo install wasm-pack` step
  (verified: wasm-pack is installed but never used; WASM build uses build-wasm.js)
- `hub-client/README.md` — remove wasm-pack prerequisite
- `crates/wasm-quarto-hub-client/README.md` — remove wasm-pack references

### Rewrite

- `dev-docs/wasm.md` — rewrite as single source of truth for WASM in this project:
  - Architecture: wasm-quarto-hub-client wraps pampa + quarto-core for hub-client
  - Build: `hub-client/scripts/build-wasm.js` → cargo build + wasm-bindgen CLI
  - Why not wasm-pack: needs `-Zbuild-std=std,panic_unwind` for Lua error handling
  - Testing: see Phase 3
  - Note: wasm-pack is deprecated (rustwasm org sunset September 2025)

## Phase 2: Remove the cfg proxy

### Code changes

- `crates/pampa/src/lua/filter.rs:123`:
  `#[cfg(any(target_arch = "wasm32", test))]` → `#[cfg(target_arch = "wasm32")]`
- `crates/pampa/src/lua/filter.rs:133`:
  `#[cfg(not(any(target_arch = "wasm32", test)))]` → `#[cfg(not(target_arch = "wasm32"))]`
- `crates/pampa/src/lua/shortcode.rs:72`: same change
- `crates/pampa/src/lua/shortcode.rs:85`: same change
- Update comments above the cfg blocks (remove mention of test environment)

### No changes

- `io_wasm.rs` unit tests — keep as `#[cfg(test)]`. They test the Lua API contract
  (read modes, write buffering, handle lifecycle) on native using NativeRuntime. Valid
  unit tests of the implementation logic; don't need to run under wasm32.
- `os_wasm.rs` unit tests — same reasoning.

### Verification

The 8 filter traversal tests that use `io.open` should pass on all platforms after this
change, since they'll use `Lua::new()` with real C stdlib instead of synthetic WASM io.

## Phase 3: Add real WASM testing

### Dependencies

- Add `wasm-bindgen-test` as dev-dependency to `crates/pampa/Cargo.toml`
- Version must match the `wasm-bindgen` version used by the project
- Install `wasm-bindgen-test-runner` CLI (version-matched)

### Configuration

Add to `.cargo/config.toml` (workspace root):

```toml
[target.wasm32-unknown-unknown]
runner = 'wasm-bindgen-test-runner'
```

Note: the `runner` setting only applies to `cargo test`, not `cargo build`. The
hub-client WASM production build (`build-wasm.js` → `cargo build`) is unaffected.

### Test file

Create `crates/pampa/tests/wasm_lua.rs`:

```rust
//! WASM integration tests for Lua filter and shortcode infrastructure.
//!
//! These tests verify that the restricted Lua stdlib setup, synthetic io/os
//! modules, and filter/shortcode execution work correctly when compiled to
//! the real wasm32 target.
//!
//! **When to add tests here:** Only when modifying WASM-specific code paths:
//! - The #[cfg(target_arch = "wasm32")] blocks in filter.rs / shortcode.rs
//! - io_wasm.rs (synthetic io module)
//! - os_wasm.rs (synthetic os module)
//!
//! Native filter logic is tested comprehensively by the existing native tests.
//! These WASM tests are smoke tests of the target-specific setup.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
// Tests run in Node.js by default. Use wasm_bindgen_test_configure!(run_in_browser)
// if browser APIs are needed — current tests don't require it.
```

### Test coverage

Focused smoke tests of WASM-specific code paths (not duplication of native tests):

1. **Restricted Lua VM creation** — `Lua::new_with()` with restricted stdlib succeeds,
   synthetic io/os get registered
2. **Filter execution** — run a simple filter on a small document, verify output
   (uses Lua table to collect results, not io.open)
3. **Shortcode engine** — create engine, dispatch a basic handler
4. **Error handling** — Lua error gets caught as Rust error (not a WASM crash).
   This validates the `-Zbuild-std=std,panic_unwind` setup.
5. **Synthetic io registration** — io.open, io.type are available as globals
6. **Synthetic os registration** — os.time, os.clock, os.difftime are available

### CI

Add a `wasm-tests` job to `.github/workflows/test-suite.yml` (the main Rust CI workflow).
Trigger on the same paths as existing Rust tests, plus `crates/pampa/tests/wasm_lua.rs`:

```yaml
wasm-tests:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@nightly
      with:
        components: rust-src
        targets: wasm32-unknown-unknown
    - name: Install dev tools
      run: cargo xtask dev-setup  # installs wasm-bindgen-cli version-matched from Cargo.lock
    - name: Run WASM tests
      run: cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

The WASM build step in hub-client workflows (`npm run build:all`, `npm run build:wasm`)
stays unchanged — it builds the production WASM artifact. WASM tests are a separate
concern testing Rust code on the wasm32 target.

## Documentation updates

| File | Audience | Content |
|------|----------|---------|
| `crates/pampa/CLAUDE.md` | AI assistants | WASM test convention: when/where to add, how to run |
| `.claude/rules/wasm.md` | AI assistants | Never add `test` to wasm32 cfg guard; verify WASM tests when editing io_wasm/os_wasm |
| `dev-docs/wasm.md` | Developers | Single source of truth for WASM architecture, build, and testing |
| `claude-notes/instructions/testing.md` | AI assistants | Brief pointer to pampa CLAUDE.md for WASM details |
| `crates/pampa/tests/wasm_lua.rs` header | All | What this file tests and when to add to it |

## Testing strategy summary

| Layer | What it tests | Where | Runs on |
|-------|--------------|-------|---------|
| Native unit tests (io_wasm, os_wasm) | Synthetic Lua API contract | Inline in source files | All OS via `cargo test` |
| Native integration tests (filter_tests) | Filter logic with real Lua stdlib | Inline in source files | All OS via `cargo test` |
| WASM integration tests (new) | WASM-specific setup works on real target | `crates/pampa/tests/wasm_lua.rs` | wasm32 in CI |

## Risks and mitigations

- **`-Zbuild-std` is nightly-only**: Project is committed to nightly for WASM. If this
  changes, WASM tests would need adjustment. Acceptable risk.
- **`wasm-bindgen-test-runner` version pinning**: Must match `wasm-bindgen` crate version
  exactly. `cargo xtask dev-setup` reads the version from Cargo.lock and installs the
  matching CLI. CI uses `cargo xtask dev-setup` so the version stays in sync automatically.
- **`--test wasm_lua` required**: Running `cargo test -p pampa --target wasm32` without
  `--test` would fail (native tests can't compile for wasm32). Document this clearly.
- **Feature flags required**: WASM test command must use `--no-default-features --features lua-filter`
  to match how wasm-quarto-hub-client consumes pampa. Document the full command.

## Local developer workflow

WASM tests require nightly Rust + `rust-src` component + `wasm-bindgen-test-runner`.
`cargo xtask dev-setup` installs the runner. The full local command is:

```bash
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

WASM tests are NOT part of `cargo xtask verify` — they require nightly + WASM toolchain
which not all contributors will have. They run in CI. Contributors modifying WASM-specific
code should run them locally; others don't need to.

## Out of scope

- Migrating wasm-pack usage (no longer needed — only stale crate used it)
- Adding WASM tests for wasm-quarto-hub-client (cdylib-only, tested via hub-client JS tests)
- VFS-backed test runtime for io_wasm under wasm32 (native unit tests cover the logic)
