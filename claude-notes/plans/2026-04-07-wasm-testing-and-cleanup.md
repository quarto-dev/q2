# WASM Testing and Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the cfg test proxy with real WASM tests, remove stale wasm-qmd-parser artifacts, and document the WASM testing convention.

**Architecture:** The `#[cfg(any(target_arch = "wasm32", test))]` guards in `filter.rs` and `shortcode.rs` force native tests through WASM-restricted Lua stdlib, causing Windows failures. We remove the `test` from these guards so native tests use `Lua::new()` with real C stdlib, and add real `wasm-bindgen-test` smoke tests that run on the actual wasm32 target in CI. Stale `wasm-qmd-parser` crate and its `build-wasm.yml` workflow are removed.

**Tech Stack:** Rust, wasm-bindgen-test, cargo test --target wasm32-unknown-unknown, GitHub Actions

**Beads:** bd-itj9

**Design spec:** `claude-notes/designs/2026-04-03-wasm-testing-and-cleanup.md`

---

## File Map

### Files to delete
- `crates/wasm-qmd-parser/` — entire crate directory (superseded by `wasm-quarto-hub-client`)
- `.github/workflows/build-wasm.yml` — manual workflow that only builds wasm-qmd-parser

### Files to create
- `crates/pampa/tests/wasm_lua.rs` — WASM integration smoke tests
- `.claude/rules/wasm.md` — AI rule: never add `test` to wasm32 cfg guard

### Files to modify
- `Cargo.toml` (workspace root) — remove wasm-qmd-parser from exclude + workspace deps, update comments
- `crates/pampa/Cargo.toml` — add `wasm-bindgen-test` dev-dependency
- `crates/pampa/src/lua/filter.rs` — change cfg guards (lines 123, 133)
- `crates/pampa/src/lua/shortcode.rs` — change cfg guards (lines 72, 85)
- `.cargo/config.toml` — add wasm32 test runner
- `.github/workflows/test-suite.yml` — add wasm-tests job
- `.github/workflows/hub-client-e2e.yml` — remove stale `cargo install wasm-pack` step
- `.github/workflows/ts-test-suite.yml` — migrate wasm-bindgen-cli install to `cargo xtask dev-setup`
- `hub-client/README.md` — remove wasm-pack prerequisite
- `crates/wasm-quarto-hub-client/README.md` — remove wasm-pack references
- `dev-docs/wasm.md` — full rewrite as WASM single source of truth
- `crates/pampa/CLAUDE.md` — add WASM test convention
- `claude-notes/instructions/testing.md` — update WASM section to reflect new approach

---

## Phase 1: Clean Up Stale WASM Artifacts

### Task 1: Remove wasm-qmd-parser crate

**Files:**
- Delete: `crates/wasm-qmd-parser/` (entire directory)

This crate is superseded by `wasm-quarto-hub-client`. It uses `wasm-pack` (deprecated, rustwasm org sunset Sep 2025) while the active crate uses `cargo build + wasm-bindgen` directly.

- [ ] **Step 1: Verify no other crate depends on wasm-qmd-parser**

Run from the worktree root:
```bash
grep -r "wasm-qmd-parser" --include="*.toml" --include="*.rs" --include="*.js" --include="*.ts" --include="*.yml" --include="*.yaml" . \
  | grep -v "crates/wasm-qmd-parser/" \
  | grep -v "claude-notes/" \
  | grep -v ".beads/"
```

Expected: Only hits in `Cargo.toml` (workspace root, lines 10 and 86-87), `.github/workflows/build-wasm.yml`, and possibly documentation. No runtime/build imports.

- [ ] **Step 2: Delete the crate directory**

```bash
rm -rf crates/wasm-qmd-parser
```

- [ ] **Step 3: Verify deletion**

```bash
ls crates/wasm-qmd-parser 2>&1
```
Expected: "No such file or directory"

---

### Task 2: Remove build-wasm.yml workflow

**Files:**
- Delete: `.github/workflows/build-wasm.yml`

This workflow is manual-dispatch only and only builds `wasm-qmd-parser` with `wasm-pack`. It has no consumers.

- [ ] **Step 1: Remove the workflow file**

```bash
rm .github/workflows/build-wasm.yml
```

---

### Task 3: Clean up workspace Cargo.toml

**Files:**
- Modify: `Cargo.toml` (workspace root, lines 7-10, 86-87, 244-249)

Three changes: remove wasm-qmd-parser from exclude list, remove its workspace dependency entry, update stale comments.

- [ ] **Step 1: Remove wasm-qmd-parser from exclude list**

