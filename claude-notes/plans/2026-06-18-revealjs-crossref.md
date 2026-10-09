---
title: 'Cross-references in `format: revealjs`'
date: 2026-06-18
description: 'Fixes cross-reference resolution under `format: revealjs`, where auto-stretch hoists figures before the crossref phase and bare tables never become float targets.'
status: draft  # Design — decisions taken 2026-06-18 (see "Decisions"), iterating before implementation
braid:
  strand: bd-w0c6d38k  # related: bd-jsbg crossref epic, bd-zkstclhl reveal auto-stretch
  priority: P1
  labels: [crossref, revealjs]
---

**Sub-strands:** bd-4ly7ne01 (Bug B: bare-table desugar, format-agnostic),
bd-zecehtnc (WASM/interactive-preview crossref support: `q2 preview` + hub-client)

---

## Overview

Audit and fix cross-reference **computation, resolution, and rendering** under
`format: revealjs`, so that all three crossref categories work:

- **floats** — Figure, Table
- **blocks** — Theorem, Lemma, … (and Proof-likes)
- **inlines** — Equation

The trigger was `examples/presentations/14-crossrefs/slides.qmd`, where `@fig-1`
renders unresolved (`?fig-1?`) and `@fig-2` is mis-numbered.

This is partly a *pipeline-ordering / format-composition* problem: a
revealjs-only AST transform (`RevealAutoStretchTransform`) runs **before** the
crossref phase and destroys crossref float targets. The footer-rendering work
(`footer_render_stage`, bd-n2w0sxgd) is the precedent for reasoning about which
transforms run under which format and in what order.

---

## Reproduction (observed 2026-06-18)

### A. revealjs is broken; HTML is fine

`cargo run --bin q2 -- render examples/presentations/14-crossrefs/slides.qmd`
emits `Warning: unresolved crossref @fig-1` and produces:

```html
<!-- body ref -->
See <a href="#fig-1" class="quarto-xref">?fig-1?</a> and
    <a href="#fig-2" class="quarto-xref">Figure 1</a>.
<!-- fig-1 slide: figure structure destroyed, no number -->
<img src="logo.svg" alt="A figure" id="fig-1" class="r-stretch" />
<!-- fig-2 slide: numbered "Figure 1" (should be Figure 2) -->
<figure id="fig-2"> … <p>Figure 1: Another figure.</p>
```

The **identical content rendered as `format: html`** resolves correctly:
`Figure 1` / `Figure 2`, both `<figure id=…>`, caption `Figure 2: …`. So the
crossref machinery is correct; the revealjs path breaks it.

### B. Per-category audit in revealjs

Test file `/tmp/xref-test/all.qmd` (one of each category):

| ref | form | revealjs result | HTML result | verdict |
|-----|------|-----------------|-------------|---------|
| `@fig-1` | `![cap](x){#fig-1}` (implicit Figure) | **unresolved `?fig-1?`** | `Figure 1` ✓ | **revealjs bug (A)** |
| `@fig-plot` | `::: {#fig-plot}` (div) | `Figure 1` ✓ | `Figure 1` ✓ | works |
| `@tbl-data` | `: cap {#tbl-data}` (pipe-table caption) | **unresolved `?tbl-data?`** | **unresolved `?tbl-data?`** | **general gap (B)** |
| `@thm-main` | `::: {#thm-main}` | `Theorem 1` ✓ | `Theorem 1` ✓ | works |
| `@eq-key` | `$$…$$ {#eq-key}` | `Equation 1` ✓ | `Equation 1` ✓ | works |

So in revealjs today: **blocks (theorem) and inlines (equation) already work**;
**div-form figure floats work**; the breakage is (A) attribute-form figures and
(B) bare-table floats. (B) is not revealjs-specific.

### C. Div-form floats are the robust canonical path (verified both formats)

The `::: {#<ref>-…}` div form works correctly in **both HTML and revealjs**
(`/tmp/xref-div/*.qmd`, 2026-06-18):

