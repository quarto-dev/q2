# SASS Cache Key Refinement

**Branch**: `feature/project-metadata`
**Plan file**: `claude-notes/plans/2026-03-10-sass-cache-key-refinement.md`
**Symlinked from**: `claude-notes/plans/CURRENT.md`

## Session Guide

This plan spans multiple sessions. After compaction or at the start of a new session:
1. Read THIS file to see which items are checked off
2. Resume from the first unchecked item
3. Each phase can be committed independently

**Suggested session splits:**
- Session A: Phases 1-2 (Rust — build.rs move + cache key rewrite)
- Session B: Phases 3-4 (JS/TS — IndexedDB LRU + dead code removal)
- Session C: Phase 5 (full verification with `cargo xtask verify`)

## Overview

The recent CSS-in-pipeline work (commits f357f5ad..60750e13) moved SASS compilation
into `CompileThemeCssStage`, which was the right architectural move. However, the
caching strategy changed in ways that were expedient rather than necessary:

1. **Hash function**: SHA-256 → `DefaultHasher` (64-bit, unstable across Rust versions)
2. **Hash input**: individual theme files → full assembled SCSS (~224KB)
3. **LRU eviction**: present → absent (both WASM IndexedDB and native filesystem)

These changes were not required by the new architecture. This plan restores SHA-256
and hash-before-assemble, adds LRU eviction with touch-on-read, and removes the old
`SassCacheManager` which is now dead code.

### Cache key design

New key: `SHA256(SCSS_RESOURCES_HASH + theme_identities + custom_file_contents + minified)`

- `SCSS_RESOURCES_HASH`: build-time SHA-256 of all `.scss` files under `resources/scss/`.
  Covers Bootstrap, Quarto customizations, title block, and built-in theme files. Moved
  from `wasm-quarto-hub-client/build.rs` to `quarto-sass` so both native and WASM use it.
- `theme_identities`: for `BuiltIn(cosmo)`, the string `"cosmo"` (content already covered
  by `SCSS_RESOURCES_HASH`). For `Custom(path)`, the resolved path string.
- `custom_file_contents`: for each `Custom` theme spec, the file contents (read via runtime).
  Only custom files need reading; built-in files are static and covered by the build hash.
- `minified`: the boolean flag.

On cache hit, assembly is skipped entirely. On cache miss, assemble and compile as before.
Custom file contents are read once for the cache key; on cache miss, `assemble_theme_scss`
reads them again via `load_custom_theme`. This double-read is acceptable: it only happens
on miss, custom files are small, and on WASM the reads hit an in-memory VFS.

### Out of scope

- The CSS version comment (hashing compiled CSS content) is correct and stays as-is.
- Native caching is only enabled for project renders (single-file renders have no cache_dir).
  Not changed here.

## Key Files Reference

Understanding where everything lives before starting:

### Rust side

| File | Role |
|------|------|
| `crates/quarto-sass/Cargo.toml` | Needs `sha2` as build-dependency (Phase 1) |
| `crates/quarto-sass/src/lib.rs` | Will expose `SCSS_RESOURCES_HASH` const (Phase 1) |
| `crates/quarto-sass/build.rs` | **Does not exist yet** — create it (Phase 1) |
| `crates/wasm-quarto-hub-client/build.rs` | Source of `compute_scss_resources_hash()` and `collect_scss_files()` to move (Phase 1) |
| `crates/wasm-quarto-hub-client/src/lib.rs` | Has `get_scss_resources_version()` export — uses its own `SCSS_RESOURCES_HASH`, will switch to `quarto_sass::SCSS_RESOURCES_HASH` (Phase 1), fn removed in Phase 4 |
| `crates/quarto-core/src/stage/stages/compile_theme_css.rs` | Main target for Phase 2. Contains `cache_key()` (line 62), `CompileThemeCssStage::run()`, and tests |
| `crates/quarto-core/Cargo.toml` | Needs `sha2` as dependency (Phase 2) |
| `crates/quarto-sass/src/themes.rs` | `ThemeSpec` enum (`BuiltIn(BuiltInTheme)` / `Custom(PathBuf)`), `ThemeContext`, `load_custom_theme()`, `process_theme_specs()` |
| `crates/quarto-sass/src/compile.rs` | `assemble_theme_scss()` — called on cache miss, reads custom files internally |