In `Cargo.toml` line 10, change the `exclude` array from:
```toml
exclude = ["crates/wasm-quarto-hub-client", "crates/wasm-qmd-parser", "crates/experiments", "crates/pampa/fuzz"]
```
to:
```toml
exclude = ["crates/wasm-quarto-hub-client", "crates/experiments", "crates/pampa/fuzz"]
```

- [ ] **Step 2: Remove workspace dependency for wasm-qmd-parser**

Delete lines 86-87:
```toml
[workspace.dependencies.wasm-qmd-parser]
path = "./crates/wasm-qmd-parser"
```

- [ ] **Step 3: Update the comment on line 7**

Change line 7 from:
```toml
# - WASM crates: build with wasm-pack or --target wasm32-unknown-unknown
```
to:
```toml
# - WASM crates: require --target wasm32-unknown-unknown and -Zbuild-std (see dev-docs/wasm.md)
```

- [ ] **Step 4: Update the dev profile comment**

In lines 244-249, change the comment from referencing wasm-pack:
```toml
[profile.dev]
# Tell `rustc` to optimize for small code size to
# work around "too many locals" error from wasm-pack
# https://github.com/wasm-bindgen/wasm-bindgen/issues/3451#issuecomment-1562982835
opt-level = "s"
```
to:
```toml
[profile.dev]
# Tell `rustc` to optimize for small code size to
# work around "too many locals" error in WASM builds
# https://github.com/wasm-bindgen/wasm-bindgen/issues/3451#issuecomment-1562982835
opt-level = "s"
```

- [ ] **Step 5: Verify workspace builds**

```bash
cargo check --workspace
```
Expected: Clean build with no errors about missing wasm-qmd-parser.

---

### Task 4: Remove stale wasm-pack install from hub-client-e2e.yml

**Files:**
- Modify: `.github/workflows/hub-client-e2e.yml` (line 47-48)

The workflow installs wasm-pack but never uses it — the WASM build step runs `npm run build:wasm` which calls `build-wasm.js` (uses `wasm-bindgen`, not wasm-pack).

- [ ] **Step 1: Read the file to confirm the exact lines**

Read `.github/workflows/hub-client-e2e.yml` around lines 45-55 to see the wasm-pack step and surrounding context.

- [ ] **Step 2: Remove the wasm-pack install step**

Delete the step:
```yaml
      - name: Install wasm-pack
        run: cargo install wasm-pack
```

- [ ] **Step 3: Verify no other reference to wasm-pack in the file**

```bash
grep -n "wasm-pack" .github/workflows/hub-client-e2e.yml
```
Expected: No output.

---

### Task 5: Update hub-client/README.md

**Files:**
- Modify: `hub-client/README.md`

Remove `wasm-pack` from prerequisites. The actual build tool is `wasm-bindgen-cli` (installed via `cargo xtask dev-setup`).

- [ ] **Step 1: Read the prerequisites section**

Read `hub-client/README.md` to find the prerequisites list (around lines 5-11).

- [ ] **Step 2: Replace the wasm-pack prerequisite**

Change:
```markdown
- `wasm-pack` (`cargo install wasm-pack`)
```
to:
```markdown
- `wasm-bindgen-cli` (`cargo xtask dev-setup` installs the correct version)
```

---

### Task 6: Update wasm-quarto-hub-client/README.md

**Files:**
- Modify: `crates/wasm-quarto-hub-client/README.md`

- [ ] **Step 1: Read the README**

Read `crates/wasm-quarto-hub-client/README.md` and identify any wasm-pack references.

- [ ] **Step 2: Remove or update wasm-pack references**

The line "Always use the build script in `hub-client/scripts/build-wasm.js` rather than running `wasm-pack` directly" should be changed to:

```markdown
Always use the build script in `hub-client/scripts/build-wasm.js` rather than running cargo/wasm-bindgen manually.
```

If there are other wasm-pack references, update them similarly.

---

### Task 7: Update workspace CLAUDE.md

**Files:**
- Modify: `CLAUDE.md` (workspace root, lines 231, 235, 269)

Three references to `wasm-qmd-parser` need updating after the crate is deleted.

- [ ] **Step 1: Read and update the WASM crate listing**

Line 231 lists `wasm-qmd-parser` under the WASM section of workspace structure. Remove the entry:
```markdown
- `wasm-qmd-parser`: WASM module with entry points from `pampa` (see [crates/wasm-qmd-parser/CLAUDE.md](crates/wasm-qmd-parser/CLAUDE.md) for build instructions)
```

- [ ] **Step 2: Update hub-client description**