| ref | div content | HTML | revealjs |
|-----|-------------|------|----------|
| `@tbl-1` | image-of-table (`![](x)` + caption) | `Table 1`, `<div id="tbl-1">` ✓ | `Table 1`, `<div id="tbl-1">` ✓ |
| `@tbl-2` | real pipe-table + caption | `Table 2` ✓ | `Table 2` ✓ |
| `@fig-1` | `![](x)` + caption | `Figure 1`, `<figure>` ✓ | `Figure 1`, `<figure>` ✓ |

Notably the single-image `::: {#tbl-1}` slide is rendered as `<div id="tbl-1">`
and is correctly **not** stretched — auto-stretch leaves plain Divs alone (it
only targets top-level `Paragraph[Image]`/`Figure`). This confirms the intended
architecture: **the div/float form is canonical and uniform; the other syntaxes
are sugar that should desugar into (or be classified like) it.** Both bugs are
exactly the sugar forms that fail to reach that canonical path:

- Bug A: `![cap](x){#fig-1}` *is* classified as a float in HTML, but revealjs
  auto-stretch destroys the `Figure` **before** classification.
- Bug B: `: cap {#tbl-data}` (bare `Block::Table`) **never desugars** into the
  float form at all.

---

## Root-cause analysis

### Bug A — auto-stretch runs before the crossref phase (revealjs-specific)

`build_transform_pipeline` (`crates/quarto-core/src/pipeline.rs`) order:

```
NORMALIZATION
  …
  RevealSlidesTransform            (is_revealjs)         ~1127
  RevealFootnotesTransform         (is_revealjs)         ~1143
  RevealAutoStretchTransform       (is_revealjs)         ~1148   ← TOO EARLY
  ExampleEmbedTransform                                  ~1159
  TheoremSugarTransform / ProofSugarTransform            ~1164
  FloatRefTargetSugarTransform     (Div/Figure → float)  ~1166
  EquationLabelTransform                                 ~1167
CROSSREF PHASE
  CrossrefIndexTransform           (number floats)        1170
  CrossrefResolveTransform         (resolve @refs)        1171
…
FINALIZATION
  CrossrefRenderTransform          (CustomNode → Figure)  1259
```

`![A figure](logo.svg){#fig-1}` parses to a Pandoc `Block::Figure` (implicit
figure: a captioned image alone in a paragraph). On a single-image slide,
`RevealAutoStretchTransform` (1148) hits its `Block::Figure` branch and
**hoists** it: replaces the `Figure` with `Plain[Image]` (id transferred onto
the `<img>`) + a caption paragraph — see
`crates/quarto-core/src/revealjs/auto_stretch.rs` `hoist_figure`.

By the time `FloatRefTargetSugarTransform` (1166) runs, there is **no Figure**
with id `fig-1` left to classify as a float — only a bare `Plain[Image]` that
happens to carry the id. So:

- `fig-1` is never registered in the crossref index → `@fig-1` unresolved.
- Only `fig-2` (the `:::`-div form, which auto-stretch ignores because at this
  point it is still a plain `Block::Div`, not a `Figure`) is counted → it
  becomes "Figure 1".

The id-transfer in `hoist_figure` was a band-aid for the *HTML anchor* (`#fig-1`
still points somewhere) but does nothing for crossref *registration/numbering*,
which needs the float node to survive into the crossref phase.

`auto_stretch.rs` (lines 50–53) already documents this as a known divergence:
> The *cross-referenceable* figure case (`::: {#fig-…}`) is still un-stretched on
> single-image slides … the crossref→`Figure` conversion runs later and is
> excluded from the preview pipeline … a documented divergence … deferred.

### Bug B — bare `Block::Table` with caption-id is never a float (general)

`FloatRefTargetSugarTransform` classifies only `Block::Div` and `Block::Figure`
(`float_ref_target.rs` `classify_div` / `classify_fig`). It handles the
`Div(#tbl-…) > Table` form, but a **bare top-level `Block::Table`** whose caption
carries `{#tbl-data}` (the `: caption {#tbl-data}` pipe-table syntax) is never
classified — it renders as `<table id="tbl-data">` with no number and `@tbl-data`
unresolved, in **both HTML and revealjs**. Q1 handles this in
`parsefiguredivs.lua` (wraps caption-bearing tables into FloatRefTargets).

### The format-composition tension (why we can't just reorder blindly)

Auto-stretch was deliberately placed *early* so it operates on a shape common to
**both** pipelines:

- **native render** (`build_transform_pipeline`): includes
  `CrossrefRenderTransform` → crossref floats become real `Figure`s by
  finalization.
- **preview** (`build_q2_preview_transform_pipeline` = full pipeline minus
  `Q2_PREVIEW_TRANSFORM_EXCLUDED`): **excludes `"crossref-render"`** (pipeline.rs
  line 1374) so the CustomNodes survive for React's type-specific components.

Therefore a crossref float has **no single concrete shape** across both
pipelines after the crossref phase: it is a `Figure` in native render but a
`CustomNode("FloatRefTarget")` in preview. That is the real reason the original
author put auto-stretch up front (where both pipelines still see a plain
`Figure`/`Paragraph[Image]`), and accepted not stretching crossref figures.

Note: `CrossrefIndexTransform` **is** included in preview — only *render* is
excluded — so **numbering is correct in preview**; only the final-Figure DOM
shape is absent there.

---

## Proposed direction (to confirm with user)

The fix has three parts. Parts 1–2 are mechanical; Part 3 is the design
decision the user flagged.

### Part 1 — Bug B: desugar caption-bearing bare tables into the canonical float Div (format-agnostic)

**Tracked separately as bd-4ly7ne01** (format-agnostic; affects HTML too).
A bare `Block::Table` whose caption carries a registered ref-type id (`tbl-…`)
should be **wrapped into a `Div(#tbl-…) > Table`** by a small desugar step, so the
*existing* uniform `FloatRefTargetSugarTransform` `Div > [Table]` arm handles it
— matching the "all syntaxes desugar into divs, then process uniformly" model
rather than adding a special top-level `Table` classification arm. Q1 ref:
`parsefiguredivs.lua`.

- **Tests first:** qmd fixture in `crossref_fixtures.rs` asserting the index
  registers `tbl-data` from `: cap {#tbl-data}`; end-to-end HTML render shows
  `Table 1` + resolved `@tbl-data`.

### Part 2 — Bug A: move revealjs float post-processing after the crossref phase

Move `RevealAutoStretchTransform` so it runs **after `CrossrefRenderTransform`**
(finalization, after line 1259 / after `ExampleEmbedRenderTransform`). Then for
native render:

1. `![cap](x){#fig-1}` → `Figure` survives to `FloatRefTargetSugarTransform` →
   numbered → resolved → rendered by `CrossrefRenderTransform` to a final
   `Figure(id=fig-1)` with caption "Figure 1: A figure".
2. Auto-stretch then hoists *that* Figure to `section > img.r-stretch` + a
   `<p class="caption">Figure 1: A figure</p>`, exactly as it already does for
   plain captioned figures.

Side benefit: the `::: {#fig-…}` div form *also* becomes a real `Figure` by then,
so auto-stretch can finally stretch div-form crossref figures too — closing the
"still un-stretched" divergence noted in `auto_stretch.rs` for native render.

Ordering constraints to preserve: auto-stretch must still run **after**
`RevealFootnotesTransform` (coalesced aside ⇒ >1 block ⇒ skip) — satisfied, since
that is in NORMALIZATION, far earlier. The reveal `<section>` tree
(`RevealSlidesTransform`) already exists by finalization.

- **Tests first:** the existing `auto_stretch.rs` unit tests operate on synthetic
  ASTs and stay valid. Add an **end-to-end** revealjs render test (via
  `render_document_to_file` / the binary) over `14-crossrefs/slides.qmd`
  asserting: `@fig-1` resolves to `Figure 1`, `@fig-2` to `Figure 2`, fig-1's
  `<img id="fig-1" class="r-stretch">` is a direct child of its `<section>`, and
  its caption reads "Figure 1: …". This is the regression the unit tests missed.

### Part 3 — Preview (`q2-slides`) behavior for crossref figures

**Decision (2026-06-18): option 3a for this strand, with a committed follow-up
(bd-zecehtnc).** After Part 2, in **native render** crossref figures number,
resolve, render, and stretch correctly. In **preview**, `crossref-render` is
excluded, so the float stays a `CustomNode("FloatRefTarget")`; numbering is still
correct (index runs in preview) but the final-Figure shape + auto-stretch do not
apply there. We accept that divergence in *this* strand (it fixes the
user-visible `q2 render` bug and matches the documented status quo), but the
WASM-based interactive previews (`q2 preview` **and** hub-client) must support
crossrefs well — tracked as **bd-zecehtnc**.

