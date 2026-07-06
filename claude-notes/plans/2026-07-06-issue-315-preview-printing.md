# Issue #315 — Printing / PDF export from the quarto-hub preview is broken

**Issue:** https://github.com/quarto-dev/q2/issues/315 — "print to pdf is
super broken in our react preview formats. We should audit this for the
formats we care about."

**Status:** planning (do not execute until the user gives the go-ahead)

**Braid strand:** bd-vhdknrvl

---

## 1. Overview / goals

Printing (and print-to-PDF) from the live preview in quarto-hub does not
produce a usable document. Two concrete symptoms reported by the user:

1. **Firefox** — right-click → "Print Frame" on the preview iframe produces a
   PDF with a **single page and a scrollbar**, instead of a paginated
   multi-page document, for long documents.
2. **Chrome** — there is **no affordance at all** to print just the frame's
   contents; Chrome does not offer "Print Frame" for these sandboxed iframes.

The goal: a user can reliably obtain a **properly paginated, correctly styled
printable/PDF version** of the previewed document, across browsers.

### Scope (resolved)

The "react preview formats" in scope are the ones on the **React/AST path**:

- **`format: q2-preview`** — the React preview for regular documents. Today
  authored explicitly as `format: q2-preview`; it will **eventually become how
  `format: html` renders in hub-client**, at which point `q2-preview` goes
  away. The printable form of such a document is therefore just "the same
  document rendered as `format: html`".
- **`format: revealjs`** — slides.

Out of primary scope: `format: dashboard` is coming later, will be structured
like `q2-preview`, and is **not a fundamental blocker** — the doc path below is
designed generically so a preview-pipeline format like dashboard can reuse it.
`q2-debug` / `q2-raw` are developer views, not meant to be printed. The legacy
`MorphIframe` (`format: html`) path is being subsumed by `q2-preview`; it can
share the same "open printable version" plumbing trivially (it already holds a
standalone HTML string) but is not the focus.

### Approach (resolved)

**Strict Option B — "Open printable version".** We do **not** loosen iframe
sandboxes (`allow-modals` was considered and rejected — see §3). Instead, a
toolbar affordance opens a **new top-level browser tab** containing a
**self-contained** document appropriate to the format. Because it is a
top-level document (not a sandboxed, clipped iframe), the browser paginates it
natively and `@media print` applies correctly. **On click we just open the
page** — the user invokes print with ⌘P themselves (no auto-print).

---

## 2. How preview rendering works today (root-cause map)

Two preview paths, selected by `getQ2Format`
(`hub-client/src/components/render/getQ2Format.ts`): `q2-*` / `revealjs` →
**React/AST path**; everything else → legacy **HTML/MorphIframe path**.

### 2a. React/AST path — `ReactPreview` → `Q2PreviewIframe` (in scope)

- Formats: `q2-preview`, `q2-slides`, `revealjs`, `q2-debug`, `q2-raw`
  (`hub-client/src/components/render/ReactRenderer.tsx:253-330`).
- The iframe loads a real page (`src="q2-preview.html"`) and receives the
  **AST as JSON over `postMessage`**; the DOM is **React-rendered inside the
  iframe** — **there is no standalone HTML document to print**
  (`ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx:435-447`).
- Theme CSS is a **blob-URL `<link>`** (`Q2PreviewIframe.tsx:422`).
- Sandbox: `allow-scripts allow-same-origin` (no `allow-modals`, no
  `allow-popups`).
- **Slides:** in the *preview* render `format: revealjs` is remapped to
  `q2-slides` (`map_format_for_preview`, `lib.rs:665-674`) and drawn as a
  **React reveal shell** — no standalone deck, and reveal's `print-pdf` mode is
  never entered.

### 2b. Legacy HTML path — `Preview.tsx` → `MorphIframe` (not the focus)

- `render_page_in_project_with_attribution` returns a complete `<!DOCTYPE
  html>` document string (`lib.rs:594`), injected via `iframe.srcdoc`
  (`MorphIframe.tsx:239-269`). Theme CSS is an external VFS `<link>` (not
  inlined). Sandbox `allow-same-origin allow-popups`.

### 2c. Why printing fails