### JS/TS side (hub-client)

| File | Role |
|------|------|
| `hub-client/src/wasm-js-bridge/cache.js` | IndexedDB cache bridge — redesign schema + add LRU (Phase 3) |
| `hub-client/src/wasm-js-bridge/cache.d.ts` | Type declarations for cache.js — update if exports change |
| `hub-client/src/wasm-js-bridge/cache.test.ts` | Tests for cache bridge — add eviction + touch tests (Phase 3) |
| `hub-client/src/services/sassCache.ts` | **Dead code** — entire file removed (Phase 4) |
| `hub-client/src/services/sassCache.test.ts` | **Dead code** — entire file removed (Phase 4) |
| `hub-client/src/services/wasmRenderer.ts` | Many dead functions to remove; `computeHash` (line 502 of sassCache.ts) used at line 710 — inline it here (Phase 4) |
| `hub-client/src/services/storage/types.ts` | Has `SassCacheEntry` type and `STORES.SASS_CACHE` — remove (Phase 4) |
| `hub-client/src/services/storage/migrations.ts` | Has sassCache store creation at line 95-98 — remove (Phase 4) |
| `hub-client/src/services/storage/index.ts` | Re-exports `SassCacheEntry` — remove (Phase 4) |
| `hub-client/src/test-utils/mockWasm.ts` | Has `compileScss` mock — remove (Phase 4) |
| `hub-client/src/types/wasm-quarto-hub-client.d.ts` | Has `get_scss_resources_version` declaration — remove (Phase 4) |

### Current state of cache_key() (before changes)

```rust
// crates/quarto-core/src/stage/stages/compile_theme_css.rs:62
fn cache_key(scss: &str, minified: bool) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    scss.hash(&mut hasher);
    minified.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
```

Problems: uses `DefaultHasher` (64-bit, unstable across Rust versions), takes the
full assembled SCSS as input (meaning assembly must happen before cache check).

### Current state of cache.js (before changes)

- DB name: `quarto-cache`, version 1
- Single object store `cache` with out-of-line keys (`namespace:key` composite)
- Records: `{ namespace, key, value, timestamp }` — timestamp written but never used
- No indexes, no eviction, no touch-on-read
- Tests in `cache.test.ts` use `fake-indexeddb/auto`

### Current state of CompileThemeCssStage::run() flow

1. Extract `ThemeConfig` from merged metadata
2. If no themes → store `DEFAULT_CSS`, return
3. Create `ThemeContext` (needs document_dir + runtime)
4. Call `assemble_theme_scss` → returns `(scss, load_paths)` — reads custom files here
5. Compute `cache_key(scss, minified)` — hashes full assembled SCSS
6. Check cache → if hit, return
7. Compile SCSS → store in cache

### Target CompileThemeCssStage::run() flow (after Phase 2)

1. Extract `ThemeConfig` from merged metadata (unchanged)
2. If no themes → store `DEFAULT_CSS`, return (unchanged)
3. Create `ThemeContext` (moved up — needed for cache key path resolution)
4. **Compute cache key** (NEW — reads custom file contents, does NOT assemble)
5. Check cache → if hit, store CSS artifact, return (moved earlier)
6. On miss: call `assemble_theme_scss` + compile (unchanged, re-reads custom files)
7. Store in cache (unchanged)

## Work Items

### Phase 1: Move SCSS_RESOURCES_HASH to quarto-sass

- [x] Add `sha2` as a build-dependency of `quarto-sass` in `crates/quarto-sass/Cargo.toml`:
      ```toml
      [build-dependencies]
      sha2 = "0.10"
      ```
      Check workspace `Cargo.toml` for existing `sha2` version to use `.workspace = true`
      if available.
