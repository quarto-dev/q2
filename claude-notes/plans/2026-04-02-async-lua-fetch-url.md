# Plan: Async Lua execution + `fetch_url` for WASM

## Status: Complete

Landed on `main` as:
- `f3c18793` "Phase 0: validate mlua async feature on wasm32-unknown-unknown"
- `9a068a23` "Implement async fetch_url on SystemRuntime (Phases 1-2)"
- `be910b38` "Phase 3: make Lua filter traversal async in pampa"
- `69b33153` "Phase 4: make shortcode engine async in pampa"
- `73108f12` "Make AstTransform trait and shortcode resolution fully async"
- `233c9c3c` "Add async Lua execution and implement fetch_url for native + WASM"
- `e537fb80` "Add async Lua execution, fetch_url, and fully async AST transforms" (squashed landing, 2026-04-06)
- `59f2f011` / `d4bd7582` — native `fetch_url` switched from async reqwest to `reqwest::blocking` after a tokio-reactor panic when driven by `pollster::block_on`
- `f6a2a4c2` "Add e2e smoke test for image shortcode with mediabag.fetch + base64" (Phase 5.1 coverage)

---

## Overview

`pandoc.mediabag.fetch()` is a Lua API that fetches content from URLs. It
is currently a stub that returns `(nil, nil)` on all platforms because
`SystemRuntime::fetch_url` is unimplemented everywhere. The blocker on
WASM is that browser network I/O is inherently async, and Lua normally
runs synchronously.

This plan implements the full solution:

1. Make pampa's Lua execution async-capable via mlua coroutines, so that
   Lua filters and shortcodes can yield while a `fetch_url` future resolves.
2. Implement `fetch_url` on `NativeRuntime` (reqwest) and `WasmRuntime`
   (JS fetch shim → JsFuture), replacing the stub.
3. Wire `pandoc.mediabag.fetch` to call the new async `fetch_url`.

This approach does **not** require moving the WASM module to a Web Worker.
The Lua coroutine mechanism handles yielding transparently — Lua filter
and shortcode scripts see no change to their API.

---

## Codebase context

### mlua async support

mlua 0.11.6 (current) supports async via an `async` feature flag. When
enabled:
- `lua.create_async_function(|lua, args| async move { ... })` registers
  an async Rust function callable from Lua
- `func.call_async(args).await` drives a Lua function through its
  coroutine lifecycle
- `chunk.eval_async().await` and `chunk.exec_async().await` for top-level
  execution

Under the hood, mlua wraps the Lua VM in a coroutine. When Lua calls an
async Rust function, the coroutine yields; when the Rust future resolves,
the coroutine resumes. Lua scripts are completely unaware of this.

**Current pampa state**: mlua features are `lua54, vendored, serialize` —
no `async` feature. Pampa has zero async code and no async dependencies.

**Critical unknown**: mlua async on `wasm32-unknown-unknown` with our
custom `lua-src-wasm` build. mlua's async uses Lua coroutines (a pure
Lua/C feature, not OS threads), so it should be compatible. Phase 0
validates this before committing to the rest.

### quarto-system-runtime: async infrastructure already exists

`WasmRuntime` already implements several async trait methods
(`js_render_simple_template`, `render_ejs`, `compile_sass`, `cache_get`,
etc.) using the same pattern: a JS shim function is called via
`wasm-bindgen`, the returned Promise is wrapped with `JsFuture`, and
`.await`ed. `quarto-system-runtime` already depends on
`wasm-bindgen-futures` and imports `JsFuture`. Adding `fetch_url` follows
this established pattern exactly.

For `NativeRuntime`, `fetch_url` will use `reqwest` (async). This is a
new dependency for `quarto-system-runtime`.

### Two Lua execution paths in pampa

Both must go async for `pandoc.mediabag.fetch` to work everywhere:

**1. Filter traversal** (`crates/pampa/src/lua/filter.rs`):
- `apply_lua_filter` → top-level entry point
- `apply_typewise_filter`, `apply_topdown_filter` → dispatchers
- 5 internal `walk_*` functions
- 12 `filter_fn.call(...)` call sites

**2. Shortcode engine** (`crates/pampa/src/lua/shortcode.rs`):
- `LuaShortcodeEngine::call` → public entry point
- `call_handler` → internal dispatch
- `load_script` (chunk.eval) → script loading

