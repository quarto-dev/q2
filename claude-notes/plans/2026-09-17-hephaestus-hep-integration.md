# Hephaestus (`.hep` plot document) support in Quarto 2

**Strand:** bd-3qych45b
**Status:** phase 1 implemented (branch `braid/bd-3qych45b-hephaestus-hep-svg`); phases 2–5 filed as child strands
**Date:** 2026-09-17

## Overview

[hephaestus](https://github.com/posit-dev/hephaestus) (cloned at
`external-sources/hephaestus`, v0.4.1) is Posit's Rust 2D scene renderer for
data visualization. Its **plot document** format — a self-contained binary
file, `.hep` by convention, magic `HEPHPLOT` — captures a plot's
*configuration* (data, scales, geoms, theme, font *names*) with nothing
size-dependent baked in. A reader rebuilds the live `PlotComposition` and
re-solves the layout at whatever size it is given, so a `.hep` reflows rather
than scales. The stated intent is for R plotting packages to emit `.hep`
natively from knitr cells.

Goal of this plan: decide how Q2 should consume `.hep` files so that

1. `q2 render` turns `![](plot.hep)` into something every output format can
   display — today HTML/revealjs, later PDF (LaTeX/Typst) — without the
   engine or the user doing anything format-specific;
2. `q2 preview` and the hub-client (the `q2-preview` / `q2-slides` React
   paths only; the plain-HTML iframe path is being deprecated) show the same
   plots live in the browser;
3. brand.yml typography and light/dark mode can be layered on afterwards
   without redoing (1) or (2).

The simplest first step the user proposed — "a pipeline stage that converts
`.hep` to `.png`" — is the right *shape*, but the survey below changes the
*target format*: hephaestus's PNG path needs a GPU adapter, while its SVG and
PDF backends are renderer-free and pure Rust. The recommendation is therefore
**`.hep` → SVG for HTML-family output** (and later `.hep` → PDF/SVG for
LaTeX/Typst), with PNG as an opt-in that is only worth doing via a CPU
rasterizer.

## Facts from the survey that shape the design

Hephaestus side (all from `external-sources/hephaestus`):

1. **Rasterizing needs a wgpu adapter.** Both raster backends (`vello`,
   `vello-hybrid`) open a wgpu device; `HybridRenderer::new()` fails with
   `BackendError::NoAdapter` on a machine with no GPU/driver
   (`src/backend/CLAUDE.md`, `examples/document_placeholder.rs`). The CPU
   backend (`blend2d`) is a feature placeholder with no code behind it.
   A headless CI box or a locked-down server renders nothing, and GPU output
   is not byte-deterministic across machines (snapshot tests can't compare
   bytes).
2. **SVG and PDF are renderer-free.** `--no-default-features --features
   document-read,svg` (or `,pdf`) builds with no wgpu at all, on rustc 1.86.
   Verified locally today: `cargo check` succeeds on our
   `nightly-2026-04-28` toolchain; `examples/document_save` wrote a 10,553-byte
   `.hep`; `examples/document_svg` rendered it at two sizes (60 KB SVG each,
   reflowed rather than scaled). The SVG keeps text as real `<text>` elements
   naming their font family, placed by one anchor plus `textLength`
   (`src/backend/svg/CLAUDE.md`). The PDF backend embeds a subset font for
   every glyph drawn, so a figure looks the same on a machine with none of the
   fonts — the right artifact for `\includegraphics`.
3. **Dependency footprint of the renderer-free build** is pure Rust:
   `parley` (fontique / harfrust / skrifa / ICU), `kurbo`, `peniko`,
   `pulldown-cmark`, `clipper2-rust` (pure Rust, just `num-traits`),
   `thiserror`, optionally `png`. None of these are in q2's `Cargo.lock` today
   except an `icu_properties`. No C/C++ build steps, wasm-clean. `png` is worth
   enabling: documents *can* embed raster images (`WriteOptions::embed_images`)
   and without the codec those rows are reported and skipped.
