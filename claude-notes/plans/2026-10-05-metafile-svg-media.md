# Plan: keep imported EMF/WMF as SVG, rasterize only for docx/pptx export

**Date:** 2026-10-05
**Status:** Planned; nothing started. Decisions D1-D3 settled with Gordon 2026-10-05 (all three recommendations accepted). Adversarial feasibility review done 2026-10-05 (two Sonnet reviewers, Rust/pipeline and browser/TS/import); its findings are folded in below. None changed D1-D3.
**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md). **Revisits I8** ("must be PNG, not SVG") and touches I9, I12, I15, I16, I20, I21 (see Epic edits in T5).
**Depends on:** P4 (import service) and P5 (import UI, `5f92e1275`), both landed. P6 is not required.
**Branch:** `import/metafile-svg-media` from `feature/hub-import` (workspace-5).

## Why

EMF/WMF are vector formats. I8 converts every imported one to a 2x PNG, because pandoc.wasm has no `rsvg-convert` and
an SVG in a docx download shows as alt text. That discards the vector data at import and makes the PNG the stored,
project-wide format to serve one output path. The cost lands everywhere else: a vector clipart becomes a raster in
the project, soft when zoomed, larger than the SVG for line art, and not editable or diffable.

**The goal is keeping the vector in the project.** Docx/pptx rasterization (T2-T3) is the safety net that makes that
shippable; as a side effect it fixes user-authored SVG figures (ggplot, drawio) in docx/pptx. (Note: clipart already
*works* in docx today via the I8 PNG, so "cheapest fix for clipart" is not the goal; raising `SCALE` would be that.)
One lossy step remains: rtf.js's EMF/WMF-to-SVG conversion covers only part of GDI (see T0).

The limitation is narrower than I8 assumes:

- It is **pandoc's docx and pptx writers only.** They shell out to `rsvg-convert` to make a PNG fallback.
  epub and typst keep the SVG (design D9, `../designs/pandoc-wasm-architecture.md:122`), as do html and the preview.
- **Native has the same gap.** Re-verified 2026-10-05 with native pandoc 3.11 and a `PATH` without
  `rsvg-convert`: `[WARNING] Could not convert image ...` and an empty `<a:blip>` with `word/media/rId9.svg`.
  Native already turns that warning into a diagnostic (`crates/quarto-core/src/stage/stages/pandoc_write.rs:1854-1880`),
  so no new bead.
- It affects **every SVG a document references**, not just imported metafiles.

So: store the SVG, and rasterize at export when the writer is docx or pptx.

## Findings that shape the design

- **The extension decides, not the bytes.** A PNG saved as `fake.svg` gets the same warning and empty blip under native
  pandoc without `rsvg-convert`. The `Image` target must be rewritten to a `.png` path. Match `.svg` case-insensitively
  (`B.SVG` fails identically). `sp%20ace.svg` also fails (pandoc decodes it); `a.svg?x=1` is not a file, skip it.
- **Precedent: R6 remote images.** `PrefetchRemoteImagesStage` (`crates/quarto-core/src/stage/stages/prefetch_remote_images.rs`)
  fetches through a runtime JS hook (`js_fetch_url_hardened`, `crates/quarto-system-runtime/src/wasm.rs`), mounts at
  `<doc_dir>/_remote/<hash>.<ext>`, rewrites `Image.src`, and keeps the original as an attribute.
- **I9 ownership forbids patching the request in TS.** Rust owns paths and `job_id`, which hashes `resource_refs`
  (`pandoc_request/types.rs:116-119`). The stage must run inside `render_pandoc_request`.
