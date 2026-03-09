# Plan: Runtime Cache — WASM/JS Implementation (Phase 3)

Parent plan: `claude-notes/plans/2026-03-09-runtime-cache.md`
Prerequisite: `claude-notes/plans/2026-03-09-runtime-cache-rust.md` (must be completed first)

This plan covers the WASM IndexedDB bridge, WasmRuntime implementation,
and JS unit tests.

## Codebase Orientation

### Rust side: WasmRuntime

**`crates/quarto-system-runtime/src/wasm.rs`** — The `WasmRuntime` struct and
its `SystemRuntime` impl. This file is guarded by
`#![cfg(target_arch = "wasm32")]` at the top — it only compiles for WASM.

The file already has two sets of `#[wasm_bindgen]` extern declarations for
JS bridge functions:

1. **Template bridge** (lines ~51-80):
   ```rust
   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/template.js")]
   extern "C" {
       #[wasm_bindgen(js_name = "jsRenderSimpleTemplate", catch)]
       fn js_render_simple_template_impl(template: &str, data_json: &str) -> Result<JsValue, JsValue>;
       // ...
   }
   ```

2. **SASS bridge** (lines ~92-120):
   ```rust
   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/sass.js")]
   extern "C" {
       #[wasm_bindgen(js_name = "jsSassAvailable")]
       fn js_sass_available_impl() -> bool;
       // ...
   }
   ```

Add a third block for the **cache bridge** following the same pattern. The
cache functions return Promises (async), so they use the same
`Result<JsValue, JsValue>` + `catch` pattern as the existing bridge functions.

The WasmRuntime implementation calls these with:
```rust
let promise = js_function_impl(args).map_err(|e| {
    RuntimeError::CacheError(format!("Failed to call jsFunction: {:?}", e))
})?;
let result = JsFuture::from(js_sys::Promise::from(promise))
    .await
    .map_err(|e| RuntimeError::CacheError(format!("Cache operation failed: {:?}", e)))?;
```

**Important**: The `WasmRuntime` uses `#[async_trait(?Send)]` (not `Send`)
because WASM is single-threaded and `JsFuture` is not `Send`.

### Uint8Array marshalling

For `cache_set`, the JS function receives a `Uint8Array` (value bytes).
For `cache_get`, the JS function returns a `Uint8Array` (or null).

In Rust WASM, conversion between `Vec<u8>` and JS `Uint8Array`:
```rust
// Rust -> JS: Vec<u8> to Uint8Array
use js_sys::Uint8Array;
let js_array = Uint8Array::from(value.as_slice());

// JS -> Rust: JsValue to Vec<u8>
let uint8_array = Uint8Array::new(&js_value);
let mut bytes = vec![0u8; uint8_array.length() as usize];
uint8_array.copy_to(&mut bytes);
```

Check if `js_sys::Uint8Array` is already imported in `wasm.rs`. If not, add
it. The crate already depends on `js-sys` and `wasm-bindgen`.

### JS side: Bridge files

**`hub-client/src/wasm-js-bridge/`** contains the JavaScript modules that
Rust calls via `wasm_bindgen(raw_module = ...)`:

- `template.js` — Simple template and EJS rendering
- `sass.js` — SCSS compilation via dart-sass (lazy-loaded)
- `sass.d.ts` — TypeScript declarations for sass.js

Each `.js` bridge file has a corresponding `.d.ts` for TypeScript consumers.

The `raw_module` path uses `/src/...` (absolute from project root in Vite's
dev server). This is resolved by Vite at build time.

**Pattern for bridge JS files** (from `sass.js`):
- Module-level state (lazy-loaded dependencies, callbacks)
- Exported functions that return Promises for async operations
- Error handling: catch and re-throw with clean messages
- JSDoc annotations for types

### Existing hub-client test infrastructure

hub-client uses **vitest** for testing. Tests are typically colocated or
in nearby test files. Run tests with:
```bash
cd hub-client && npm run test        # Interactive watch mode
cd hub-client && npm run test:ci     # CI mode (no watch, exits)
```

For IndexedDB testing, use `fake-indexeddb` (already a dev dependency at
`^6.0.0`).

In the test file, set up fake-indexeddb before tests:
```typescript
import 'fake-indexeddb/auto';
```
This globally polyfills `indexedDB`, `IDBFactory`, etc.

### hub-client build and verification

- **`npm run build:all`** (from `hub-client/`) — Builds WASM + hub-client
- **`cargo xtask verify`** (from repo root) — Full verification:
  Rust build + tests + WASM build + hub-client tests
- **`cargo xtask verify --skip-rust-tests`** — Skip Rust tests if already
  verified in the prerequisite plan