Line 235 says hub-client "Uses Automerge for real-time sync and the WASM build of `wasm-qmd-parser`". Change to reference the correct crate:
```markdown
A React/TypeScript web application for collaborative editing of Quarto projects. Uses Automerge for real-time sync and the WASM build of `wasm-quarto-hub-client` for live preview rendering.
```

- [ ] **Step 3: Update crate layout note**

Line 269 says `wasm-quarto-hub-client` is "the WASM client (NOT wasm-qmd-parser)". Since wasm-qmd-parser no longer exists, simplify to:
```markdown
- `wasm-quarto-hub-client` is the WASM client for hub-client
```

---

### Task 8: Note on wasm-pack in dev-setup

The design spec mentions removing wasm-pack from `cargo xtask dev-setup`. However, wasm-pack
is **not** in the dev-setup install list (`crates/xtask/src/dev_setup.rs`). It was only installed
via `cargo install wasm-pack` in workflow files (already addressed in Tasks 4 and 2).
No action needed here.

---

### Task 9: Commit Phase 1 cleanup

- [ ] **Step 1: Stage all Phase 1 changes**

```bash
git add -A
git status
```

Review: should show deleted `crates/wasm-qmd-parser/`, deleted `.github/workflows/build-wasm.yml`, modified `Cargo.toml`, modified workflow files, modified READMEs.

- [ ] **Step 2: Commit**

```bash
git commit -m "$(cat <<'EOF'
Remove stale wasm-qmd-parser crate and wasm-pack references

wasm-qmd-parser is fully superseded by wasm-quarto-hub-client.
The build-wasm.yml workflow only built the stale crate.
wasm-pack is not used by the active WASM build pipeline
(build-wasm.js uses cargo build + wasm-bindgen CLI directly).

- Delete crates/wasm-qmd-parser/ entirely
- Delete .github/workflows/build-wasm.yml
- Remove wasm-qmd-parser from workspace exclude and deps
- Remove stale wasm-pack install from hub-client-e2e.yml
- Update hub-client and wasm-quarto-hub-client READMEs
- Update workspace CLAUDE.md and Cargo.toml comments
EOF
)"
```

---

## Phase 2+3: Remove cfg Proxy and Add WASM Tests

These phases land together per the design spec to avoid any validation gap. We add the WASM test infrastructure first (Phase 3 setup), then remove the cfg proxy (Phase 2).

### Task 10: Add wasm-bindgen-test dependency to pampa

**Files:**
- Modify: `crates/pampa/Cargo.toml` (dev-dependencies section, around line 70)

- [ ] **Step 1: Add the dev-dependency**

Add `wasm-bindgen-test` to the `[dev-dependencies]` section. Also add `wasm-bindgen` since
`wasm_bindgen_test` macros require it:

```toml
[dev-dependencies]
insta = { version = "1.46", features = ["json", "redactions"] }
proptest = "1.10"
quarto-util.workspace = true
tempfile = "3.24"
wasm-bindgen = "0.2"
wasm-bindgen-test = "0.3"
```

- [ ] **Step 2: Verify it resolves**

```bash
cargo check -p pampa
```
Expected: compiles. The `wasm-bindgen` and `wasm-bindgen-test` crates are only pulled in for test compilation.

- [ ] **Step 3: Check the resolved wasm-bindgen version**

```bash
cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "wasm-bindgen") | .version'
```

Note the version. If it differs from `0.2.108` (the version `cargo xtask dev-setup` installs for `wasm-bindgen-cli`), the dev-setup pinned version in `crates/xtask/src/dev_setup.rs` will need updating. The versions must match exactly or `wasm-bindgen-test-runner` will refuse to run.

---

### Task 11: Add wasm32 test runner to .cargo/config.toml

**Files:**
- Modify: `.cargo/config.toml` (workspace root)

Current content is just aliases. Add the runner configuration so `cargo test --target wasm32-unknown-unknown` knows to use `wasm-bindgen-test-runner`.

- [ ] **Step 1: Append the runner config**

Add to `.cargo/config.toml`:

```toml

[target.wasm32-unknown-unknown]
runner = "wasm-bindgen-test-runner"
```

The full file should now be:
```toml
# Cargo configuration for the Quarto Rust workspace

[alias]
# Run project-specific tasks via: cargo xtask <command>
# See crates/xtask/src/main.rs for available commands
xtask = "run --package xtask --"
dev-setup = "xtask dev-setup"

[target.wasm32-unknown-unknown]
runner = "wasm-bindgen-test-runner"
```