4. **Fonts are named, not carried.** A `.hep` names families (the theme's
   `text.font.family`, e.g. `sans-serif`); `embed_fonts` is off by default
   because macOS resolves `sans-serif` to a 2.4 MB Helvetica collection.
   Natively, fontique enumerates system fonts, so shaping uses whatever the
   render machine has. On wasm it enumerates *nothing* — a page must
   `registerFont` before the first draw or every label lays out as empty
   (`crates/hephaestus-wasm/CLAUDE.md` § "Fonts are the thing that surprises
   people"). The wasm clients bundle four Roboto faces (258 kB brotli) and
   `registerDefaultFonts()` registers them with the shaper *and* injects an
   `@font-face` so the browser draws the same face it measured.
5. **Themes travel in the document and invert cleanly.** The theme is
   palette-driven (`paper` / `ink` / `accent`); `Theme::dark()` is
   `Theme::default().invert()`. The wasm client exposes `setDark(bool)` /
   `colorScheme: 'light' | 'dark' | 'auto'`; natively it is
   `composition.update_theme(|t| *t = t.invert())` (or a paper/ink swap).
   Brand colors and fonts can be applied the same way after `read_document`.
6. **Two wasm clients exist, both versioned in lockstep with the crate**
   (crates.io `hephaestus` 0.4.1; npm `hephaestus-wasm` and
   `hephaestus-svg-wasm` both 0.4.1, the latter 2.98 MB unpacked — all
   confirmed 2026-09-17) and published to npm as `hephaestus-wasm` (canvas, WebGL2/WebGPU) and
   `hephaestus-svg-wasm` (no rasterizer; 2.4 MB raw / 800 kB brotli).
   The SVG client is the fit for a document page: no GPU requirement, no
   WebGL context budget (a docs page with twenty plots is fine), no device
   pixel ratio, reflow on `ResizeObserver`, light/dark follows
   `prefers-color-scheme`, picking is plain DOM. Its cost is one DOM element
   per mark — dense (100k-point) plots belong on a canvas. Public API:
   `init`, `registerFont`, `registerDefaultFonts`, `setGenericFamily`,
   `renderSvg(doc, w, h, idPrefix)` one-shot, and `PlotView.create(container,
   bytes, opts)` with `redraw` / `resize` / `setColorScheme` / `pick` / `free`.
7. **Format versioning is strict.** `documentFormatVersion()` "is compared
   for equality, not as a floor, so a document written at a different major
   never loads". A `.hep` written by a newer R package than the hephaestus Q2
   links will be refused with `DocumentError` — Q2 needs a clear diagnostic
   and a documented upgrade path, not a broken image.
8. **Size hints are advisory.** `read_hints` returns the writer's intended
   `size` (CSS px) and `dpi`; the reader decides the real size. There is a
   `ReadContext::builtin()` for the built-in geom/formatter set; custom geoms
   or named formatters in a document fail to load in both wasm clients (same
   limit for us).
9. **Pre-1.0.** "The API is expected to change"; `kurbo`/`peniko` types are
   in the public surface. Pin an exact minor (`=0.4.x` or `0.4` with a
   lockfile) and expect bumps to be deliberate work.

Q2 side:

10. **The pipeline already has the two precedents this needs.**
    `MermaidRenderTransform` (`crates/quarto-core/src/transforms/mermaid.rs`):
    a `Finalization`-phase `AstTransform`, HTML-family self-gated, listed in
    `Q2_PREVIEW_TRANSFORM_EXCLUDED` so the raw node reaches the React layer,
    where `MermaidCodeBlock.tsx` owns rendering for both `q2-preview` and
    `q2-slides` with an on-demand `import()` and a version-parity test on each
    side. And `ResourceCollectorTransform` / `LinkRewriteTransform`, which
    are the only things that touch `Image.target` late in the pipeline.
11. **Artifacts are how a transform emits a generated file.**
    `Artifact::from_bytes(bytes, "image/svg+xml").with_path("figure-html/x.svg")`
    (Page scope) flushes to `<out>/<stem>_files/figure-html/x.svg` on the
    native sink and to the VFS in wasm; `ResourceResolverContext::html_url_for`
    gives the page-relative URL to put in the AST (`artifact.rs`,
    `artifact_flush.rs`, `resource_resolver.rs`). `brand_fonts.rs` is a
    worked example of a producer that reads bytes through `SystemRuntime` and
    stores artifacts.
12. **Transforms reach the filesystem through an injected runtime**, not
    through `RenderContext`: `DateNormalizeTransform::new(runtime)` is the
    pattern (`build_transform_pipeline` already receives
    `Arc<dyn SystemRuntime>`). `SystemRuntime::file_read` works on both the
    native FS and the preview VFS.
13. **Engine figure files already reach the preview VFS.** bd-qbhp2cvv
    (closed) embeds knitr/jupyter supporting files in the engine capture and
    materializes them next to the document at splice time; the parent-side
    `assetWalker.ts` then reads every `Image.target` from the VFS and mints a
    blob URL (MIME by extension, `application/octet-stream` for unknown). So a
    `.hep` written by knitr into `<stem>_files/figure-html/` is reachable in
    preview *today* as bytes behind a blob URL — the React `Image` component
    just doesn't know what to do with it.
14. **A second on-demand wasm module is an established shape**: the
    preview runtime already loads `web-tree-sitter.wasm?url` beside the 28 MB
    main `wasm_quarto_hub_client_bg.wasm` (which is runtime-cached, not
    precached; the PWA precache cap is 16 MB per file — `vite.config.ts`).
15. **Q2 has no light/dark *image* convention yet.** The theme toggle ships
    light and dark sheets keyed by `data-mode` (`compile_theme_css.rs`), but
    nothing like Q1's `.light-content` / `.dark-content` image pairing exists,
    and `preview-renderer` has no color-scheme plumbing of its own.
16. **knitr's `fig-format` is passed straight through as `dev`**
    (`engine/knitr/resources/rmd/hooks.R` ~L801). Whether a `.hep` device
    exists is the R package's business; Q2 needs nothing there for a
    hand-written `![](plot.hep)`, and only a `dev`-name pass-through once the
    R side ships.

## Design options

### A. Finalization-phase `AstTransform`: `.hep` → SVG artifact, rewrite `Image.target` (recommended)

A `HephaestusRenderTransform` in `build_transform_pipeline`, phase
`Finalization`, HTML-family self-gated (`html`, `revealjs`), placed **after
`resource-collector`** and before `responsive-image`. For every
`Inline::Image` whose target ends in `.hep` (and is not an external URL):

1. resolve the target against the document (relative) or project root
   (leading `/`) per `claude-notes/designs/path-resolution-model.md`;
2. `runtime.file_read` the bytes; `read_document(&bytes,
   &ReadContext::builtin())`;
3. choose a size: explicit `width`/`height` attrs → else `hints.size` → else
   a default (e.g. 7in × 5in at 96 dpi, matching knitr's `fig-width` /
   `fig-height` defaults);
4. `SvgScene::with_config(size, 96.0, SvgConfig::new().background(hints.background).id_prefix(<unique per image>).pick_ids(false))`,
   `composition.render(...)`, `encode_svg`;
5. store `Artifact::from_bytes(svg, "image/svg+xml").with_path("figure-html/<stem>-<content-hash>.svg")`
   (Page scope) and rewrite `target.0` to `resolver.html_url_for(Page, …)`;
6. on any failure emit a diagnostic and leave the image untouched.

Why after `resource-collector`: the collector then still sees the `.hep` and
copies it beside the page, which costs a few KB and is exactly what option E
(progressive enhancement) needs later; and it never tries to copy the
generated SVG from a source tree where it does not exist. `link-rewrite` runs
earlier and leaves relative targets alone, so the artifact URL survives.

Excluded from the preview pipeline (`Q2_PREVIEW_TRANSFORM_EXCLUDED`), exactly
like mermaid: the raw `.hep` `Image` reaches React (option D).

Dependency placement: `hephaestus = { version = "0.4", default-features =
false, features = ["document-read", "svg", "png"] }` in quarto-core's
existing **native-only** dependency table, module `#[cfg(not(target_arch =
"wasm32"))]`. The wasm build of quarto-core never compiles parley/ICU; the
preview uses the npm client instead (D). Adding it unconditionally would grow
the 28 MB main bundle by roughly 2–3 MB raw for a code path preview never
runs.

### B. A `PipelineStage` that converts files on disk after engine execution

Scan `ExecuteResult::supporting_files` for `.hep`, write SVGs beside them,
then rewrite. Rejected: it only sees engine output (not hand-written
`![](plot.hep)`), it writes into the *source* tree (the bd-cfl67 class of bug
the output sink exists to prevent), and it still needs an AST rewrite.

### C. Push conversion into the engine (R writes SVG/PNG itself)

Then Q2 does nothing. Rejected as the *primary* path: it forfeits the point
of the format — one document, re-solved per output (HTML gets SVG, LaTeX gets
an embedded-font PDF, the browser reflows live) — and every plotting package
would have to know every Quarto output format.

### D. Preview / hub-client: React component over the npm `hephaestus-svg-wasm` client (recommended, with A)

In `ts-packages/preview-renderer`, an `Image` wrapper (same layering trick as
`MermaidCodeBlock` over `CodeBlock`): if `target.0` ends in `.hep`, render a
`HepImage` that fetches the blob URL the asset walker already minted,
`await import('hephaestus-svg-wasm')` (once, cached promise), registers the
bundled fonts once per page, and mounts `PlotView.create(container, bytes,
{ colorScheme, autoResize: true, picking: false })`; `free()` on unmount.
Serves `q2-preview` and `q2-slides` through the one registry.

Fonts in the browser: `registerDefaultFonts()` fetches Roboto from the
package's `fonts/` directory; the vite build must ship those four files as
assets (`new URL(..., import.meta.url)` or copy) — a packaging detail to
verify in the spike. Cross-origin sandboxed preview
(`q2-sandboxed-preview-separate-domain.md`) needs the wasm + fonts to be
served from the iframe's origin, like `web-tree-sitter.wasm` is.

Alternative rejected: link hephaestus into `wasm-quarto-hub-client` and run
transform A in the browser. It couples the R package's format version to the
main wasm bundle, adds ~2.4 MB raw to it, and throws away what the JS client
gives for free (reflow on resize, `setColorScheme`, picking).

### E. Progressive enhancement of rendered sites (future, optional)

Hephaestus's own first-paint story: the page ships the natively-rendered SVG
(from A) *and* the `.hep` (copied by `resource-collector`), and a small
after-body script mounts `PlotView` over the placeholder so the published
plot reflows on resize and follows the site's dark toggle. Costs ~800 kB
brotli of wasm per page load and requires JS; strictly opt-in
(`hephaestus: { live: true }` or similar), vendored like mermaid rather than
CDN-loaded. Not part of the first cut; listed so that A's ordering decision
(keep the `.hep` copy) is understood.

### PNG, and why it is not the first target

Two ways to get a PNG, neither free:

- **GPU** (`vello-hybrid`): needs a wgpu adapter → fails on headless CI and
  servers, output not byte-stable across GPUs. Could be offered as an opt-in
  (`fig-format: png` on a `.hep`) with a clear "no adapter" diagnostic, never
  the default.
- **CPU via SVG**: `.hep` → SVG (A) → PNG with `resvg`/`tiny-skia` (pure
  Rust, wasm-clean, deterministic; `resvg` supports `textLength`). Needs a
  spike to confirm the SVG the backend emits (`textLength`,
  `xml:space="preserve"`, clip paths, gradients) rasterizes faithfully, and
  needs fonts in `fontdb`. This is the path to recommend *if* a raster output
  is ever required (e.g. a Word/PPTX writer, or LaTeX engines that reject
  SVG — though PDF via hephaestus's own backend covers LaTeX better).

Nothing in HTML/revealjs/Typst needs a PNG.

### Where the future PDF formats plug in

The same transform, keyed on `ctx.format`: `latex` → hephaestus `pdf`
backend (`.pdf` artifact, fonts embedded); `typst` → SVG (Typst's `image()`
takes SVG natively; recent Typst also takes PDF). One document, one
transform, one artifact per target. This is the "Quarto 2 documents naturally
support all hephaestus features" property the user is after, and it falls out
of A with no new architecture.

## Decisions (settled 2026-09-17 with the user)

All five recommendations below were accepted as written:

1. SVG first; PNG only as a later opt-in via a CPU route (bd-br9bmysi).
2. Bundle the four Roboto faces; `resources/hephaestus/fonts/`.
3. New `image` diagnostics subsystem, codes `Q-18-*`.
4. Light/dark deferred to a follow-up (bd-myfwwmki), blocked on a Q2
   light/dark image-pair convention (bd-74sxnthr).
5. Brand fonts (bd-l6e3sd45): measure with the bundled fallback, name the
   brand family in the markup, warn; no network font fetching at render
   time. Keep it lean to see how the subsystem feels.

Child strands: bd-sxiv2tio (phase 2, preview client), bd-l6e3sd45
(phase 3, brand fonts), bd-myfwwmki (phase 4, light/dark), bd-74sxnthr
(image-pair convention prerequisite), bd-br9bmysi (PNG / PDF / typst).

### Found during phase 1

- **Linux builds would have needed `libfontconfig1-dev`.** hephaestus →
  parley → fontique enumerates system fonts through
  `yeslogic-fontconfig-sys`, which links libfontconfig at build time by
  default. quarto-core now enables fontique's `fontconfig-dlopen` feature
  on Linux (a target-specific direct dependency; features are additive
  across the graph), so the library is loaded at run time and a machine
  without it just gets an empty system collection — harmless, because
  plot text is shaped with the bundled faces.
- **First `.hep` render pays ~2 s once per process** for font-context
  initialization (system font enumeration). Documents without `.hep`
  images never pay it; the registration is lazy.
- **A missing `.hep` warns twice** — `Q-18-1` (plot not rendered) and
  the resource collector's `Q-5-6` (file not copied), like any missing
  image. Deliberate; documented on the `Q-18-1` page.
- The SVG root carries `font-family="sans-serif"`: the reader's browser
  resolves the generic, and hephaestus's `textLength` keeps every run at
  the width measured with Roboto. Phase 3 revisits the family name.
- **Brand colors were pulled into phase 1** (user request, 2026-09-17):
  the light brand's `background` / `foreground` / `primary` become the
  plot palette's `paper` / `ink` / `accent` (`BrandPalette` in the
  transform). Only hex values cross; anything else warns once per
  document (`Q-18-4`). The palette is part of the artifact hash. Phase 3
  (bd-l6e3sd45) stays typography-only. Example:
  `examples/plots/01-hephaestus-basic/brand.yml`.

## Decisions as originally proposed

1. **Target format: SVG first, not PNG.** Recommendation: yes (facts 1–2).
   PNG stays a later opt-in via the CPU route.
2. **Font determinism on the native path.** With system fonts, the same
   `.hep` produces different SVG bytes on macOS vs Linux (different advances →
   different `textLength`, different tick label widths → different layout),
   which breaks snapshot tests and makes rendered output machine-dependent.
   Recommendation: vendor the four Roboto faces hephaestus's wasm clients
   ship (~500 kB in `resources/hephaestus/fonts/`, OFL) and register them +
   map `sans-serif` to them in the transform, exactly as
   `examples/document_svg.rs` does. Output then matches what the preview
   client draws, byte for byte at the same size. Alternative: no bundled
   fonts, accept machine-dependence, and snapshot only structure.
3. **Ordering relative to `resource-collector`** (keep the `.hep` copy in
   the output, enabling E). Recommendation: after, as described in A.
4. **Diagnostics subsystem.** New `Q-*` codes are needed for: file not
   readable, not a hephaestus document / format-version mismatch, document
   uses an unsupported (custom) geom or formatter, SVG backend warnings.
   Existing subsystems don't obviously fit (`writer`? `include`?).
   Recommendation: a new `image` (or `figure`) subsystem; each code needs a
   docs page + sidebar entry in the same commit (lint rules).
5. **Sizing rules.** Recommendation: `width`/`height` image attrs (px) →
   `hints.size` → 672 × 480 (7in × 5in at 96 dpi). Emit `width`/`height` on
   the SVG root and let `img-fluid` scale it.
6. **Preview client packaging.** `hephaestus-svg-wasm` as an npm dependency
   of `preview-renderer` (vendored into the bundle via vite-plugin-wasm), not
   CDN-loaded — the sandboxed preview origin and offline use argue for
   vendoring. Needs the version-parity test between the Rust crate version
   and the npm package version (they are released in lockstep; a `.hep` that
   one reads the other must too).
7. **Dark mode (phase 4).** Native: emit two SVG artifacts (`Theme::invert`)
   and a light/dark image pairing the theme toggle understands — which
   means first porting Q1's `.light-content` / `.dark-content` convention
   (fact 15), a separate strand. Preview: `PlotView.setColorScheme` driven by
   the preview's theme `data-mode` (needs plumbing preview-renderer doesn't
   have) or `'auto'` as a first cut.