- **No `window.print()` route.** Neither iframe has `allow-modals`, so scripts
  cannot open the print dialog; the only route is the browser "Print Frame"
  context item, which treats the frame as a clipped viewport (Firefox → one
  page + scrollbar; Chrome → doesn't offer it for sandboxed frames).
- **No standalone document on the React path** to hand to a top-level tab.
- **Slides need `?print-pdf`**, never triggered by the React shell.

### 2d. Print CSS that already exists

- **Always inlined** in every HTML-format render's `<head>` via
  `crates/pampa/resources/templates/html/styles.html:39`:
  ```css
  @media print {
    html { background-color: white; }
    body { background-color: transparent; color: black; font-size: 12pt; }
    p, h2, h3 { orphans: 3; widows: 3; }
    h2, h3, h4 { page-break-after: avoid; }
  }
  ```
- **Bootstrap theme** (when active), compiled into `styles.css`:
  `resources/scss/bootstrap/_bootstrap-rules.scss:408` (`@media print { .nav-page
  { display:none } }`) and `:2367` (page-columns / grid print layout).
- **Slides:** `resources/revealjs/reveal.css` has extensive `@media print`
  keyed off reveal's `print-pdf`. `resources/revealjs/reveal.js` supports the
  `?print-pdf` query.
- No dedicated `_print.scss`, no `self_contained` / `embed_resources` mode
  anywhere.

---

## 3. Why not Option A (`allow-modals`)

`allow-modals` re-enables the whole modal family for the frame — `print()` but
also `alert()`, `confirm()`, `prompt()`, and the `beforeunload` prompt. On the
React path (which has `allow-scripts` and renders **user-authored content**),
that lets a document pop blocking dialogs — a soft-DoS / annoyance vector in a
collaborative editor, and it still wouldn't fix the clipped-viewport pagination
problem or the slides `print-pdf` gap. **Rejected. Strict Option B.**

---

## 4. Design — "Open printable version"

One new UI affordance (a preview-toolbar button). On click it builds a
**self-contained, top-level** document for the current format and opens it in a
new tab (via a Blob URL). No auto-print.

### 4a. A single path-aware printable render (docs **and** slides)

The printable form of an in-scope document is **the same document rendered
through the HTML pipeline** (`render_qmd_to_html` → `ApplyTemplateStage` → full
`<!DOCTYPE html>` string): for docs that's a normal HTML page; for `revealjs`
it's the native `crates/quarto-core/src/revealjs/assemble.rs` standalone deck.
Two hard constraints, both confirmed in Phase 0:

- **Must be path-aware.** Relative image resolution is anchored on the
  document's real directory (JS side: `resolveRelativePath(currentFilePath, …)`,
  `vfsPaths.ts:29-40`; the parent-side walker reads bytes from the VFS at that
  resolved key). Path-less `render_qmd_content` (synthetic `/input.qmd`,
  anchored at `/`) silently breaks subdir / `../` images. So we render **with
  the real VFS path**.
- **Must force the HTML pipeline.** A doc authored `format: q2-preview` (or a
  future `dashboard`) dispatches on `pipeline_kind == "preview"` and returns
  AST, not HTML. `format: revealjs` already dispatches to the HTML/assemble
  path (Phase 0: confirmed `q2 render` on a revealjs deck yields a full
  `<div class="reveal"><div class="slides">` shell with reveal.js/.css
  included).

**Decision (resolves Q-impl-1): add a small path-aware WASM export
`render_printable(path)`** that:
1. reads the VFS file at `path`,
2. detects the format and **coerces preview formats to their HTML-output
   equivalent** — the inverse of `map_format_for_preview`: `q2-preview → html`,
   `q2-slides → revealjs` (and the future `dashboard → html`); non-preview
   formats (incl. `revealjs`, `html`) pass through unchanged,
3. renders via the existing `render_single_doc_to_response` HTML branch
   (`prefer_preview_format = false`) **with the real path**,
4. returns the standalone `RenderResponse.html` string.

This is the sound choice over a JS-only path-less swap (which is a workaround
that breaks images). It is a modest, purpose-built export; it reuses the entire
Rust render + native deck assembler (single-sourced — answers Q3), and needs a
WASM rebuild. The format coercion is a pure, well-defined inverse of an existing
mapping.

### 4b. Self-contained inlining (JS-side — resolved Q2)

The standalone HTML/deck references its assets two ways, and the inliner must
handle **both** (Phase 0 finding):

- **Generated artifacts** — theme `styles.css`, bootstrap JS, reveal.js/.css,
  fonts — as `/.quarto/project-artifacts/…` `<link href>` / `<script src>`.
  These resolve only inside the app runtime; a bare top-level tab must have
  them **inlined**: CSS `<link>` → `<style>`, JS `<script src>` → inline
  `<script>`, and any `url(/.quarto/…)` (fonts) inside that CSS → `data:` URIs.
- **User images** — left as the **original relative `src`** (e.g.
  `figures/plot.png`); never artifact-rewritten. Resolve against the doc's
  directory in the VFS and replace with a base64 `data:` URI.

Do this **JS-side** (Q2). The existing
`ts-packages/preview-renderer/src/utils/iframePostProcessor.ts:203-235`
**already** does the image half (relative `<img>` → `data:` URI) and reads
`/.quarto/…` artifacts via `vfsReadFile` — factor its helpers into a
string-in/string-out `makeSelfContainedHtml(html, currentFilePath)` that also
inlines the CSS/JS `<link>`/`<script>` refs the live post-processor leaves
alone. (`assetWalker.ts` is the AST-side analogue for the React preview; the
new module is the serialized-HTML analogue.)

