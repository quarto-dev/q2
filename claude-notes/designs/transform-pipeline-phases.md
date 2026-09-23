# Transform pipeline phases & the format-agnostic-first invariant

**Status:** Implemented under bd-w0c6d38k (2026-06-18). `TransformPhase` and the
defaulted `phase()` method live on `AstTransform` (`crates/quarto-core/src/transform.rs`);
all render-pipeline transforms are classified; the invariant is enforced by
`test_build_transform_pipeline_phase_ordering` in `pipeline.rs`. The first
consumer of the seam is the revealjs auto-stretch move (auto-stretch now runs in
`Finalization`, after `crossref-render`).

**Audience:** anyone adding or reordering an `AstTransform` in
`build_transform_pipeline` (`crates/quarto-core/src/pipeline.rs`), and especially
anyone adding a new output format (`dashboard`, `typst`, `pdf`, …) with its own
format-specific transforms.

---

## Why this exists

Q2 renders every output format through **one** transform pipeline
(`build_transform_pipeline`, `pipeline.rs:1078`), with format-specific transforms
spliced in at chosen positions (today via inline `if is_revealjs { … }`). The
pipeline mixes two fundamentally different kinds of work:

1. **Format-agnostic semantic normalization** — establishing *what the document
   means*: callouts, theorems, proofs, floats (Figure/Table), equations, and the
   crossref index/resolve that numbers them and rewrites `@refs`.
2. **Format-specific presentation** — reshaping the resolved document for *one*
   output: revealjs slide construction, single-image auto-stretch, etc.

These have a hard dependency direction: **presentation depends on semantics, not
the other way around.** A revealjs transform that hoists a `Figure` to a bare
`<img>` is only correct *after* that `Figure` has been recognized, numbered, and
resolved as a crossref float.

When that direction is violated, the failure is silent and format-specific. The
motivating incident (bd-w0c6d38k): `RevealAutoStretchTransform` (`pipeline.rs:1148`)
ran *before* `FloatRefTargetSugarTransform` (`:1166`) and the crossref phase
(`:1170`). On a single-image slide it hoisted `![cap](x){#fig-1}` (a Pandoc
`Figure`) into a bare `Plain[Image]` before the float was ever registered — so
`@fig-1` rendered unresolved and a sibling figure was mis-numbered. Every test
passed; only `format: revealjs` was affected, and only for that figure syntax.

---

## The phase model

Every transform belongs to exactly one phase. Phases are **totally ordered**;
their order is the contract.

| rank | `TransformPhase` | purpose | examples |
|------|------------------|---------|----------|
| 1 | `Normalization` | format-agnostic semantic sugar **and** format-specific *structural scaffolding* that does not consume crossref semantics | `callout`, `theorem-sugar`, `proof-sugar`, `float-ref-target-sugar`, `equation-label`, `metadata-normalize`, `footnotes`, `example-embed`; **`reveal-columns`, `reveal-slides`, `reveal-footnotes`, `reveal-footer-alias`** |
| 2 | `Crossref` | assign section-scoped numbers; rewrite `@refs` to custom nodes | `crossref-index`, `crossref-resolve` |
| 3 | `Navigation` | TOC / navbar / sidebar / page-nav / footer / listings generate + render | `toc-*`, `navbar-*`, `sidebar-*`, `footer-generate`, `listing-*` |
| 4 | `Finalization` | render custom nodes to writer-visible shapes, then format presentation that *consumes* those shapes, then resource/attribution baking | `link-rewrite`, `appendix-structure`, **`crossref-render`**, **`reveal-auto-stretch`** (after the move), `code-block-render`, `resource-collector`, `attribution-render` |

Two subtleties this model encodes:

- **Crossref spans two phases.** Index + resolve happen in `Crossref` (rank 2);
  the conversion of crossref custom nodes into real `Figure`/`Div`/`Span`/`Link`
  shapes happens in `crossref-render` (`pipeline.rs:1259`), which is `Finalization`
  (rank 4). This is *why* a presentation transform that needs the final `Figure`
  (auto-stretch) belongs in `Finalization`, after `crossref-render` — not in
  `Crossref`.
- **Not all format-specific transforms are late.** `reveal-slides`/`reveal-columns`
  build the `<section>` scaffolding from raw blocks and metadata; they do **not**
  read or mutate floats/captions/numbers, so they correctly live in
  `Normalization`. The distinction is *what a transform consumes*, not merely that
  it is format-specific (see the author rule below).

