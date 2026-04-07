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
- Install `wasm-bindgen-cli` via `cargo xtask dev-setup` (this provides the
  `wasm-bindgen-test-runner` binary, version-matched from Cargo.lock)

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
Trigger on the same paths as existing Rust tests, plus `crates/pampa/tests/wasm_lua.rs`.

**C toolchain prerequisite:** pampa with `lua-filter` pulls in `mlua` → `lua-src-wasm`,
which compiles Lua from C source via the `cc` crate. When targeting wasm32, this requires
Clang + `CC_wasm32_unknown_unknown` + `CFLAGS_wasm32_unknown_unknown` pointing to the
wasm-sysroot. This is the same setup already used by `ts-test-suite.yml` for the
production WASM build — the new job mirrors that toolchain setup.

Note: `ts-test-suite.yml` currently hardcodes `wasm-bindgen-cli --version 0.2.108`.
This should be migrated to `cargo xtask dev-setup` as part of this work.

Test command:
```bash
CC_wasm32_unknown_unknown=clang \
CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
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
- **C toolchain for wasm32**: Required because mlua/lua-src compiles Lua from C. Both the
  WASM test job and the existing TS test suite WASM build need this. Opportunity to share
  the setup (composite action or reusable workflow) rather than duplicating Clang + env
  vars across workflows.
- **`--test wasm_lua` required**: Running `cargo test -p pampa --target wasm32` without
  `--test` would fail (native tests can't compile for wasm32). Document this clearly.
- **Feature flags required**: WASM test command must use `--no-default-features --features lua-filter`
  to match how wasm-quarto-hub-client consumes pampa. Document the full command.

## Local developer workflow

WASM tests require nightly Rust + `rust-src` component + Clang (for C compilation of
Lua source) + `wasm-bindgen-test-runner` (installed via `cargo xtask dev-setup`).

```bash
CC_wasm32_unknown_unknown=clang \
CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

WASM tests are NOT part of `cargo xtask verify` — they require nightly + Clang with
wasm32 support + wasm-sysroot, which is Linux/macOS only. The WASM build itself
(`build-wasm.js`) also doesn't support Windows (no Clang wasm32 target). WASM tests
run in Linux CI only, matching the existing WASM build behavior.

On Windows, skip WASM tests — this is consistent with the WASM build being skipped.
On macOS/Linux with LLVM installed, contributors modifying WASM-specific code can run
them locally.

## dofile_wasm interaction (discovered during CI)

Removing the cfg proxy exposed a hidden coupling: `register_wasm_dofile` (called only on
WASM) overrides Lua's built-in `dofile` to push/pop the script-dir stack, enabling
`quarto.utils.resolve_path()` to resolve relative to the dofile'd script's directory.
The native path uses the C Lua `dofile` which doesn't interact with the stack.

The `test_dofile_script_dir_stack` test in `dofile_wasm.rs` was passing on main because
the `cfg(any(wasm32, test))` proxy caused `register_wasm_dofile` to run in native tests.
After removing the proxy, native tests get the C `dofile` and the test fails.

**Research finding:** Neither Pandoc nor Quarto CLI (TypeScript) provide script-dir tracking
for raw `dofile()`. Pandoc uses `PANDOC_SCRIPT_FILE` (set once, never updated). Quarto CLI
has an internal `scriptFile` stack used for shortcodes/wrapped filters, but raw `dofile()`
uses standard Lua CWD-relative resolution.

