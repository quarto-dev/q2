# Plan: Built-in Extensions Infrastructure

## Status: Complete (Phases 1-5)

---

## Overview

Add infrastructure for built-in extensions that ship with the q2 binary,
matching TS Quarto's `src/resources/extensions/quarto/` pattern. Built-in
extensions are normal extensions discovered before user extensions, but user
extensions with the same name override them (last-writer-wins in the
extensions map).

The first built-in extension is `quarto/lipsum`, copied verbatim from
TS Quarto.

## Prerequisites (completed)

- **Pandoc-compatible fuzzy type coercion** (`72d5d7af`): `pandoc.Para("text")`
  now auto-coerces plain strings via `peek_inlines_fuzzy`, matching
  pandoc-lua-marshal behavior. This is required because TS Quarto's
  `lipsum.lua` calls `pandoc.Para(paras[outIdx])` with a bare string.

## Codebase Context

### Extension discovery (current)
- `crates/quarto-core/src/extension/discover.rs` — `discover_extensions()`
  walks from input dir up to project root, scanning `_extensions/` dirs.
  Returns `Vec<Extension>` with project-level first (lower priority),
  subdirectory-level last (higher priority).
- `scan_extension_entry()` is already generic: it checks for
  `_extension.yml` directly (unorganized), or recurses one level treating
  the entry as an organization dir (org/name pattern). This means built-in
  extension dirs with `quarto/lipsum/_extension.yml` structure will be
  scanned correctly without changes to the scanning logic.
- `crates/quarto-core/src/stage/context.rs:103` — calls
  `discover_extensions()` and stores result in `StageContext.extensions`.

### Extension lookup callers
`find_extension()` is called from **3 places** — all must use last-match
semantics for user-wins override:
1. `shortcode_resolve.rs:331` — shortcode extension lookup
2. `filter_resolve.rs:200` — filter extension lookup
3. `metadata_merge.rs:95` — format extension lookup

### Shortcode resolution priority (current)
`shortcode_resolve.rs:304`:
1. Built-in Rust handlers (e.g., `meta`) — highest priority
2. Already-loaded Lua handlers
3. Name-based extension lookup (on-demand loading) — lowest priority

### Resource embedding patterns
- **Native**: `include_dir!` + `ResourceBundle` (lazy extract to temp dir)
- **WASM**: `EmbeddedResources` populated into VFS at
  `/__quarto_resources__/` prefix, preserved across `vfs.clear()`

### TS Quarto behavior to match
- Built-in extensions live under org `"quarto"` (`kBuiltInExtOrg`)
- `builtinExtensions()` returns path to `resources/extensions/` (no
  `_extensions/` wrapper needed)
- `readExtensions()` is generic: entries without `_extension.yml` are
  treated as org dirs and recursed one level — same as our
  `scan_extension_entry()`
- `inputExtensionDirs()` returns `[builtinExtensions(), ...user dirs]`
- `loadExtensions()` iterates in order, later entries overwrite earlier
  → **user extensions override built-ins with same ID**
- Built-in shortcode handlers (meta, env, etc.) are separate and always
  override — they are NOT extensions

### Key files
| File | Role |
|---|---|
| `crates/quarto-core/src/extension/discover.rs` | Extension discovery |
| `crates/quarto-core/src/stage/context.rs` | Pipeline wiring |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Shortcode resolution |
| `crates/quarto-core/src/filter_resolve.rs` | Filter extension lookup |
| `crates/quarto-core/src/stage/stages/metadata_merge.rs` | Format extension lookup |
| `crates/quarto-core/src/resources.rs` | `ResourceBundle` for native |
| `crates/quarto-sass/src/resources.rs` | `RESOURCE_PATH_PREFIX` = `/__quarto_resources__` |
| `crates/wasm-quarto-hub-client/src/lib.rs` | WASM init, VFS population |
| `crates/quarto-system-runtime/src/wasm.rs` | VFS, `clear_preserving_prefix` |

---

## Work Items

### Phase 1: Add `resources/extensions/` directory with lipsum

