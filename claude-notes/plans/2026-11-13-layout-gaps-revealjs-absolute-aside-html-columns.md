---
title: 'Layout gaps, first slices: RevealJS `.absolute`, RevealJS aside placement, HTML `.columns` widths'
date: 2026-11-13
description: 'Ports three small Quarto 1 layout behaviours that q2 renders in the wrong place: absolutely positioned elements and bottom-of-slide asides in RevealJS, and side-by-side column widths in HTML.'
status: draft  # implemented locally on andrew/layout-gaps-abc; no strand yet; decisions D1 to D8 need sign-off
---

**Strand:** none yet. This plan was written before the work was agreed with the team, so no strand was filed. **Related strands:** bd-bea550b0 (RevealJS authoring features, open), bd-34rd2y86 (RevealJS columns, closed), bd-0zosmiq8 (RevealJS asides and footnotes, closed), bd-y5j0m776 (RevealJS tabsets, open), bd-9fz5fweg (layout-panel CSS, open), bd-hcp8m3ve (float and layout class taxonomy, closed).

**Process:** `claude-notes/instructions/q1-feature-porting.md`. This is the Phase B document for three slices of a larger piece of work. The larger piece, "layout: columns, tabsets, margins", also needs RevealJS tabsets, figure panels (`layout-ncol`), HTML margin and wide content, and sidebar panels. None of those is in this plan.

## Overview

Three things a Quarto 1 author can write today come out in the wrong place in q2. In each case the content is present in the output. Only its position is wrong.

- **Slice A.** In RevealJS, `{.absolute top=200 left=400}` is ignored. The element stays in the normal flow.
- **Slice B.** In RevealJS, a `::: {.aside}` lands on top of the body text, not at the bottom of the slide.
- **Slice C.** In HTML, `::: {.column width="40%"}` is ignored. The columns are each half the body width and stack one above the other.

Each slice is one commit on the branch and can be split into its own pull request.

## The Quarto 1 contract, condensed

Paths are in `external-sources/quarto-cli/` unless they start with `quarto-web/`.

### Absolute positioning (RevealJS)

- Documented in `quarto-web/docs/presentations/revealjs/advanced.qmd:148-169`. An element with the class `absolute` is taken out of the flow and placed by `top`, `left`, `bottom`, `right`, sized by `width`, `height`. Values are CSS lengths. "If a number with no units is specified then pixels are assumed."
- Implemented in `src/resources/filters/quarto-post/reveal.lua:27-54` (`applyPosition`), registered for `Div`, `Span` and `Image` at lines 17 to 19, RevealJS only.
  - The six attributes are removed and appended to `style` in the fixed order `top`, `left`, `bottom`, `right`, `width`, `height`, each as `name: value; `.
  - A value that Lua `tonumber` accepts gets `px`. Anything else passes through.
  - An existing `style` is kept and the new declarations follow it.
- CSS: `src/resources/formats/revealjs/styles.html:27-30`, `.reveal .slide .absolute { position: absolute; display: block; }`.
- Auto-stretch leaves an `.absolute` image alone (`format-reveal.ts`, `applyStretch`). q2 already does this in `revealjs/auto_stretch.rs`.

### Asides (RevealJS)

- Documented in `quarto-web/docs/presentations/revealjs/index.qmd:91`: asides "are displayed in a smaller, lighter font at the bottom of the slide".
- All asides and footnotes on a slide are merged into one `<aside>` that is the last child of the slide (`format-reveal.ts`, `coalesceAsides`). q2 already does this in `revealjs/footnotes.rs`.
- CSS, `src/resources/formats/revealjs/quarto.scss`:
  - `:402-408`: `.reveal .slide aside { position: absolute; bottom: 20px; ... }`.
  - `:414-418`: on a `.scrollable` slide the aside is `position: relative; margin-top: 1em`.
  - `:264-267`: in a centered deck (`.reveal.center`) the aside is `position: initial`.
- The slide is the full height of the deck, which is what makes `bottom: 20px` the slide bottom: `src/resources/formats/revealjs/styles.html:13-16`, `.reveal .slide:not(.center) { height: 100%; }`. The same file has `.reveal .slide.scrollable { overflow-y: auto; }` at lines 18 to 20.
- For `center: true`, every slide gets the class `center` (`format-reveal.ts:659-687`, `handleCenteredSlides`), so the full-height rule does not apply to it and Reveal can center it.