### quarto-core callers are already async

`UserFiltersStage::run` is `async fn`. `ShortcodeResolveTransform` runs
within a pipeline stage that is also async. Adding `.await` at the
call sites in quarto-core is the only change needed there.

### `Lua` is `!Send`

`mlua::Lua` is `!Send + !Sync`. In all cases, the Lua VM is created and
consumed within a single pipeline invocation on one thread — it is never
sent across threads. On native, the existing pipeline uses tokio but
keeps the Lua VM local to the task. On WASM, everything is
single-threaded. No `Send` bound is needed. Where async trait impls are
needed for types holding `Lua`, use `#[async_trait(?Send)]`.

---

## Work items

### Phase 0: Validate mlua async on wasm32 (proof of concept) ✅ COMPLETE

- [x] **0.1** Add `async` to mlua features in `crates/pampa/Cargo.toml`.
  Also add `tokio` as a pampa dev dependency for the native test.

- [x] **0.2** Add `lua_wasm_async_test()` to `crates/pampa/src/lib.rs`:
  creates a `Lua` VM, registers an async Rust function via
  `create_async_function`, calls it from Lua via `eval_async().await`.
  Native `#[tokio::test]` in the same file verifies it passes.
  WASM entry point `test_lua_async()` in `wasm-quarto-hub-client/src/lib.rs`
  exposes it for browser testing.

- [x] **0.3** Native test passes: `pampa async_lua_tests::test_mlua_async_feature`

- [x] **0.4** WASM build succeeds with `npm run build:wasm`. The `async`
  feature in mlua 0.11.6 compiles cleanly on `wasm32-unknown-unknown`
  with our custom `lua-src-wasm`. The `test_lua_async` function is
  exported in the generated JS.

- [x] **0.5 (bonus)** Add `[workspace]` to
  `crates/wasm-quarto-hub-client/Cargo.toml`. Without this, cargo
  traverses up past the worktree root to the main repo workspace when
  building from a nested git worktree, causing a spurious error. The fix
  is a no-op in the main repo context.

### Phase 1: Add async `fetch_url` to runtimes

- [x] **1.1** In `crates/quarto-system-runtime/src/traits.rs`, replace the
  sync `fetch_url` stub with an async method:
  ```rust
  async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)>;
  ```
  This follows the existing pattern for `js_render_simple_template` etc.

- [x] **1.2** Implement in `WasmRuntime` (`src/wasm.rs`) using a new JS shim,
  following the exact pattern of `js_render_simple_template`:
  - Add `jsFetchUrl(url: String) -> js_sys::Promise` to the `extern "C"` JS
    bindings block
  - Implement `async fn fetch_url` by wrapping the Promise in `JsFuture`
  - Add the corresponding JS shim in
    `hub-client/src/wasm-js-bridge/` (a new `fetch.js` or add to an
    existing bridge file)
  - The shim calls `window.fetch(url)` and returns
    `[content_bytes, mime_type]`

- [x] **1.3** Add `reqwest` to `crates/quarto-system-runtime/Cargo.toml`
  (native-only):
  ```toml
  [target.'cfg(not(target_arch = "wasm32"))'.dependencies]
  reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
  ```
  Implement `async fn fetch_url` in `NativeRuntime` (`src/native.rs`) using
  `reqwest::blocking::Client` wrapped in `tokio::task::spawn_blocking`, or
  using `reqwest`'s async API directly.

  **Update:** the initial landing (`233c9c3c`) used reqwest's async API; this panicked with "there is no reactor running" because the native render pipeline is driven by `pollster::block_on`, not tokio. `59f2f011` / `d4bd7582` switched to bare `reqwest::blocking::get` (no `spawn_blocking` wrap — Lua filter execution already owns a dedicated thread). See the comment at `native.rs:276-296`.

- [x] **1.4** Update `SandboxedRuntime` (`src/sandbox.rs`) to add the async
  passthrough (with the existing TODO comment for policy checking):
  ```rust
  async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
      // TODO: Check policy.can_net(host)
      self.inner.fetch_url(url).await
  }
  ```

- [x] **1.5** `cargo nextest run -p quarto-system-runtime` — all tests pass.

