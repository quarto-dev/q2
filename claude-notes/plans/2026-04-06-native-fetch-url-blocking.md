# Plan: Use reqwest::blocking for native fetch_url

## Status: Ready to commit

---

## Problem

`NativeRuntime::fetch_url` uses `reqwest`'s async client, which requires
a tokio reactor (I/O driver) to poll network sockets. However, the
native render pipeline is driven by `pollster::block_on` in multiple
places:

- `crates/quarto-core/src/render_to_file.rs:216` — CLI render entry
- `crates/quarto-core/src/pipeline.rs` — 12+ test call sites

`pollster` is a minimal executor that polls futures in a tight loop.
It does not start a tokio reactor. When a Lua filter calls
`pandoc.mediabag.fetch(url)`, the async chain reaches
`reqwest::Client::get(url).send().await`, which tries to register with
a tokio I/O driver that doesn't exist, and panics:

```
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

The WASM path is unaffected — `WasmRuntime::fetch_url` uses a JS fetch
shim driven by the browser event loop, not tokio.

## Why blocking is the right fix

The alternative — replacing all `pollster::block_on` call sites with
`tokio::runtime` — is a larger change that affects the CLI entry point
and all pipeline tests. It's the right long-term direction but is out
of scope for this branch.

Using `reqwest::blocking` is appropriate here because:

1. **No reactor needed.** The blocking client uses its own internal
   thread pool for I/O, independent of any async runtime.

2. **Thread blocking is a non-issue.** Lua filter execution already
   blocks a dedicated thread — the native pipeline uses
   `tokio::task::block_in_place` + `new_current_thread` in
   `user_filters.rs`, and shortcodes run on a single-threaded local
   runtime. Blocking on a network request inside that thread adds
   latency proportional to the HTTP round-trip, which is inherent and
   unavoidable.

3. **fetch_url is rare.** It is only called when Lua code explicitly
   requests a URL via `pandoc.mediabag.fetch`. Most renders never
   trigger it.

4. **Trait signature is unchanged.** The method stays `async fn` to
   satisfy the `SystemRuntime` trait. The blocking call completes
   synchronously within the async fn body — this is valid and
   equivalent to a future that resolves immediately.

## Work items

- [x] **1** Add `"blocking"` to reqwest features in
  `crates/quarto-system-runtime/Cargo.toml`

- [x] **2** Change `NativeRuntime::fetch_url` in `native.rs` to use
  `reqwest::blocking::get(url)` instead of async client

- [x] **3** Checked — `reqwest` import still needed for
  `reqwest::blocking::get`, no dead imports

- [x] **4** Test native CLI: rendered `{{< placeholder 200 format=png >}}`
  — HTML output contains embedded PNG image

- [x] **5** `cargo nextest run --workspace` — 7232 passed, 0 failed

- [ ] **6** Commit