### Columns (HTML)

- This is Pandoc behaviour that Quarto 1 inherits. Pandoc\'s HTML writer turns `width` on a `div.column` into `style="width:40%;"` and writes the children of a `div.columns` with no newline between them (Pandoc issue 4028).
- CSS: `src/resources/formats/html/_quarto-rules.scss:649-662`. `div.columns { display: initial; gap: initial; }` and `div.column { display: inline-block; overflow-x: initial; vertical-align: top; width: 50%; }`. The comment there says Quarto restored Pandoc\'s older `inline-block` layout on purpose.
- Because the columns are `inline-block`, whitespace between them takes room. Pandoc writes none. Quarto 1 also strips it for RevealJS (`format-reveal.ts:871-889`, `handleInterColumnDivSpaces`).

## Gap table

Measured on 2026-11-13 with q2 0.45.0 at `936c29e13` and Quarto 1.9.38, in a 1400 by 900 viewport. RevealJS numbers are in slide units: position relative to the slide `<section>`, divided by the deck scale.

| Contract item | q2 before | Gap | Fix |
|---|---|---|---|
| A1. `.absolute` position attributes become a style | ❌ | Written as `data-top`, `data-left`. Image at x=0, y=101, static. Quarto 1: x=400, y=212, absolute. | New `RevealAbsoluteTransform`. |
| A2. `.absolute` elements are `position: absolute` | ❌ | No CSS rule. | Rule in `quarto-reveal.css`. |
| A3. Bare numbers get `px`, units pass through | ❌ | Not applicable until A1. | Same transform. |
| A4. Auto-stretch skips `.absolute` images | ✅ | None. | Keep, and test that it still holds. |
| B1. All asides merge into one bottom `<aside>` | ✅ | None. | None. |
| B2. The aside sits at the slide bottom | ❌ | `<aside>` at y=78 on a 153 tall section. Quarto 1: y=620 on a 700 tall section. | Full-height rule for leaf slides. |
| B3. `.scrollable` slide: aside flows after the content | ❌ | q2 has no `.scrollable` rules at all. | Add the `overflow-y` rule and the aside override. |
| B4. Centered deck: aside is not pinned | ❌ | No rule. | Add the override. |
| C1. `width` on `div.column` becomes an inline width | ❌ | Written as a bare `width="40%"` attribute, which CSS ignores. | New `HtmlColumnsTransform`. |
| C2. Columns sit side by side at the given widths | ❌ | Both 400 wide, stacked. Quarto 1: 320 and 479, side by side. | C1, plus a layout that tolerates the newline q2 writes between the column divs. |
| C3. RevealJS columns keep their current behaviour | ✅ | None in this plan (but see D8). | Test that it is unchanged. |

## Design decisions

Tags: \[decided\] means it follows an existing q2 rule or a direct instruction. \[needs sign-off\] means the q2 team has not seen it.

