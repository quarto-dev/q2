# Preview: engine-generated images missing (`q2 preview` + hub-client q2-preview)

**Braid strand:** bd-qbhp2cvv
**Status:** plan draft — awaiting review, do not execute yet

## Overview

Knitr (and, by the same mechanism, jupyter) execution results that include
images appear correctly in `q2 render` output but are missing in `q2 preview`
and in the hub-client q2-preview format. The engine's figure files
(`<doc>_files/figure-html/*.png`) never travel with the engine capture that
the preview replays, so the browser has nothing to show.

## Reproduction

Fixture: `~/Desktop/daily-log/2026/07/23/test-knitr-images-quarto-hub/hello.qmd`
(knitr doc with a ggplot cell; copied to a scratch dir for the runs below).

```bash
# baseline — works
cargo run --bin q2 -- render <dir>/hello.qmd
# → hello.html references hello_files/figure-html/unnamed-chunk-2-1.png,
#   file exists on disk, image renders.
# → .quarto/render-manifest.json records hello_files as an
#   Engine-origin resource (kind: Engine, engine: knitr).

# broken — repro
cargo run --bin q2 -- preview <dir>/hello.qmd --no-browser
# then:
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' \
  http://127.0.0.1:<port>/hello_files/figure-html/unnamed-chunk-2-1.png
# → 200 text/html   ← SPA index fallback, NOT the PNG. Broken image.
```

Verified against the capture cache of the running preview
(`<data_dir>/captures/<sha256>.bin`, gzipped JSON):

```json
{
  "supporting_files": [
    "/abs/path/to/repro/hello_files"          // path only — no bytes
  ],
  "markdown": "... ![](hello_files/figure-html/unnamed-chunk-2-1.png) ..."
}
```

## Diagnosis

The preview architecture records an **engine capture** server-side and
**replays** it in the browser WASM pipeline:

1. **Record** — `record_capture` (`crates/quarto-core/src/engine/preview_record.rs:130`)
   runs the q2-preview pipeline truncated at `EngineExecutionStage`. The stage
   emits an `EngineCapture` aux event
   (`crates/quarto-core/src/stage/stages/engine_execution.rs:343-355`)
   containing `{engine_name, input_qmd, result}` where `result` is the
   serialized `ExecuteResult`.
2. **Store** — the capture is gzipped and written as a samod **binary doc**;
   the sidecar/index entry (`CaptureRef.capture_doc_id`) points at it
   (`crates/quarto-preview/src/re_execute.rs:337-354`, same in
   `capture_driver.rs`). This is the "JSON sidecar deposited in the automerge
   document" from the issue description.
3. **Replay** — the browser-side pipeline substitutes `ReplayEngine`
   (`crates/quarto-core/src/engine/replay.rs`) for the real engine; it
   returns the captured `ExecuteResult` verbatim.

The hole: `ExecuteResult.supporting_files` is `Vec<PathBuf>`
(`crates/quarto-core/src/engine/context.rs:151`) — **paths, not bytes**.
During recording, knitr writes the real figure files to disk next to the
source doc, and the capture records only the path. On replay:

- **hub-client**: the WASM has no access to the recording machine's disk at
  all. The rendered AST references `hello_files/figure-html/….png`; the asset
  walker looks it up in the WASM VFS; it isn't there; broken image.
- **`q2 preview` CLI**: same replay path in the embedded SPA. Although the
  figure files *do* exist on the server's disk, the preview server only
  serves *declared* `resources:` (the `RESOURCE_DISK_MAP` artifact route,
  `crates/quarto-preview/src/lib.rs:389`) — engine-generated files are not in
  that map, so the request falls through to the SPA index fallback
  (hence `200 text/html`).

