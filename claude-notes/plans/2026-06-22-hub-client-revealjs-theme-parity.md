# hub-client revealjs ≠ q2 render/preview — assessment & plan

**Strand:** bd-vwp4y5ku
**Date:** 2026-06-22
**Status:** assessment — awaiting go-ahead to implement

## Symptom

Recent Q2 revealjs work (the reveal.js 6 theming/authoring epic, esp.
`650cbddc` "apply the document's compiled reveal theme in q2 preview") made
`format: revealjs` decks render with Quarto's opinionated defaults —
non-uppercase headings, left-aligned content, the document's compiled theme.
These show up correctly in **both** `q2 render` and `q2 preview`.

They do **not** show up in **hub-client** (and therefore not on quarto-hub.com).
A bare deck:

```
---
format: revealjs
---

## A slide
* A thing
```

renders in the hub-client live editor with **uppercase titles** and
**centered content** — reveal.js's stock `white.css` look.

### Live confirmation (2026-06-22, localhost:5173)

Inspected the live editor deck via Chrome DevTools on the running hub-client:

- `.revealjs-container` **is present** → the active renderer is the hand-rolled
  `RevealjsSlideAst`, *not* the shared preview iframe.
- `<h2>` "A slide" → computed `text-transform: uppercase`.
- `.reveal .slides` → computed `text-align: center`.
- 7 stylesheet rules in the DOM carry `text-transform: uppercase`.

Both symptoms trace directly to the stock `white.css` that the hand-rolled
component imports.

## Root cause

hub-client has **two** revealjs surfaces, and the editor picks the wrong one:

1. **Shared, render-accurate path** — `@quarto/preview-renderer`
   `PreviewRoot` (`ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx`),
   reached through `Q2PreviewIframe` when the format is `q2-preview`. This is
   the **same code `q2 preview` embeds and runs**. It already treats
   `revealjs`/`q2-slides` as slides (`isSlides`, PreviewRoot.tsx:1396), runs the
   full WASM q2-preview pipeline (shortcodes, Lua, transforms, **`CompileThemeCssStage`**),
   and applies the document's compiled reveal theme via the existing
   `css:theme:<fp>` → styles.css transport. This is the path `650cbddc` fixed.

2. **Hand-rolled React deck** —
   `hub-client/src/components/render/RevealjsReactAstSlideRenderer.tsx`
   (`RevealjsSlideAst`). A **parallel TypeScript/React reimplementation** of the
   revealjs writer: it re-parses the Pandoc AST in JS (`parseSlides` /
   `renderBlock` from `ReactAstSlideRenderer.tsx`), renders into
   `@revealjs/react` `<Deck>`/`<Slide>`, **hardcodes** `white.css`
   (line 11) and the reveal config object (lines 125-164), hardcodes inline
   `<h1>`/author styling on title slides (lines 43-60), and **never receives a
   `themeFingerprint`** — so it cannot apply the per-document compiled theme.

The routing that sends bare revealjs to surface #2:

- `getQ2Format.ts` returns `'revealjs'` for `format: revealjs`.
- `PreviewRouter.tsx` → `ReactPreview` (because the format is non-null).
- `ReactRenderer.tsx:305-319`: `format === 'q2-preview'` → `Q2PreviewIframe`
  (surface #1); **everything else revealjs** → `RevealjsSlideAst` (surface #2).

So a plain `format: revealjs` deck in the editor *always* lands on the
hand-rolled deck and never touches the shared, theme-aware pipeline.

### Why this is the answer to "why don't they share code paths?"

`q2 render` and `q2 preview` both consume the **Rust writer / Rust pipeline**
as the single source of truth (preview via the WASM build of the same crates).
hub-client's *live editor*, however, predates that convergence: for revealjs it
runs an **independent JS reimplementation** of slide assembly and styling. The
shared path exists and is wired up (`Q2PreviewIframe`), but the editor only
routes the `q2-preview` pseudo-format to it — not `revealjs`. The divergence is
not a stale-WASM/build artifact problem; it is two genuinely different
renderers, and the editor chose the legacy one for this format.

## Evidence (file:line)

Native / shared (correct):
- `crates/quarto-core/src/revealjs/assemble.rs` — `reveal_config_json`
  (\~188-289) builds the reveal init from merged metadata; `register_reveal_assets`
  emits theme links in cascade order.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:275-343` — compiles
  the reveal theme, gated on `is_revealjs_target(...)` (true for both `revealjs`
  and the `q2-slides` preview pseudo-format, post-`650cbddc`).
- `crates/quarto-core/src/pipeline.rs` `build_q2_preview_pipeline_stages` —
  preview pipeline includes `CompileThemeCssStage` + `AstTransformsStage`.
- `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx:1396` — shared
  renderer already handles `revealjs`/`q2-slides` as slides.

hub-client (divergent):
- `hub-client/src/components/render/RevealjsReactAstSlideRenderer.tsx:11` —
  hardcoded `white.css`; `:125-164` hardcoded reveal config; `:43-60` hardcoded
  title styles; component signature has **no** `themeFingerprint`.
- `hub-client/src/components/render/ReactRenderer.tsx:305-319` — routes
  non-`q2-preview` revealjs to `RevealjsSlideAst` without `themeFingerprint`.
- `hub-client/src/components/render/getQ2Format.ts:14` &
  `PreviewRouter.tsx` — format classification that keeps `revealjs` as `revealjs`.

## Fix options

### Option A — Converge the editor onto the shared preview path (recommended)

Route `format: revealjs` in the hub-client editor through the same shared
`@quarto/preview-renderer` path that `q2 preview` uses (the `q2-preview` /
`q2-slides` flow → `Q2PreviewIframe` → `PreviewRoot` `isSlides`), and **retire
`RevealjsSlideAst`** (and the reveal-specific parts of `ReactAstSlideRenderer`).

- **Pro:** single source of truth. Every present and future Rust-side reveal
  change (theme, defaults, transforms, crossrefs, auto-stretch) appears in
  hub-client automatically. Eliminates the divergence class, not just this
  instance. Matches the project's stated "no parallel reimplementation" stance
  and the `q2 render`/`q2 preview` architecture.
- **Con / open questions:** must verify the iframe path covers the editor
  affordances the hand-rolled deck provides today:
  - controlled slide navigation (`currentSlide` / `onSlideChange`) synced with
    the editor and the slide thumbnail rail (`Editor.tsx`, `enabled:
    currentFormat === 'q2-slides'`),
  - slide thumbnail generation,
  - click-to-edit / nested editing (the iframe path *has* `usePreviewEdit`),
  - presence / attribution overlays (the iframe path *has* `useAttribution`).
  Editing + attribution already exist on the shared path; **slide navigation and
  thumbnails are the items to confirm** before committing to A.

### Option B — Patch the hand-rolled deck (tactical stopgap)

Thread `themeFingerprint` into `RevealjsSlideAst`, drop the static `white.css`
import, inject the compiled theme CSS via the same `css:theme:<fp>` → styles.css
transport `Q2PreviewIframe` uses, and drive the reveal config from document
metadata instead of the hardcoded object.

- **Pro:** small, localized; fixes the visible symptom quickly.
- **Con:** perpetuates the parallel renderer — a **divergence treadmill** where
  every future Rust reveal change must be hand-mirrored in TS (this incident is
  exactly that treadmill failing). Does not address the architectural root cause
  and conflicts with the single-source-of-truth principle.

**Recommendation:** pursue **A**. Use **B** only as a short-lived stopgap if A's
affordance gaps (slide nav / thumbnails on the iframe path) turn out to be large
and we need the visual fix shipped sooner.

## Open questions for the user

1. **A vs B.** Converge on the shared iframe path (A, recommended) or patch the
   hand-rolled deck (B)? If A, are we OK potentially deferring/reworking slide
   thumbnails and in-editor slide navigation to match the iframe path?
2. Is `RevealjsSlideAst` (and the reveal branch of `ReactAstSlideRenderer`)
   load-bearing for anything beyond what `Q2PreviewIframe` already does? (I will
   audit, but a steer helps.)

## Phase 0 findings (audit complete, 2026-06-22)

**Decision:** Option A (converge), confirmed by user. Work on branch
`braid/bd-vwp4y5ku-hub-client-revealjs-theme-parity`.

### The two WASM render entries (the real fork)

- `render_page_in_project[_with_attribution]` (`crates/wasm-quarto-hub-client/src/lib.rs:1044/1101`)
  — `prefer_preview_format = false`. **hub-client uses this.** No format
  substitution; `format: revealjs` stays `revealjs` (a real format,
  `pipeline_kind = None`).
- `render_page_for_preview` (`lib.rs:1188`) — `prefer_preview_format = true`.
  **The `q2 preview` CLI / q2-preview-spa use this**
  (`q2-preview-spa/src/PreviewApp.tsx:1026` via
  `ts-packages/preview-runtime/src/wasmRenderer.ts:521`
  `renderPageForPreview(path, userGrammars, captureGzJson)`).
  Runs `map_format_for_preview` (`lib.rs:662`) which maps `revealjs` →
  `q2-slides`. `q2-slides` has base `html` + `pipeline_kind = Some("preview")`
  and `is_revealjs_target("q2-slides")` is true, so the render goes through
  `render_qmd_to_preview_ast` (full transform pipeline incl.
  `CompileThemeCssStage` → compiled reveal theme) and returns
  `RenderResponse { is_slides: true, theme_fingerprint, ast_json, … }`
  (`lib.rs:1377-1452`).

So the WASM already produces a correctly-themed slides AST — the editor just
never calls the entry that asks for it.

### JS-side fork

- `getQ2Format.ts` → `'revealjs'`; `PreviewRouter` → `ReactPreview`.
- `ReactPreview.doRender` (`hub-client/.../ReactPreview.tsx:189`) dispatches on
  `pipelineKindForFormat(format)`: `'preview'` (q2-preview only) →
  `renderPageInProjectWithAttribution`; **everything else (incl. revealjs) →
  `parseQmdToAst`** (parse-only, no transforms, no theme).
- `ReactRenderer.tsx:305-319`: `q2-preview` → `Q2PreviewIframe`; **revealjs →
  `RevealjsSlideAst`** (hand-rolled, hardcoded `white.css`).

### The iframe path is already correct

`ts-packages/preview-renderer/src/q2-preview/RevealDeck.tsx` was fixed by
bd-y259zb57 (commit `650cbddc`): it does **not** statically import `white.css`
(lines 30-40), imports only theme-independent `reset.css`/`reveal.css`/
`quarto-reveal.css`, sets `center: false` (line 278), and applies the
per-document compiled theme via the `<style data-q2-theme>` / `css:theme:<fp>`
transport. `PreviewRoot.tsx:1396` already treats `revealjs`/`q2-slides` as
`isSlides`. Routing the editor's revealjs here resolves the theme parity with no
change to the shared renderer's theme handling.

### Affordance gap matrix (hand-rolled deck → iframe path)

| Affordance | Hand-rolled (today, revealjs) | Iframe path | Action |
|---|---|---|---|
| Compiled theme | ✗ (white.css) | ✓ | **fixed by convergence** |
| Full transform pipeline | ✗ (parse-only) | ✓ | **fixed by convergence** |
| Cursor→slide nav (`currentSlide`/`onSlideChange`) | ✓ | ✗ (no editor sync) | **must port** (postMessage) — current feature, do not regress |
| Reveal menu plugin | ✓ | ✗ | port (small) — follow-up |
| Slide thumbnails | ✗ (gated on `q2-slides`, off for revealjs) | ✗ | not a regression — follow-up strand |
| Click-to-edit | ✗ (read-only) | ✓ (disabled) | gain — follow-up strand |
| Attribution overlay | ✗ | ✓ (needs identities threading) | gain — follow-up strand |

## Design (Option A)

Point the editor's revealjs at the same entry + surface the `q2 preview` SPA
uses — the canonical preview path — and retire the hand-rolled deck.

1. **`ReactPreview.doRender`** — for revealjs, call
   `renderPageForPreview(documentPath, userGrammars, undefined)` (returns themed
   `ast_json` + `theme_fingerprint` + `is_slides`). Requires `documentPath`
   (editor always has it). `captureGzJson` is `undefined` in the live editor
   (no engine-capture replay). This is the same call `q2-preview-spa` makes.
2. **`ReactRenderer`** — route revealjs (and `q2-slides`) to `Q2PreviewIframe`
   with `themeFingerprint`, not `RevealjsSlideAst`.
3. **Slide-nav port** — add `currentSlide`/`onSlideChange` to the iframe path
   (`Q2PreviewIframe` ↔ `entry.tsx` postMessage ↔ `PreviewRoot`/`RevealDeck`) so
   cursor→slide sync survives the move. (Current feature; not a follow-up.)
4. **Retire** `RevealjsSlideAst` + the reveal branch of `ReactAstSlideRenderer`
   and the hub-client `white.css` import once unreferenced (the `q2-debug`
   surface keeps its own import; out of scope).

`pipelineKindForFormat` is documented as the JS mirror of Rust
`Format::pipeline_kind`; rather than overload it (revealjs's *real* format is
`None`; only the preview *substitution* makes it `preview`), `doRender` branches
on the slides format explicitly. **No Rust change required** for the core fix.

Follow-up strands to file: reveal menu in iframe; slide thumbnails for revealjs;
click-to-edit on slides; attribution on slides.

## Plan (TDD)

### Phase 0 — Decision & affordance audit
- [x] Confirm A vs B with the user. → **A**
- [x] Audit `Q2PreviewIframe`/`PreviewRoot` vs `RevealjsSlideAst` (matrix above).

### Phase 1 — Tests first
- [x] Routing test: `ReactRenderer` with `format: revealjs` renders
      `Q2PreviewIframe`, **not** `RevealjsSlideAst`. Added to
      `ReactRenderer.integration.test.tsx`; confirmed red, then green.
- [\~] `doRender` dispatch: `doRender` is module-private with no existing test
      harness; covered instead by the routing test + the end-to-end browser
      verification below (calling `renderPageForPreview` is what makes the iframe
      receive a themed AST — observable as non-uppercase `<h2>`). A unit test
      would require extracting/exporting the dispatch; deferred as low-value.
- [x] Theme parity asserted **end-to-end in the browser** (computed styles), the
      authoritative check per CLAUDE.md — see Phase 3 record below.

### Phase 2 — Implement (A)
- [x] `ReactPreview.doRender`: revealjs → `renderPageForPreview(documentPath,
      userGrammars, undefined)`; threads `theme_fingerprint` through (commit
      `70f5cb4c`).
- [x] `ReactRenderer`: route revealjs (+ q2-preview) → `Q2PreviewIframe` with
      `themeFingerprint` (commit `70f5cb4c`).
- [x] **Port cursor→slide sync** (bd-mwbsdmel) — DONE (commit `5b45e8be`).
      Imperative two-way channel: `RevealNavSync` (useReveal escape-hatch) ↔
      `SET_SLIDE`/`SLIDE_CHANGED` postMessage, deduped/echo-guarded. Browser-
      verified inbound (SET_SLIDE moves `.present`); both directions unit-tested.
- [x] Removed `RevealjsSlideAst` + its `white.css` import + the dead reveal
      branch in `ReactRenderer` (commit `5b45e8be`). Generic `q2-slides` keeps
      `SlideAst`.
- [→] Reveal menu / thumbnails / click-to-edit / attribution on the shared
      path → **bd-ktuojk26** (P2 task, discovered-from this). Out of scope here.
- [x] Filed follow-up strands (bd-mwbsdmel [done], bd-ktuojk26 [open]).

### Phase 3 — Verify end-to-end
- [x] Browser check (share-link project): `<h2>` `text-transform: none`, slides
      `text-align: left`, top-aligned section, hand-rolled deck gone, rendered in
      the shared iframe. Screenshot captured.
- [x] No Rust change — pure TS (hub-client bundles ts-packages from source). WASM
      untouched.
- [x] `tsc -b` clean; hub-client 614 unit + 74 integration; preview-renderer 453
      unit + 484 integration; hub-client `vite build` (production) clean;
      preview-renderer `tsc` (dist) clean.
- [x] Slide-nav verified in-browser (SET_SLIDE moves the deck) + unit-tested.
- [ ] `npm run test:wasm` not run (no Rust/WASM change). `cargo xtask verify`
      likewise unneeded for the core fix; run before final push if desired.
- [ ] Optional: side-by-side visual cross-check vs `q2 preview` of the same deck.

## References
- `650cbddc` fix(revealjs): apply the document's compiled reveal theme in q2
  preview (Level 2) — the sibling fix for `q2 preview`; this strand is the
  hub-client editor counterpart.
- `claude-notes/plans/2026-06-16-revealjs-render-preview-theme-parity.md`
- `claude-notes/designs/transform-pipeline-phases.md`
- `.claude/skills` → `preview-render-parity`
