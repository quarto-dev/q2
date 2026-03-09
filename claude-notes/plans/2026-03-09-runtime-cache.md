# Plan: Add General Caching to SystemRuntime

## Overview

Add a platform-abstracted caching interface to `SystemRuntime` so that any
subsystem (SASS compilation, metadata parsing, template rendering, etc.) can
cache expensive results across renders and sessions.

- **Native**: Per-project filesystem cache at `{project_dir}/.quarto/cache/`
  (matches TS Quarto's `.quarto/` convention). For single-file renders without
  a project, caching is disabled (the process-level `OnceLock` for default CSS
  is sufficient).
- **WASM**: JS IndexedDB via bridge functions (inherently per-origin; keys
  prefixed with project identifier for isolation).

This plan adds the interface and implementations with thorough tests, but does
not wire up any callers. The first consumer will be SASS compilation (see
`claude-notes/plans/2026-03-09-css-in-pipeline.md`).

## Cache Location

### Native

```
{project_dir}/.quarto/cache/{namespace}/{key}
```

The `.quarto/` directory is the standard per-project state directory, matching
TS Quarto's convention. It should be gitignored (TS Quarto gitignores it).
The cache persists across render sessions for project renders.

For single-file renders (no `_quarto.yml`), the runtime has no cache dir
configured → cache methods return `Ok(None)` / `Ok(())` (no-op). This is
acceptable because single-file renders typically have one theme, and the
process-level `OnceLock` in `compile_default_css` handles the common case.

### WASM

IndexedDB is per-origin (per hub instance). Keys are prefixed with the project
path from VFS (e.g., `"/project"`) to isolate projects that share the same
hub instance.

### Runtime configuration

`NativeRuntime` is configured with an optional cache directory at construction
time. The rendering orchestration code discovers the project first, then
creates the runtime with the cache dir:

```rust
// In render_document_to_file or similar orchestration code:
let basic_runtime = NativeRuntime::new();
let project = ProjectContext::discover(&input_path, &basic_runtime)?;
let runtime = Arc::new(
    NativeRuntime::with_cache_dir(project.dir.join(".quarto/cache"))
);
```

There is a small chicken-and-egg: `ProjectContext::discover` needs a runtime
for filesystem access, but the cache dir comes from the project. This is solved
by either: (a) creating a basic runtime for discovery, then a configured one
for rendering, or (b) using a two-phase setup where `set_cache_dir` is called
after discovery but before the runtime is wrapped in `Arc` and shared.

For WASM, no cache dir configuration is needed — IndexedDB is always available.

## API Design

```rust
// In SystemRuntime trait:

/// Get a cached value by namespace and key.
///
/// Returns `Ok(None)` if the key is not found or caching is not available
/// (e.g., no cache dir configured for native single-file renders).
/// Never fails on cache miss — errors are reserved for I/O failures.
async fn cache_get(&self, namespace: &str, key: &str) -> RuntimeResult<Option<Vec<u8>>>;

/// Store a value in the cache.
///
/// Overwrites any existing entry with the same namespace+key.
/// No-op if caching is not available.
async fn cache_set(&self, namespace: &str, key: &str, value: &[u8]) -> RuntimeResult<()>;

/// Remove a cached value by namespace and key.
///
/// Returns `Ok(())` whether or not the key existed.
async fn cache_delete(&self, namespace: &str, key: &str) -> RuntimeResult<()>;

/// Remove all cached values in a namespace.
///
/// Used for cache invalidation (e.g., when SCSS resources change version).
async fn cache_clear_namespace(&self, namespace: &str) -> RuntimeResult<()>;
```

Default implementations return `Ok(None)` / `Ok(())` (no-op for runtimes that
don't support caching, or when no cache dir is set).

`SandboxedRuntime` delegates cache methods to its inner runtime, consistent
with how it handles all other `SystemRuntime` methods today.

### Conventions

- **Namespace**: Short lowercase identifier for the subsystem (e.g., `"sass"`,
  `"metadata"`, `"template"`). Used as a directory name (native) or store
  prefix (WASM).
- **Key**: Opaque string, typically a hex-encoded hash. Must be safe for use as
  a filename (alphanumeric + hyphen + underscore, max 128 chars). Callers are
  responsible for hashing their input into a safe key.
- **Value**: Raw bytes. Callers handle serialization/deserialization.

## Work Items

This plan is split into two sub-plans for independent sessions:

1. **Rust implementation (Phases 1-2)**: `claude-notes/plans/2026-03-09-runtime-cache-rust.md`
   - Trait methods, defaults, validation, error variant, NativeRuntime impl,
     SandboxedRuntime delegation, and all Rust tests.
2. **WASM/JS implementation (Phase 3)**: `claude-notes/plans/2026-03-09-runtime-cache-wasm.md`
   - JS IndexedDB bridge, WasmRuntime impl, vitest unit tests, full verification.

## Design Decisions

### Why per-project `.quarto/cache/` instead of global XDG cache?

- **Matches TS Quarto**: quarto-cli uses `.quarto/` for per-project state
  (freeze, cache, temp files). Users already expect this directory.
- **Natural scoping**: Different projects have different themes and custom SCSS.
  A global cache accumulates stale entries from unrelated projects.
- **Easy cleanup**: `rm -rf .quarto/cache` clears a project's cache.
  `rm -rf .quarto` clears all project state (also done by `quarto clean`).
- **Gitignore**: `.quarto/` should be gitignored (TS Quarto convention).
- **Single-file renders**: No `.quarto/` directory created — caching is
  simply disabled. The process-level `OnceLock` for default Bootstrap CSS
  handles the common case, and single-file renders rarely use custom themes.

### Why not reuse SassCacheManager?

The existing `SassCacheManager` in `sassCache.ts` is SASS-specific (cache key
computation tied to SCSS content, LRU eviction tuned for CSS, version
invalidation tied to SCSS resources). The general cache is a simpler key-value
store that any subsystem can use. `SassCacheManager` can be migrated to use the
general cache as a backend in a future cleanup, or kept as a higher-level
abstraction on top of it.

### Why no LRU or size limits in v1?

- Native filesystem cache: disk space is abundant, and the cache directory can
  be manually cleared. TS Quarto also uses a simple filesystem cache without
  automatic eviction.
- WASM IndexedDB: browser storage quotas provide natural limits. We can add
  LRU eviction later if needed.
- Keeping v1 simple means fewer bugs and faster delivery. The SASS compilation
  cache will have a small number of entries (one per unique theme configuration
  in the project, typically 1-3).

### Why async?

- WASM IndexedDB access is inherently async
- Native filesystem I/O could be async but sync is fine for small cache files
- Using async for the trait keeps the interface uniform
- Native implementation can use blocking I/O inside the async method (the files
  are small, and this matches the existing `compile_sass` pattern where native
  uses sync grass inside an async trait method)

### Why `Vec<u8>` not `String`?

Generality. While the first use case (CSS) is text, future caches might store
binary data (compiled templates, serialized ASTs). Callers can trivially convert
with `String::from_utf8` / `.as_bytes()`.

### Atomic writes (native)

`cache_set` should write to a temp file in the same directory, then rename.
This prevents readers from seeing partial writes. On Unix, rename is atomic
within the same filesystem. On Windows, it's close enough for a cache.

## Future Extensions (not in this plan)

- **TTL/expiry**: Add timestamp metadata, check on read
- **LRU eviction**: Track access times, prune oldest when size exceeds limit
- **Cache stats**: Entry count, total size, hit/miss ratio
- **Namespace versioning**: Automatic invalidation when a version string changes
  (e.g., SCSS resources version). This would subsume the manual version check
  in `wasmRenderer.ts`.
- **Global fallback cache**: For single-file renders, optionally use XDG cache
  dir (`~/.cache/quarto/`) if per-project caching isn't available. Low priority
  since single-file renders are fast.
