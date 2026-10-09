---
title: 'RevealJS auto-stretch — captioned & cross-referenceable figures'
date: 2026-06-17  # (Case 1 implemented 2026-06-18)
description: 'Extends reveal auto-stretch to captioned figures so a lone captioned image becomes a direct child of the slide section and stretches, leaving cross-referenceable figure divs as a separate case.'
status: in-progress  # **Case 1 implemented** (markdown captioned figure hoist, with `<p class="caption">` via bd-itqcfxc3). Case 2 (crossref figures) deferred — Option 2A. Centering deferred (flush-left, consistent with the bare-image case).
braid:
  strand: bd-38ioql41  # follow-up to bd-zkstclhl
  priority: P2
  labels: [revealjs]
---

## Overview

bd-zkstclhl fixed auto-stretch for a **bare image** (`![](x)` → Pandoc
`Paragraph[Image]`): we unwrap the paragraph into a `Plain[Image]` so the
`.r-stretch` image is a *direct child* of `<section>` (reveal sizes only
`section > .r-stretch`). This strand extends that to **figures** — an image
**with a caption** — in two syntaxes:

1. **Markdown captioned figure** — `![This is a caption.](diagram.svg)`
2. **Cross-referenceable figure** — `::: {#fig-diagram} ![](diagram.svg)
   This is a caption. :::`

Example fixture: `examples/presentations/13-figure-stretch/`.

## Reproduction (2026-06-17)

`cargo run --bin q2 -- render examples/presentations/13-figure-stretch` — both
figures fail to stretch, but for **different reasons**:

### Case 1 — markdown `![caption](x)` → Pandoc `Figure`

```html
<section id="captioned-figure-markdown-syntax" class="section">
  <figure>
    <img src="diagram.svg" alt="This is a caption." class="r-stretch" />
    <figcaption>…</figcaption>
  </figure>
</section>
```

The image **does** get `.r-stretch` (the current transform's in-place figure
handling), but it sits at `section > figure > img` — not a direct child — so
reveal never sizes it.

### Case 2 — crossref `::: {#fig-diagram}` → `<figure id>` (rendered late)

```html
<section id="crossreferenceable-figure-div-syntax" class="section">
  <figure id="fig-diagram">
    <p><img src="diagram.svg" alt="" /></p>
    <figcaption>…</figcaption>
  </figure>
  <p>See <a href="#fig-diagram" class="quarto-xref">Figure 1</a>.</p>
</section>
```

The image gets **no `.r-stretch` at all** — at auto-stretch time this is still
a plain `Block::Div(#fig-diagram)` (the crossref→figure conversion happens
later), so the "standalone top-level block" gate skips the nested image.

## Q1 parity target (`quarto render`, v99.9.9)

Q1 stretches **both** (its end-of-pipeline DOM postprocessor `applyStretch`
sees both as final HTML):

```html
<!-- Case 1 -->
<img data-src="diagram.svg" class="r-stretch quarto-figure-center">
<p class="caption">This is a caption.</p>

<!-- Case 2 -->
<img data-src="diagram.svg" class="r-stretch quarto-figure-center" id="fig-diagram">
<p class="caption">…</p>
<p>See <a href="#/fig-diagram" class="quarto-xref">Figure&nbsp;1</a>.</p>
```

Q1's recipe (both cases): hoist the `<img>` to be a direct child of the slide,
**move the figure `id` onto the `<img>`** (so the xref anchor still resolves),
add `quarto-figure-center` (centering), and re-emit the caption as a sibling
`<p class="caption">`. The `<figure>` element is discarded.

## Why this is hard in Q2 (the core analysis)

Q2 has **no DOM postprocessor** (deliberate — see `CLAUDE.md` → Architecture
Notes). We must reach the same DOM via AST transforms. Two facts make the
figure case much harder than the bare-image case:

### Fact A — the two cases are at different pipeline stages

`RevealAutoStretchTransform` runs **early** (`pipeline.rs:1148`), *before*
`FloatRefTargetSugarTransform` (1166), `CrossrefIndexTransform` (1170), and
`CrossrefRenderTransform` (1259). Therefore at auto-stretch time:

| Syntax | AST node at auto-stretch time | After crossref render |
| ------ | ----------------------------- | --------------------- |
| Case 1 `![cap](x)` | `Block::Figure` (native Pandoc) | unchanged `Block::Figure` |
| Case 2 `::: {#fig-…}` | `Block::Div(#fig-…)` | `Block::Figure` (id + "Figure N:" caption) |