- **The rasterizer needs a DOM** (`<img>` + `<canvas>`), so it is main-thread TS reached through a wasm-js-bridge
  function. The hub wasm is only imported on the main thread (the only `new Worker` sites are `pandoc.worker.ts` and
  `typst.worker.ts`, which don't import it); node tests have no `document`.
- **Rust-side rasterizing (resvg) is not a good fit:** wasm has no system fonts, and EMF text becomes SVG `<text>`.
- **docx/pptx image display size.** `canvas.toBlob` writes no pHYs chunk, and pandoc then sizes the image at a different
  dpi than intended (measured, native pandoc 3.11: 400x300 PNG without pHYs gives `wp:extent` 5.56 in; the same PNG
  with pHYs 192 dpi gives 2.09 in; native with `rsvg-convert` on a 200x150 SVG gives 2.08 in). The rasterizer therefore
  renders at 2x intrinsic size **and writes a 192 dpi pHYs chunk (7559 px/m)**, so pandoc displays it at intrinsic
  size. The `Image`'s own width/height only change display size, so they need not reach the rasterizer. The existing
  `convertMetafileToPng` has the same missing-pHYs property today.
- **Browser intrinsic size is unreliable:** `naturalWidth x naturalHeight` for a viewBox-only SVG is 300x150 in
  Chromium and 100x50 in WebKit (Firefox not yet measured; it launches now, see T0). The JS rasterizer parses the SVG root
  (`width`/`height` with units, else `viewBox`, else a defined 300x150 default) and keeps that aspect ratio in
  `drawImage(img, 0, 0, w, h)`.
- **rtf.js output is not byte-stable.** Clip/pattern ids come from a static counter (`rtf.js/src/emfjs/Helper.ts:324,332-333`,
  `src/wmfjs/Helper.ts:279-280`, written at `src/util/SVG.ts:253,276`), so importing the same EMF twice in one page
  session gives different bytes and different `<sha12>.svg` names. Needs normalizing (T1) or D2's hash-named
  dedupe (I12/I20) fails. The `emf-docx` fixture has no clip/pattern, so it won't reveal this.
- **rtf.js emits a safe element set** (svg, defs, clipPath, pattern, filter, feFlood, feComposite, image, rect, line,
  polygon, polyline, ellipse, path, text; hrefs always `data:`; no `<script>`, `<foreignObject>`, `<style>`). No
  sanitizer is needed; lock it in with a unit test (T1). rtf.js bug (fidelity today as well): `src/emfjs/Bitmap.ts:174-179,202`
  produces `data:data:image/jpeg;base64,...` for BI_JPEG/BI_PNG bitmaps, so those don't render. Out of scope; record in T0.
- **Fidelity trade, stated honestly.** Today text is baked at import time. With a stored SVG, text renders with the
  viewer's fonts (preview and the PNG both use the browser's `<img>` engine, so they match each other; typst and
  Word use their own fonts).
- **Cost.** No per-keystroke cost: the PDF preview writer is typst (`PdfPreviewPane.tsx:92`), so the stage is gated off;
  docx/pptx render only on click. Measured in Chromium (3000-path SVG): 20 ms at 200x100, 540 ms at 4096x2048,
  1.2 s at 4096x4096. No content-hash cache needed for v1 (dedupe by hash within one render).

## Decisions (settled with Gordon, 2026-10-05)

- **D1 Where the rasterize step lives.** Decided: a Rust stage `RasterizeSvgImagesStage` in the pandoc-request stage
  list, only for writers docx and pptx, beside `PrefetchRemoteImagesStage`. Alternatives: a TS pre-pass, or a TS
  `{path: png}` map like `capturesByPath`. Review costed all three (rough, unbuilt): total LOC and files touched are
  comparable (~300-400 lines, 8-12 files); D1 is Rust-heavy with ~3 new TS files and no edits to existing TS. The map
  option is *not* simpler overall and also (a) must rasterize every project SVG on every click (~1-5 s for 50 SVGs,
  making a cache mandatory), (b) can't see R6-fetched remote SVGs, (c) doesn't know each Image's size. I9 does not
  forbid the map option, but D1 stays: the reason is Rust's ownership of Image enumeration/rewrite/`job_id`, and
  rasterizing only referenced SVGs, not LOC.
- **D2 Stored extension for imported metafiles.** Decided: `<12 hex>.svg`, hash over the SVG bytes (I12), after id
  normalization (T1). Alternative: keep `.emf`/`.wmf` next to a generated `.svg`; rejected, browsers can't show the
  original and it doubles the media.