- **D1. Each rewrite is an AST transform in `quarto-core`.** \[decided\] for slice A: `AGENTS.md` says to port Quarto 1 post-processing as AST transforms, and `RevealColumnsTransform` is the model. \[needs sign-off\] for slice C: the alternative is to do the `width` rewrite in the pampa HTML writer, which is what Pandoc does and would fix every caller of the writer. The transform was chosen so the change is small and easy to move. It also makes the style visible to `q2 preview`, which renders from the AST.
- **D2. Both transforms run in the Normalization phase.** \[decided\]. Neither reads a float, a caption or a resolved reference, so the Finalization rule in `claude-notes/designs/transform-pipeline-phases.md` does not apply. `RevealAbsoluteTransform` is pushed next to `RevealColumnsTransform`. `HtmlColumnsTransform` is pushed for the `HtmlRender` and `HtmlPreview` profiles only.
- **D3. Selectors follow q2\'s markup, not Quarto 1\'s.** \[needs sign-off\]. Quarto 1 selectors use `.slide`. q2 slide sections carry `section`, and so do the wrappers of vertical stacks, so the new rules use `.reveal .slides section` and Reveal\'s own run-time `stack` class to pick out leaf slides. The alternative is to add `slide` to q2 leaf sections once, so Quarto 1 CSS and user themes match unchanged. That is a wider change and is left for the team.
- **D4. A centered deck is recognised by `.reveal.center`.** \[needs sign-off\]. Quarto 1 marks every slide with `center`. q2 does not. It passes `center: true` to Reveal, which sets the class on the `.reveal` wrapper. The full-height rule is therefore `.reveal:not(.center) .slides section:not(.stack):not(.center)`. The alternative is to add `center` to each slide in `RevealSlidesTransform` as Quarto 1 does.
- **D5. `.scrollable` slides get their `overflow-y` rule in this slice.** \[needs sign-off\]. It is one line from the same Quarto 1 block as the full-height rule, and the aside override for scrollable slides means nothing without it. The deck-wide `scrollable: true` option is not part of this plan.
- **D6. HTML columns are laid out as a wrapping flex row.** \[needs sign-off\]. q2 writes a newline between sibling column divs. With Quarto 1\'s `inline-block` rule, 40% plus 60% plus that whitespace does not fit on one line, so the second column wraps even with the right widths. Two ways to fix it:
  - (chosen) `div.columns { display: flex; flex-wrap: wrap; }` in `_bootstrap-rules.scss`, with `min-width: 0` on the columns. Column widths, the 50% default and wrapping when the widths add up to more than 100% all behave as they do with `inline-block`. No writer change.
  - (alternative) The pampa HTML writer writes the children of a `div.columns` with no newline, as Pandoc does. Then Quarto 1\'s CSS works unchanged. This goes with the writer alternative in D1.
- **D7. Slice C is `format: html` only.** \[needs sign-off\]. In a dashboard, a div with the class `columns` is a dashboard layout container, and the dashboard transform reads `width` from its children. Rewriting `width` earlier would take it away. Pandoc-writer formats (docx, pptx) need the raw `width` for their own writers.
- **D8. RevealJS columns are left as they are.** \[needs sign-off\] as a follow-up. q2 lays RevealJS columns out as a flex row with a gap, so 30% and 70% measure 298 and 696 with a 56 gap. Quarto 1 gives 315 and 735 with no gap, because it uses the same `inline-block` rule as HTML. The doc comment in `revealjs/columns.rs` says Quarto 1 uses `flex-basis` for columns. It does not: `layout/html.lua` sets `flex-basis` on layout-panel cells.

Small differences from Quarto 1 in slice A, each \[decided\] as a fix for an undocumented Quarto 1 quirk:

- Quarto 1 leaves a trailing space in the style and adds `style=""` to an `.absolute` element with no position attributes. q2 writes neither.
- Quarto 1 appends to an author `style` with no separator, so `style="color: red"` plus `top=10` gives the broken `color: redtop: 10px; `. q2 adds the missing `; `.
- Quarto 1 lets Lua decide what a number is, so `1e2` becomes `100.0px` and `0x10` becomes `16px`. q2 adds `px` to plain decimal numbers only (`200`, `12.5`, `-10`) and passes everything else through.

## Checklist

### Phase 0: study and baseline

- [x] Read `AGENTS.md` and `claude-notes/instructions/q1-feature-porting.md`.
- [x] Re-read the Quarto 1 sources named above and the q2 files each slice touches.
- [x] Render the two fixture documents with Quarto 1 and q2 and measure them in a browser.
- [x] Check braid (live skein, read only) and GitHub for layout strands and issues newer than 2026-11-10. None found.

### Slice A: RevealJS `.absolute`

- [x] Unit tests in `revealjs/absolute.rs`: Div, Span and Image; `px` rule; existing style; no attributes; non-absolute untouched; inside a custom node slot. Written first against a stub; 8 of 14 failed as expected.
- [x] Integration tests in `revealjs_features.rs`: the three element kinds end to end; `format: html` untouched; the CSS rule ships; an `.absolute` image is not stretched. 3 of 5 failed before the change. The other two are guards that already held.
- [x] `RevealAbsoluteTransform`, pushed for RevealJS next to `RevealColumnsTransform`. The exact-name-list test for the RevealJS pipeline gained `reveal-absolute`.
- [x] CSS rule in `resources/revealjs/quarto-reveal.css`.
- [x] smoke-all fixture `revealjs/absolute.qmd` (a port of Quarto 1 `tests/docs/reveal/mini/absolute.qmd`, which had no assertions).
- [x] Render the layout fixture, measure against Quarto 1, record below.
- [ ] Preview: `ts-packages/preview-renderer/src/q2-preview/inlines/Image.tsx` does not pass an AST `style` through, so an `.absolute` image is not positioned in `q2 preview` yet. `Div.tsx` and `Span.tsx` do pass it.