8. **Brand typography (phase 3).** Map `brand.typography.base.family` onto
   the document's generic `sans-serif` (and `monospace` if the theme uses
   one) after `read_document`. For `source: file` fonts we already have the
   bytes (`brand_fonts.rs`) → `register_font_bytes` + `set_generic_family`.
   For Google/Bunny fonts the render machine has no bytes; recommendation:
   *name* the family in the markup (browser draws it) but *measure* with the
   bundled fallback — `textLength` keeps layout stable. Do **not** enable
   hephaestus's `google-fonts` feature (synchronous network at render time).
9. **Does the R-side format exist yet?** No R package producing `.hep` is in
   the tree or referenced; the fixture for TDD comes from hephaestus's
   `document_save` example. Fine for phases 1–2; the knitr `dev` pass-through
   (fact 16) gets exercised only once the R device exists.

If 1–6 are accepted as recommended, phase 1 is small enough to implement in
this session's follow-up: one transform file, one fixture, four tests, one
cargo dependency, `Q2_PREVIEW_TRANSFORM_EXCLUDED` entry, error codes + docs
pages.

## Checklist

### Phase 0 — spike (done in this session)

- [x] Confirm renderer-free build (`document-read,svg`) compiles on q2's
      toolchain; write a fixture `.hep`; render it to SVG at two sizes.
      Fixture regeneration: `cargo run --example document_save --features
      document-write` in `external-sources/hephaestus` (10,553 bytes).