Options considered for the eventual preview fix (decided in bd-zecehtnc):

- **3b (teach auto-stretch the CustomNode form):** give auto-stretch a path that
  also recognizes single-image `CustomNode("FloatRefTarget")` and hoists it,
  preserving the custom node for React.
- **3c (include crossref-render in the slides preview):** drop `"crossref-render"`
  from `Q2_PREVIEW_TRANSFORM_EXCLUDED` for the `q2-slides` path; React then
  receives `Figure` instead of the custom node — revisit component expectations.
- React-side rendering of crossref CustomNodes (incl. reveal stretch) is a third
  candidate. bd-zecehtnc will pick among these.

### Part 4 — formalize "which transforms run under which format, in what order"

Today reveal transforms are `pipeline.push`-ed inline under `if is_revealjs`, at
fixed positions. The footer work introduced `footer_render_stage(is_revealjs)` as
"the one place that maps format → render stage … the seam where a format →
stage-sequence mapping would grow." Part 2 adds a *second* format-gated
finalization slot (a reveal float-postprocess step after crossref).

The **named helper** = the direct analogue of `footer_render_stage`: one
documented function owning "which reveal transforms run after crossref, and
where". Sketch:

```rust
// returns the reveal-specific finalization transforms (today: just auto-stretch)
fn reveal_finalization_transforms(is_revealjs: bool) -> Vec<Box<dyn AstTransform>> {
    if is_revealjs { vec![Box::new(RevealAutoStretchTransform::new())] }
    else           { vec![] }
}
// spliced in once, right after CrossrefRenderTransform (finalization):
for t in reveal_finalization_transforms(is_revealjs) { pipeline.push(t); }
```

It is **not** a pipeline DSL or a format→stage map — just a named, greppable
function (CLAUDE.md forbids new architectural surface without strong reason).
If it feels like overkill for one transform, "defer the seam" (inline push at the
new position) is acceptable. Decision pending; default to the small helper since
Part 2 does add a second gated slot.

---

## Decisions (2026-06-18)

1. **Preview (Part 3):** option **3a** for this strand (native render fixed; preview
   numbers/resolves but doesn't stretch). WASM interactive previews (`q2 preview`
   + hub-client) must support crossrefs well — committed follow-up **bd-zecehtnc**.
2. **Bug B (Part 1):** **split** into format-agnostic sub-strand **bd-4ly7ne01**
   (it affects HTML too). Will be done this session or a direct follow-up.
3. **Part 4 seam:** default to the small named helper
   (`reveal_finalization_transforms`); finalize during implementation. Still
   confirming whether to do it now vs. inline-push + defer.
4. **Structural guardrail (Part 5, NEW):** add a required `phase()` method to the
   `AstTransform` trait (`transform.rs:69`) returning a `TransformPhase` enum
   (`Normalization` < `Crossref` < `Navigation` < `Finalization`), and a
   **format-neutral ordering-invariant test** over the real `build_transform_pipeline`
   asserting phase ranks are non-decreasing by position. Trait-method approach
   chosen over a test-only table (negligible cost — one vtable slot per type, zero
   render-time/per-instance cost; may find other uses). The test is **format-neutral**
   (loops over supported format strings, no `is_revealjs` branch) so future formats
   (`dashboard`, `typst`, `pdf`) are covered for free. It **fails on today's code**
   and **passes after Part 2's reorder** — i.e. it is the red/green TDD test for the
   structural fix as well as the permanent guardrail.
5. **Architecture doc:** written to
   `claude-notes/designs/transform-pipeline-phases.md` (phase model, invariant,
   author rule, and the **preview-pipeline shape contract** — the anti-recurrence
   rule). To be linked from `CLAUDE.md` → Architecture Notes **when the fix lands**
   (so CLAUDE.md never points at an unenforced invariant).

## Remaining checks during implementation

- Are there other reveal-only transforms that mutate float/caption/identity shape
  and would also need to move after crossref? Audit: `RevealColumns`,
  `RevealSlides`, `RevealFootnotes`, `RevealFooterAlias`, `RevealFooterLogo` —
  none appear to touch float identity, but confirm.
