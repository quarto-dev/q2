# Plan: VFS snapshot and remote images (pandoc-request R6)

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
- [ ] A mock `SystemRuntime::fetch_url` that mutates the VFS mid-flight: the request still carries the click-time bytes.
- [ ] A bridge test that `credentials: 'omit'` is set and that relative and protocol-relative URLs are rejected.
- [ ] A typst fixture with a remote image yields a request containing the `_remote/…` file and the rewritten `src` (the exit-83 failure only appears when pandoc runs; the host's parity net checks that once H3 exists).

### Tasks
Five items; the prefetch uses the hardened fetch, so do the snapshot, the hardened fetch, the prefetch, the Lua check and the failure handling, each with its own gate.
- [ ] **VFS snapshot:** a click-time snapshot mechanism so every read in a request goes through one consistent view. `VirtualFileSystem` is not `Clone`, and `WasmRuntime` also holds `runtime_metadata` (`wasm.rs:~221`), which the snapshot copies; decide whether the bootstrap SCSS under `/__quarto_resources__` is copied or shared. The snapshot belongs to one call and is dropped when it ends or fails; no global state is written, so a superseded call finishing late cannot disturb the next.
- [ ] **Prefetch stage:** walk Images including Figures and Custom-block slots plus meta image fields; ignore `data:` and protocol-relative URLs; dedupe; cap size and time; fetch in parallel; take the extension from MIME over URL; rewrite `src` to the mounted local path, keeping the original URL as an attribute for diagnostics.
- [ ] Check the vendored Lua ignores that attribute (run the fixtures through the host's gate).
- [ ] **Hardened fetch variant** of `jsFetchUrl` (`ts-packages/wasm-js-bridge/src/fetch.js`), which today has no scheme, size, timeout, abort-signal (tied to the click, D8.5) or credentials policy and base64s through JSON (~2.3× memory for big images, accepted). `SystemRuntime::fetch_url(&self, url)` has no parameter for a policy or signal and about 15 test mocks implement it, so add a defaulted trait method `fetch_url_hardened(url, policy)` (mocks untouched), implemented by the per-call snapshot runtime, which carries the click's `AbortSignal`; the JS side adds `jsFetchUrlHardened` beside `jsFetchUrl` in `fetch.js`.
- [ ] For docx/pptx/epub a failed fetch leaves the URL, giving pandoc's `Q-11-1` warning + alt text; for typst, a failed fetch is reported as a diagnostic rather than pandoc's exit 83, so the prefetch replaces the failed `Image` with its alt text (`typst.lua:260` calls `mediabag.fetch` on any URL left in the AST).

## Verification

Crate-scoped clippy/nextest, the wasm compile check, the hub-client wasm test, one browser smoke through host H3's harness.

## Exit

Remote images work in all four formats within the CORS limits of the browser.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
