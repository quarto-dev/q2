---
title: 'RevealJS auto-stretch: image not sized because `r-stretch` img stays wrapped in `<p>`'
date: 2026-06-17
---

**Strand:** bd-zkstclhl (figure follow-up: bd-38ioql41)
**Date:** 2026-06-17
**Status:** investigation complete, design decided, implementation not started

## Decisions (locked 2026-06-17, with user)

- **Scope:** fix the **Paragraph\[Image\]** case now (the reported bug). The
  Pandoc **Figure** case is a follow-up (strand **bd-38ioql41**).
- **Architecture:** fix at the **AST level** in the existing
  `RevealAutoStretchTransform`. **Do NOT** introduce a DOM-postprocessor stage
  to mirror Q1. Q2 renders HTML directly from the AST and deliberately has no
  post-Pandoc DOM-mutation stage; adopting Q1's `applyStretch`-style
  postprocessor would add a large architectural surface with no other consumer.
  The bar for ever adding one is "an extremely strong reason," not yet met.
- **Communicating the anti-pattern to future readers** (so nobody is tempted to
  port Q1's DOM postprocessor): two pointers at the two places people look —
  1. a corrected note at the temptation site (`auto_stretch.rs` module doc,
     replacing the current wrong comment) stating the principle positively:
     *"reveal needs `section > img.r-stretch`; we achieve it by unwrapping the
     container in the AST, NOT by mutating the DOM after Pandoc — Q2 has no DOM
     postprocessor and intentionally so."*
  2. a one-line rule in `CLAUDE.md` → *Architecture Notes*: *"Q2 emits HTML
     directly from the AST and has no DOM-postprocessor stage. When porting a
     Quarto 1 reveal/HTML DOM postprocessor, re-express it as an AST transform;
     do not add a DOM-mutation stage without an extremely strong, discussed
     reason."* (CLAUDE.md is loaded every session, so this shapes behavior
     before anyone opens the file.)

## Overview

Q2's auto-stretch transform (`crates/quarto-core/src/revealjs/auto_stretch.rs`)
correctly adds the `.r-stretch` class to a lone slide image, but the image is
emitted as:

```html
<section id="..." class="section">
  <h2>...</h2>
  <p><img src="diagram.svg" alt="" class="r-stretch" /></p>   <!-- wrapped in <p> -->
</section>
```

reveal.js's stretch-layout routine selects stretch targets with
`section > .stretch, section > .r-stretch` — **direct children of the section
only** (verified in `slides_files/revealjs/reveal.js`: the layout function
`...forEach` iterates `b(l.slides,"section > .stretch, section > .r-stretch")`).
Because our image is `section > p > img.r-stretch`, reveal never matches it and
never resizes it. The image renders at its natural size; the `.r-stretch` class
is inert.

The DOM looks correct (the class is present), which is why this slipped through
— but the *structure* is wrong.

## Evidence (reproduced 2026-06-17)

Repro:
```bash
cargo run --bin q2 -- render examples/presentations/11-auto-stretch/slides.qmd
# serve examples/presentations/11-auto-stretch/ and open slides.html in Chrome
```

Chrome DevTools on the first content slide (`#a-single-image-fills-the-slide`):

- `img.r-stretch` computed height = **400px** (the SVG's natural height), parent
  = `<p>`. Reveal did not size it.
- Manually hoisting the img to be a direct child of `<section>` (removing the
  `<p>`) and calling `Reveal.layout()` → computed height jumps to **443px**,
  dynamically fit to the remaining slide space. **This confirms the `<p>`
  wrapper is the bug and the direct-child structure is the fix.**

### Quarto 1 comparison (`quarto render`, v99.9.9 dev)

Q1 output for the same slide:
```html
<section id="a-single-image-fills-the-slide" class="slide level2">
<h2>...</h2>
<p>When a slide holds just one image...</p>
<img data-src="diagram.svg" class="r-stretch"></section>   <!-- bare img, direct child of section -->
```
Q1 **hoists the img out of the `<p>`**. It also ships CSS keyed on exactly that
structure: `.reveal .slide > img.r-stretch.quarto-figure-center { ... }`. The
nostretch image, by contrast, stays wrapped: `<p><img class=""></p>`.

So Q1's behavior is: stretched image → direct child of section; non-stretched →
left wrapped. Q2 must do the same hoisting.

### The misleading code comment

`auto_stretch.rs:29-32` currently claims:

> We still **don't** do Q1's DOM hoisting ...; Chrome E2E confirmed reveal sizes
> the nested `<p><img class=r-stretch>` / figure image correctly without it.

This conclusion was **wrong** — reveal's `section > .r-stretch` selector cannot
match a nested image. The comment must be corrected/removed as part of the fix.

## Root cause

The writer renders `Block::Paragraph` as `<p>...</p>`
(`crates/pampa/src/writers/html.rs:1286`) and `Block::Plain` as bare inlines
(`html.rs:1282`, no `<p>` wrapper). The auto-stretch transform leaves the lone
image inside a `Paragraph`, so it renders wrapped.

## How Quarto 1 does it — a DOM postprocessor (not the AST)

`applyStretch` in
`external-sources/quarto-cli/src/format/reveal/format-reveal.ts:949-1100` is a
**post-Pandoc DOM postprocessor** (operates on a parsed `Document`, deno-dom).
After it adds `.r-stretch` to the image it does explicit DOM surgery
(lines 1041-1096):

1. Guard: only act `if (hasStretchClass(imageEl) && imageEl.parentNode !== SECTION)`.
2. Find the slide's top-level node that contains the img (`nodeEl` — the `<p>`
   or the `div.quarto-figure`).