**Resolution:** The dofile script-dir tracking is a WASM-only feature (needed because
WASM's dofile is fully reimplemented via SystemRuntime). The failing test should be gated
on `wasm32` or moved to `wasm_lua.rs`. A follow-up issue tracks adding this feature to
native as an improvement over both Pandoc and Quarto CLI behavior.

## wasm-bindgen-cli install method (reverted)

Migrating `ts-test-suite.yml` from `cargo install wasm-bindgen-cli --version 0.2.108` to
`cargo xtask dev-setup` caused all hub-client `.wasm.test.ts` tests to fail with an
`externref` type mismatch in the compiled WASM module. Main uses the hardcoded install
and passes. The difference is that `cargo xtask dev-setup` adds `--locked` to the install.

Reverted in #109 — the TS Test Suite keeps the hardcoded install. The `test-suite.yml`
WASM Tests job still uses `cargo xtask dev-setup` (it installs `wasm-bindgen-test-runner`,
not the production `wasm-bindgen` CLI used by `build-wasm.js`).

Tracked as `bd-jakt` for investigation.

## WASM test CI build configuration (discovered during CI)

The WASM Tests CI job failed with two independent build errors. Both stem from
differences between how the production WASM build (`npm run build:all`) and the
new WASM test build are configured.

### Bug 1: Duplicate `core` lang item (E0152)

**Symptom:** `error[E0152]: duplicate lang item in crate core: sized` — two copies of
`libcore` are linked.

**Root cause:** The CI toolchain setup installs both the prebuilt `wasm32-unknown-unknown`
target (via `targets: wasm32-unknown-unknown`) AND uses `-Zbuild-std=std,panic_unwind`.
`-Zbuild-std` rebuilds the entire std dependency chain (`core` → `alloc` → `std`) from
source. The prebuilt target already ships a compiled `core`. Rust sees two definitions
of every lang item and refuses to link.

This is a known conflict:
- rust-lang/cargo#10200 (duplicate use of std core with -Z build-std)
- rust-lang/rust#69090 (nightly regression with -Z build-std for wasm32)

**Why the production build works:** `ts-test-suite.yml` sets up the toolchain as
`dtolnay/rust-toolchain@nightly` with NO `targets:` — it does not install the prebuilt
wasm32 target. The `-Zbuild-std` comes from `crates/wasm-quarto-hub-client/.cargo/config.toml`
and rebuilds everything from `rust-src` (included by default in nightly).

**Fix:** Removing `targets:` from the CI toolchain step is necessary but not sufficient.
The repo's `rust-toolchain.toml` specifies `targets = ["wasm32-unknown-unknown"]`, which
rustup applies automatically. The production build avoids the conflict because
`wasm-quarto-hub-client` is excluded from the workspace and gets an isolated `target/`
directory. The WASM test runs within the workspace, where the conflict manifests.

The CI job must explicitly remove the prebuilt target before running tests:
```yaml
- name: Remove prebuilt wasm32 target (conflicts with -Zbuild-std)
  run: rustup target remove wasm32-unknown-unknown
```

### Bug 2: Bin targets compiled for wasm32

**Symptom:** `error[E0433]: cannot find NativeRuntime` and `cannot find tokio` in
`pampa/src/main.rs` — the `pampa` and `ast-reconcile` binaries are being compiled for
wasm32, where native-only types don't exist.

**Root cause:** When running integration tests, Cargo automatically builds the package's
binary targets so tests can access them via `CARGO_BIN_EXE_<name>`. The `--test wasm_lua`
flag selects which test to run, but Cargo still builds all bin targets. This is documented
Cargo behavior (rust-lang/cargo#12980).

**Why the production build doesn't hit this:** `npm run build:all` runs `cargo build` on
`wasm-quarto-hub-client` (which has no `[[bin]]` targets), not on `pampa`.

**Fix:** Add `required-features = ["terminal-support"]` to both `[[bin]]` targets in
`crates/pampa/Cargo.toml`. The WASM test command uses `--no-default-features --features lua-filter`,
so `terminal-support` is absent and the bins are silently skipped. Normal builds use default
features (which include `terminal-support`), so nothing changes for development or CI test suite.

### Key insight: two different `-Zbuild-std` paths

The repo has two independent WASM build configurations:

| Aspect | Production build | WASM tests |
|--------|-----------------|------------|
| Crate | `wasm-quarto-hub-client` | `pampa` (test target) |
| Cargo cwd | `crates/wasm-quarto-hub-client/` | repo root |
| Config | crate-local `.cargo/config.toml` | root `.cargo/config.toml` |
| `-Zbuild-std` | via `[unstable]` in crate config | explicit CLI flag |
| Build mode | `--release` | debug (default) |
| Prebuilt target | not installed | was installed (bug) |
| `-fno-builtin` | not needed (release) | needed (debug) |

Both use `-Zbuild-std=std,panic_unwind` but through different mechanisms.
The WASM test path must match the production path's approach of NOT installing
the prebuilt target.

## CI toolchain simplification (2026-04-13)

### Removing dtolnay/rust-toolchain action

All CI workflows used `dtolnay/rust-toolchain@nightly` to set up the Rust toolchain.
This is redundant — `rust-toolchain.toml` already specifies the full configuration
(nightly channel, components, targets), and `rustup` reads it natively via proxied
`cargo` commands.

Replaced in all workflows with:
```yaml
- name: Set up Rust
  run: rustup show active-toolchain
```

This triggers auto-install from `rust-toolchain.toml` and shows the resolved toolchain.

### RUSTUP_TOOLCHAIN for WASM tests (supersedes Bug 1 fix)

The original fix for Bug 1 (E0152 duplicate core) was `rustup target remove
wasm32-unknown-unknown`. This failed because the rustup proxy reads
`rust-toolchain.toml` and auto-reinstalls the target on the next `cargo` command.

The correct fix: set `RUSTUP_TOOLCHAIN=nightly` as a job-level env var. This
bypasses `rust-toolchain.toml` entirely, preventing the target from ever being
installed. The job uses explicit `rustup toolchain install nightly --component
rust-src --profile minimal` instead of relying on `rust-toolchain.toml`.

### panic_abort in -Zbuild-std

The test binary (not the production library build) needs `panic_abort` in the
`-Zbuild-std` list. The WASM test command is:
```bash
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind,panic_abort
```

The production build (`wasm-quarto-hub-client`) only needs `std,panic_unwind` because
it builds a library, not a test binary with its own main/harness.

## wasm-c-shim: shared C stdlib stubs (2026-04-13)

### Problem

The WASM integration tests (filter execution, synthetic io/os verification) link
`pampa` for wasm32, which pulls in tree-sitter and Lua — both C libraries that
reference libc symbols (`calloc`, `fprintf`, `snprintf`, `abort`, etc.). On
`wasm32-unknown-unknown` there is no libc; these symbols must be provided by
Rust `#[no_mangle]` shim functions.

The production build works because `wasm-quarto-hub-client/src/c_shim.rs` provides
~980 lines of these shims. The WASM test only builds `pampa` and doesn't include
that crate, so the linker can't resolve the symbols.

### Solution

Extract `c_shim.rs` into a new `crates/wasm-c-shim/` crate:

- **Workspace member** (not excluded — it compiles for both native and wasm32,
  but the `#[no_mangle]` exports are gated on `target_arch = "wasm32"`)
- **Dependency of `wasm-quarto-hub-client`** (replacing the inline `c_shim` module)
- **Dev-dependency of `pampa`** (gated on `target_arch = "wasm32"`)

The test file imports `wasm_c_shim` to pull the shim symbols into the link:
```rust
// Pull in C stdlib shims for wasm32 (calloc, fprintf, snprintf, etc.)
// These are needed by tree-sitter and Lua's C code on wasm32-unknown-unknown.
extern crate wasm_c_shim;
```

### Why not alternatives

- **Include via `#[path]`**: Brittle, the file uses `pub` items and module-level statics
  that could conflict. Can't be tested independently.
- **Drop integration tests**: Leaves a gap — core tests verify the restricted VM
  registers synthetic io/os, but only integration tests verify they actually work
  when called from Lua during filter execution on real wasm32.

## Out of scope

- Migrating wasm-pack usage (no longer needed — only stale crate used it)
- Adding WASM tests for wasm-quarto-hub-client (cdylib-only, tested via hub-client JS tests)
- VFS-backed test runtime for io_wasm under wasm32 (native unit tests cover the logic)
