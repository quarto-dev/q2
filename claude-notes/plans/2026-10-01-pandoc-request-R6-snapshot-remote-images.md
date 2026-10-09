---
title: 'Plan: VFS snapshot and remote images (pandoc-request R6)'
date: 2026-10-01
description: 'Lets browser exports fetch remote images through a hardened fetch and a click-time VFS snapshot, so docx, pptx, epub and typst requests stay consistent while the live project keeps changing.'
---

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D5, Snapshot)
**Depends on:** R2; R3's capture splice (the prefetch runs after it); R4 for the typst case. **Unblocks:** remote images in docx/pptx/epub/typst; typst documents with remote images stop failing (and host H8 can chain them).
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** R (workspace-6). **Start gate:** R3 and R4 are fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** R8 (its Start gate decides whether it can begin).

## Overview

Remote images are fetched through `SystemRuntime::fetch_url`, mounted as `<doc_dir>/_remote/<hash>.<ext>`, and the AST's image `src` is rewritten to that local path. Because the fetch is the first `await` in a request, a click-time VFS snapshot is built first so the request stays consistent while Automerge keeps mutating the live VFS.

## Decisions

- The VFS has no snapshot facility: `VirtualFileSystem` (`quarto-system-runtime/src/vfs.rs:59`) derives only `Debug, Default`, and the runtime is a process-wide `OnceLock<Arc<WasmRuntime>>` (`lib.rs:42`) holding `vfs: RwLock<…>`. The earlier phases are atomic only because nothing yields; a snapshot is likely a second `WasmRuntime` over a cloned VFS, passed to the pipeline as its `Arc<dyn SystemRuntime>`.
- The prefetch is an async stage (`#[async_trait(?Send)]`) before the prepare step; it only *adds* files.
- The fetch is hardened for hub use: https only, `credentials: 'omit'`, a hard byte cap and a timeout, and relative URLs are not resolved against the hub origin.

## Checklist

### Tests first
- [x] (real wasm, `pandocRequest.wasm.test.ts`, with a stubbed global `fetch`) A fetch that mutates the VFS mid-flight: the request still carries the click-time bytes.
- [x] A bridge test that `credentials: 'omit'` is set and that relative and protocol-relative URLs are rejected.
- [x] A typst fixture with a remote image yields a request containing the `_remote/…` file and the rewritten `src` (the exit-83 failure only appears when pandoc runs; the host's parity net checks that once H3 exists). The `_remote/…` file is in the `pdf` request (the compile reads it); the `.typ`-only request does not mount images (R4: pandoc's typst writer only names them), so it carries the rewritten `src` alone.