- Confirm `CrossrefRenderTransform` renders figure-floats as `Block::Figure`
  (so auto-stretch's `Figure` arm catches them) and table-floats as `Div` (left
  un-stretched, matching div-form behavior verified above).

---

## Work items (provisional — finalize after design sign-off)

- [ ] **Tests first (Part 5):** add `TransformPhase` enum + required `phase()` on
      `AstTransform`; classify every existing transform; write the format-neutral
      ordering-invariant test. Confirm it **FAILS** today (auto-stretch inversion).
- [ ] **Tests first:** end-to-end revealjs render regression over `14-crossrefs`
      (fig-1/fig-2 number + resolve + r-stretch + caption); confirm it FAILS today.
- [ ] **Tests first:** caption-bearing bare-table float fixture (HTML), confirm FAILS.
- [ ] Part 2: move `RevealAutoStretchTransform` to after `CrossrefRenderTransform`
      (declared phase `Finalization`); confirm the ordering test now **PASSES**.
- [ ] Part 1 (bd-4ly7ne01): desugar bare `Block::Table` w/ caption-id into
      `Div(#tbl-…) > Table` so the uniform classifier handles it.
- [ ] Part 3: implement the chosen preview behavior (default 3a).
- [ ] Part 4 (optional): extract `reveal_finalization_transforms` seam.
- [ ] Part 5 doc: link `transform-pipeline-phases.md` from `CLAUDE.md` →
      Architecture Notes once the invariant is enforced.
- [ ] Re-render `14-crossrefs/slides.qmd`; inspect HTML; record the end-to-end
      output in this plan per CLAUDE.md "End-to-end verification".
- [ ] `cargo nextest run --workspace`; `cargo xtask verify` (WASM leg, since
      `quarto-core` changes affect `wasm-quarto-hub-client`).

---

## Part 5 sub-plan — `phase()` trait method + ordering-invariant test (IN PROGRESS)

Logically first: it's the red/green TDD test for Part 2. Approach: `phase()`
defaults to `Unclassified`; every transform in `build_transform_pipeline` overrides
it; the test rejects `Unclassified` in the pipeline (exhaustiveness) + asserts
non-decreasing rank by position. Design: `claude-notes/designs/transform-pipeline-phases.md`.

Phase assignments (47 pipeline structs; `JupyterTransform`,
`AttributionGenerateTransform`, test doubles stay `Unclassified`):

- **Normalization:** Callout, CalloutResolve, ShortcodeResolve, MetadataNormalize,
  CodeBlockGenerate, WebsiteTitlePrefix, WebsiteFavicon, WebsiteBootstrapIcons,
  WebsiteCanonicalUrl, RevealColumns, RevealSlides, RevealFooterAlias, TitleBlock,
  Sectionize, Footnotes, RevealFootnotes, ExampleEmbed, TheoremSugar, ProofSugar,
  FloatRefTargetSugar, EquationLabel
- **Crossref:** CrossrefIndex, CrossrefResolve
- **Navigation:** TocGenerate, NavbarGenerate, SidebarGenerate, PageNavGenerate,
  FooterGenerate, ListingGenerate, ListingRender, CategoriesSidebar,
  ListingFeedStage, ListingFeedLink, TocRender, NavbarRender, SidebarRender,
  PageNavRender, FooterRender, RevealFooterLogo
- **Finalization:** LinkRewrite, AppendixStructure, CrossrefRender,
  ExampleEmbedRender, CodeBlockRender, ResourceCollector, TableBootstrapClass,
  AttributionRender, AttributionViewer, **RevealAutoStretch** (← currently
  mis-ordered before Crossref; Part 2 moves it after CrossrefRender)

### Part 5 checklist

- [x] 5.1 Add `TransformPhase` enum (`Normalization` < `Crossref` < `Navigation` <
      `Finalization` < `Unclassified`) + defaulted `phase()` on `AstTransform`
      (`transform.rs`). Done — enum + defaulted method, doc-commented.