Note: this `runner` setting only applies to `cargo test`, not `cargo build`. The hub-client WASM production build (`build-wasm.js` → `cargo build`) is unaffected. The `wasm-quarto-hub-client` crate has its own `.cargo/config.toml` with `-Zbuild-std` and rustflags; those settings are scoped to that crate's directory.

---

### Task 12: Create WASM test file

**Files:**
- Create: `crates/pampa/tests/wasm_lua.rs`

These are smoke tests of WASM-specific code paths. They only compile for `wasm32`. They run in Node.js via `wasm-bindgen-test-runner` (default, no browser needed).

**Important context for the implementer:**
- `filter.rs` line 123: The `#[cfg(target_arch = "wasm32")]` block creates a restricted Lua VM via `Lua::new_with()` and registers synthetic `io_wasm` and `os_wasm` modules.
- `shortcode.rs` line 72: Same pattern for the shortcode engine.
- `io_wasm.rs` provides `register_wasm_io()` which registers `io.open`, `io.type`, etc. as Lua globals backed by `SystemRuntime`.
- `os_wasm.rs` provides `register_wasm_os()` which registers `os.time`, `os.clock`, `os.difftime`.
- The tests need access to pampa's internal types. Since this is an integration test file (in `tests/`), it can only use pampa's public API. Check what pampa exports.

- [ ] **Step 1: Check pampa's public API for what we need**

Read `crates/pampa/src/lib.rs` to see what's publicly exported. We need to find:
- How to create a `SystemRuntime` (or equivalent) for WASM
- How to invoke filter execution
- How to invoke shortcode execution
- Whether `io_wasm` / `os_wasm` registration functions are public

If key functions are not public, we may need to add `#[cfg(target_arch = "wasm32")]` pub exports or use a different test approach. The design spec lists 6 smoke tests:

1. Restricted Lua VM creation — `Lua::new_with()` with restricted stdlib succeeds
2. Filter execution — run a simple filter on a small document
3. Shortcode engine — create engine, dispatch a basic handler
4. Error handling — Lua error gets caught as Rust error (not WASM crash)
5. Synthetic io registration — `io.open`, `io.type` available as globals
6. Synthetic os registration — `os.time`, `os.clock`, `os.difftime` available

- [ ] **Step 2: Write the test file**

Create `crates/pampa/tests/wasm_lua.rs`. The exact test implementations depend on what pampa exports (determined in Step 1). Here is the skeleton with the tests we can write for certain:

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
//!
//! **How to run:** (Linux/macOS only, requires nightly + Clang + wasm-sysroot)
//! ```
//! CC_wasm32_unknown_unknown=clang \
//! CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
//! cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
//!   --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
//! ```

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
// Tests run in Node.js by default. No wasm_bindgen_test_configure!(run_in_browser)
// needed — current tests don't require browser APIs.
```

The actual test function bodies depend on pampa's public API discovered in Step 1. The implementer must:

1. Check which types/functions pampa re-exports for WASM consumers
2. For each of the 6 smoke tests, write a `#[wasm_bindgen_test]` function
3. Tests should be minimal — verify the setup works, not duplicate native test coverage

Example patterns for the test bodies (adapt to actual API):

```rust
/// Smoke test: restricted Lua VM creation succeeds on real wasm32 target.
#[wasm_bindgen_test]
fn restricted_lua_vm_creation() {
    // Create a Lua VM the same way filter.rs does under #[cfg(target_arch = "wasm32")]
    use mlua::{Lua, StdLib};
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua = Lua::new_with(libs, mlua::LuaOptions::default())
        .expect("restricted Lua VM creation should succeed on wasm32");
    // Verify a basic operation works
    let result: i64 = lua.load("1 + 1").eval().unwrap();
    assert_eq!(result, 2);
}

/// Smoke test: Lua error is caught as Rust error, not a WASM crash.
/// Validates that -Zbuild-std=std,panic_unwind works correctly.
#[wasm_bindgen_test]
fn lua_error_caught_as_rust_error() {
    use mlua::{Lua, StdLib};
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua = Lua::new_with(libs, mlua::LuaOptions::default()).unwrap();
    let result: Result<(), _> = lua.load("error('test error')").exec();
    assert!(result.is_err(), "Lua error should propagate as Rust error");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("test error"), "Error message should be preserved");
}
```

For tests 2, 3, 5, 6 (filter execution, shortcode engine, io/os registration): these require pampa internals. If pampa doesn't export enough API, the implementer should either:
- Add minimal `#[cfg(target_arch = "wasm32")] pub` exports to pampa's `lib.rs`
- Or test via Lua: create the restricted VM, manually call `register_wasm_io`/`register_wasm_os`, then run Lua code that exercises those modules