### 4c. Opening + slides `?print-pdf`

Open the self-contained string in a new top-level tab via a Blob URL. For
slides, reveal.js enters PDF layout when `print-pdf` is in `location.search`
(Phase 0: `reveal.js` reads it, `Reveal.initialize` present). **Open question
(Q-impl-3):** a `blob:` URL's query string is not reliably exposed via
`location.search`, so appending `?print-pdf` to the Blob URL may not trigger
reveal's detection. Candidate fixes, decided in Phase 3: (i) inject a tiny
`<script>` into the deck HTML that sets reveal's print view / config directly
rather than relying on the query, or (ii) open via a same-origin URL that
carries the query. Prefer (i) — it keeps the artifact self-contained.

### 4d. Where this lives

A new preview-toolbar affordance, format-aware, wired at the
`PreviewRouter` / `ReactPreview` surface (it needs the current file path +
format). Label TBD (e.g. "Open printable version" / "Print…"). Behavior: open a
new tab; no auto-print.

---

## 5. Proposed work plan (TDD-first, per CLAUDE.md)

### Phase 0 — Reproduce & confirm assumptions (no code) — DONE
- [x] **`revealjs` → standalone deck via the HTML pipeline** — confirmed with
      `q2 render deck.qmd`: output has `<div class="reveal"><div class="slides">`
      + `reveal.js`/`reveal.css`/theme included. (Native proxy for the WASM
      `render_qmd_to_html` path.)
- [x] **`reveal.js` honors `?print-pdf` via `location.search`** — confirmed
      (`Reveal.initialize` present; `reveal.js` reads `print-pdf`). Surfaces
      Q-impl-3 (blob-URL query).
- [x] **User `<img src>` stays relative** (`figures/plot.png`) — never
      artifact-rewritten; must be inlined by resolving against the doc dir. →
      printable render must be **path-aware** (kills the JS-only path-less
      option; drives the `render_printable` decision in §4a).
