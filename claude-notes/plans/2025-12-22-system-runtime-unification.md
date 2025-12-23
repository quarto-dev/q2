# SystemRuntime Unification

**Date**: 2025-12-22
**Issue**: k-6zaq
**Parent Issue**: k-nkhl (QuartoRuntime abstraction)
**Status**: ✅ COMPLETED (2025-12-22)

---

## Summary

Rename the existing `LuaRuntime` trait to `SystemRuntime` and move it to a shared location (`quarto-util`) so it can be used uniformly across the codebase. This eliminates the need for a separate `QuartoRuntime` trait proposed in the web frontend plan.

---

## Problem

The web frontend plan (2025-12-22-quarto-hub-web-frontend-and-wasm.md) proposes a `QuartoRuntime` trait for abstracting filesystem access in `quarto-core`. However, this trait is nearly identical to the existing `LuaRuntime` trait in `pampa`:

| LuaRuntime | Proposed QuartoRuntime |
|------------|------------------------|
| `file_read(path)` | `read_file(path)` |
| `file_write(path, contents)` | `write_file(path, contents)` |
| `path_exists(path, kind)` | `path_exists(path)` |
| `dir_create(path, recursive)` | `create_dir(path, recursive)` |
| `dir_list(path)` | `read_dir(path)` |
| `cwd()` | `current_dir()` |
| `env_get(name)` | `env_var(name)` |

Creating two nearly-identical traits would:
- Double the implementation burden (NativeRuntime, WasmRuntime, SandboxedRuntime for each)
- Create confusion about which to use
- Make code sharing difficult between pampa and quarto-core

---

## Solution

1. **Rename** `LuaRuntime` → `SystemRuntime`
2. **Move** to `quarto-util` (or a new `quarto-runtime` crate)
3. **Update** pampa to import from the new location
4. **Use** `SystemRuntime` in quarto-core instead of creating `QuartoRuntime`

---

## Current Location

```
crates/pampa/src/lua/runtime/
├── mod.rs           # Re-exports
├── traits.rs        # LuaRuntime trait + supporting types
├── native.rs        # NativeRuntime implementation
├── wasm.rs          # WasmRuntime (stub)
└── sandbox.rs       # SandboxedRuntime + SecurityPolicy
```

---

## Target Location (Implemented)

```
crates/quarto-system-runtime/src/
├── lib.rs           # Re-exports and default_runtime()
├── traits.rs        # SystemRuntime trait + supporting types
├── native.rs        # NativeRuntime implementation
├── wasm.rs          # WasmRuntime (stub, WASM-only)
└── sandbox.rs       # SandboxedRuntime + SecurityPolicy
```

Note: We used a dedicated `quarto-system-runtime` crate instead of `quarto-util` for cleaner separation.

---

## API Changes

### Trait Rename

```rust
// Before (in pampa)
pub trait LuaRuntime: Send + Sync { ... }

// After (in quarto-util)
pub trait SystemRuntime: Send + Sync { ... }
```

### Minor Signature Adjustments

The `path_exists` method has a slightly different signature:

```rust
// Current LuaRuntime
fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool>;

// Keep this signature - it's more flexible than the QuartoRuntime proposal
// which only had fn path_exists(&self, path: &Path) -> RuntimeResult<bool>
```

### Add Missing Methods

The proposed `QuartoRuntime` has `find_binary` which `LuaRuntime` lacks:

```rust
/// Find a binary by checking environment variable, then PATH
fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf> {
    // Default implementation
    if let Ok(Some(path)) = self.env_get(env_var) {
        let path = PathBuf::from(path);
        if self.path_exists(&path, Some(PathKind::File)).unwrap_or(false) {
            return Some(path);
        }
    }
    // PATH lookup requires platform-specific logic
    None
}
```

For native, override to use `which::which()`. For WASM, return `None`.

---

## Implementation Plan

### Phase 1: Move Without Renaming

1. Copy `crates/pampa/src/lua/runtime/` to `crates/quarto-util/src/runtime/`
2. Keep the trait name as `LuaRuntime` temporarily
3. Update `quarto-util/Cargo.toml` to add dependencies (tempfile, etc.)
4. Update pampa to re-export from quarto-util:
   ```rust
   // pampa/src/lua/runtime/mod.rs
   pub use quarto_util::runtime::*;
   ```
5. Run tests, ensure nothing breaks

### Phase 2: Rename

1. Rename `LuaRuntime` → `SystemRuntime` in quarto-util
2. Update all references in pampa
3. Run tests

### Phase 3: Add find_binary