The Lua-based approach is preferred since it tests the actual code path without needing to modify pampa's public API:

```rust
/// Smoke test: synthetic io module is available after registration.
#[wasm_bindgen_test]
fn synthetic_io_registration() {
    // This test verifies that io_wasm registers correctly on real wasm32.
    // It creates the VM + registers modules the same way filter.rs does.
    use mlua::{Lua, StdLib};
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua = Lua::new_with(libs, mlua::LuaOptions::default()).unwrap();
    // Register synthetic io (needs SystemRuntime — check pampa API)
    // pampa::lua::io_wasm::register_wasm_io(&lua, runtime)?;
    //
    // Then verify:
    let has_io_open: bool = lua.load("type(io.open) == 'function'").eval().unwrap();
    assert!(has_io_open, "io.open should be registered");
    let has_io_type: bool = lua.load("type(io.type) == 'function'").eval().unwrap();
    assert!(has_io_type, "io.type should be registered");
}
```

**Note for implementer:** `mlua` must be used directly from `crates/pampa/tests/wasm_lua.rs`. Since `mlua` is a dependency of pampa (behind the `lua-filter` feature), and this test file is compiled with `--features lua-filter`, `mlua` should be available. If not, add `mlua` as a dev-dependency of pampa with the same features.

- [ ] **Step 3: Verify the test file compiles for native (should be skipped)**

```bash
cargo check -p pampa --tests
```

Expected: compiles cleanly. The `#![cfg(target_arch = "wasm32")]` means the entire file is excluded on native. No compilation errors.

---

### Task 13: Remove cfg proxy from filter.rs

**Files:**
- Modify: `crates/pampa/src/lua/filter.rs` (lines 120-134)

- [ ] **Step 1: Read the current code**

Read `crates/pampa/src/lua/filter.rs` lines 118-136 to see the exact current state.

- [ ] **Step 2: Change the cfg guards**

Change line 123 from:
```rust
    #[cfg(any(target_arch = "wasm32", test))]
```
to:
```rust
    #[cfg(target_arch = "wasm32")]
```

Change line 133 from:
```rust
    #[cfg(not(any(target_arch = "wasm32", test)))]
```
to:
```rust
    #[cfg(not(target_arch = "wasm32"))]
```

- [ ] **Step 3: Update the comment above the cfg blocks**

The comment on lines 121-122 currently reads:
```rust
    // On WASM, we can't load all libraries (no package/io/os/debug support),
    // so use a restricted set. On native, load everything for full compatibility.
```

Keep this comment as-is — it's accurate. The comment about test environment (if any additional
comment exists) should be removed.

---

### Task 14: Remove cfg proxy from shortcode.rs

**Files:**
- Modify: `crates/pampa/src/lua/shortcode.rs` (lines 72-86)

- [ ] **Step 1: Read the current code**

Read `crates/pampa/src/lua/shortcode.rs` lines 70-88.

- [ ] **Step 2: Change the cfg guards**

Change line 72 from:
```rust
        #[cfg(any(target_arch = "wasm32", test))]
```
to:
```rust
        #[cfg(target_arch = "wasm32")]
```

Change line 85 from:
```rust
        #[cfg(not(any(target_arch = "wasm32", test)))]
```
to:
```rust
        #[cfg(not(target_arch = "wasm32"))]
```

---

### Task 15: Verify native tests pass after cfg proxy removal

This is the critical verification step. The 8 filter traversal tests that use `io.open` should now pass on all platforms because they use `Lua::new()` with real C stdlib instead of the synthetic WASM io.

- [ ] **Step 1: Run pampa tests**

```bash
cargo nextest run -p pampa
```

Expected: All tests pass, including the 8 filter traversal tests that previously failed on Windows with "os error 123".

- [ ] **Step 2: Run full workspace tests**

```bash
cargo nextest run --workspace
```

Expected: All tests pass. Changes to pampa's cfg guards could theoretically affect downstream crates.

---

### Task 16: Commit Phase 2+3

- [ ] **Step 1: Stage and review**

```bash
git add -A
git diff --cached --stat
```

Expected changes: `crates/pampa/Cargo.toml` (new dev-deps), `.cargo/config.toml` (runner), `crates/pampa/tests/wasm_lua.rs` (new), `crates/pampa/src/lua/filter.rs` (cfg change), `crates/pampa/src/lua/shortcode.rs` (cfg change).

- [ ] **Step 2: Commit**

