# Plan: Make AstTransform trait and shortcode resolution async

## Status: Complete

---

## Problem

After implementing async Lua execution (Phases 0–4 of
`2026-04-02-async-lua-fetch-url.md`), the shortcode path used
`pollster::block_on` on WASM to bridge async Lua engine calls into the
sync `AstTransform::transform` method. This failed at runtime:

```
condvar wait not supported
```

`pollster::block_on` drives futures by polling in a tight loop using a
condvar for wakeup signaling. On `wasm32-unknown-unknown`, condvars are
not available (no threading support). More fundamentally, even if
polling worked, browser Promises (used by the JS fetch shim) require
the browser event loop to resolve — a busy-polling executor will spin
forever because the event loop never runs.

The filter path didn't have this problem because `apply_filters` was
made truly `async fn` (Phase 3), and the pipeline stage `.await`s it
directly on WASM (`?Send` bound). The shortcode path was the gap.

## Solution

Make the `AstTransform` trait async. This allows the shortcode
transform to `.await` Lua engine calls directly, without any `block_on`
bridge. The async propagates through:

```
AstTransformsStage::run (already async)
  → TransformPipeline::execute (sync → async)
    → AstTransform::transform (sync → async)
      → ShortcodeResolveTransform::transform
        → resolve_shortcode → dispatch_lua_shortcode
          → engine.call().await → func.call_async().await
            → pandoc.mediabag.fetch → rt.fetch_url().await
```

All other transforms (appendix, callout, footnotes, sectionize, etc.)
get `async fn transform` but complete synchronously — the async is
zero-cost for them.

## Why `?Send`

`mlua::Lua` is `!Send`. The shortcode engine holds a `Lua` VM, and the
async transform future captures a reference to it. On native, the
pipeline stages previously used `#[async_trait]` (which requires `Send`
futures). Changing to `#[async_trait(?Send)]` throughout is safe because:

- The pipeline already runs on a single thread per render (WASM is
  single-threaded; native uses `block_in_place` + local runtime)
- No pipeline stage future is ever sent across threads
- The `Send` bound was aspirational, not load-bearing

## Work items

- [x] **1** Change `AstTransform` trait in `transform.rs`: add
  `#[async_trait(?Send)]`, change `fn transform` to `async fn transform`

- [x] **2** Change `TransformPipeline::execute` to `async fn`, `.await`
  each transform

- [x] **3** Update `AstTransformsStage::run` to `.await` the
  `pipeline.execute()` call

- [x] **4** Make shortcode resolution chain async:
  - `resolve_shortcode` → async (`.await` on dispatch + load_script)
  - `dispatch_lua_shortcode` → async (`.await` on engine.call)
  - `resolve_blocks`, `resolve_block`, `resolve_inlines`,
    `recurse_inline` → return `Pin<Box<dyn Future>>` for mutual
    recursion

- [x] **5** Remove `shortcode_block_on` helpers (both native tokio
  and WASM pollster variants) — no longer needed

- [x] **6** Update all 12 `impl AstTransform` blocks across the
  codebase: add `#[async_trait(?Send)]`, change to `async fn transform`

- [x] **7** Update all `PipelineStage` impls to `#[async_trait(?Send)]`

- [x] **8** Update all test functions calling `.transform()` or
  `.execute()`: `#[tokio::test]` + `async fn` + `.await`

## Files touched

| File | Change |
|---|---|
| `crates/quarto-core/src/transform.rs` | Trait + pipeline async |
| `crates/quarto-core/src/transforms/shortcode_resolve.rs` | Full async chain, remove block_on |
| `crates/quarto-core/src/transforms/*.rs` (10 files) | `async fn transform` |
| `crates/quarto-core/src/engine/jupyter/transform.rs` | `async fn transform` |
| `crates/quarto-core/src/stage/traits.rs` | `?Send` |
| `crates/quarto-core/src/stage/pipeline.rs` | `?Send` |
| `crates/quarto-core/src/stage/mod.rs` | `?Send` + test updates |
| `crates/quarto-core/src/stage/stages/*.rs` (7 files) | `?Send` |
| `crates/quarto-core/tests/jupyter_integration.rs` | Test async |
| `crates/quarto/tests/render_integration.rs` | Test async |

## Commit

`29fa9475` — Make AstTransform trait and shortcode resolution fully async