- [ ] Spike the preview client: `npm i hephaestus-svg-wasm` in a scratch
      vite app, mount `PlotView` from a blob URL, check font asset serving and
      cross-origin (sandboxed iframe) loading.

### Phase 1 — native `.hep` → SVG for HTML-family output (tests first)

- [x] Fixture: `crates/quarto-core/tests/fixtures/hephaestus/basic.hep`
      (copied from the spike) + a `.qmd` referencing it relatively and with a
      leading `/`.
- [x] Tests (integration, `tests/integration/hephaestus_render.rs`, 11 end-to-end + 4 unit):
      transform rewrites `target.0` to `<stem>_files/figure-html/basic-<hash>.svg`
      and stores an `image/svg+xml` artifact whose bytes start with `<svg`;
      `render_document_to_file` end-to-end writes the SVG and the HTML
      references it; missing file → diagnostic, image untouched;
      non-hephaestus bytes → diagnostic; external URL / `.png` untouched;
      `revealjs` target also converts; `q2-preview` target does not.
- [x] Add `hephaestus` to quarto-core's native-only dependency table
      (`default-features = false, features = ["document-read","svg","png"]`).
- [x] Vendor Roboto faces under `resources/hephaestus/fonts/` (decision 2)
      with a README (source, licence, regeneration).
