# Phase 6b WASM Support: Custom SCSS Cross-Platform Compatibility

**Parent Plan**: `2026-01-23-phase6b-custom-scss.md`
**Created**: 2026-01-23
**Status**: COMPLETED - All phases (W1-W5) done 2026-01-23

---

## Problem Statement

The current `load_custom_theme()` implementation uses `std::fs` directly:

```rust
// Current implementation - BROKEN for WASM
if !resolved_path.exists() {
    return Err(SassError::CustomThemeNotFound { path: resolved_path });
}
let content = std::fs::read_to_string(&resolved_path)?;
```

This only works on native targets. For WASM (hub-client), files are stored in Automerge and accessed through the `VirtualFileSystem`.

---

## Solution Overview

The codebase already has the necessary abstraction: **`SystemRuntime`** trait in `quarto-system-runtime`. This trait provides:

```rust
fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>>;
fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool>;
```

**The fix**: Make custom theme loading use `SystemRuntime` instead of `std::fs`.

---

## Existing Infrastructure

### 1. SystemRuntime Trait (`quarto-system-runtime/src/traits.rs`)
- `NativeRuntime` - uses `std::fs`
- `WasmRuntime` - uses `VirtualFileSystem` (in-memory HashMap synced from Automerge)

### 2. RuntimeFs Adapter (`quarto-system-runtime/src/sass_native.rs`)
Already exists! Implements `grass::Fs` using `SystemRuntime`:

```rust
pub struct RuntimeFs<'a> {
    runtime: &'a dyn SystemRuntime,
    embedded: Option<&'a dyn EmbeddedResourceProvider>,
}
```

### 3. Hub-Client VFS Integration
- Files from Automerge are synced to VFS via `onFileAdded`/`onFileChanged` callbacks
- Custom SCSS files will automatically be available in the VFS when synced

---

## Implementation Plan

### Phase 6b.W1: Update ThemeContext to Use SystemRuntime

**Changes to `themes.rs`**:

```rust
// BEFORE
pub struct ThemeContext {
    document_dir: PathBuf,
    load_paths: Vec<PathBuf>,
}

// AFTER
pub struct ThemeContext<'a> {
    document_dir: PathBuf,
    load_paths: Vec<PathBuf>,
    runtime: &'a dyn SystemRuntime,  // NEW
}
```

**Work items**:
- [ ] Add `runtime` field to `ThemeContext`
- [ ] Update `ThemeContext::new()` to require runtime parameter
- [ ] Add convenience constructor for native (uses `NativeRuntime`)

### Phase 6b.W2: Update load_custom_theme to Use Runtime

**Changes to `themes.rs`**:

```rust
// BEFORE
pub fn load_custom_theme(path: &Path, context: &ThemeContext) -> Result<...> {
    if !resolved_path.exists() { ... }
    let content = std::fs::read_to_string(&resolved_path)?;
    ...
}

// AFTER
pub fn load_custom_theme(path: &Path, context: &ThemeContext) -> Result<...> {
    let exists = context.runtime.path_exists(&resolved_path, Some(PathKind::File))
        .map_err(|e| SassError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    if !exists {
        return Err(SassError::CustomThemeNotFound { path: resolved_path });
    }

    let content_bytes = context.runtime.file_read(&resolved_path)
        .map_err(|e| SassError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    let content = String::from_utf8(content_bytes)
        .map_err(|e| SassError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    ...
}
```

**Work items**:
- [ ] Replace `path.exists()` with `runtime.path_exists()`
- [ ] Replace `std::fs::read_to_string()` with `runtime.file_read()` + UTF-8 decode
- [ ] Update error handling for runtime errors

### Phase 6b.W3: Update Dependent Functions

Functions that call `load_custom_theme` need updates:

- [ ] `process_theme_specs()` - already takes `ThemeContext`
- [ ] `resolve_theme_spec()` - already takes `ThemeContext`
- [ ] `assemble_themes()` in `bundle.rs` - already takes `ThemeContext`

No signature changes needed for these - they just pass context through.

### Phase 6b.W4: Update Tests

**Unit tests** (`themes.rs`):
- [ ] Create mock/test runtime for unit testing
- [ ] Update existing tests to provide runtime

**Integration tests** (`custom_theme_test.rs`):
- [ ] Use `NativeRuntime` for integration tests
- [ ] Verify tests still pass