---

## The invariant

> **Format-agnostic semantic structure is fully established before any transform
> that consumes it.** Concretely, in the built pipeline the transforms' phase
> ranks are **non-decreasing by position** (a transform never precedes one of a
> lower rank).

Because `crossref-render` is `Finalization` and a presentation transform that
hoists a crossref float is also `Finalization`, the rank order alone forbids the
bug: a `Finalization` transform cannot sit among `Normalization` transforms ahead
of the `Crossref` phase.

### Author rule (which phase do I pick?)

When adding a format-specific transform, ask: **does it read or mutate a structure
produced by the Crossref phase — a float, a caption, a number, or a resolved
`@ref`?**

- **Yes** → phase `Finalization`, and it must run *after* `crossref-render`. (This
  is auto-stretch.)
- **No, it only builds format scaffolding from raw blocks/metadata** → phase
  `Normalization` is acceptable. (This is `reveal-slides`.)

If unsure, choose `Finalization`. Running a presentation transform too late is at
worst a missed optimization; running it too early silently corrupts semantics.

---

## Enforcement

### `phase()` on the `AstTransform` trait

`AstTransform` (`crates/quarto-core/src/transform.rs:69`) gains a method:

```rust
fn phase(&self) -> TransformPhase { TransformPhase::Unclassified }
```

It **defaults to `Unclassified`** rather than being required-with-no-default. The
reason is honesty: a handful of `AstTransform` impls are *not* part of the ordered
multi-format render pipeline — engine-stage transforms (`JupyterTransform`), the
stage-level `AttributionGenerateTransform`, and in-module test doubles. Forcing
them to fabricate a render-pipeline phase would be a lie. Instead, `Unclassified`
is the honest default for "not a member of the ordered pipeline," and **every
transform that actually appears in `build_transform_pipeline` overrides it** with
a real phase. The anti-drift guarantee is preserved by the test, not the compiler:
the ordering test **fails if any transform in the built pipeline reports
`Unclassified`** (exhaustiveness), so a new pipeline transform that forgets to
classify itself is caught loudly. This mirrors the existing deny-list-name
validator (`pipeline.rs:1344`), which is likewise a test, not a compiler check.

Cost: one extra vtable slot per transform *type* (a fn pointer in static rodata);
**zero per-instance memory and zero render-time cost** — `phase()` is called only
by the ordering test, never on the render hot path. (`is_format_specific()` may be
added later as a sibling method if a real consumer appears; the ordering invariant
needs only `phase()`.)

`TransformPhase` derives `PartialOrd`/`Ord` from declaration order; `Unclassified`
is declared **last** so a leaked value sorts high, though the exhaustiveness check
rejects it before the monotonicity comparison runs.

### The ordering-invariant test

A test over the **real** `build_transform_pipeline` (the analogue of the existing
*analysis*-pipeline test, `pipeline.rs:1851`) that:

1. builds the pipeline for **each supported format string** (`html`, `revealjs`,
   and `dashboard`/`typst`/`pdf` as they land — a list, not an `is_revealjs`
   branch, so new formats are covered without new assertions);
2. reads each transform's `phase()`;
3. asserts ranks are **non-decreasing by position**;
4. on failure, names the offending transform and its neighbours.

This test **fails on today's code** (auto-stretch is `Finalization`-rank sitting
before the `Crossref` phase) and **passes after auto-stretch moves** past
`crossref-render` — so it is simultaneously the red/green TDD test for the
structural fix and the permanent guardrail. It is format-neutral by construction:
it never mentions `revealjs`, only phases.

---

## The post-filter presentation slot (stages, not transforms)

Every Finalization transform runs inside `AstTransformsStage`, which the
HTML stage list places **between** `UserFiltersStage::pre` and
`UserFiltersStage::post`. So a Finalization transform, however late, runs
*before* any user post filter. That is the right place for most
presentation work, but not for a step whose intermediate representation is
part of the filter-author contract.

