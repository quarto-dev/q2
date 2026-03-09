# Plan: Runtime Cache — Rust Implementation (Phases 1-2)

Parent plan: `claude-notes/plans/2026-03-09-runtime-cache.md`

This plan covers the trait definition, validation helpers, error variant,
NativeRuntime implementation, and SandboxedRuntime delegation. All Rust-only —
no WASM or JS changes.

## Codebase Orientation

All work is in the `crates/quarto-system-runtime/` crate. Read these files
before starting:

### Key files

- **`src/traits.rs`** — Defines the `SystemRuntime` trait and `RuntimeError`
  enum. The trait uses conditional `async_trait`:
  ```rust
  #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
  #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
  pub trait SystemRuntime: Send + Sync { ... }
  ```
  Methods are organized in sections with `// ═══` banner comments. The new
  CACHING section goes after the SASS COMPILATION section (the last section
  in the trait, around line 598-652).

  Trait methods with default implementations use the pattern:
  ```rust
  async fn method_name(&self, ...) -> RuntimeResult<T> {
      let _ = (param1, param2); // silence unused warnings
      Err(RuntimeError::NotSupported("...".to_string()))
  }
  ```
  For cache defaults, return `Ok(None)` / `Ok(())` instead of `Err`, since
  caching being unavailable is normal (not an error).