- [x] **Fixtures are fully programmatic** — reuse the existing inline
      `PNG_BYTES` literal + `vfs_add_binary_file`; no on-disk image fixture
      exists or is needed. (Answers the user's fixture caveat.)
- [x] **Print CSS is thin** — the pandoc `@media print` partial is **not**
      inlined even with `theme: none`; a `@media print` block appears only in
      the compiled `styles.css` when a Bootstrap theme is active. → real Phase 5
      work.
- [ ] Repro both browser symptoms against a running hub (deferred; the code
      analysis already establishes the mechanism — do the visual repro
      alongside the Phase 2/3 E2E).

### Phase 1 — JS self-contained inliner (`makeSelfContainedHtml`) — DONE
- [x] **Test first (vitest, jsdom):** 9 cases covering CSS `<link>`→`<style>`,
      `<script src>`→inline `<script>`, relative `<img>`→`data:` URI, `/.quarto`
      `<img>`→`data:` URI, font `url()` inside inlined CSS→`data:` URI, external/
      `data:` refs untouched, VFS-miss left verbatim, DOCTYPE emitted.
      `ts-packages/preview-renderer/src/utils/makeSelfContainedHtml.integration.test.ts`.
- [x] Implemented `makeSelfContainedHtml(html, currentFilePath, readers)` at
      `ts-packages/preview-renderer/src/utils/makeSelfContainedHtml.ts` with
      injected `SelfContainedReaders` (decoupled from the WASM singleton;
      production binding to `vfsReadFile`/`vfsReadBinaryFile` deferred to the
      Phase 3 glue). Added web-font MIME types to `vfsPaths.guessMimeType`.
- [x] Full preview-renderer suite green (473 unit + 524 integration) + `tsc`
      clean.

### Phase 2 — `render_printable(path)` WASM export (path-aware, HTML-forced) — DONE
- [x] **Test first (quarto-core native integration, runs in nextest):**
      `crates/quarto-core/tests/integration/printable_render.rs` — a
      `format: q2-preview` fixture renders (via `render_qmd_to_html` with
      `Format::html()`, mirroring the coercion) to a full HTML doc with the
      relative image `src` preserved; a `format: revealjs` fixture renders to a
      standalone reveal deck (`class="reveal"`/`"slides"`). Both pass. (The
      WASM crate is `cdylib`-only / not native-testable, so the render-level
      contract lives here.)
- [x] Implemented: `coerce_format_for_print` (inverse of
      `map_format_for_preview`: `q2-preview→html`, `q2-slides→revealjs`,
      `q2-debug`/`q2-raw`→html, else passthrough) + a `format_override:
      Option<&str>` param threaded through `render_single_doc_to_response`
      (4 existing callers pass `None`) + the `render_printable(path)`
      `#[wasm_bindgen]` export. `wasmRenderer.ts` gets a `renderPrintable`
      wrapper + `WasmModuleExtended.render_printable` + `.d.ts` decl;
      auto-exported from `@quarto/preview-runtime`. `is_slides` unchanged — the
      JS caller already knows the format from `getQ2Format`.
- [x] WASM rebuilt (`npm run build:wasm`) — compiles for wasm32, passes
      wasm-bindgen, `render_printable` present in pkg bindings, no new
      warnings; `tsc` clean for preview-runtime.

### Phase 3 — Wire producer + slides `?print-pdf`, open in tab
- [ ] Resolve Q-impl-3 (reveal print mode without a working query string).
- [ ] Glue: `render_printable(path)` → `makeSelfContainedHtml` → Blob URL →
      `window.open`; for `is_slides`, force reveal print mode.
- [ ] **E2E:** open in a top-level tab; verify doc **paginates** (multi-page)
      and deck shows **paged slides**, both in Firefox + Chrome; screenshots
      here.

### Phase 4 — UI affordance
- [ ] **Test first (component):** the preview toolbar shows the control;
      clicking opens the correct producer for the current format.
- [ ] Implement the button + wiring at the `PreviewRouter`/`ReactPreview`
      surface; label per Q-impl-2.

### Phase 5 — Print-CSS audit & polish
- [ ] Ensure a usable `@media print` baseline reaches the printable doc
      (Phase 0 showed it's currently thin): hide app/preview chrome + any
      sidebar/TOC/nav, tune margins + page breaks. Decide where it lives
      (template partial vs. injected by the inliner).
- [ ] Verify Bootstrap-theme vs non-theme docs both print cleanly.

### Phase 6 — Verification & docs
- [ ] Full `cargo xtask verify` (WASM export is a Rust change) + hub-client
      `npm run build:all` + `test:ci`.
- [ ] Manual cross-browser E2E (Firefox + Chrome) with screenshots recorded
      here.
- [ ] Update `hub-client/changelog.md` (two-commit workflow per CLAUDE.md).

---

## 6. Resolved decisions

- **Option:** strict B ("Open printable version"); **no** `allow-modals` /
  Option A. (Q1)
- **Inlining seam:** JS-side. (Q2)
- **Slides deck:** reuse the Rust `assemble.rs` path via the existing
  `render_qmd` export — no new export, no JS reconstruction; single-sourced
  deck assembly minimizes maintenance surface. (Q3)
- **On click:** just open the page; user prints with ⌘P (no auto-print). (Q4)
- **Scope:** `q2-preview` (doc) + `revealjs` (slides) now; `dashboard` later
  via the generic doc path; `q2-debug`/`q2-raw` excluded; legacy
  `format: html`/MorphIframe can reuse the same plumbing but isn't the focus.
  (Q5)

## 7. Remaining implementation questions (not blockers)

- **Q-impl-1 (RESOLVED):** path-aware `render_printable(path)` WASM export (see
  §4a). The JS-only path-less swap was rejected — Phase 0 confirmed it breaks
  relative/subdir images.
- **Q-impl-2:** affordance label ("Open printable version" vs "Print…") and
  exact toolbar placement.
- **Q-impl-3:** how slides enter reveal print-pdf mode when opened from a
  `blob:` URL (query string likely not exposed via `location.search`). Prefer
  injecting a small config `<script>` into the self-contained deck. Decide in
  Phase 3.

---

## 8. Key files (anchors for implementation)

- `hub-client/src/components/render/ReactRenderer.tsx` /
  `ReactPreview.tsx` / `PreviewRouter.tsx` — React/slides path + toolbar surface.
- `ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx` — React iframe
  (sandbox, theme blob URL).
- `ts-packages/preview-renderer/src/q2-preview/assetWalker.ts` — asset-reference
  walker to adapt for inlining.
- `ts-packages/preview-runtime/src/wasmRenderer.ts` — WASM render wrappers;
  `render_qmd` at `:439`, `render_qmd_content` at `:543`; VFS CSS read at `:1191`.
- `crates/wasm-quarto-hub-client/src/lib.rs` — `render_qmd` (`:979`,
  `prefer_preview_format=false`), `render_qmd_content` (`:1018`),
  `render_single_doc_to_response` (`:1317`, format dispatch at `:1380`),
  `map_format_for_preview` (`:665`), `detect_format_from_content` (`:679`).
- `crates/quarto-core/src/pipeline.rs` — `render_qmd_to_html`, `ApplyTemplateStage`.
- `crates/quarto-core/src/revealjs/assemble.rs` — native standalone deck (reused).
- `crates/pampa/resources/templates/html/{main,styles}.html` — HTML template +
  inlined `@media print`.
- `resources/revealjs/reveal.{js,css}` — reveal `print-pdf` support.