### Phase 6b.W5: Add quarto-system-runtime Dependency

**Changes to `quarto-sass/Cargo.toml`**:
- [ ] Add `quarto-system-runtime` as dependency
- [ ] May need feature flags for native vs WASM

---

## Design Decisions

### Q1: Should ThemeContext own or borrow the runtime?

**Decision**: Borrow (`&'a dyn SystemRuntime`)

**Rationale**:
- Runtime is typically long-lived (application lifetime)
- Avoids cloning or Arc overhead
- Matches pattern in `RuntimeFs`

### Q2: How to handle lifetime in ThemeContext?

**Options**:
1. Add lifetime parameter: `ThemeContext<'a>`
2. Use `Arc<dyn SystemRuntime>`

**Decision**: Option 1 (lifetime parameter)

**Rationale**:
- More explicit about borrowing semantics
- No runtime overhead
- Matches existing `RuntimeFs<'a>` pattern

### Q3: Convenience for native-only callers?

Many callers (CLI, tests) just want native filesystem access.

**Decision**: Provide helper function:

```rust
impl ThemeContext<'static> {
    /// Create a context using the native runtime.
    /// Only available on native targets (not WASM).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native(document_dir: PathBuf) -> Self {
        use quarto_system_runtime::NativeRuntime;
        static NATIVE: NativeRuntime = NativeRuntime::new();
        Self::new(document_dir, &NATIVE)
    }
}
```

Or use a lazy_static/once_cell for the native runtime singleton.

---

## Testing Strategy

### Unit Tests
- Create `MockRuntime` that uses in-memory HashMap (similar to `VirtualFileSystem`)
- Test file loading, path resolution, error cases

### Integration Tests
- Native tests use `NativeRuntime`
- WASM tests would need to populate VFS first (future work)

### Existing Tests
- Update to use `ThemeContext::native()` for minimal changes

---

## Migration Impact

### Breaking Changes
- `ThemeContext::new()` signature changes (requires runtime)
- `ThemeContext` gains lifetime parameter

### Mitigation
- Provide `ThemeContext::native()` convenience constructor
- Clear compiler errors guide migration

### Affected Callers
- All test files using `ThemeContext`
- Future hub-client integration code

---

## Work Items Summary

### Phase 6b.W1: ThemeContext Runtime Field - COMPLETED 2026-01-23
- [x] Add `runtime: &'a dyn SystemRuntime` to `ThemeContext`
- [x] Update constructor
- [x] Add `ThemeContext::native()` convenience (cfg'd for native only)
- [x] Add `ThemeContext::native_with_load_paths()` convenience

### Phase 6b.W2: load_custom_theme Updates - COMPLETED 2026-01-23
- [x] Replace `std::fs` calls with `runtime` calls
- [x] Handle runtime error types

### Phase 6b.W3: Update Dependent Code - COMPLETED 2026-01-23
- [x] Verify `process_theme_specs`, `resolve_theme_spec`, `assemble_themes` work unchanged
- [x] Update function signatures to use `ThemeContext<'_>`

### Phase 6b.W4: Update Tests - COMPLETED 2026-01-23
- [x] Update unit tests to provide runtime
- [x] Update integration tests to use `NativeRuntime`
- [x] Verify all tests pass (100/100 in quarto-sass, 5940/5940 workspace)

### Phase 6b.W5: Cargo Dependencies - COMPLETED 2026-01-23
- [x] Add `quarto-system-runtime` dependency to `quarto-sass` for all targets

---

## Future Work (Not in This Phase)

- WASM integration tests (requires test harness for VirtualFileSystem)
- Hub-client UI for custom themes
- Light/dark theme support (Phase 6b.6)

---

## Files to Modify

| File | Changes |
|------|---------|
| `quarto-sass/Cargo.toml` | Add `quarto-system-runtime` dependency |
| `quarto-sass/src/themes.rs` | Update `ThemeContext`, `load_custom_theme` |
| `quarto-sass/src/lib.rs` | Re-export runtime types if needed |
| `quarto-sass/tests/custom_theme_test.rs` | Use `NativeRuntime` |
| Unit tests in `themes.rs` | Provide runtime to `ThemeContext` |

---

## Estimated Scope

- **Code changes**: ~100-150 lines modified
- **New code**: ~20-30 lines (convenience constructors, error mapping)
- **Risk**: Low - using existing, well-tested abstractions