### Tasks
Five items; the prefetch uses the hardened fetch, so do the snapshot, the hardened fetch, the prefetch, the Lua check and the failure handling, each with its own gate.
- [x] **VFS snapshot:** a click-time snapshot mechanism so every read in a request goes through one consistent view. `VirtualFileSystem` is not `Clone`, and `WasmRuntime` also holds `runtime_metadata` (`wasm.rs:~221`), which the snapshot copies; decide whether the bootstrap SCSS under `/__quarto_resources__` is copied or shared. The snapshot belongs to one call and is dropped when it ends or fails; no global state is written, so a superseded call finishing late cannot disturb the next.
- [x] **Prefetch stage:** walk Images including Figures and Custom-block slots (meta image fields are not walked: the only one pandoc reads, `epub-cover-image`, is a path key that the metadata merge has already rebased onto the document directory, so a URL there is a different, unfixed problem; `listing-item.image` is metadata nothing fetches); ignore `data:` and protocol-relative URLs; dedupe; cap size and time; fetch in parallel; take the extension from MIME over URL; rewrite `src` to the mounted local path, keeping the original URL as an attribute for diagnostics.
- [x] Check the vendored Lua ignores that attribute (run the fixtures through the host's gate).
- [x] **Hardened fetch variant** of `jsFetchUrl` (`ts-packages/wasm-js-bridge/src/fetch.js`), which today has no scheme, size, timeout, abort-signal (tied to the click, D8.5) or credentials policy and base64s through JSON (~2.3× memory for big images, accepted). `SystemRuntime::fetch_url(&self, url)` has no parameter for a policy or signal and about 15 test mocks implement it, so add a defaulted trait method `fetch_url_hardened(url, policy)` (mocks untouched), implemented by the per-call snapshot runtime, which carries the click's `AbortSignal`; the JS side adds `jsFetchUrlHardened` beside `jsFetchUrl` in `fetch.js`.
- [x] For docx/pptx/epub a failed fetch leaves the URL, giving pandoc's `Q-11-1` warning + alt text; for typst, a failed fetch is reported as a diagnostic rather than pandoc's exit 83, so the prefetch replaces the failed `Image` with its alt text (`typst.lua:260` calls `mediabag.fetch` on any URL left in the AST).

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test, one browser smoke through host H3's harness.

## Exit

Remote images work in all four formats within the CORS limits of the browser.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/r6-snapshot-remote-images`, workspace-6.
- Last commit; tasks ticked: VFS snapshot; hardened fetch (plus its bridge test, `ts-packages/wasm-js-bridge/src/fetch.test.ts`); prefetch stage. Start-of-phase baseline not yet re-measured (R5 ended at 15593 run / 202 skipped).
- State and gotchas:
  - Snapshot = `VirtualFileSystem: Clone` + `WasmRuntime::snapshot()` (copies VFS and runtime metadata; the bootstrap SCSS is copied, not shared: one code path, a few MB, one call). `render_pandoc_request` in `wasm-quarto-hub-client/src/lib.rs` takes it first and passes it to `ProjectContext::discover`, the file read and the pipeline. `WasmRuntime` is wasm-only, so the "mock fetch mutates the VFS mid-flight" test (Tests first, box 1) is a real-wasm vitest in `pandocRequest.wasm.test.ts`, written with the prefetch stage; native tests cover the VFS clone independence.
  - Hardened fetch: `FetchPolicy {max_bytes, timeout_ms}`, `validate_fetch_url` and the defaulted `SystemRuntime::fetch_url_hardened` are in `quarto-system-runtime/src/traits.rs` (default: validate, `fetch_url`, cap check; mocks untouched). `WasmRuntime` overrides it with `jsFetchUrlHardened` (`fetch.js`: https only, `credentials: 'omit'`, no referrer, streamed byte cap, timeout, the click's `AbortSignal`). The signal is a new last parameter of `render_pandoc_request` (`abort_signal`, optional; `.d.ts`, `wasmRenderer.ts` option `signal`), held in a `SingleThreaded<JsValue>` wrapper because `JsValue` is `!Send`.
  - Prefetch: `PrefetchRemoteImagesStage` (`stage/stages/prefetch_remote_images.rs`) rewrites `src` to the absolute `<doc_dir>/_remote/<sha256(url)[..16]>.<ext>` (MIME over URL), mounted through the runtime (the snapshot in the browser), original URL kept as the `q2-remote-src` attribute; fetches `http(s)://` targets only, deduped, six at a time, capped at the image limit each. It is NOT in `build_pandoc_request_stages` (tests of `prepare` run natively with `NativeRuntime` and must not touch the network): `build_pandoc_request_stages_fetching` adds it before the tail and only `render_pandoc_request` (render.rs) uses that. `futures` (alloc only) became a `quarto-core` dependency for `join_all`; both lockfiles gained that line (the wasm lock also picked up a stale `cssparser` line).
  - Typst: a failed fetch replaces the `Image` with a `Span` of its alt text and warns `Q-20-9`; docx/pptx/epub keep the URL and warn `Q-11-1` (pandoc adds its own). R4's pre-emptive `Q-20-9` scan of the Pandoc JSON (`typst_limits.rs`) is gone, and the `Q-20-9` catalog entry and docs page now describe the failed fetch. Host H5 text updated.
  - The scripted-network integration tests are `tests/integration/pandoc_remote_images.rs` (a `NativeRuntime` decorator with a table for `fetch_url`; the default `fetch_url_hardened` runs, so its URL and size checks are covered).
  - The Lua check (`q2-remote-src` ignored by the vendored filters) and the failure handling's browser side are verified with the real pandoc.wasm run.
- Landed on `feature/pandoc-wasm` (rebased over H3's parity-net commit, which touched only TS tests; wasm suite re-run green after). Phase complete; next in lane: R8.
- Next step (superseded; those tests are written and pass: 33 in `pandocRequest.wasm.test.ts`, run against pandoc.wasm for docx and typst): real pandoc.wasm runs in `pandocRequest.wasm.test.ts` (docx, typst .typ and pdf request with a scripted `fetch`; the mid-flight VFS mutation test; the abort signal); that closes the Lua-attribute check and the failure-handling box. Then the browser smoke through H3's harness, the workspace nextest and the TS suites.
  - Gotcha: a `#[wasm_bindgen] async fn` does not run until its promise is polled (a microtask), so the snapshot is taken then, not synchronously in the call; the mid-flight test mutates the VFS only after fetch was called.
  - Browser smoke: `hub-client/e2e/pandoc-remote-images.harness.spec.ts` (Playwright route as the remote host: a docx embeds the image with no cookie/authorization header; an aborted request still downloads with the alt text; `route.fulfill` does not enforce CORS, so abort stands in for a CORS block). Run: `VITE_E2E=1 npm run build`, then `npx playwright test --config playwright.harness.config.ts e2e/pandoc-remote-images.harness.spec.ts`.
  - Phase-boundary gates: workspace nextest 15605 run, 15605 passed, 202 skipped (R5: 15593; +12 = 3 in system-runtime (vfs clone, URL policy x2), 3 prefetch unit tests, 1 stage-list test, 6 `pandoc_remote_images`, minus R4's `a_remote_image_is_reported_for_typst_only`; `typst_limits` swapped one test for one). Crate clippy `-D warnings` clean for system-runtime and quarto-core; wasm `cargo check` clean. TS: wasm-js-bridge 37, preview-runtime 79, pandoc-host 33, hub-client unit 1331, integration 143, wasm 239 passed (10 skipped), tsc clean, Playwright harness: remote images 2, parity and loader specs passed.