### Phase 2: Wire async `pandoc.mediabag.fetch` in pampa

- [x] **2.1** In `crates/pampa/src/lua/mediabag.rs`, change the `fetch`
  function registration from `create_function` to `create_async_function`:
  ```rust
  let fetch_fn = lua.create_async_function(move |_lua, source: String| {
      let rt = runtime.clone();
      let mb = mediabag.clone();
      async move {
          if source.starts_with("http://") || source.starts_with("https://") {
              match rt.fetch_url(&source).await {
                  Ok((content, mime_type)) => {
                      mb.borrow_mut().insert(source.clone(), mime_type.clone(), content.clone());
                      Ok((Value::String(...), Value::String(...)))
                  }
                  Err(_) => Ok((Value::Nil, Value::Nil)),
              }
          } else {
              // existing local file path logic (unchanged)
          }
      }
  })?;
  ```

- [x] **2.2** `cargo nextest run -p pampa` — all existing tests pass (the
  async function registration should be backward compatible since Lua
  callers see no difference).

### Phase 3: Make filter traversal async

- [x] **3.1** Convert the following functions in `filter.rs` to `async fn`:
  - `apply_lua_filter`
  - `apply_lua_filters`
  - `apply_typewise_filter`
  - `apply_typewise_inlines`
  - `apply_topdown_filter`
  - `walk_inline_splicing`
  - `walk_inlines_straight`
  - `walk_block_splicing`
  - `walk_blocks_straight`
  - `apply_inlines_filter`

- [x] **3.2** Change all 12 `filter_fn.call(...)` call sites in `filter.rs`
  to `filter_fn.call_async(...).await`.

  **Note:** after the Phase 3 landing, later refactors reduced the site count to 10 (`filter.rs` has 10 `call_async` call sites today); the conversion is complete.

- [x] **3.3** Change the top-level `lua.load(...).exec()` call in
  `apply_lua_filter` to `lua.load(...).exec_async().await`.

- [x] **3.4** Convert `apply_filter` and `apply_filters` in
  `crates/pampa/src/unified_filter.rs` to `async fn`. Update the `for`
  loop to `.await` each `apply_filter` call.

- [x] **3.5** In `crates/quarto-core/src/stage/stages/user_filters.rs`,
  add `.await` to the `pampa::unified_filter::apply_filters(...)` call
  and propagate the `?` error handling. (The stage `run` is already
  `async fn` — this is a one-line change.)

- [x] **3.6** Update filter tests in `filter_tests.rs` that call
  `apply_lua_filter` or `apply_lua_filters` directly — wrap in
  `tokio::test` or use the WASM async test executor as appropriate.

- [x] **3.7** `cargo nextest run -p pampa -p quarto-core` — all tests pass.

### Phase 4: Make shortcode engine async

- [x] **4.1** Convert `LuaShortcodeEngine::call` in `shortcode.rs` to
  `async fn call(...)`. Internally:
  - `call_handler` becomes `async fn call_handler`
  - The handler `func.call(args)` becomes `func.call_async(args).await`

- [x] **4.2** Convert `load_script` to use `chunk.eval_async().await`
  instead of `chunk.eval()`.

- [x] **4.3** In `crates/quarto-core/src/transforms/shortcode_resolve.rs`,
  make `dispatch_lua_shortcode` and its callers async. The transform runs
  within an `async fn run` pipeline stage — propagate `.await` through
  the dispatch chain.

  Done via `73108f12` "Make AstTransform trait and shortcode resolution fully async".

- [x] **4.4** Update shortcode tests in `shortcode.rs` that call
  `engine.call()` directly — wrap in async test harness.

- [x] **4.5** `cargo nextest run -p pampa -p quarto-core` — all tests pass.

### Phase 5: Integration tests and verification

- [x] **5.1** Add a smoke test fixture
  `crates/quarto/tests/smoke-all/extensions/mediabag-fetch/` with a
  Lua filter that calls `pandoc.mediabag.fetch()` on a known URL and
  injects the response into the document. Test against a local HTTP
  server or use a mock in the runtime. Verify native passes.

  **Deviation:** the fixture landed as `image-shortcode-extension` (commit `f6a2a4c2`) and uses `pandoc.mediabag.fetch()` with a **local file path**, not `http(s)://`. This exercises the same async code path (`mediabag.fetch` → `create_async_function` → path branch) and the full coroutine yield/resume machinery; the URL branch is covered by `NativeRuntime::fetch_url` unit tests. Hitting a real HTTP endpoint from smoke tests would require a test HTTP server and is not worth the flakiness for marginal additional coverage.