### The WASM crate is separate

`crates/wasm-quarto-hub-client/` has its own `Cargo.toml` and is **excluded
from the workspace**. It imports `quarto-system-runtime` types. The
`WasmRuntime` type and its `SystemRuntime` impl live in
`quarto-system-runtime/src/wasm.rs`, NOT in the wasm-quarto-hub-client crate.
The WASM crate just uses `WasmRuntime` — it doesn't define it.

## Phase 3: WASM implementation (JS IndexedDB bridge)

The WASM runtime delegates to JavaScript for persistent caching.

**JS bridge functions** (in `hub-client/src/wasm-js-bridge/`):

- [x] Create `hub-client/src/wasm-js-bridge/cache.js`:
  ```javascript
  // Called from Rust via wasm-bindgen
  export async function jsCacheGet(namespace, key) { ... }
  export async function jsCacheSet(namespace, key, value) { ... }
  export async function jsCacheDelete(namespace, key) { ... }
  export async function jsCacheClearNamespace(namespace) { ... }
  ```
  Follow the style of `sass.js`: JSDoc annotations, clean error messages,
  module-level lazy initialization of IndexedDB.
- [x] Create `hub-client/src/wasm-js-bridge/cache.d.ts` with TypeScript
  declarations for the bridge functions (matching the pattern of `sass.d.ts`).
- [x] Storage: Use IndexedDB with a `quarto-cache` database and a `cache`
  object store. Key format: `"<namespace>:<key>"`. Value stored as
  `{ namespace, key, value: Uint8Array, timestamp }`.
  This is simpler than the existing `SassCacheManager` — no LRU needed for v1.
  Lazy-open the database on first access (similar to how `sass.js` lazy-loads
  the sass module).
- [x] Wire up in `crates/quarto-system-runtime/src/wasm.rs`:
  - Add `#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/cache.js")]`
    extern declarations for the JS functions (matching the existing pattern
    for `sass.js` and `template.js`)
  - For `jsCacheGet`: takes `(namespace: &str, key: &str)`, returns
    `Result<JsValue, JsValue>` where the JsValue is a Promise resolving to
    `Uint8Array | null`
  - For `jsCacheSet`: takes `(namespace: &str, key: &str, value: &Uint8Array)`,
    returns `Result<JsValue, JsValue>` (Promise resolving to undefined)
  - For `jsCacheDelete` and `jsCacheClearNamespace`: similar patterns
  - Implement `cache_get`/`cache_set`/`cache_delete`/`cache_clear_namespace`
    on `WasmRuntime` by calling the JS bridge
  - Validate namespace and key at the top of each method using
    `crate::traits::validate_cache_namespace`/`validate_cache_key` before
    calling into JS (matching NativeRuntime, which also validates early)
  - Convert between `Vec<u8>` and JS `Uint8Array` (see orientation above)
  - Handle JS promise results via `JsFuture::from(js_sys::Promise::from(...))`
  - On JS errors, return `Err(RuntimeError::CacheError(...))`
  - For `cache_get`, check if result `.is_null() || .is_undefined()` before
    attempting `Uint8Array` conversion — null means cache miss

**Tests (JS unit tests only — true Rust→JS→IndexedDB integration testing
deferred to the first consumer plan):**

- [x] JS unit tests for the bridge functions in vitest
  (`hub-client/src/wasm-js-bridge/cache.test.ts`):
  - Import `fake-indexeddb/auto` at top for IndexedDB polyfill
  - Import the bridge functions directly from `./cache.js`
  - Test IndexedDB interactions:
  - `test_cache_roundtrip` — set then get returns same bytes
  - `test_cache_get_missing` — returns null
  - `test_cache_namespaces_isolated` — different namespaces don't collide
  - `test_cache_clear_namespace` — only clears targeted namespace
  - `test_cache_delete` — removes single entry
  - Clean up IndexedDB between tests: delete the database in `beforeEach`
    for full isolation (using `indexedDB.deleteDatabase("quarto-cache")`
    and resetting the module-level db handle)

## Verification

- [x] `cargo build --workspace` — compiles (Rust workspace, excludes WASM)
- [x] `cargo nextest run --workspace` — 6582 tests pass
- [x] `cargo xtask verify` — Full verification passes (WASM builds, 50
  hub-client tests pass including 5 new cache bridge tests)
- [x] No callers yet — this is infrastructure only

## Reference

See parent plan (`claude-notes/plans/2026-03-09-runtime-cache.md`) for API
design, conventions, and design decisions (async rationale, Vec<u8> rationale,
IndexedDB key format rationale, etc.).