- [x] Create `crates/quarto-sass/build.rs` — move the `compute_scss_resources_hash()` and
      `collect_scss_files()` logic from `crates/wasm-quarto-hub-client/build.rs`.
      The relative path `../../resources/scss` is the same from both `crates/quarto-sass/`
      and `crates/wasm-quarto-hub-client/`. Include `cargo:rerun-if-changed` directives.
- [x] Write the hash to `$OUT_DIR/scss_resources_hash.txt`
- [x] Expose as `pub const SCSS_RESOURCES_HASH: &str` in `quarto-sass/src/lib.rs`
      (via `include_str!`). Also add it to the `pub use` exports.
- [x] Update `wasm-quarto-hub-client/build.rs` to remove the duplicated hash computation.
      The build.rs can be deleted entirely if the hash was its only purpose. Then update
      `wasm-quarto-hub-client/src/lib.rs`: the `get_scss_resources_version()` function
      should use `quarto_sass::SCSS_RESOURCES_HASH` instead of its own `include_str!`.
      (The function itself is removed later in Phase 4, but keep it working for now.)
- [x] Verify: `cargo build -p quarto-sass` and `cargo build -p wasm-quarto-hub-client`
      both succeed. Run `cargo nextest run -p quarto-sass` to check nothing broke.

### Phase 2: SHA-256 hash-before-assemble in CompileThemeCssStage

**TDD: write/update cache_key tests first, then implement.**

The `cache_key()` function signature changes significantly. It currently takes
`(scss: &str, minified: bool)` and returns a 16-hex-char string. The new version
needs: theme specs, a runtime ref (for reading custom files), document_dir (for
path resolution), and the resources hash. It returns a SHA-256 hex string.

Key types from `quarto-sass::themes`:
- `ThemeSpec::BuiltIn(BuiltInTheme)` — name is sufficient (content in SCSS_RESOURCES_HASH)
- `ThemeSpec::Custom(PathBuf)` — need resolved path + file contents for key
- `ThemeContext::new(document_dir, runtime)` — resolves custom paths via `resolve_path()`

The test file is at the bottom of `compile_theme_css.rs` (line 249+). It has a
`MockRuntime` that returns `Ok(vec![])` for `file_read`. Tests to update/add:

- [x] Update existing `test_cache_key_deterministic`, `test_cache_key_differs_for_minified`,
      `test_cache_key_differs_for_content` to use new signature (tests first — they will
      fail until implementation)
- [x] Add test: same theme name with different `SCSS_RESOURCES_HASH` → different key
- [x] Add test: same custom file path with different content → different key
- [x] Add test: built-in theme cache key does NOT require file reads (only name + build hash)
- [x] Add `sha2` as a dependency of `quarto-core` in `crates/quarto-core/Cargo.toml`
- [x] Implement new `cache_key()` in `compile_theme_css.rs`:
      - Input: `SCSS_RESOURCES_HASH` + for each theme spec: identity string (built-in name
        or resolved custom path) + custom file contents (read via runtime) + `minified`
      - Output: SHA-256 hex string (full 64 hex chars is fine)
      - Uses `sha2::{Sha256, Digest}`
- [x] Restructure `CompileThemeCssStage::run()` to match the target flow described above
      (ThemeContext creation moved up, cache key before assembly)
- [x] Verify: `cargo nextest run -p quarto-core` — all tests pass including updated ones
- [x] Verify: `cargo nextest run --workspace` — no regressions in other crates

### Phase 3: IndexedDB schema and LRU eviction in cache.js (WASM)

No backward compatibility needed: `quarto-cache` DB only exists on this branch and
has only been tested locally. The user will clear their IndexedDB. The schema can be
redesigned in-place at DB_VERSION=1. The old `quarto-hub` DB's `sassCache` store
(from main branch) will be left as an inert orphan — harmless since nothing reads
from it after Phase 4 removes `sassCache.ts`.

**TDD: write cache.test.ts tests first, then implement.**

Tests use `fake-indexeddb/auto` (already a dev dependency). Run with:
`cd hub-client && npm test -- --run src/wasm-js-bridge/cache.test.ts`