- [x] 5.2 Override `phase()` on all 21 Normalization transforms.
- [x] 5.3 Override `phase()` on the 2 Crossref transforms.
- [x] 5.4 Override `phase()` on all 16 Navigation transforms.
- [x] 5.5 Override `phase()` on all 10 Finalization transforms (incl.
      RevealAutoStretch = Finalization). 49 total inserted via script keyed by
      struct name; `quarto-core` compiles clean.
- [x] 5.6 Wrote `test_build_transform_pipeline_phase_ordering` in `pipeline.rs`:
      loops over `["html","revealjs"]`, asserts no `Unclassified` + non-decreasing
      rank; failure names offender + full order.
- [x] 5.7 Confirmed **RED**: `[revealjs] phase ordering inversion:
      reveal-auto-stretch (Finalization) runs before example-embed (Normalization)`.
      HTML pipeline passes (no auto-stretch). Validates all phase assignments.

### Part 2 checklist (makes 5.7 green)

- [x] 2.1 Added `reveal_finalization_transforms(is_revealjs)` helper (Part 4 seam,
      analogue of `footer_render_stage`).
- [x] 2.2 Removed `RevealAutoStretchTransform` from the early `is_revealjs` block;
      left a NOTE pointing to its new home + the bd-w0c6d38k rationale.
- [x] 2.3 Spliced the helper after `ExampleEmbedRenderTransform` (post
      `CrossrefRenderTransform`) and before `CodeBlockRenderTransform`/
      `ResourceCollectorTransform`.
- [x] 2.4 Ordering test **GREEN** (was RED).
- [x] 2.5 e2e regression test `revealjs_crossref_attribute_figure_resolves_and_stretches`
      added; RED before the reorder, GREEN after. (Caption *number-prefix*
      assertion intentionally dropped — see bd-uwv2eec2 below; ref numbering +
      stretch + direct-section-child are asserted. Links use U+00A0.)
- [x] 2.6 Re-rendered `14-crossrefs/slides.qmd` via `q2 render` — see results below.
- [x] 2.7 Full `cargo nextest run -p quarto-core` → **2385 passed, 0 failed**. No
      auto-stretch/snapshot/crossref regressions from the reorder.

### Part 2 end-to-end verification (CLAUDE.md)

Invocation: `cargo run --bin q2 -- render examples/presentations/14-crossrefs/slides.qmd`

- **Before:** `Warning: unresolved crossref @fig-1 …` + `1 warning`.
- **After:** `Rendered 1 of 1 files … ` — **no warning.** Inspected `slides.html`:
  - body refs: `<a href="#fig-1" class="quarto-xref">Figure 1</a>` and
    `<a href="#fig-2" …>Figure 2</a>` (correctly numbered; was `?fig-1?` + "Figure 1").
  - fig-1 (`![A figure]{#fig-1}`): `<img … id="fig-1" class="r-stretch" />` direct
    child of `<section>`, caption `<p class="caption">A figure</p>`.
  - fig-2 (`::: {#fig-2}`): now ALSO `r-stretch` (the predicted side benefit — a
    div-form crossref figure on a single-image slide is stretched now that
    auto-stretch sees the rendered `Figure`), caption `<p class="caption">Figure 2:
    Another figure.</p>`.
  - `<figure>` count = 0 (both hoisted).

### Discovered (filed, out of scope here)

- **bd-uwv2eec2** — attribute-form figure `![cap](x){#fig-N}` gets **no "Figure N:"
  caption-number prefix** (the div form does). Pre-existing crossref-render
  asymmetry; reproduces in plain HTML (`<figcaption>A figure</figcaption>`), so it
  is **not** caused by the reorder. Fix belongs in `crossref_render.rs`.

## Part 1 sub-plan — bare-table caption desugar (bd-4ly7ne01) — DONE

`: caption {#tbl-…}` parses to a **bare `Block::Table`** with the id on the
table's own `attr` (verified via `pampa -t native`:
`Table ("tbl-bare",[],[]) (Caption Nothing [Plain …])`). The uniform
`classify_div`/`convert_div` path never saw it.