- **D3 Failure policy.** Decided: if rasterizing an SVG fails or times out, leave the SVG (pandoc warns, alt text,
  exactly today's D9 behavior) and surface a Rust diagnostic naming the file. Never fail the download for it.
  Clarification from review: this includes PNG-size overruns (T3) and a tainted-canvas `toBlob` throw (e.g.
  `<foreignObject>` in drawio/mermaid SVGs). "Rasterizer unavailable" (native, node) is **not** a failure: silent no-op.

## Key files

- `hub-client/src/pandoc/metafileToPng.ts`: today `convertMetafileToPng` (rtf.js to SVG, then `<img>`/`<canvas>`/`toBlob`).
  Splits into `convertMetafileToSvg` (render + serialize, no canvas) and the rasterizer, which moves to the bridge (T2).
- `hub-client/src/pandoc/importService.ts:144,309` and `importMedia.ts:21,84-87`: `convertImage` dep, its type, the 10 MB rule, naming.
- `crates/quarto-core/src/import/report.rs:124,135` (Q-24-9, Q-24-10 text) and `crates/quarto-core/src/import/media.rs:52-54` (DISPLAYABLE already includes svg).
- `crates/quarto-core/src/stage/stages/prefetch_remote_images.rs` and `crates/quarto-core/src/pipeline.rs:606-613`: stage to copy, and where it is inserted.
- `crates/quarto-core/src/pandoc_request/resources.rs:242-291`: `ResourceCollector::mount`/`add_image` (limits, target resolution).
- `crates/quarto-system-runtime/src/traits.rs:313-314,513` and `wasm.rs:175,199-211`: runtime trait (default-method precedent `fetch_url_hardened`), static `raw_module` extern, js-bridge-off stub.
- `ts-packages/wasm-js-bridge/src/` (`fetch.js`, `fetch.d.ts`, `fetch.test.ts`, `sass.js:97` `setVfsCallbacks` for the module-state seam); `crates/quarto-system-runtime/Cargo.toml:12` comment; `hub-client/vite.config.ts:267` aliases.
- Native `q2 preview` embed and native CLI use `build_pandoc_pipeline_stages` (untouched) and are unaffected.

## Tasks

Order: **T0, T2, T3, T4, T1, T5.** T1 is the flip that makes imports store SVG; it must not land before T3, or
imported EMF/WMF show as alt text in docx/pptx on the integration branch in between.

- [x] **T0 Spike (stop before T2 if it fails; failure means staying on I8).** Use P1's `emf-docx` fixture **and a real
  clipart WMF, `/Users/gordon/docs/Clipart/Powerpnt/book3.wmf`** (6454 bytes; the one Gordon tests with; record whether it has text, a clip or a bitmap; the fixture is a plain rectangle). Pass requires all of:
  (1) the SVG renders non-blank through `<img>` in Chromium and WebKit, and visually matches the original EMF rendering
  closely enough (compare against the old PNG; record differences); (2) `typst compile` and `typst.ts` succeed with a non-empty PDF;
  (3) serialized SVG has only the element allowlist and `data:` hrefs; (4) ids normalized and bytes stable across two
  conversions in one session; (5) SVG byte size is recorded against the EMF and the old PNG (I16 inflation check: a
  bitmap-wrapping EMF becomes ~1.33x base64). Also probe: user SVGs with `<foreignObject>` (drawio, mermaid) taint the
  canvas, so `toBlob` must throw into D3's path; text-heavy SVG; the `data:data:` Bitmap.ts bug. Also run Firefox (`--project=firefox` in `playwright.harness.config.ts`; the macOS 27 launch fix `ad29129b4` was ported there on this branch and verified by running `new-file-dialog.harness.spec.ts`: 3 passed). Record dimensions and a scratchpad path in the Handoff log.
- [x] **T2 JS rasterizer hook.** `ts-packages/wasm-js-bridge/src/rasterize.js` (+ `.d.ts`, `.test.ts`), a `raw_module`
  extern like `fetch.js`, **module-level** (so `test-hooks.ts`/`devHarness.ts`/`wasmDeps` callers all get it).
  `rasterizeSvg(svg: Uint8Array, maxSide): Promise<Uint8Array>` (`Uint8Array` both ways, not the fetch bridge's base64
  JSON). It parses the SVG root for intrinsic size (see Findings), normalizes explicit root width/height before loading,
  renders at 2x intrinsic with aspect ratio kept and longest side <= **2048** (a 6.5 in page at 300 dpi is ~1950 px),
  writes a 192 dpi pHYs chunk, caps input size at the I16 limit, uses the import per-image timeout (note: a timeout
  cannot cancel `<img>` decode; the input cap bounds it), and rejects with a typed "unavailable" error when there is
  no `document`. `setRasterizer` seam (cf. `setVfsCallbacks`) for node tests. Rust side: `SystemRuntime` gets
  `async fn rasterize_svg(&self, svg: &[u8], max_side: u32) -> RuntimeResult<Vec<u8>>` defaulting to `NotSupported` and
  a sync `fn can_rasterize_svg(&self) -> bool` defaulting to false (as `fetch_url_hardened`, `traits.rs:513`); the wasm
  impl, the extern and the js-bridge-off stub follow `wasm.rs:175,199-211`, and the snapshot runtime carries the abort
  signal as fetch does. Native runtime and test doubles (e.g. `ScriptedNet`) need no change. "Unavailable" maps to a
  silent no-op, not a diagnostic (unlike prefetch's `NotSupported`).
- [x] **T3 `RasterizeSvgImagesStage`.** Inserted **after** `PrefetchRemoteImagesStage` (so remote SVGs fetched into
  `_remote/` are rasterized too); key on `Docx | Pptx` only (odt shares the Rust list), reading `ctx.format.identifier`
  at run time like prefetch. Checks `can_rasterize_svg()` first, so native and node never walk the AST. For each local
  `Image` whose target ends `.svg` (case-insensitive), resolve the target with a shared `resolve_image_target(doc_dir, target)`
  extracted from `ResourceCollector::add_image` (`resources.rs:273-291`: percent-decode, absolute vs `doc_dir.join`, `admit`),
  and look it up in the snapshot (missing: skip; pandoc reports the original path as Q-11-1). Call the hook
  **sequentially** (not R6's join_all of 6: six 4096 canvases is ~64 MB each at once); the hook decides pixel size
  (see T2), Rust never parses SVG or units. Mount at `<doc_dir>/_raster/<hash>.png`, hash over SVG bytes; rewrite
  `Image.src` via `normalize_request_path`; keep the original on `q2-raster-src` (cf. `q2-remote-src`). Dedupe by SVG hash
  (one mount for an SVG referenced twice; in a book the merged page is at project root, `_raster` is created once).
  **Limits (D3):** before mounting, check the PNG against `constants().limits.image_bytes` (25 MB) and a running total
  (as prefetch tracks `fetched`, `prefetch_remote_images.rs:197-210`); over the limit, leave the SVG and warn, because
  `ResourceCollector::mount` (`resources.rs:242-258`) would otherwise push a Q-11-1 *error* and `render.rs:280-293`
  would drop the whole request. Also a total-pixels budget (SVG and PNG both pass through Rust wasm, which grows ~4.5x
  per payload and never shrinks). Tests: Rust with a scripted hook (pattern: `pandoc_remote_images.rs`); no-op without
  `can_rasterize_svg`; `job_id` determinism with a scripted hook; uppercase `.SVG`, `%20`, query-string and
  twice-referenced cases; images inside figure, callout and table; a **docx `wp:extent` test** that a fixture SVG's
  extent equals native-with-`rsvg-convert` (guards the pHYs fix); an oversize-PNG-keeps-SVG test. Update the two
  existing pipeline tests that pin prefetch's position: `pipeline.rs:6945` (`plain.len()-1`) and `pipeline.rs:7030`
  (`finishing[n-2]`). No docx golden/recording fixture contains an SVG, so no goldens change.
- [x] **T4 Real-browser test (the gate).** Node suites can't rasterize, so a fake hook there (via `setRasterizer`) is
  not enough alone. The Playwright harness spec must run through `wasmDeps`: render a docx from a **user-authored SVG**,
  unzip, assert `word/media/*.png` is a valid, non-blank PNG and the `<a:blip>` has an `r:embed`. Chromium, WebKit and Firefox
  all launch here (`test:harness` is chromium-only; run the pandoc-* spec with `--project=webkit --workers=1`,
  `playwright.harness.config.ts:65-67`). `hub-client/dist` is stale (Oct 3, lacks `convertMetafile`): needs
  `VITE_E2E=1 npm run build` plus `build:wasm` first. Extend to the imported-EMF case after T1.
- [x] **T1 Import stores SVG (the flip; after T4).** `convertMetafileToSvg(bytes, format): Promise<{svg: Uint8Array, width, height}>`.
  - **Normalize ids** after serializing: renumber `EMFJS_[a-z]\d+` and `wmfjs_[a-z]\d+` by order of appearance; test that a
    clipped EMF converted twice in one session gives identical bytes.
  - **Store at 1x:** root `width`/`height` = inches x 96 (today's `metafileSize` bakes `SCALE=2`: `width="200px"` on
    `viewBox="0 0 100 50"`); keep the imported `Image`'s own `{width="…in" height="…in"}`.
  - `importService` stores `<hash>.svg`, mime `image/svg+xml` (`svg` is already in `BINARY_EXTENSIONS`
    `ts-packages/quarto-automerge-schema/src/index.ts:485`, image list `:531`, `inferMimeType` `:619`, and
    `crates/quarto-hub/src/resource.rs:38`; DISPLAYABLE already includes svg, so no Q-24-11). I16's 10 MB rule applies
    to the SVG's size; a bitmap-wrapping EMF can exceed it (base64 ~1.33x): it falls back to the original with
    `conversion_failed` (`importMedia.ts:87`); add a test. Drop `MAX_PNG_SIDE` from the import path.
  - On failure keep the original `.emf`/`.wmf` and `conversion_failed`, unchanged (I8, I21).
  - Permanent unit test: element allowlist plus "every href starts with `data:`".
  - Files to update: `importMedia.ts:21` and `importService.ts:144` (`convertImage` type), `importMedia.test.ts`,
    `importService.test.ts`, `importDocument.wasm.test.ts`, `test-hooks.ts:198` (`convertMetafile`),
    `metafileToPng.test.ts`, the E2E assertions in `pandoc-import-emf.harness.spec.ts` (currently a 200x100 PNG), and
    the Rust report text `report.rs:124` (Q-24-9 "couldn't be converted to PNG") and `:135` (Q-24-10 "converted to PNG")
    with their snapshots/tests, plus the stub text at `importService.ts:119`. The P5 UI has no PNG-specific assumption
    (`STUB_PNG` still works).
- [x] **T5 Docs and close-out.** (Done in `f8689311e` and `6d52cfd2f`; two items consciously not done: the user-facing "export embeds a PNG" note has no natural page, and the `bd-myoj9kp5` reference was not added to the epic text. Import report header softened to "Notes from the import:".) Epic edits: I8 (now "stored as SVG; rasterized at docx/pptx export"), I15, **I9** (PNG
  bytes now enter the Rust wasm at export; scope-limited, since project SVGs are already in the VFS snapshot and R6
  already brings remote bytes in), **I12/I20** (names are stable only because ids are normalized), I16 (size basis flips
  from PNG to SVG). Update the D9 bullet in `pandoc-wasm-architecture.md` ("SVG images in docx/pptx" now rasterized).
  User-facing note that docx/pptx export embeds a PNG of each SVG. Reference `bd-myoj9kp5` (pipeline-wide SVG posture;
  `pasteImages.ts:9-11` excludes SVG from silent paste because SVG can carry script) so this plan doesn't pre-empt it.
  Run the P5 import E2E against `book3.wmf`.
- [x] **T6 odt in the "Download as" menu; answers the open "odt out of scope?" question (2026-10-05).** odt is a menu row
  (`odt`, `.odt`, `application/vnd.oasis.opendocument.text`) directly below docx, in `PANDOC_FORMATS`,
  `MENU_FORMATS`, and their tests. It is not in `NATIVE_FORMATS` (the embed's native route is unchanged), and not in
  `STAMPED_FORMATS` (that is OOXML comment/change authorship). **SVG is not a problem in odt, so the rasterize stage
  stays `Docx | Pptx` only.** pandoc's odt writer embeds the SVG as is (`Pictures/N.svg`, manifest
  `image/svg+xml`, `draw:image xlink:href`), sized right (a 100x50 px viewBox-only SVG gets 75pt x 37.5pt, a 2in x 1in
  one 144pt x 72pt), and `startDownload` reports no diagnostics. An imported EMF (now SVG) exports the same way. Checked
  by Playwright (`pandoc-svg-rasterize`: 7/7 on Chromium, Firefox, WebKit), and LibreOffice (headless to PDF) draws the
  EMF fixture's black rectangle. Gordon opened an odt in the app and the SVG came through. Word was not tested.

## Verification

Per-crate gate each task: `cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p quarto-core`
(also `-p quarto-system-runtime` for T2); hub-client `vitest` for the TS packages touched. One
`cargo nextest run --workspace` at the end, reported as a delta against the live baseline. T4/T5 need Playwright
(chromium and webkit work here; see T4 for rebuild steps).

## Out of scope

- pdf/latex output through pandoc (not a hub path; typst handles SVG).
- Embedding SVG in docx with a PNG fallback the way native pandoc does with `rsvg-convert`; we emit a PNG only.
- Improving rtf.js's GDI coverage, including the `data:data:` JPEG/PNG bitmap bug (T0 records it).
- SVGs that are not `Image` nodes at this stage: brand logos, reference-doc images, Lua-filter-generated images stay unfixed.
- Persisting a rasterization cache (not needed under D1).
- Known latent issue shared with `_remote`: a literal `%` in a directory name is misread by the collector's percent-decode.

## Handoff log

### STATE (2026-10-05, final): T0-T5 done; branch ready to finish

T1 landed in `3764dd6b4` (on top of the WIP `c331944c6`). All of it is on `import/metafile-svg-media` only: the local `feature/hub-import` ref is still `448c270c1`, so T2-T4 (`70e23fac8`) have not landed there either.

**Verified for T1:** vitest 135 files / 1602 tests green; `tsc -b` clean; `cargo clippy -p quarto-core -p quarto-error-catalog --all-targets -D warnings` clean; Playwright (throwaway configs on :5199 and :5198, because other worktrees' previews hold :5173 and :5174) `pandoc-import-emf` + `pandoc-svg-rasterize` 24/24 on Chromium and Firefox, 12/12 on WebKit; `import-dialog` 36/36 Chromium; P5 `import-document.spec.ts` 13/13 Chromium (rebuilt with `VITE_DEFAULT_SYNC_SERVER=/ws`). The Rust planner names converted media `<sha12>.<ext>` from the manifest's `ext`, so `.svg` needs no Rust change; no snapshot pins `.png` for converted media; Q-24-9/Q-24-10 docs pages now say SVG.
**book3.wmf** (local only, not committed) through the real converter in all three browsers: 367x220 at 1x, 3267 B, elements svg/defs/clipPath/rect/polygon/ellipse, byte-identical when converted twice (it drifted `wmfjs_c0` -> `wmfjs_c1` before normalization), non-blank.
**Workspace nextest (once, at the end):** 15965 passed, 202 skipped, 2 slow, 226 s. No live baseline was run (that needs a cold build in workspace-1 at `448c270c1`). What the run shows: 12 passing tests in the two new Rust modules (`rasterize_svg_images::`, `pandoc_svg_rasterize::`); one existing `pipeline.rs` test was renamed/updated, none removed. The delta against a baseline is therefore expected to be +12 passed, +0 skipped, but that is inferred from the run's test names, not measured.

**T5 done** (Gordon approved the epic edits; committed in `f8689311e`). Not done: the user-facing export note and the `bd-myoj9kp5` cross-reference.

**Decisions for Gordon still open** (unchanged): how faithful the SVG must be for T0 (book3 looks right by eye; no independent renderer); fix the rtf.js id counter upstream vs keep the normalization; ~~odt out of scope~~ (answered by T6: odt embeds SVG natively, stays out of the rasterize stage); `bd-myoj9kp5` overlap. Made without confirmation: 192 dpi (pHYs 7560), 2048 cap, no pixel budget in T3, the T5 epic amendments.

### T2-T4 implementation notes (2026-10-05)

- **pHYs is 7560, not 7559.** pandoc truncates the dpi it derives from pHYs: 7559 px/m is 191.9986, read as 191, and the image came out 0.5% too large (`wp:extent` off by ~5000 EMU). 7560 reads as 192. The plan's "192 dpi (7559 px/m)" is superseded in `rasterize.js`.
- **No total-pixels budget in T3.** Rust only holds compressed PNG bytes (the pixel cost is the JS canvas, rasterized sequentially), so T3 enforces the per-file and running-total byte limits and no pixel budget.
- **The `wp:extent` guard moved from T3 to T4.** The pHYs chunk is written by the JS rasterizer and pandoc sizes it, so only the real-browser spec can test it. It asserts the extents equal the SVG's intrinsic size (100x50 px at 96 dpi; 2in x 1in) in Chromium, WebKit and Firefox.
- **T4 spec** `hub-client/e2e/pandoc-svg-rasterize.harness.spec.ts` runs through `pandoc.startDownload` (the production controller; `pandoc.download` returns only pandoc's diagnostics, not the request build's). Files go in with `vfsAddBinaryFile`, as the project stores SVG. 4 tests x Chromium, WebKit, Firefox pass.
- **Playwright gotcha:** the harness config has `reuseExistingServer`, so a `vite preview` already on :5173 from another worktree is silently tested instead of this worktree's dist. Check `lsof -iTCP:5173` or run a throwaway config on another port.
- `<foreignObject>`: Chromium/WebKit taint the canvas (SVG kept, warning); Firefox rasterizes it.

### T0 spike (2026-10-05): PASSED

Scratchpad (session-local, may not persist): `entry.ts` + `spike.mjs` + `probe.mjs` (esbuild IIFE of `metafileToPng.ts` driven by Playwright).

- **Inputs.** `book3.wmf`: 6454 B, a red book drawn from polygons and ellipses, **one clip (`wmfjs_c0`), no text, no bitmap**; renders 734x440 (2x). `emf-docx` fixture: 132 B plain rect, 200x100.
- **(1) Renders non-blank** through `<img>`+canvas in Chromium, WebKit and Firefox (book3: ~197.4k of 323k px opaque in all three; fixture 10044 of 20000). Viewed the PNG: a clean red book with shaded pages. The old PNG *is* this SVG rasterized, so "matches the old PNG" holds by construction; no independent EMF/WMF reference renderer exists here (no inkscape/LibreOffice), so fidelity against the *original* metafile is judged by eye only.
- **(2) typst** (`typst compile`, `#image("x.svg")`): both SVGs compile to non-empty PDFs (4735 B, 3236 B). `typst.ts` not run separately; it embeds the same typst core.
- **(3) Element allowlist:** book3 uses svg, defs, clipPath, rect, polygon, ellipse; fixture uses svg, rect. No `href`, `<text>`, `<script>`, `<style>`, `<foreignObject>`.
- **(4) Ids NOT stable:** converting book3 twice in one page gives `wmfjs_c0` then `wmfjs_c1` (all three browsers), so bytes differ. The fixture (no clip) is stable. Confirms the T1 normalization requirement.
- **(5) Sizes:** book3 WMF 6454 B, SVG 3267 B (at 2x; 1x differs only in width/height digits), old PNG 45-56 KB depending on browser. Fixture: EMF 132 B, SVG 388 B, PNG 0.8-1 KB. Both SVGs are smaller than the PNG. Bitmap-wrapping EMF inflation not measured (no sample).
- **Probes.** `<foreignObject>` SVG: `toBlob` throws `SecurityError` in Chromium and WebKit (D3 path), but **Firefox does not taint** (`toBlob` returns a blob). viewBox-only SVG `naturalWidth x naturalHeight`: Chromium 300x150, WebKit 100x50, Firefox 300x150. `data:data:` bug confirmed in `Bitmap.ts:179,182,202` (`mime` already carries the `data:` prefix); out of scope.
- **Open for Gordon:** whether "by eye, clean render of one clipart" is enough fidelity evidence for the gate.