- [x] `transforms/hephaestus.rs`: the transform per option A (plus a shared `transforms/image_walk.rs` walker, extracted from `responsive_image`); register in
      `build_transform_pipeline` after `resource-collector`; add to
      `Q2_PREVIEW_TRANSFORM_EXCLUDED`; phase-ordering test stays green.
- [x] Error codes `Q-18-1`..`Q-18-3` in `quarto-error-catalog` + `docs/errors/<subsystem>/`
      pages + sidebar entries (decision 4).
- [x] End-to-end (2026-09-17): a scratch `doc.qmd` with
      `![Sine and scatter](figs/plot.hep){#fig-sine}` and
      `![](figs/plot.hep){width=320 height=240}`, rendered with
      `cargo run --bin q2 -- render doc.qmd`. Output inspected:
      `doc_files/figure-html/plot-96a31b474adf.svg` (900×420, the hint) and
      `plot-bb79b6a77bca.svg` (320×240); the HTML carries
      `<img src="doc_files/figure-html/plot-96a31b474adf.svg" alt="Sine and scatter" class="img-fluid" />`
      and `<img src="doc_files/figure-html/plot-bb79b6a77bca.svg" alt="" width="320" height="240" />`;
      the SVG rasterized through QuickLook shows both panels, axes, ticks
      and titles correctly.