Fix (matches "all syntaxes desugar into divs"): a `maybe_wrap_bare_table_into_div`
step at the top of `FloatRefTargetSugarTransform::transform_block` wraps such a
table into `Div(#tbl-…) > Table` — id (and classes/kvs) move onto the Div, the
Table is left anonymous (no duplicate id) — so the existing `[Block::Table(_)]`
arm of `convert_div` handles it. Integrating it inside the sugar transform means
it runs everywhere the transform runs (full pipeline, analysis pipeline, fixtures)
with no new registration.

### Part 1 checklist

- [x] 1.1 Confirm AST shape (`pampa -t native`): id on bare `Block::Table`.
- [x] 1.2 Tests first: `fixture_bare_table_caption_target` in `crossref_fixtures.rs`
      — confirmed **RED** (`idx.get("tbl-bare")` = None).
- [x] 1.3 Implement `maybe_wrap_bare_table_into_div` in `float_ref_target.rs`.
- [x] 1.4 Fixture **GREEN**; index entry `("tbl-bare","tbl",vec![],1)`.
- [x] 1.5 End-to-end: rendered two bare-caption tables + `@tbl-data`/`@tbl-two`
      in HTML **and** revealjs — both resolve to `Table 1`/`Table 2`, no
      unresolved placeholders.
- [x] 1.6 Full `cargo nextest run -p quarto-core` → **2386 passed**.

## Part 3 outcome — interactive-preview crossref (bd-zecehtnc) — CLOSED (semantics done)

Investigation (2026-06-18) found the interactive preview already renders crossrefs
correctly, and Part 2 improved it:

- The q2-preview React renderer (`ts-packages/preview-renderer/src/q2-preview/`)
  has dedicated components for every crossref custom node — `custom/FloatRefTarget.tsx`,
  `CrossrefResolvedRef.tsx`, `Theorem.tsx`, `Proof.tsx`, `Equation.tsx` — registered
  in `registry.ts` and dispatched by `type_name` (`dispatchers.tsx:207`).
  `FloatRefTarget` composes "Figure N:" captions; `CrossrefResolvedRef` renders
  `<a class="quarto-xref" href="#id">{kind} {n}</a>`.
- **Numbering, links, captions, figure-vs-div, theorems/proofs/equations all work**
  — 53 `custom-components.integration.test.tsx` tests pass.
- **Part 2 also fixed preview's attribute-form figure crossref**: the
  `![cap](x){#fig-1}` figure is no longer destroyed by early auto-stretch, so it
  becomes a proper numbered `FloatRefTarget` custom node in preview too.
- **nbsp parity: already correct.** `CrossrefResolvedRef.tsx` already emits a
  non-breaking space between kind and number (byte-confirmed: `…7d c2 a0 24…`);
  the "regular space" I first reported was a terminal-rendering artifact. No fix.

Decision (user): **close bd-zecehtnc as semantics-done.** The only remaining gap
is *visual* revealjs auto-stretch parity for crossref figures in preview (a
single-image crossref figure fills the slide in `render` but shows at natural size
in `preview`, because the Rust auto-stretch transform skips `Block::Custom` and
`FloatRefTarget.tsx` adds no `r-stretch`). Deferred to **bd-hbloemff** (recommended
approach there: make `RevealAutoStretchTransform` CustomNode-aware + have
`FloatRefTarget.tsx` honor the mark).

## Reference: key files

- Pipeline order: `crates/quarto-core/src/pipeline.rs:1078-1309`
  (`build_transform_pipeline`), `:1348-1375` (`Q2_PREVIEW_TRANSFORM_EXCLUDED`),
  `:1070` (`footer_render_stage` precedent).
- Auto-stretch: `crates/quarto-core/src/revealjs/auto_stretch.rs` (esp.
  `hoist_figure`, doc lines 44–67 on the crossref divergence).
- Float sugar: `crates/quarto-core/src/transforms/float_ref_target.rs`
  (`classify_div`/`classify_fig`; Table handling at the `[Block::Table(_)]` arm).
- Crossref transforms: `crates/quarto-core/src/transforms/crossref_{index,resolve,render}.rs`.
- Crossref design epic: `claude-notes/plans/2026-04-15-crossref-design.md` (bd-jsbg).
- Q1 references: `external-sources/quarto-cli/src/resources/filters/quarto-pre/parsefiguredivs.lua`
  (table-caption float wrapping), `.../format-reveal.ts` `applyStretch`.