```bash
git commit -m "$(cat <<'EOF'
Replace cfg test proxy with real WASM tests

Remove `test` from `#[cfg(any(target_arch = "wasm32", test))]` guards
in filter.rs and shortcode.rs so native tests use Lua::new() with real
C stdlib on all platforms. This fixes the 8 filter traversal tests that
failed on Windows because the synthetic io.open only handles POSIX VFS
paths.

Add wasm-bindgen-test smoke tests in crates/pampa/tests/wasm_lua.rs
that run on the real wasm32 target, validating:
- Restricted Lua VM creation
- Filter execution through WASM code path
- Shortcode engine on WASM
- Error handling (panic_unwind works)
- Synthetic io/os module registration

Configure wasm-bindgen-test-runner in .cargo/config.toml.
WASM tests require nightly + Clang + wasm-sysroot (Linux/macOS CI only).
EOF
)"
```

---

## Phase 4: CI Integration

### Task 17: Add wasm-tests job to test-suite.yml

**Files:**
- Modify: `.github/workflows/test-suite.yml`

Add a new job that runs the WASM tests on Linux only. Mirror the Clang/wasm-sysroot setup from `ts-test-suite.yml` and `hub-client/scripts/build-wasm.js`.

- [ ] **Step 1: Read the current workflow**

Read `.github/workflows/test-suite.yml` to understand the structure, triggers, and existing jobs.

- [ ] **Step 2: Add the wasm-tests job**

Add a new job after the existing `test-suite` job. The job should:
- Run on `ubuntu-latest` only (no matrix — WASM tests are Linux-only)
- Use the same trigger paths as the existing test suite, plus `crates/pampa/tests/wasm_lua.rs`
- Set up: Rust nightly, Clang, rust-src component, wasm-bindgen-test-runner (via `cargo xtask dev-setup`)
- Run the WASM test command

```yaml
  wasm-tests:
    name: WASM Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 1

      - name: Set up Rust nightly
        uses: dtolnay/rust-toolchain@nightly
        with:
          targets: wasm32-unknown-unknown
          components: rust-src

      - name: Set up Clang
        uses: egor-tensin/setup-clang@v1
        with:
          version: latest
          platform: x64

      - name: Rust cache
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: rust-wasm-tests

      - name: Install wasm-bindgen-cli
        run: cargo xtask dev-setup

      - name: Run WASM tests
        run: |
          CC_wasm32_unknown_unknown=clang \
          CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
          cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
            --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

- [ ] **Step 3: Verify the workflow YAML is valid**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/test-suite.yml'))"
```
Or use `yq` if available. Expected: no parse errors.

---

### Task 18: Migrate wasm-bindgen-cli install in ts-test-suite.yml

**Files:**
- Modify: `.github/workflows/ts-test-suite.yml` (line 125-127)

The design spec says to migrate the hardcoded `cargo install wasm-bindgen-cli --version 0.2.108` to use `cargo xtask dev-setup`, which reads the version from Cargo.lock and keeps it in sync.

- [ ] **Step 1: Read the current install step**

Read `.github/workflows/ts-test-suite.yml` around lines 123-130.

- [ ] **Step 2: Replace the install step**

Change:
```yaml
      - name: Install wasm-bindgen-cli
        run: cargo install wasm-bindgen-cli --version 0.2.108
```
to:
```yaml
      - name: Install dev tools (wasm-bindgen-cli)
        run: cargo xtask dev-setup
```

Note: `cargo xtask dev-setup` also installs `cargo-nextest` and `cargo-insta`, but those may already be installed by a previous step. The setup is idempotent so this is fine — the tools are cached and skip reinstall if present.

---

### Task 19: Commit CI changes

- [ ] **Step 1: Stage and commit**

```bash
git add .github/workflows/test-suite.yml .github/workflows/ts-test-suite.yml
git commit -m "$(cat <<'EOF'
Add WASM test CI job and migrate wasm-bindgen-cli to dev-setup

Add wasm-tests job to test-suite.yml that runs pampa WASM smoke tests
on Linux with nightly Rust + Clang + wasm-sysroot. Uses cargo xtask
dev-setup for version-matched wasm-bindgen-cli installation.

Migrate ts-test-suite.yml from hardcoded wasm-bindgen-cli version to
cargo xtask dev-setup for consistent version management.
EOF
)"
```

---

## Phase 5: Documentation

### Task 20: Rewrite dev-docs/wasm.md

**Files:**
- Modify: `dev-docs/wasm.md` (full rewrite)

This becomes the single source of truth for WASM in this project. Current content is outdated (references wasm-qmd-parser and wasm-pack).

- [ ] **Step 1: Write the new content**

```markdown
# WASM in the Quarto Rust Monorepo

## Architecture