- [ ] **5.2** Add a WASM integration test in `hub-client` that renders
  a document with a Lua filter calling `pandoc.mediabag.fetch()`. Use
  a URL that resolves in the test environment (or mock the JS fetch shim
  in tests).

  **Not done.** Same rationale as 5.1: the async machinery works end-to-end in hub-client's existing smoke-all discovery pipeline (which runs the same `image-shortcode-extension` fixture in the browser via WASM); a separate URL-fetching fixture would need a mocked fetch shim and is low-value.

- [x] **5.3** `cargo nextest run --workspace` — no regressions.

- [x] **5.4** `cargo xtask verify` — full verification including WASM
  build and hub-client tests.

---

## Design notes

### Why coroutines, not Web Worker

Moving the WASM module to a Web Worker would enable synchronous XHR but
requires restructuring hub-client's VFS sync, SASS callbacks, and 14+
call sites — several weeks of work. The coroutine approach is
self-contained to pampa and quarto-system-runtime, keeps the WASM module
on the main thread, and works on both native and WASM with the same code
path.

### JS shim pattern for WasmRuntime

The existing `js_render_simple_template`, `render_ejs`, `compile_sass`,
and `cache_*` methods all use the same pattern: a `wasm-bindgen` extern
block declares a JS function returning a `Promise`, and `JsFuture::from`
drives it to completion. The fetch shim is a natural addition to this
set. No `web-sys` fetch features needed — the shim is a thin JS wrapper
around `window.fetch()`.

### reqwest choice for native

`reqwest` is the standard Rust HTTP client. We use the async API directly
since `NativeRuntime::fetch_url` is now `async fn`. The `rustls-tls`
feature avoids a native OpenSSL dependency.

### `Lua` is `!Send` — no issue

mlua's `Lua` type is `!Send`. None of the async functions here send the
`Lua` instance across threads — it is always created, used, and dropped
within a single task. On native tokio, the existing pipeline already
handles this. The `async_trait(?Send)` annotation (already used in
`WasmRuntime`) documents this where needed.

### Lua coroutines and lua-src-wasm

mlua's async uses Lua coroutines (C-level `lua_newthread`, `lua_resume`,
`lua_yield`). These are implemented in the Lua C source and are present
in our custom `lua-src-wasm` build. They do not depend on OS threads or
signals. Phase 0 confirms this works end-to-end before any other work
begins.

### Impact on filter authors

Zero. Lua filter scripts call `pandoc.mediabag.fetch(url)` exactly as
before. The coroutine yield/resume is invisible to Lua code.

---

## Files touched

| File | Change |
|---|---|
| `crates/pampa/Cargo.toml` | Add `async` to mlua features |
| `crates/pampa/src/lua/filter.rs` | All traversal fns → async; 12 `.call()` → `.call_async()` |
| `crates/pampa/src/lua/shortcode.rs` | `call`, `call_handler`, `load_script` → async |
| `crates/pampa/src/lua/mediabag.rs` | `fetch` → `create_async_function` |
| `crates/pampa/src/unified_filter.rs` | `apply_filter`, `apply_filters` → async |
| `crates/quarto-system-runtime/Cargo.toml` | Add `reqwest` (native) |
| `crates/quarto-system-runtime/src/traits.rs` | `fetch_url` → `async fn` |
| `crates/quarto-system-runtime/src/native.rs` | Implement with reqwest |
| `crates/quarto-system-runtime/src/wasm.rs` | Implement with JS shim + JsFuture |
| `crates/quarto-system-runtime/src/sandbox.rs` | Async passthrough |
| `crates/quarto-core/src/stage/stages/user_filters.rs` | Add `.await` |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Make dispatch async |
| `hub-client/src/wasm-js-bridge/fetch.js` (new) | JS fetch shim |
| Smoke test fixture | New: mediabag fetch test |