So **Case 1 is fixable early** (it's already a Figure). **Case 2 is not** —
flattening the div before crossref indexing would destroy `@fig-diagram`
numbering.

### Fact B — preview excludes `crossref-render`

`crossref-render` is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1374`):
the q2-preview pipeline keeps crossref targets as `FloatRefTarget` **CustomNodes**
(rendered by React components), and they **never become Pandoc `Figure`s**.

Consequence: a "hoist after `crossref-render`" transform would run in **render
but not preview** → preview-vs-render divergence for crossref-figure stretch
(exactly what the `/preview-parity` skill exists to prevent). There is **no
single late AST shape** common to both pipelines for a crossref figure (render:
`Figure`; preview: `FloatRefTarget` CustomNode).

## Design

### Case 1 (markdown captioned figure) — recommend implement now

It is a `Block::Figure` at auto-stretch time in **both** pipelines (crossref
never touches an id-less captioned figure). Unwrap it in the existing
`RevealAutoStretchTransform`, analogous to the Paragraph→Plain fix:

- Replace the `Block::Figure` with:
  1. `Plain[Image]` — the figure's lone image, with `.r-stretch` added (and the
     figure's `id`, if any, transferred onto the image); then
  2. a **caption block** carrying the figure caption inlines.
- Output: `section > img.r-stretch` followed by the caption → matches Q1
  (modulo `quarto-figure-center`, see Open Questions).

Open sub-questions for Case 1:
- **Caption representation.** Q1 emits `<p class="caption">`. Pandoc `Para` has
  no attributes; options are (a) a plain `Paragraph` (renders `<p>…</p>`, no
  `.caption` class — caption looks like body text), (b) a `Div.caption`
  (renders `<div class="caption"><p>…</p></div>`), or (c) emit a proper
  `<p class="caption">`. **Decided direction:** (c), via the new general
  capability **bd-itqcfxc3** (block-level attributes through a collected trailing
  `Inline::Attr`) — see `claude-notes/plans/2026-06-17-block-level-attrs-inline-attr.md`.
  That capability is its own implementation session; this figure work may ship
  first with a plain `<p>` (option a) and adopt `<p class="caption">` once
  bd-itqcfxc3 lands. Related, not a hard blocker.
- **Centering.** Q1 adds `quarto-figure-center` + ships centering CSS; Q2 has
  neither, so the stretched figure image would be flush-left (consistent with
  the bare-image case we already shipped). Match-Q1-centering is a separate
  decision (would need a Q2-side rule). See Open Questions.
- Does `FloatRefTargetSugarTransform` wrap an **id-less** captioned figure into
  a float target later? If so, unwrapping early changes that path — verify it
  only claims figures with crossref ids/labels (expected), so an id-less
  captioned figure is safe to flatten.

### Case 2 (cross-referenceable figure) — needs a decision before implementing

The crossref id + numbering must survive, and the fix must hold in **both**
render and preview. Options:

- **Option 2A — Defer (recommended for now).** Ship Case 1; leave crossref
  figures un-stretched on single-image slides as a documented divergence from
  Q1, tracked by its own strand. Rationale: it's the least-common case, and a
  correct fix is a real design effort entangled with crossref + preview. Lowest
  risk; keeps render≡preview (both currently don't stretch crossref figures, so
  no *new* divergence).
- **Option 2B — Reveal-aware crossref rendering (full parity, large).** Make
  the FloatRefTarget renderer reveal-aware so that, on a qualifying single-image
  slide, it emits the hoisted shape (img as direct section child, id on img,
  caption sibling) instead of a `<figure>`. Must be done in **both**
  `CrossrefRenderTransform` (render) **and** the React `FloatRefTarget`
  component (preview) to preserve parity, and needs the "is this a stretch
  slide" signal plumbed into crossref render. Couples three subsystems
  (reveal/auto-stretch/crossref) and touches hub-client. High cost.
- **Option 2C — Render-only late AST hoist (parity in render, gap in preview).**
  A reveal transform after `crossref-render` that hoists the now-`Figure`\'s img
  (move id, add `.r-stretch`, sibling caption). Matches Q1 in `q2 render`;
  preview keeps the un-stretched CustomNode. Violates preview-parity for this
  case — only acceptable if we explicitly accept it and `log`/document it.

Recommendation: **Option 2A now, revisit 2B** when a Q2 figure-alignment story
exists (the same milestone that would add `quarto-figure-center`). Capture 2B/2C
in a dedicated strand.

## Proposed scope for THIS strand (to confirm with user)

- [x] Implement **Case 1** (markdown captioned figure) — early AST unwrap.
      `RevealAutoStretchTransform`\'s Figure branch now replaces the `Block::Figure`
      with `Plain[Image]` (`.r-stretch`, figure `id` transferred onto the img)
      followed by `caption_paragraph(...)` — a `Paragraph` with a trailing
      `Inline::Attr{.caption}`. See `hoist_figure` / `figure_caption_inlines` /
      `caption_paragraph` in `auto_stretch.rs`.
- [x] Decide caption representation + centering. **Caption:** option (c) —
      `<p class="caption">` via the merged bd-itqcfxc3 capability (no longer a
      plain `<p>`). **Centering:** deferred — bare stretched img is flush-left,
      consistent with the shipped bare-image case; matches Q1 only when Q2 grows
      a figure-alignment story (then revisit `quarto-figure-center`).
- [x] **Case 2**: **Option 2A** (defer). Crossref figures stay un-stretched on
      single-image slides — render≡preview (neither stretches them), so no *new*
      divergence. Documented in the `auto_stretch.rs` module doc-comment and here.
      A dedicated strand for 2B/2C can be filed when a figure-alignment milestone
      arrives.
- [x] Example fixture `13-figure-stretch` (done — created).

## Implementation results (2026-06-18) — Case 1 PASS

**Tests (TDD):** added 3 unit tests in `auto_stretch.rs`
(`lone_figure_becomes_plain_image_plus_caption`, `figure_id_transferred_onto_image`,
`nostretch_figure_left_intact`) + 1 render-path test in `revealjs_format.rs`
(`revealjs_auto_stretch_figure_hoists_image_and_caption`). Confirmed the 3 new
behavior tests failed first (`<figure>`/`<figcaption>` retained), then passed
after the implementation. Full `quarto-core` + `pampa` suites green (6352
tests); `cargo xtask verify` all 14 steps green (incl. lint + WASM/hub-client).

**End-to-end (render path), inspected:**
```bash
cargo run --bin q2 -- render examples/presentations/13-figure-stretch/slides.qmd
```
Slide 1 (`![This is a caption.](diagram.svg)`) — the figure is hoisted exactly
like Q1:
```html
<section id="captioned-figure-markdown-syntax" class="section">
  <h2>Captioned figure (markdown syntax)</h2>
  <p>A figure written with the markdown image+caption syntax. …</p>
  <img src="diagram.svg" alt="This is a caption." class="r-stretch" />
  <p class="caption">This is a caption.</p>
</section>
```
Slide 3 (`{.nostretch}`) — left intact as `<figure>`, no `r-stretch` (opt-out
honored). Slide 2 (crossref `::: {#fig-diagram}`) — unchanged, no `r-stretch`,
`@fig-diagram` still resolves to "Figure 1" (Case 2 deferred as designed).

**Preview transport (feeds the React reveal renderer), verified:** the caption
`Paragraph[…, Attr{.caption}]` serializes through the JSON writer as a Para with
the safe `attr` key `["",["caption"],[]]` and round-trips to `<p class="caption">`
(the json.rs → React Para.tsx channel proven in #310). The structural hoist
(`Plain[Image]`) uses only standard nodes. **Not run in a live browser** this
session — the transport that feeds preview is verified, but a visual browser
check of the q2-preview reveal renderer was not performed.

## Open questions for the user

1. **Case 2 strategy:** 2A (defer, recommended), 2B (full parity, large,
   touches React), or 2C (render-only, preview gap)?
2. **Centering:** match Q1 (`quarto-figure-center` centering) for stretched
   figures now, or accept flush-left (consistent with the bare-image case) and
   handle alignment in a later figure-alignment effort?
3. **Caption class:** is `<p class="caption">` parity important, or is a plain
   `<p>` caption acceptable for the first cut?

## Test plan (TDD — once scope is locked)

- Unit (`auto_stretch.rs`): a single-image **Figure** slide → after the walk,
  the figure is replaced by `Plain[Image]` (with `.r-stretch`, id transferred)
  + a caption block; `.nostretch` figure is left intact; non-stretch gates
  (sized image, two images, aside) still skip.
- Render-path (`revealjs_format.rs`): Case 1 deck → assert
  `section > img.r-stretch` (no enclosing `<figure>`/`<p>` wrapper) and the
  caption text survives. If Case 2 is in scope, assert the img carries
  `id="fig-diagram"` and `@fig-diagram` still resolves to "Figure 1".
- Browser re-verify (Chrome): stretched figure image computed height > natural;
  compare geometry to Q1.

## Files / references

- `crates/quarto-core/src/revealjs/auto_stretch.rs` — the transform (figure
  branch currently adds `.r-stretch` in place; needs unwrap).
- `crates/quarto-core/src/pipeline.rs:1148` (auto-stretch), `:1166`
  (float sugar), `:1259` (crossref render), `:1374` (preview exclusion).
- `crates/quarto-core/src/transforms/crossref_render.rs` — FloatRefTarget →
  native `Figure`.
- `external-sources/quarto-cli/src/format/reveal/format-reveal.ts:949` —
  Q1 `applyStretch` (figure branch lines 1059-1095) for parity reference.
- Companion plan (Paragraph case, shipped):
  `claude-notes/plans/2026-06-17-revealjs-autostretch-p-wrapper.md`.