- [x] **1.1** Create `resources/extensions/quarto/lipsum/` with files copied
  verbatim from TS Quarto (`~/src/quarto-cli/src/resources/extensions/quarto/lipsum/`):
  - `_extension.yml` — use TS Quarto's version (title: Lipsum, author: Charles Teague, version: 1.0.2)
  - `lipsum.lua` — TS Quarto's version (uses `pandoc.Para(paras[outIdx])`,
    NOT the q2 test fixture's `pandoc.Para({pandoc.Str(paras[outIdx])})`)
  - `lipsum.json` — full 17-paragraph version from TS Quarto

  TS Quarto's `lipsum.lua` calls `pandoc.Para(paras[outIdx])` with a bare
  string. This works in q2 as of `72d5d7af` (fuzzy type coercion).

### Phase 2: Native — embed and discover built-in extensions

- [x] **2.1** Add `ResourceBundle` in `crates/quarto-core/src/extension/mod.rs`
  (or a new `builtin.rs`):
  ```rust
  use include_dir::{include_dir, Dir};
  use crate::resources::ResourceBundle;

  static BUILTIN_EXTENSIONS_DIR: Dir =
      include_dir!("$CARGO_MANIFEST_DIR/../../resources/extensions");
  pub static BUILTIN_EXTENSIONS: ResourceBundle =
      ResourceBundle::new("builtin-extensions", &BUILTIN_EXTENSIONS_DIR);
  ```

- [x] **2.2** Modify `discover_extensions()` signature to accept an optional
  built-in extensions path:
  ```rust
  pub fn discover_extensions(
      input: &Path,
      project_dir: Option<&Path>,
      builtin_extensions_dir: Option<&Path>,
      runtime: &dyn SystemRuntime,
  ) -> Vec<Extension>
  ```
  When `builtin_extensions_dir` is `Some`, scan it **first** (before user
  dirs). Since user dirs are scanned after, and `find_extension()` currently
  returns the first match, we need to change `find_extension()` to return
  the **last** match instead. This matches TS Quarto's "later overwrites
  earlier" semantics.

  No changes needed to `scan_extension_entry()` — it already handles the
  org/name directory structure generically (checks for `_extension.yml`,
  else recurses one level treating the entry as an org dir).

- [x] **2.3** Update `StageContext::new()` in `context.rs` to extract the
  `ResourceBundle` path and pass it to `discover_extensions()`:
  ```rust
  let builtin_ext_path = crate::extension::BUILTIN_EXTENSIONS.path().ok();
  let extensions = crate::extension::discover_extensions(
      &document.input,
      project_dir,
      builtin_ext_path,
      runtime.as_ref(),
  );
  ```

- [x] **2.4** Write unit tests for built-in extension discovery:
  - Test that built-in lipsum is discovered when no user extensions exist
  - Test that a user `_extensions/lipsum/` overrides the built-in
  - Test that a user `_extensions/quarto/lipsum/` (with org) also overrides

### Phase 3: WASM — populate VFS with built-in extensions

- [x] **3.1** In `crates/wasm-quarto-hub-client/src/lib.rs`, add the
  built-in extensions to `populate_vfs_with_embedded_resources()`.
  The extensions need to be embedded in the WASM crate separately (it has
  its own Cargo.toml). Add an `EmbeddedResources` or `include_dir!` for
  the extensions directory, and populate files under
  `/__quarto_resources__/extensions/quarto/lipsum/...`.

- [x] **3.2** Modify `discover_extensions()` in WASM context: pass
  `/__quarto_resources__/extensions` as the `builtin_extensions_dir`.
  The VFS's `dir_list()` and `path_exists()` already work on these paths.

- [x] **3.3** WASM path resolution verification: the lipsum Lua script uses
  `quarto.utils.resolve_path("lipsum.json")` which resolves relative to
  the extension's directory, and `io.open()` which uses the synthetic Lua
  io tables (added in `96635fb2`). Both need to work with VFS paths under
  `/__quarto_resources__/extensions/`. Verify this in the end-to-end test.