- **`src/native.rs`** — `NativeRuntime` struct. Currently has **no fields**
  and derives `Default`:
  ```rust
  #[derive(Debug, Default)]
  pub struct NativeRuntime {
      // Note: JsEngine is NOT stored here because V8's JsRuntime is not Send+Sync.
  }
  ```
  Adding `cache_dir: Option<PathBuf>` means you can no longer derive `Default`
  automatically — implement it manually or keep `#[derive(Debug)]` and add
  a manual `Default` impl that sets `cache_dir: None`.

  Tests in this file use `tempfile::TempDir` (aliased as `TempFileTempDir`
  to avoid collision with the crate's own `TempDir` type). Async trait
  methods are tested with `pollster::block_on()`.

- **`src/sandbox.rs`** — `SandboxedRuntime<R: SystemRuntime>` is a generic
  decorator. Every `SystemRuntime` method delegates to `self.inner.method()`.
  Permission checking is stubbed with `// TODO` comments. Add cache method
  delegation following exactly the same pattern.

- **`src/wasm.rs`** — `WasmRuntime` for WASM targets. Guarded by
  `#![cfg(target_arch = "wasm32")]` at the top. **Do NOT modify this file
  in this plan** — WASM changes are in the sibling plan.

- **`src/lib.rs`** — Re-exports public types. `RuntimeError` is already
  re-exported via `pub use traits::RuntimeError`. New public functions
  (validation helpers) need to be added to the re-exports here.

### RuntimeError enum

Located in `traits.rs`. Current variants: `Io`, `PermissionDenied`,
`NotSupported`, `PathViolation`, `Network`, `ProcessFailed`, `SassError`.

Adding `CacheError(String)` requires updating:
1. The enum definition
2. The `Display` impl (`match` block)
3. The `Error::source()` impl (returns `None` for `CacheError`)

Follow the pattern of `SassError(String)` — it's the closest analog.

### Async trait pattern

Cache methods are `async` on the trait (for WASM IndexedDB compatibility),
but the native implementation uses synchronous filesystem I/O inside the
async method body. This is fine — the files are small, and it matches the
existing `compile_sass` pattern:
```rust
async fn compile_sass(&self, scss: &str, ...) -> RuntimeResult<String> {
    // Synchronous grass call inside async method
    sass_native::compile_scss(self, scss, load_paths, minified)
}
```

### Testing patterns

- Use `cargo nextest run` (NOT `cargo test`) — see CLAUDE.md
- Do NOT pipe nextest through `tail` or other commands — it hangs
- Tests in `native.rs` use `tempfile::TempDir` for temp directories
- Async methods are tested with `pollster::block_on(rt.method(...))`
- The crate already depends on `tempfile` and `pollster` for tests
- Run single-crate tests during development:
  `cargo nextest run -p quarto-system-runtime`
- Run full workspace tests before committing:
  `cargo nextest run --workspace`

### Validation helpers

Place `validate_cache_key` and `validate_cache_namespace` in `traits.rs`
as standalone public functions (not methods on the trait). Export them from
`lib.rs`. They should return `Result<(), RuntimeError>` using the new
`CacheError` variant.

### Atomic write pattern for `cache_set`

Write to a temp file in the same directory, then `std::fs::rename()`.
Use `tempfile::NamedTempFile::new_in(parent_dir)` to create the temp file
in the correct directory (ensures same filesystem for atomic rename).
The `tempfile` crate is already a dependency.

## Phase 1: Trait methods, defaults, and NativeRuntime cache_dir

- [x] Add the four cache methods to `SystemRuntime` in
  `crates/quarto-system-runtime/src/traits.rs` under a new
  `// CACHING` section, after the SASS section.
- [x] Default implementations: `cache_get` returns `Ok(None)`, others return
  `Ok(())`. Caching is optional — no error when unavailable.
- [x] Add `validate_cache_key` and `validate_cache_namespace` helper functions
  (not on the trait) that check safety: alphanumeric + hyphen + underscore,
  max 128 chars, no empty strings. Both namespaces and keys are used as path
  components on native, so both need path-traversal protection. Callers can
  use these; implementations must also validate.
- [x] Add `RuntimeError::CacheError(String)` variant for cache operation
  failures (distinct from `Io` to clearly identify cache-related errors).
- [x] Add `NativeRuntime::with_cache_dir(cache_dir: PathBuf) -> Self`
  constructor. Stores the cache dir as `Option<PathBuf>`.
  `NativeRuntime::new()` continues to work with `cache_dir: None` (caching
  disabled). Note: `Option<PathBuf>` implements `Default` as `None`, so
  `#[derive(Default)]` still works — no manual impl needed.
- [x] Do NOT add `SandboxedRuntime` delegation for cache methods.
  `SandboxedRuntime` is unused outside of tests and already falls through
  to trait defaults for SASS methods. Cache methods will similarly use the
  trait defaults (`Ok(None)` / `Ok(())`), meaning caching is silently
  disabled on sandboxed runtimes.
- [x] Export new public items from `src/lib.rs`: `validate_cache_key`,
  `validate_cache_namespace`.

**Tests:**

- [x] Test that the default impls return expected values — use
  `SandboxedRuntime<NativeRuntime>` which doesn't override cache methods
  and thus exercises the trait defaults.
- [x] Test `validate_cache_key` with valid keys, empty key, too-long key,
  keys with special characters
- [x] Test `validate_cache_namespace` with same cases (same validation rules)
- [x] Test that `NativeRuntime::new()` has `cache_dir == None`
- [x] Test that `NativeRuntime::with_cache_dir(path)` stores the path

## Phase 2: Native implementation (filesystem)

Cache layout on disk:
```
{cache_dir}/{namespace}/{key}
```

Where `cache_dir` is set via `NativeRuntime::with_cache_dir()`, typically
`{project_dir}/.quarto/cache/`. If `cache_dir` is `None`, all cache methods
return `Ok(None)` / `Ok(())` (no-op).

Each entry is a single file. The filename is the key. The file content is the
raw cached bytes. No metadata file needed for v1 — filesystem mtime can be
used for future LRU eviction if needed.

- [x] Implement `cache_get` on `NativeRuntime`
- [x] Implement `cache_set` on `NativeRuntime` (atomic write via tempfile + persist)
- [x] Implement `cache_delete`
- [x] Implement `cache_clear_namespace`

**Tests (unit, using temp directories):**

- [x] `test_cache_roundtrip` — set then get returns same bytes
- [x] `test_cache_get_missing` — get nonexistent key returns None
- [x] `test_cache_get_no_cache_dir` — runtime with no cache dir returns None
- [x] `test_cache_set_no_cache_dir` — runtime with no cache dir is silent no-op
- [x] `test_cache_overwrite` — set twice, get returns latest value
- [x] `test_cache_delete` — set, delete, get returns None
- [x] `test_cache_delete_nonexistent` — delete missing key is Ok
- [x] `test_cache_clear_namespace` — set multiple keys, clear, all return None
- [x] `test_cache_clear_nonexistent_namespace` — clear missing namespace is Ok
- [x] `test_cache_namespaces_isolated` — same key in different namespaces
  returns different values
- [x] `test_cache_invalid_key_rejected` — key with `/` or `..` is rejected
- [x] `test_cache_invalid_namespace_rejected` — namespace with `/` or `..` is
  rejected
- [x] `test_cache_empty_value` — storing and retrieving empty bytes works
- [x] `test_cache_large_value` — storing and retrieving a ~1MB value works
- [x] `test_cache_binary_value` — non-UTF8 bytes roundtrip correctly
- [x] `test_cache_creates_directories` — set creates namespace directory
  hierarchy if it doesn't exist

## Verification

- [x] `cargo build --workspace` — compiles
- [x] `cargo nextest run --workspace` — 6582 tests pass (97 in quarto-system-runtime, 16 new cache tests)

## Reference

See parent plan (`claude-notes/plans/2026-03-09-runtime-cache.md`) for API
design, conventions, and design decisions (async rationale, Vec<u8> rationale,
atomic write rationale, etc.).