3. **Figure case** (`div.quarto-figure`): copy the `quarto-figure-(center|left|right)`
   alignment class onto the img, copy the figure `id` onto the img, and lift the
   `<figcaption>` HTML into a fresh `<p class="caption">`.
4. Compute target position = `nodeEl.nextElementSibling`.
5. `removeEmpty(imageEl)` — detach the img, then recursively delete now-empty
   ancestors, stopping at `<section>`.
6. `slideEl.insertBefore(image, nextEl)` — re-attach the img as a **direct child
   of the section**, where its old container was.
7. Re-insert the caption `<p>` after the img; remove the empty figure container.

So Q1's correct rendering depends entirely on this DOM-level hoisting. Q2 has
**no DOM postprocessor stage** — the HTML writer emits the final string directly
from the AST. We cannot port `applyStretch` literally; we must achieve the same
final DOM (`section > img.r-stretch`) at the AST level instead.

**Scoping fact:** Q2 currently emits **no `quarto-figure` wrappers, no
`<figure>`/`<figcaption>`, no alignment classes** (verified: none appear in the
rendered example). So the entire figure/caption/alignment branch of Q1's logic
has no Q2 counterpart *yet*. The Q2 problem reduces to the `<p><img>` (Pandoc
Paragraph) case plus, separately, a Pandoc `Figure` block (from
`![caption](x)`), which the reveal writer would render as `<figure>`.

## Fix approach (AST-level, no DOM postprocessor)

The key insight: Q2 doesn't need to replicate Q1's DOM surgery. The same final
DOM (`section > img.r-stretch`) is reachable by an **AST transform** — which is
where `RevealAutoStretchTransform` already lives. When we stretch, also **unwrap**
the container so the writer emits the image as a direct child of the section:

1. **`Paragraph[Image]` case** (the common case, and the reported example):
   replace the `Block::Paragraph` with a `Block::Plain` holding the same single
   image inline. The HTML writer renders `Plain` inlines bare (no `<p>`), so the
   `<img class="r-stretch">` becomes a direct child of the `<section>` div.
   Confirmed by the writer (`html.rs:1282` vs `1286`) and by the live-DOM repro.

2. **`Figure` case** (`![caption](x)`): more involved because of the caption.
   Q1 lifts the img to section level and re-inserts the caption as a sibling
   `<p class="caption">`. The AST equivalent: replace the `Figure` block with a
   `Plain[Image]` (id transferred onto the img) followed by a caption block.
   **Open question** — see design discussion: do we do this now, or scope the
   first fix to the Paragraph case and file a follow-up? The reported bug only
   exercises the Paragraph case, and Q2 has no `quarto-figure`/alignment story
   yet, so the figure path is lower-fidelity-to-Q1 by necessity.