- [x] Write new tests in `cache.test.ts` first (they will fail):
      - Eviction test: fill cache past entry limit, verify oldest entries evicted first
      - Cross-namespace eviction: entries from any namespace can be evicted
      - Touch-on-read test: read an old entry, verify it survives eviction of newer unread entries
      - Size tracking: verify stored record has correct `size` field
- [x] Redesign IndexedDB schema in `cache.js`:
      - Keep DB_VERSION=1 (no bump needed)
      - Record format: `{ namespace, key, value, timestamp, size }`
      - Create index on `timestamp` (for ordered eviction — iterate oldest-first)
      - Compute `size` from `value.length` in `jsCacheSet`
- [x] Add LRU eviction to `jsCacheSet`:
      - After storing, query total entry count and total size **globally** (all namespaces)
      - If over limits (e.g., 50MB total, 200 entries), open cursor on `timestamp`
        index (oldest first), delete entries regardless of namespace until under limits
      - Global eviction is simpler and avoids one namespace starving another
- [x] Touch-on-read: in `jsCacheGet`, on a cache hit, update the record's `timestamp`
      to `Date.now()` so that actively-used entries are not evicted (true LRU, not FIFO)
- [x] Add `MAX_ENTRIES` / `MAX_TOTAL_SIZE` constants at top of module
- [x] Update `cache.d.ts` if any exported function signatures changed
- [x] Verify: all cache tests pass

### Phase 4: Remove dead SassCacheManager code

This is mostly deletion. Verify no callers exist before removing each item.
Use grep to confirm zero references outside the files being deleted.

- [x] Remove `hub-client/src/services/sassCache.ts` (entire file)
- [x] Remove `hub-client/src/services/sassCache.test.ts` (entire file)
- [x] Remove `SassCacheEntry` from `hub-client/src/services/storage/types.ts`
- [x] Remove the `sassCache` store creation from `hub-client/src/services/storage/migrations.ts`
      (lines 95-98 area). Check if removing it requires adjusting migration version numbering.
      → Kept as no-op migration to maintain DB version compatibility.
- [x] Remove `SassCacheEntry` re-export from `hub-client/src/services/storage/index.ts`
- [x] In `hub-client/src/services/wasmRenderer.ts`:
      - Remove `import { getSassCache, computeHash } from './sassCache'`
      - **Inline `computeHash`** as a module-private function
      - Remove `checkAndInvalidateSassCache()` function and its call in `initWasm()`
      - Remove `SCSS_VERSION_STORAGE_KEY` constant
      - Remove functions: `compileScss()`, `compileScssWithBootstrap()`,
        `compileThemeCssByName()`, `compileDefaultBootstrapCss()`,
        `clearSassCache()`, `getSassCacheStats()`
      - Remove types: `SassCompileOptions`, `SassCompileResponse`, `ThemeCssResponse`
- [x] Remove `compileScss` mock method from `hub-client/src/test-utils/mockWasm.ts`
- [x] Remove `get_scss_resources_version` WASM export from
      `crates/wasm-quarto-hub-client/src/lib.rs` (only caller was
      `checkAndInvalidateSassCache`, now removed)
- [x] Remove `get_scss_resources_version` declaration from
      `hub-client/src/types/wasm-quarto-hub-client.d.ts`
- [x] Remove `get_scss_resources_version` from the `WasmModuleExtended` interface
      in `wasmRenderer.ts`
- [x] Grep for any remaining references to removed symbols. Fix any stragglers.
- [x] Verify: `cd hub-client && npm test` — all tests pass (300 passed)
- [x] Verify: `cd hub-client && npm run preflight` — builds cleanly (WASM + typecheck)

### Phase 5: Final verification

Cache key tests are written in Phase 2 (TDD). IndexedDB tests are written in Phase 3.
This phase is the full cross-ecosystem verification.

- [x] Run `cargo nextest run --workspace` — verify no Rust regressions (6608 passed)
- [x] Run `cargo xtask verify` — verify WASM builds + hub-client builds + all tests
