# VFS Clear Bug - Theme Compilation Failure

## Status: FIXED

## Work Items

- [x] Add `clear_preserving_prefix()` method to `VirtualFileSystem` in `wasm.rs`
- [x] Add `clear_user_files()` method to `WasmRuntime` that calls the new method
- [x] Update `vfs_clear()` in `lib.rs` to use `clear_user_files()` with `RESOURCE_PATH_PREFIX`
- [x] Add test for `clear_preserving_prefix()` method
- [x] Rebuild WASM module
- [x] Clean up debug logging in `sass.js` and `wasmRenderer.ts`
- [x] Run workspace tests (6071 passed)

## Problem Summary

Theme compilation (cosmo, quartz, flatly, etc.) fails intermittently in hub-client with:
```
Can't find stylesheet to import.
@import "vendor/rfs";
```

## Root Cause

**Found:** `vfsClear()` in `hub-client/src/services/automergeSync.ts` clears ALL files from the VFS, including the embedded Bootstrap SCSS resources that were populated during WASM initialization.

### Call sites that clear the VFS:
- `automergeSync.ts:104` - in `connect()`
- `automergeSync.ts:112` - in `disconnect()`
- `automergeSync.ts:195` - elsewhere

### Why it's intermittent:
- Theme CSS is cached in IndexedDB
- If a theme was compiled before `vfsClear()` was called, it has a cache hit and works
- New themes that need compilation after `vfsClear()` fail because bootstrap files are gone

## Technical Details

### How embedded resources work:
1. `crates/wasm-quarto-hub-client/src/lib.rs` has `populate_vfs_with_embedded_resources()`
2. Called once during `get_runtime()` via `OnceLock::get_or_init()`
3. Populates VFS with 93 Bootstrap SCSS files under `/__quarto_resources__/bootstrap/scss/`

### The problematic flow:
1. WASM init → `populate_vfs_with_embedded_resources()` → VFS has 93 files
2. User connects to sync server → `vfsClear()` → VFS has 0 files
3. Theme compilation → Can't find `vendor/_rfs.scss` → FAILS

### Evidence from debug logs:
```
[initWasm] Total VFS files: 93
[initWasm] Bootstrap files count: 93
...
[jsCompileSass] VFS quarto_resources files: 0  <-- EMPTY after vfsClear!
[jsCompileSass] VFS vendor files: []
```

## Proposed Fix

Modify `vfs_clear()` in `crates/wasm-quarto-hub-client/src/lib.rs` to preserve embedded resources.

### Option 1: Filter in vfs_clear (Recommended)
Change `vfs_clear()` to not remove paths starting with `/__quarto_resources__`:

```rust
#[wasm_bindgen]
pub fn vfs_clear() -> String {
    get_runtime().clear_user_files(); // New method that preserves embedded resources
    VfsResponse::ok()
}
```

Then in `crates/quarto-system-runtime/src/wasm.rs`, add:
```rust
impl VirtualFileSystem {
    pub fn clear_user_files(&mut self) {
        self.files.retain(|path, _| {
            path.to_string_lossy().starts_with("/__quarto_resources__")
        });
        // Also update directories set similarly
    }
}
```

### Option 2: Re-populate after clear
Add a `repopulate_embedded_resources()` function and call it after `vfs_clear()`.

### Option 3: Separate clear function
Add `vfs_clear_user_files()` that only clears non-embedded files, keep `vfs_clear()` as-is.

## Files to Modify

1. `crates/quarto-system-runtime/src/wasm.rs` - Add `clear_user_files()` method to VirtualFileSystem
2. `crates/wasm-quarto-hub-client/src/lib.rs` - Update `vfs_clear()` to use new method
3. Rebuild WASM: `cd hub-client && npm run build:wasm`

## Debug Code Added (Can Be Removed)

These files have debug logging that should be cleaned up after the fix:
- `hub-client/src/wasm-js-bridge/sass.js` - logging in `jsCompileSass()` and `tryResolve()`
- `hub-client/src/services/wasmRenderer.ts` - logging in `setupSassVfsCallbacks()`

## Related Files

- `crates/quarto-sass/src/resources.rs` - Defines `RESOURCE_PATH_PREFIX = "/__quarto_resources__"`
- `crates/quarto-sass/src/compile.rs` - `compile_theme_css()` and `default_load_paths()`