`wasm-quarto-hub-client` wraps `pampa` + `quarto-core` for the hub-client web app.
It compiles to a WASM module that runs in the browser, providing live preview rendering.

The crate is **excluded from the default workspace** (`Cargo.toml` `exclude` list) because
it requires `--target wasm32-unknown-unknown` and `-Zbuild-std=std,panic_unwind`.

## Build

The production WASM build is handled by `hub-client/scripts/build-wasm.js`:

```bash
cd hub-client
npm run build:wasm    # WASM module only
npm run build:all     # WASM + TypeScript
```

The build script runs:
1. `cargo build -p wasm-quarto-hub-client --target wasm32-unknown-unknown`
   with `-Zbuild-std=std,panic_unwind` (via `crates/wasm-quarto-hub-client/.cargo/config.toml`)
2. `wasm-bindgen` CLI to generate JS/TS bindings

### Why not wasm-pack?

This project uses `cargo build` + `wasm-bindgen` CLI directly because:
- `-Zbuild-std=std,panic_unwind` is required for Lua error handling (setjmp/longjmp to
  panic/catch_unwind). wasm-pack doesn't support `-Zbuild-std`.
- wasm-pack is deprecated (rustwasm org sunset September 2025).

### C toolchain requirement

`pampa` with `lua-filter` pulls in `mlua` → `lua-src-wasm`, which compiles Lua from C source
via the `cc` crate. When targeting wasm32, this requires Clang with wasm32 support:

```bash
# Set by build-wasm.js automatically for production builds.
# For manual builds or tests:
export CC_wasm32_unknown_unknown=clang
export CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin"
```

## Testing

### Native tests (all platforms)

Native Rust tests (`cargo nextest run`) test filter and shortcode logic using `Lua::new()`
with the full C stdlib. These run on all platforms including Windows.

### WASM smoke tests (Linux CI)

`crates/pampa/tests/wasm_lua.rs` contains smoke tests that compile and run on the real
`wasm32-unknown-unknown` target. They verify the WASM-specific Lua VM setup:
- Restricted stdlib creation (`Lua::new_with()`)
- Synthetic `io`/`os` module registration
- Filter and shortcode execution through the WASM code path
- Error handling (`panic_unwind` works correctly)

Run locally (Linux/macOS with LLVM):
```bash
CC_wasm32_unknown_unknown=clang \
CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

**Important:** You must use `--test wasm_lua` to select only the WASM test file.
Running `cargo test -p pampa --target wasm32` without `--test` will fail because
native tests can't compile for wasm32.

WASM tests are **not** part of `cargo xtask verify` — they require nightly + Clang with
wasm32 support, which is Linux/macOS only. They run in the `wasm-tests` CI job.

### Hub-client integration tests

The hub-client test suite (`npm run test:ci`) tests the compiled WASM module through
JavaScript, covering rendering, templates, and format detection. These complement
the Rust-level WASM smoke tests.
```

- [ ] **Step 2: Verify no broken links**

Check that all referenced files exist:
```bash
ls hub-client/scripts/build-wasm.js crates/wasm-quarto-hub-client/.cargo/config.toml crates/pampa/tests/wasm_lua.rs crates/wasm-quarto-hub-client/wasm-sysroot/
```

---

### Task 21: Update pampa/CLAUDE.md

**Files:**
- Modify: `crates/pampa/CLAUDE.md`

Add a section about WASM tests so AI assistants know when and how to add them.

- [ ] **Step 1: Append WASM testing section**

Add at the end of `crates/pampa/CLAUDE.md`:

```markdown

## WASM Testing

When modifying WASM-specific code paths (the `#[cfg(target_arch = "wasm32")]` blocks in
`filter.rs`/`shortcode.rs`, `io_wasm.rs`, or `os_wasm.rs`), add or update smoke tests in
`tests/wasm_lua.rs`.

**Never add `test` to the `target_arch = "wasm32"` cfg guard.** Native tests must use
`Lua::new()` with the real C stdlib. WASM-specific setup is validated by the dedicated
WASM tests.

WASM tests can't run on Windows. On Linux/macOS with LLVM:
```
CC_wasm32_unknown_unknown=clang \
CFLAGS_wasm32_unknown_unknown="-isystem crates/wasm-quarto-hub-client/wasm-sysroot -fno-builtin" \
cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown \
  --no-default-features --features lua-filter -Zbuild-std=std,panic_unwind
```

See `dev-docs/wasm.md` for full WASM architecture and testing details.
```

---

### Task 22: Create .claude/rules/wasm.md

**Files:**
- Create: `.claude/rules/wasm.md`

This AI rule prevents future regression of the cfg proxy pattern.

- [ ] **Step 1: Write the rule**

```markdown
# WASM Code Rules