### Slice B: RevealJS aside placement

- [x] Confirm the cause in a browser by giving the section full height. The four rules were injected into the unchanged q2 deck with a `<style>` element first: the aside moved from y=78 to y=625.
- [x] Integration tests: the four rules ship in the deck stylesheet. 3 of 4 failed before the change. The fourth is a guard that the `scrollable` and `center` classes reach the section.
- [x] Rules in `quarto-reveal.css`: full height for leaf slides, `.scrollable` overflow, and the two aside overrides.
- [x] smoke-all fixture `revealjs/aside-bottom.qmd`. It asserts markup only. The CSS is asserted in the Rust integration tests, because no RevealJS fixture uses `ensureCssRegexMatches` yet and the WASM runner for it was not run here.
- [x] Measure the aside, and check centered slides, vertical stacks, auto-stretch and scrollable slides against Quarto 1. Record below.
- [ ] Preview: `quarto-reveal.css` is imported by the preview deck too, so the same rules apply there. Not checked in `q2 preview`.

### Slice C: HTML `.columns`

- [ ] Unit tests in `transforms/html_columns.rs`.
- [ ] Integration tests: `format: html` gets `style="width: 40%;"`; RevealJS keeps `flex-basis`; dashboard and a Pandoc-writer format keep the raw `width`.
- [ ] `HtmlColumnsTransform`, pushed for the HTML profiles.
- [ ] Flex rule in `_bootstrap-rules.scss`.
- [ ] smoke-all fixture `layout/columns-width.qmd`.
- [ ] Measure against Quarto 1, record below.

### Close-out

- [ ] `cargo nextest run --workspace` and `cargo xtask verify --skip-hub-build`.
- [ ] `q2 preview` checked for the same documents.
- [ ] Decisions D1 to D8 taken to the team.

## Measurements

How they were taken: each fixture is rendered into its own folder, served by `python3 -m http.server` on 127.0.0.1, and opened at 1400 by 900. A script reads `getBoundingClientRect()` for each marker text. For RevealJS it calls `Reveal.slide(n)` first and reports positions relative to the current slide, divided by `Reveal.getScale()`.

### Baseline (before any change)

RevealJS fixture, slide units (x, y, width, height):

| Element | Quarto 1 | q2 at `936c29e13` |
|---|---|---|
| Aside slide `<section>` height | 700 | 153 |
| `<aside>` | 0, 620, 136, 60, `position: absolute` | 0, 78, 116, 55, `position: absolute` |
| Body paragraph on the aside slide | 0, 89, 1050, 52 | 0, 89, 1050, 52 |
| `img.absolute` | 400, 212, 100, 75, `position: absolute`, `style="top: 200px; left: 400px; width: 100px; "` | 0, 101, 100, 75, `position: static`, no style |
| Body paragraph on the absolute slide | 0, 89, 1050, 52 | 0, 212, 1050, 52 |
| Columns 30% and 70% | 315 and 735 wide, no gap | 298 and 696 wide, 56 gap |

HTML fixture, CSS pixels (x, y, width, height):

| Element | Quarto 1 | q2 at `936c29e13` |
|---|---|---|
| Body column | x=221, 799 wide | x=293, 799 wide |
| `div.column` 40% | 221, 134, 320, 43, `style="width:40%;"` | 293, 134, 400, 43, bare `width="40%"` |
| `div.column` 60% | 540, 134, 479, 43, `style="width:60%;"` | 293, 176, 400, 43, bare `width="60%"` |

The body column starts further left in Quarto 1 because the fixture has margin content, which switches the page to the `page-full` grid. That is part of the margin-content work, not of slice C. What slice C has to match is the two widths and the shared y.

### After slice A

Invocation, from a folder holding `revealjs-layout.qmd` and `x.png`:

```sh
target/debug/q2 render revealjs-layout.qmd
```

Observed markup (inspected in the output file):