- [x] **3.4** Verify the lipsum extension works end-to-end in WASM:
  build WASM, add a test qmd with `{{< lipsum 1 >}}` to the VFS, render,
  and check output contains lorem ipsum text.

### Phase 4: Update lipsum smoke test to use built-in

- [x] **4.1** Remove the local `_extensions/lipsum/` from the lipsum
  smoke test fixture at
  `crates/quarto/tests/smoke-all/extensions/lipsum-shortcode/`. The test
  should now discover lipsum from the built-in extensions. Rename the test
  directory to reflect it's testing built-in extension discovery (e.g.,
  `builtin-lipsum-shortcode/`).

- [x] **4.2** Add a new smoke test that has BOTH a local `_extensions/lipsum/`
  AND the built-in, to verify user override behavior. The local extension
  should produce different output (e.g., always return "USER_OVERRIDE") so
  the test can assert which one ran.

### Phase 5: Verify

- [x] **5.1** `cargo nextest run -p quarto-core` — extension discovery tests
- [x] **5.2** `cargo nextest run -p quarto --test smoke_all` — lipsum smoke tests
- [x] **5.3** `cargo nextest run --workspace` — no regressions
- [x] **5.4** `cargo xtask verify` — full verification including WASM build

## Design Notes

### Why `find_extension` should return the last match

Currently `find_extension()` uses `.find()` which returns the first match.
With built-ins prepended to the vec, that means built-ins would win. TS
Quarto's `loadExtensions()` uses a map where later entries overwrite
earlier ones, achieving "user wins". Changing to `.rfind()` (or reversing
iteration) is the minimal change to match this behavior.

This affects all 3 callers (`shortcode_resolve.rs`, `filter_resolve.rs`,
`metadata_merge.rs`) — the user-wins semantics is correct for all of them.

### No directory structure mismatch

Our `scan_extension_entry()` already handles both patterns:
1. Direct extension: entry has `_extension.yml` → load it
2. Organization dir: entry has no `_extension.yml` → recurse one level

This matches TS Quarto's `readExtensions()` which uses the same two-level
scan. The built-in `resources/extensions/quarto/lipsum/_extension.yml`
structure will be scanned correctly: `quarto/` has no `_extension.yml` so
it's treated as an org dir, then `lipsum/` has `_extension.yml` and is
loaded with `organization: "quarto"`.

### WASM crate has separate dependencies

`crates/wasm-quarto-hub-client/` is excluded from the workspace and has
its own `Cargo.toml`. If we use `include_dir!` there, it needs its own
dependency on the `include_dir` crate, and the path must be relative to
that crate's `CARGO_MANIFEST_DIR`.

### WASM path resolution for extension resources

In WASM, `quarto.utils.resolve_path("lipsum.json")` resolves relative to
the extension's directory path. For built-in extensions, this path will be
under `/__quarto_resources__/extensions/quarto/lipsum/`. The synthetic
`io.open()` (added in `96635fb2`) reads from the VFS. Both should work
since the extension's `path` field will point to the VFS location, but
this is explicitly verified in Phase 3.3.

### No changes to hub-client discovery.rs

The hub project discovery (`crates/quarto-hub/src/discovery.rs`) only
handles file syncing for collaborative editing. Built-in extensions don't
need to be synced to collaborators — they're embedded in the binary on
both sides. No changes needed there.

## Files Touched

| File | Change |
|---|---|
| `resources/extensions/quarto/lipsum/` | New: verbatim copy from TS Quarto |
| `crates/quarto-core/src/extension/discover.rs` | Add `builtin_extensions_dir` param, change `find_extension` to last-match |
| `crates/quarto-core/src/extension/mod.rs` (or `builtin.rs`) | New: `ResourceBundle` for built-in extensions |
| `crates/quarto-core/src/stage/context.rs` | Pass built-in path to discovery |
| `crates/wasm-quarto-hub-client/src/lib.rs` | Populate VFS with built-in extensions |
| `crates/quarto/tests/smoke-all/extensions/lipsum-shortcode/` | Remove local extension, test built-in |
| New smoke test fixture | Test user-override of built-in extension |