Contrast with `q2 render`: the orchestrator drains
`ctx.resource_report.add_engine_files(...)`
(`crates/quarto-core/src/stage/stages/engine_execution.rs:383-389`, contract
bd-o8pr) and copies engine supporting files into the output directory via
`copy_resources_to_output_dir`
(`crates/quarto-core/src/project/orchestrator.rs:928-964`), which is why
render works. **That copy is the exact divergence point: it is
`#[cfg(not(target_arch = "wasm32"))]`** (line 940, "Native only — the WASM
hub-client preview doesn't write to a real output dir"). On the WASM side,
replay re-adds the recorded paths to `ctx.resource_report`, but nothing
consumes them: the VFS flush (`flush_artifacts_to_vfs`,
`crates/wasm-quarto-hub-client/src/lib.rs:1547`) flushes only
`ctx.artifacts` (theme CSS/JS/fonts), never the resource report.

Both browser-side image resolvers therefore miss:

- AST/q2-preview path: `buildAssetManifest`
  (`ts-packages/preview-renderer/src/q2-preview/assetWalker.ts:51`) →
  `vfsReadBinaryFile(resolved)` misses → `continue` → image silently
  omitted.
- HTML path (hub-client default `format: html`):
  `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts:203-235` →
  VFS miss leaves the relative src, which the sandboxed iframe cannot
  fetch → broken image. (So the bug affects the plain-HTML preview too,
  not just the q2-preview AST format.)

The figures also never enter the automerge project VFS by other routes: for
hub-client, knitr runs on a remote exec server
(`crates/quarto-hub-provider/src/execute.rs:301`) that never uploads the
generated `*_files/` output; the initial project scan happened before the
capture ran.

### Why the fix belongs in the capture, not a server route

A disk-backed server route would fix only the CLI preview. The hub-client
replays the same capture doc on machines that never ran the engine, so the
bytes must travel **inside the capture binary doc** (which already syncs via
automerge). One fix covers both consumers — this matches the existing
design where the capture doc is the single unit of engine-result transport.

### Existing browser-side asset mechanism (reuse, don't invent)

`buildAssetManifest`
(`ts-packages/preview-renderer/src/q2-preview/assetWalker.ts`) already walks
the rendered AST for `Image` nodes, resolves each target against the doc's
`/project/…` VFS path, reads bytes via `vfsReadBinaryFile`, and mints blob
URLs consumed by the iframe's `<Image>` component. If the engine-generated
figures are **materialized into the WASM VFS** at the right `/project/…`
path at replay time, images light up with **zero renderer changes**, in both
hub-client and the q2-preview SPA.

## Proposed fix (sketch — to be refined)

1. **Capture side** (`EngineExecutionStage`, native only): when emitting the
   `EngineCapture` aux event, enumerate `result.supporting_files` (files and
   directories, recursively), read the bytes, and attach a
   `files: [{path: <doc-relative>, contents_base64}]` array to the capture
   payload. Paths are stored doc-relative (e.g.
   `hello_files/figure-html/unnamed-chunk-2-1.png`) so replay is
   machine-independent.
2. **Schema** (`quarto_trace::EngineCapture`): new `#[serde(default)]`
   field so old captures (no `files`) still deserialize; replay treats the
   absence as "no files" (current behavior).
3. **Replay side**: implementation-scoping correction discovered during
   execution — the preview does **not** consume captures via
   `ReplayEngine` (that remains the bd-45yw regression tool). Both the
   SPA and hub-client thread `capture_gz_json` into the q2-preview
   pipeline's **`CaptureSpliceStage`**
   (`crates/quarto-core/src/stage/stages/capture_splice.rs`, bd-lucp),
   which splices captured output blocks into the live AST. That stage
   has `ctx.runtime` — and in WASM, `SystemRuntime::file_write` *is*
   the VFS write. So materialization lives in `CaptureSpliceStage::run`:
   for each capture, write its embedded `files` to
   `<doc_ast.path.parent()>/<rel_path>` via the runtime before splicing.
   This is a pure quarto-core change — no wasm-bindgen surface changes,
   and it works identically under native tests (writes to the temp
   project dir) and WASM (writes to the VFS the
   assetWalker/iframePostProcessor resolvers read).
4. **No SPA/renderer changes expected** — the existing resolvers
   (`assetWalker.ts` blob URLs for the AST path,
   `iframePostProcessor.ts` data URIs for the HTML path) pick the files
   out of the VFS once they're present.

### Design alternative considered and rejected

Uploading the figures as separate automerge binary docs (or into the
project file tree) keyed by path: more moving parts, pollutes the synced
project with generated artifacts, and requires the remote exec server
(hub) and local preview to each grow an upload path. Embedding in the
capture doc keeps "one capture = one self-contained engine run" and rides
the existing sync/binary-doc plumbing unchanged.

### Decisions (reviewed with Carlos, 2026-07-23)

- **Size policy:** v1 unbounded, with a `tracing::warn!` when the
  gzipped capture doc exceeds **10 MB**. Hard caps deferred until a real
  doc hurts.
- **Directories:** embed `supporting_files` directory entries wholesale
  in v1 (the `*_files` dir contains only what the engine generated for
  this doc).
- **jupyter:** in scope — verify with a basic matplotlib doc (see test
  plan; fixture below).
- **Old captures:** fine to replay without images until re-executed; the
  existing staleness/re-execute machinery refreshes them. `#[serde(default)]`
  keeps deserialization working.
- **No disk-serving route in `q2 preview`:** not needed. If execution of
  this plan hits an unexpected wall that would require it, stop and
  discuss before building it.

jupyter verification fixture:

```qmd
---
title: hello jupyter
engine: jupyter
---

```{python}
import matplotlib.pyplot as plt
plt.plot([1,2,3])
```
```

## Related work

- `claude-notes/plans/2026-06-09-preview-embed-vfs-resolution.md`
  (bd-kjrpya2d) names this exact gap: `copy_resources_to_output_dir` is
  native-only, and the assetWalker fallback "relies on referenced static
  assets being present in the VFS source" — which generated figures are
  not.
- `claude-notes/plans/2026-05-13-q2-preview-phase-c.md` — capture
  architecture (Phase C); risk #4 is the capture-size discussion.
- bd-eiku4ymo (related, filed from this plan's review) — uncompressed
  audit/GC metadata envelope on capture binary docs (createdAt,
  sourcePath, engines) for sync-server audits and orphaned-capture
  garbage collection. Deliberately kept out of this bugfix's scope.
- bd-o8pr — supporting-files / resource-report contract.
- bd-45yw / bd-5yff4 — replay engine + multi-engine captures.

## Test plan (TDD — first phase of execution)

- [ ] Rust unit: capture recorded for an engine whose `ExecuteResult` lists a
      supporting file embeds the file bytes (passthrough test engine writing
      a fake PNG; assert `files` in the emitted capture payload).
- [ ] Rust unit: capture without `files` field (old shape) still
      deserializes and replays (serde default).
- [ ] Rust unit: replay materializes embedded files into the runtime VFS at
      the doc-relative path.
- [ ] Rust unit: capture-doc size warning fires above 10 MB gzipped (and
      not below).
- [ ] hub-client/WASM test: `assetManifestProject`-style test where a
      captured doc with an embedded figure renders an `<img>` with a blob
      URL (follow existing `*.wasm.test.ts` patterns).
- [ ] E2E (knitr): `q2 preview` on the repro doc; fetch the figure
      URL/observe the preview in a browser; image renders (manual
      verification recorded in this plan per the end-to-end policy).
- [ ] E2E (jupyter): same with the matplotlib fixture above.

## Implementation notes (finalized at execution start, 2026-07-23)

- **Cost gating:** file collection runs only when
  `PipelineObserver::wants_engine_capture_files()` returns true (new
  trait method, default `false`). Only the preview's `CaptureCollector`
  (`preview_record.rs`) opts in — plain `q2 render` and trace observers
  pay zero extra I/O. (`JsonTraceObserver` deliberately stays opted
  out for v1 so `-v` trace files don't bloat; revisit if ReplayEngine
  ever wants materialization.)
- **New module** `crates/quarto-core/src/engine/capture_files.rs`:
  `collect_capture_files` (record side) + `materialize_capture_files`
  (splice side) + `capture_doc_size_warning` (10 MB, pure fn) +
  shared `gzip_captures` used by all three writers
  (`capture_driver.rs`, `re_execute.rs`, hub-provider `execute.rs` —
  currently three duplicated gzip blocks).
- **Path convention:** `CaptureFile.path` is doc-relative with forward
  slashes. Recording resolves `supporting_files` entries (absolute or
  doc-relative) against the doc's parent dir; entries that don't
  resolve under the doc dir are warn+skipped (relative image refs
  couldn't reach them anyway). hub-provider runs engines in a deleted
  temp dir, so recording-time embedding is the only moment the bytes
  exist — confirming the design.
- **Serialization stability:** `files` uses
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]` — old
  captures deserialize, and captures without files serialize
  byte-identically to today (existing snapshots unaffected).

## Work items

- [x] Phase 1: `EngineCapture.files` schema (quarto-trace) + literal-site updates
- [ ] Phase 2: capture-side embedding (observer gate, collect_capture_files,
      engine_execution wiring) — failing test first
- [ ] Phase 3: replay-side VFS materialization in CaptureSpliceStage —
      failing test first
- [ ] Phase 4: shared gzip helper + 10 MB warning in the three writers
- [ ] Phase 5: end-to-end verification (knitr + jupyter; `q2 preview` +
      hub-client WASM build; `cargo xtask verify`)
- [ ] Phase 6: docs/changelog (hub-client changelog if hub-client touched)