- [x] User docs: "Plot Documents" section in `docs/guides/authoring/figures.qmd`.

### Phase 2 — preview / hub-client (`q2-preview`, `q2-slides`)

- [ ] `preview-renderer`: `HepImage` component + `Image` wrapper registered
      in `registry.ts`; loader injection for tests (mirror
      `MermaidCodeBlock`'s `activeLoader`); vitest coverage.
- [ ] Bundle `hephaestus-svg-wasm` + its fonts; sandboxed-preview origin
      check; `npm run build:all` green.
- [ ] Version-parity test: Rust `hephaestus` crate version ==
      npm `hephaestus-svg-wasm` version.
- [ ] Browser verification in `q2 preview --no-browser` + Playwright (the
      fallback noted in memory) and in hub-client.
- [ ] hub-client `changelog.md` entry.

### Phase 3 — brand.yml typography

- [ ] Map brand `typography` families onto the document's generic families
      (decision 8), native and preview; tests with a `_brand.yml` fixture.

### Phase 4 — light / dark

- [ ] Port a light/dark image-pair convention (prerequisite strand).
- [ ] Native: two artifacts via `Theme::invert`; preview: `setColorScheme`.

### Phase 5 — PDF outputs (when `latex` / `typst` formats land)

- [ ] `latex` → `pdf` backend artifact; `typst` → SVG artifact; tests.

### Phase 6 (optional) — progressive enhancement of rendered sites (option E)

## Risks

- **Format-version lockstep** between the R package, the Q2 crate dep and
  the npm client (fact 7). Mitigation: exact pins, a parity test, and a
  diagnostic that names both versions.
- **Pre-1.0 API churn** (fact 9). Keep the surface we use tiny:
  `read_document`, `read_hints`, `SvgScene` / `SvgConfig` / `encode_svg`,
  `text::register_font_families` / `set_generic_family`, `Theme::invert`.
- **Build time**: parley + ICU + harfrust is a noticeable clean-build cost
  for quarto-core natively. Measure in phase 1 and report.
- **Dense plots in the SVG client** (one DOM node per mark). The native SVG
  has the same cost in the browser; a `.hep` with 100k points is a bad SVG
  regardless. Document the limit; a canvas client is a later option.
- **Documents with custom geoms / formatters** cannot be read by
  `ReadContext::builtin()`; surface as a diagnostic, not a panic.

## References

- `external-sources/hephaestus/CLAUDE.md`, `src/document/CLAUDE.md`,
  `src/backend/CLAUDE.md`, `src/backend/svg/CLAUDE.md`,
  `src/plot/theme/CLAUDE.md`, `crates/hephaestus-svg-wasm/CLAUDE.md`,
  `crates/hephaestus-wasm/CLAUDE.md`, `examples/document_svg.rs`,
  `examples/document_placeholder.rs`, `tests/document_svg.rs`.
- Q2: `crates/quarto-core/src/transforms/mermaid.rs`,
  `transforms/resource_collector.rs`, `transforms/link_rewrite.rs`,
  `artifact.rs`, `artifact_flush.rs`, `resource_resolver.rs`,
  `brand_fonts.rs`, `engine/capture_files.rs`,
  `ts-packages/preview-renderer/src/q2-preview/{assetWalker.ts,inlines/Image.tsx,blocks/MermaidCodeBlock.tsx,registry.ts}`,
  `claude-notes/designs/transform-pipeline-phases.md`,
  `claude-notes/designs/path-resolution-model.md`.
- Related strands: bd-qbhp2cvv (closed; engine files reach the preview VFS),
  bd-w59hlv0s (open; jupyter image outputs not served in preview — the
  path-ref case), bd-5m4ga0s1 (mermaid: the render/preview split precedent).