1. Add `find_binary` method with default implementation
2. Override in `NativeRuntime` to use `which::which()`
3. Add tests

### Phase 4: Integration with quarto-core

1. Add `quarto-util` dependency to `quarto-core`
2. Update `ProjectContext::discover()` to accept `&dyn SystemRuntime`
3. Update `BinaryDependencies::discover()` to use `SystemRuntime::find_binary()`
4. Update render command to thread runtime through
5. Ensure CLI tests pass

---

## Files to Modify

### quarto-util
- `Cargo.toml` - add dependencies
- `src/lib.rs` - add runtime module
- `src/runtime/mod.rs` - new file
- `src/runtime/traits.rs` - moved from pampa
- `src/runtime/native.rs` - moved from pampa
- `src/runtime/wasm.rs` - moved from pampa
- `src/runtime/sandbox.rs` - moved from pampa

### pampa
- `Cargo.toml` - ensure quarto-util dependency
- `src/lua/runtime/mod.rs` - change to re-export
- `src/lua/runtime/traits.rs` - delete (moved)
- `src/lua/runtime/native.rs` - delete (moved)
- `src/lua/runtime/wasm.rs` - delete (moved)
- `src/lua/runtime/sandbox.rs` - delete (moved)
- `src/lua/filter.rs` - update imports

### quarto-core (later, part of k-nkhl)
- `Cargo.toml` - add quarto-util dependency
- `src/project.rs` - use SystemRuntime
- `src/render.rs` - use SystemRuntime for binary discovery
- `src/resources.rs` - use SystemRuntime for file writes

### quarto (later, part of k-nkhl)
- `src/commands/render.rs` - pass runtime to core functions

---

## Testing Strategy

1. **Unit tests move with code** - all existing tests in `native.rs` etc. should continue to pass
2. **Integration test** - verify pampa's re-export works correctly
3. **Compile check** - ensure all crates still compile after each phase
4. **Full test suite** - run `cargo nextest run` after each phase

---

## Risk Assessment

**Low risk:**
- Moving code between crates is mechanical
- Renaming is straightforward with IDE support
- No behavioral changes in Phase 1-3

**Medium risk:**
- Phase 4 (quarto-core integration) touches more code
- Need to thread runtime parameter through call chains
- May uncover places where runtime isn't easily available

---

## Dependencies

- This issue should be completed **before** implementing the `QuartoRuntime` abstraction (k-nkhl)
- Once complete, k-nkhl can use `SystemRuntime` instead of creating a new trait

---

## Open Questions

1. **Crate location**: Should this go in `quarto-util` or a dedicated `quarto-runtime` crate?
   - Pro quarto-util: fewer crates, simpler dependency graph
   - Pro quarto-runtime: cleaner separation, runtime could be used by external tools
   - **Recommendation**: Start with quarto-util, extract later if needed

2. **Feature flags**: Should WASM-specific code be behind a feature flag?
   - Current: `#[cfg(target_arch = "wasm32")]` on WasmRuntime
   - This is probably fine as-is

3. **Async support**: Should there be an `AsyncSystemRuntime` variant?
   - The web frontend plan mentions this as future consideration
   - **Recommendation**: Defer to separate issue if/when needed

---

## Implementation Notes (2025-12-22)

### What Was Done

1. **Created `quarto-system-runtime` crate** (not `quarto-util` - cleaner separation)
2. **Moved runtime code from pampa**:
   - `traits.rs` → `SystemRuntime` trait with all supporting types
   - `native.rs` → `NativeRuntime` implementation
   - `sandbox.rs` → `SandboxedRuntime` + `SecurityPolicy`
   - `wasm.rs` → `WasmRuntime` stub (WASM-only compilation)
3. **Renamed `LuaRuntime` → `SystemRuntime`** throughout
4. **Added new methods**:
   - `is_file(&self, path: &Path)` - convenience method
   - `is_dir(&self, path: &Path)` - convenience method
   - `find_binary(&self, name: &str, env_var: &str)` - binary discovery
5. **Updated pampa** to re-export from new crate
6. **Added `which` crate dependency** for PATH lookup in `NativeRuntime::find_binary()`

### Test Results

- 41 tests in `quarto-system-runtime` (all passing)
- 867 tests in `pampa` (all passing)
- 2563 total tests in workspace (all passing, 160 skipped)

### Phase 4 (quarto-core integration) Not Yet Done

This issue covers Phases 1-3 only. Phase 4 (threading `SystemRuntime` through `quarto-core`) is part of the parent issue k-nkhl and will be done when the web frontend needs it.