The concrete case is equation numbering (bd-vlhi2zkj). `crossref-render`
numbers an equation but does not choose how the number is typeset: `\tag{N}`
is an `amsmath` command only MathJax and KaTeX read, so a MathML converter
(or no engine at all) needs ` \qquad(N)` or a label outside the math. The
transform records the number on the reserved span attribute
`quarto-eq-number` and leaves the TeX alone; `EquationNumberStage` picks the
encoding from the format and `html-math-method` and removes the attribute.
Had that stage been a Finalization transform, a Lua post filter could never
see the attribute: it would already be gone. As a stage after
`UserFiltersStage::post` it can be read, rewritten or deleted from Lua
(`el.attributes["quarto-eq-number"]`), which is the escape hatch we want.

**Rule:** a format-specific step that consumes crossref output belongs in
`Finalization` (the author rule above) **unless** its input is a reserved
attribute or node that user post filters are meant to see; then it is a
*stage* between `UserFiltersStage::post` and `RenderHtmlBodyStage`, next to
`CodeHighlightStage` (AST-level annotation, same slot, same reason) and
`EquationNumberStage`. Such a stage still satisfies the invariant's intent:
it runs after every transform, so all crossref structure is final. It is
not covered by the transform ordering test; document the placement at the
`stages.push` site and cover it with an end-to-end test that runs a post
filter against the attribute (see
`crates/quarto-core/tests/integration/equation_numbering_pipeline.rs`).

---

## The preview-pipeline shape contract (the anti-recurrence rule)

The bug had a deeper enabler worth stating as its own rule.

The preview pipeline (`build_q2_preview_transform_pipeline`, `pipeline.rs:1387`) is
**the render pipeline minus a deny-list** (`Q2_PREVIEW_TRANSFORM_EXCLUDED`,
`:1348`), which removes `"crossref-render"` (`:1374`) so crossref custom nodes
survive for React. That subtraction means a crossref float has **different
concrete AST shapes in the two pipelines** after the crossref phase: a real
`Figure` in native render, a `CustomNode("FloatRefTarget")` in preview. Faced with
that divergence, the original author placed auto-stretch *before* the sugar
transforms — the only point where both pipelines still show a plain `Figure` — and
accepted not stretching crossref figures. That early placement is what created the
ordering inversion.

**Rule:** *removing (or adding) a transform in the preview deny-list must not
create a post-crossref AST-shape divergence that forces a presentation transform
to run earlier in the render pipeline.* If preview and render must differ in
post-crossref shape, the presentation transform must handle **both** shapes (e.g.
recognize the custom-node form), or the preview must be brought back onto the
common shape — never the reverse, where the render pipeline contorts to match
preview's reduced shape. Tracking the preview's own crossref story is bd-zecehtnc.

---

## Worked example: the auto-stretch inversion

| | before (buggy) | after (fixed) |
|---|---|---|
| `reveal-auto-stretch` declared phase | `Finalization` | `Finalization` |
| its position | `pipeline.rs:1148`, **before** `Crossref` (`:1170`) | after `crossref-render` (`:1259`) |
| `![cap](x){#fig-1}` at that point | a plain `Figure` → hoisted to bare `<img>`; never registered as a float | a fully-rendered `Figure(id=fig-1)` with caption "Figure 1: …" → hoisted to `section > img.r-stretch` + `<p class="caption">` |
| ordering test | **fails** (rank 4 before rank 2) | **passes** (ranks non-decreasing) |
| `@fig-1` in output | unresolved `?fig-1?` | `Figure 1` |

---

## References

- Pipeline: `crates/quarto-core/src/pipeline.rs` — `build_transform_pipeline`
  (`:1078`), phase headers (`NORMALIZATION` `:1090`, `CROSSREF` `:1169`,
  `NAVIGATION` `:1173`, `FINALIZATION` `:1249`), `crossref-render` (`:1259`),
  `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`:1348`), preview builder (`:1387`), existing
  analysis-pipeline ordering test (`:1851`), `footer_render_stage` format seam
  (`:1070`).
- Trait: `crates/quarto-core/src/transform.rs:69` (`AstTransform`).
- Auto-stretch transform + its ordering rationale: `crates/quarto-core/src/revealjs/auto_stretch.rs`.
- Plan: `claude-notes/plans/2026-06-18-revealjs-crossref.md` (bd-w0c6d38k);
  follow-ups bd-4ly7ne01 (bare-table desugar), bd-zecehtnc (preview crossref).
- Crossref design epic: `claude-notes/plans/2026-04-15-crossref-design.md` (bd-jsbg).