Implementation note: the current loop borrows `image: &mut Image` from inside the
block. Unwrapping means replacing the block itself, so restructure to operate on
the block (compute the decision, then `*block = Block::Plain(...)`) rather than
mutating through the image reference. Keep the existing opt-out gates
(`.nostretch`, `.absolute`, explicit size, already-stretched) intact, and only
unwrap on the slides we actually stretch (don't disturb non-stretched images).

### Design questions to resolve before implementing

- **A. Scope of this fix.** Paragraph-only now + figure follow-up strand, or
  both Paragraph and Figure together? (Lean: Paragraph now; it's the reported
  bug and a clean, low-risk change. File a figure-caption follow-up.)
- **B. Centering.** A bare stretched img: does it center horizontally like Q1's?
  Q1 leans on `.reveal .slide > img.r-stretch.quarto-figure-center` CSS that Q2
  doesn't ship. Reveal core may center r-stretch block images on its own — must
  be visually verified in the browser as part of acceptance, and if not, decide
  whether to add a small CSS rule (Q2-side) rather than per-image classes.
- **C. Keep it an AST transform** (recommended) vs introduce a DOM-postprocessor
  stage to mirror Q1 structurally. The AST route reuses the existing transform
  and avoids a new architectural surface; a DOM postprocessor would be a large
  new stage with no other current consumer. Recommend AST.

## Test plan (TDD — write tests first, confirm they fail)

Unit (in `auto_stretch.rs` tests): the existing tests check that the class is
added but not the block type. Add assertions that after stretching, the image's
container block is `Plain` (not `Paragraph`). Update the `stretch_classes`
helper or add a sibling helper that returns the container block kind.

- [ ] `lone_paragraph_image_becomes_plain` — Paragraph\[Image\] → Plain\[Image\]
      with `r-stretch`.
- [ ] non-stretched images (auto-stretch false, nostretch, sized, two-image)
      keep their original `Paragraph` container (no unwrap when not stretching).
- [ ] figure case: assert whatever behavior we commit to (stretch + structure).

End-to-end (required per CLAUDE.md — CLI + inspect output): an integration test
in `crates/quarto-core/tests/integration/revealjs_format.rs` that renders the
fixture and asserts the emitted HTML contains `r-stretch` as a **direct child of
the section** (`<img ... class="r-stretch">` not inside `<p>`). A regex/string
check that the `r-stretch` img is not preceded by an open `<p>` on the same
nesting, or simpler: assert the substring `<p><img` ... `r-stretch` does **not**
appear, and `class="r-stretch"` does.

- [ ] Render-path regression test asserting bare `section > img.r-stretch`.

Browser re-verify (manual, per end-to-end policy): re-render the example, serve,
confirm in Chrome that `img.r-stretch` computed height > natural height (i.e.
reveal sized it). Record the invocation + observed height in this doc.

### Browser re-verification results (2026-06-17) — PASS, Q1-parity confirmed

Invocation:
```bash
cargo run --bin q2 -- render examples/presentations/11-auto-stretch/slides.qmd
# served examples/presentations/11-auto-stretch/ on :8731, Chrome DevTools
```

Rendered HTML (inspected) — the stretched img is now a **direct child of the
section**, no `<p>`:
```html
<section id="a-single-image-fills-the-slide" class="section">
  <h2>A single image fills the slide</h2>
  <p>When a slide holds just one image ...</p>
  <img src="diagram.svg" alt="" class="r-stretch" />
</section>
```

Chrome computed-style comparison, fixed Q2 vs Q1 (`quarto render`, v99.9.9) on
the same slide — **identical**:

| metric              | Q2 (fixed) | Q1      |
| ------------------- | ---------: | ------: |
| `img.parentElement` | `SECTION`  | `SECTION` |
| natural height      | 400px      | 400px   |
| **computed height** | **443px**  | **443px** (reveal sized it) |
| `display`           | inline     | inline  |
| margin-left/right   | 0 / 0      | 0 / 0   |
| gap left / right    | 0 / 413    | 0 / 413 |

So the bug (image stuck at natural 400px) is fixed and Q2's geometry now matches
Q1 exactly.

**Design question B (centering) — resolved: no action.** The bare stretched image
is flush-left in *both* Q1 and Q2 (`gapLeft 0, gapRight 413`). Q1's
`.reveal .slide > img.r-stretch.quarto-figure-center` centering CSS does not
apply here because Q1's img carries only `r-stretch` (no `quarto-figure-center`).
Matching Q1 is the spec, so we deliberately do **not** add a centering rule that
would diverge from Q1. (If Q2 later grows a figure-alignment story, revisit
alongside the figure-unwrap follow-up bd-38ioql41.)

## Work items

- [x] Write failing unit test: stretched Paragraph\[Image\] becomes Plain\[Image\].
      (`lone_paragraph_image_becomes_plain` + `non_stretched_image_keeps_paragraph`;
      confirmed failing: `left "Paragraph", right "Plain"`.)
- [x] Write failing render-path test: bare `section > img.r-stretch`, no `<p>`.
      (`revealjs_auto_stretch_img_is_direct_section_child`; confirmed failing on
      `<p><img ... class="r-stretch" /></p>`.)
- [x] Implement unwrap (Paragraph → Plain) in `maybe_stretch_section`.
      (Restructured into `decide_stretch` + `StretchOutcome`; Paragraph\[Image\]
      becomes `Plain[Image]` when stretched. All 18 unit+integration tests pass.)
- [x] Decide + implement/defer the Figure caption case. (Deferred to bd-38ioql41;
      figure still gets `.r-stretch` in place for now.)
- [x] Correct the misleading comment at `auto_stretch.rs:29-32`. (Replaced with a
      positive "unwrap in the AST, no DOM postprocessor" note + CLAUDE.md rule.)
- [x] `cargo nextest run --workspace` (monorepo regression check). 10207 passed.
- [x] Re-render example, serve, re-verify in Chrome; record height here.
      443px computed (vs 400px natural); Q1-parity confirmed (see above).
- [x] `cargo xtask verify` (WASM leg: quarto-core is in hub-client's closure).
      All 14 steps passed. NB: the lint leg caught two `collapsible_if` clippy
      warnings in the new code (stricter than build/nextest, as CLAUDE.md warns);
      collapsed both into edition-2024 let-chains, re-verified green.

## Files

- `crates/quarto-core/src/revealjs/auto_stretch.rs` — the transform + comment.
- `crates/quarto-core/tests/integration/revealjs_format.rs` — e2e test home.
- `crates/pampa/src/writers/html.rs:1282-1292` — Plain vs Paragraph emission.
- `examples/presentations/11-auto-stretch/slides.qmd` — repro fixture.