```html
<img src="x.png" alt="" class="absolute" style="top: 200px; left: 400px; width: 100px;" />
```

Measured in the browser, slide units:

| Element | Quarto 1 | q2 before | q2 after slice A |
|---|---|---|---|
| `img.absolute` | 400, 212, 100, 75, `position: absolute` | 0, 101, 100, 75, `position: static` | 400, 212, 100, 75, `position: absolute` |
| Its paragraph | height 0 | height 111 | height 0 |
| Body paragraph below it | 0, 89, 1050, 52 | 0, 212, 1050, 52 | 0, 89, 1050, 52 |

The image and the text below it are where Quarto 1 puts them. No other slide of the fixture moved: the measurements of the other seven slides are identical before and after.

### After slice B

Two decks were added to the measurement folder for this slice: one with a plain slide, a `.center` slide, a `.scrollable` slide, a single-image slide, a vertical stack and a footnote, and the same deck with `center: true`. Slide units throughout.

Top-aligned deck (the default):

| Element | Quarto 1 | q2 before | q2 after slice B |
|---|---|---|---|
| Plain slide `<section>` height | 700 | 153 | 700 |
| Its `<aside>` | y=620, 60 tall, absolute | y=78, 55 tall, absolute | y=625, 55 tall, absolute |
| Aside bottom edge | 680 | 133 | 680 |
| `.center` slide `<section>` | 153 tall, `top: 273.5px` | 153 tall, `top: 273.5px` | 153 tall, `top: 273.5px` |
| `.scrollable` slide `<section>` | 700 tall, `overflow-y: auto`, content 1545 | 1165 tall, `overflow-y: visible` | 700 tall, `overflow-y: auto`, content 1220 |
| Its `<aside>` | relative, after the last paragraph | absolute, on top of the last paragraph | relative, after the last paragraph |
| Single-image slide: `img.r-stretch` | 0, 165, 660, 495 | 0, 165, 660, 495 | 0, 165, 660, 495 |
| Slide in a vertical stack: `<aside>` | y=620, absolute | y=78, absolute | y=625, absolute |
| Footnote slide: `<aside>` | y=638, 42 tall (bottom 680) | y=85, 55 tall | y=625, 55 tall (bottom 680) |

Deck with `center: true`:

| Element | Quarto 1 | q2 before | q2 after slice B |
|---|---|---|---|
| Plain slide `<section>` | 201 tall, `top: 249.5px` | 153 tall, `top: 273.5px` | 196 tall, `top: 252px` |
| Its `<aside>` | y=153, static, under the body text | y=78, absolute, on top of the body text | y=153, static, under the body text |
| Single-image slide: `img.r-stretch` | 0, 165, 660, 495 | 0, 165, 660, 495 | 0, 165, 660, 495 |

What this shows:

- The aside reaches the slide bottom. Its bottom edge is at 680 in both renderers.
- Centering, auto-stretch and vertical stacks are not disturbed.
- Scrollable slides now scroll, and their aside follows the content.

What still differs from Quarto 1, and why. None of it is part of this slice:

- The aside is 55 tall in q2 and 60 in Quarto 1, so its top edge is 5 lower. q2's rule has `font-size: 0.6em`. Quarto 1 uses `$presentation-font-smaller`, which is 0.7. The 0.6 is written into `plans/2026-06-08-revealjs-presentations.md`, so it was left alone.
- Quarto 1 centers the first slide of a vertical stack (the level-1 heading slide gets `title-slide center`). q2 top-aligns it, before and after this slice.
- A footnote-only aside is 42 tall in Quarto 1 and 55 in q2. The cause was not investigated. Its bottom edge is at 680 in both.

In the main layout fixture, slice B changed nothing except the section heights (all 700 now) and the aside position.

## Not covered

- RevealJS tabsets, figure panels, HTML margin and wide content, sidebar panels.
- The deck-wide `scrollable: true` option.
- `.absolute` on a captioned image that becomes a cross-referenced figure.
- RevealJS column sizes (D8).
- The aside font size (0.6em in q2, 0.7em in Quarto 1), centering of the first slide of a vertical stack, and the height of a footnote-only aside. All three are described under "After slice B".
- A small difference in deck scale between the two renderers (1.1571 in Quarto 1, 1.1600 in q2) seen in the baseline. Not investigated.