## Never add `test` to wasm32 cfg guards

The cfg pattern `#[cfg(any(target_arch = "wasm32", test))]` is prohibited. It forces
native tests through the WASM-restricted Lua stdlib, which fails on Windows.

Correct pattern:
```rust
#[cfg(target_arch = "wasm32")]
// WASM-specific code (restricted Lua stdlib, synthetic io/os)

#[cfg(not(target_arch = "wasm32"))]
// Native code (full Lua stdlib via Lua::new())
```

## Verify WASM tests when editing WASM code

When modifying any of these files, update `crates/pampa/tests/wasm_lua.rs`:
- `crates/pampa/src/lua/filter.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/shortcode.rs` (cfg(target_arch = "wasm32") blocks)
- `crates/pampa/src/lua/io_wasm.rs`
- `crates/pampa/src/lua/os_wasm.rs`

WASM tests can't run locally on Windows — they run in Linux CI.
See `dev-docs/wasm.md` for the local run command (Linux/macOS).
```

---

### Task 23: Update claude-notes/instructions/testing.md

**Files:**
- Modify: `claude-notes/instructions/testing.md` (lines 9-22, the WASM-Restricted Stdlib section)

This section currently describes the cfg proxy pattern. Update it to reflect the new approach.

- [ ] **Step 1: Read the current section**

Read `claude-notes/instructions/testing.md` lines 1-30 to see the exact text.

- [ ] **Step 2: Replace the WASM-Restricted Stdlib section**

Replace the section (approximately lines 9-22) that starts with "Shortcode and filter tests always run against the WASM-restricted Lua stdlib" with:

```markdown
## Native vs WASM Lua Testing

Native tests (`cargo nextest run`) use `Lua::new()` with the full C stdlib on all platforms.
This is the standard Lua environment — tests can use `io.open`, `os.time`, and all standard
library functions.

WASM-specific code paths (restricted Lua stdlib, synthetic io/os modules) are tested by
dedicated smoke tests in `crates/pampa/tests/wasm_lua.rs` that run on the real
`wasm32-unknown-unknown` target in CI. See `crates/pampa/CLAUDE.md` for details on when
to add WASM tests.

**Never add `test` to the `#[cfg(target_arch = "wasm32")]` guard.** This was a prior pattern
that caused Windows test failures. WASM coverage is provided by the real WASM tests in CI.
```

---

### Task 24: Commit documentation

- [ ] **Step 1: Stage and commit**

```bash
git add dev-docs/wasm.md crates/pampa/CLAUDE.md .claude/rules/wasm.md claude-notes/instructions/testing.md
git commit -m "$(cat <<'EOF'
Document WASM testing convention and architecture

Rewrite dev-docs/wasm.md as single source of truth for WASM in this
project: architecture, build pipeline, testing strategy, and C toolchain
requirements.

Add WASM testing guidance to pampa/CLAUDE.md and .claude/rules/wasm.md
to prevent regression of the cfg test proxy pattern.

Update testing.md to reflect the new native vs WASM testing approach.
EOF
)"
```

---

## Phase 6: Final Verification

### Task 25: Full workspace verification

- [ ] **Step 1: Build the full workspace**

```bash
cargo build --workspace
```
Expected: clean build.

- [ ] **Step 2: Run full workspace tests**

```bash
cargo nextest run --workspace
```
Expected: all tests pass, including the 8 previously-failing Windows filter traversal tests.

- [ ] **Step 3: Run cargo xtask lint**

```bash
cargo xtask lint
```
Expected: no lint violations.

- [ ] **Step 4: Ask Chris to verify hub-client build**

The hub-client WASM build (`npm run build:all`) should be unaffected since we only changed:
- `.cargo/config.toml` (added runner, only affects `cargo test`)
- pampa dev-dependencies (only affects test compilation)
- cfg guards (WASM path unchanged, native path simplified)

Ask Chris: "Should I run `cargo xtask verify` to confirm the hub-client build is unaffected?"

---

### Task 26: Update beads issue

- [ ] **Step 1: Close the beads issue**

```bash
br close bd-itj9 --reason "All phases complete: stale wasm-qmd-parser removed, cfg proxy replaced with real WASM tests, CI job added, documentation updated"
```

- [ ] **Step 2: Sync beads**

From the **main repo** (not the worktree, since beads redirect is active):
```bash
cd /c/Users/chris/Documents/DEV_R/q2
br sync --flush-only
git add .beads/
git commit -m "Sync beads: close bd-itj9 WASM testing and cleanup"
```
