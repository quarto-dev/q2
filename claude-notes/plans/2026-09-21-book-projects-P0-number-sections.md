# Plan: Number-sections / `@sec-` foundation (book-projects P0)

**Date:** 2026-09-21
**Epic:** [`2026-09-21-book-projects-epic.md`](2026-09-21-book-projects-epic.md)
**Design (authoritative):** [`../designs/book-projects-architecture.md`](../designs/book-projects-architecture.md) §10
**Tracks:** `bd-5aklrxgi` ("Q2 has no number-sections / @sec- header-numbering implementation for any format") — scoped narrowly here to what book mode needs, not the strand's full original scope.

## Overview

`bd-5aklrxgi`'s own investigation (2026-09-17) found the hard part already built: `CrossrefIndexTransform` (`crates/quarto-core/src/transforms/crossref_index.rs`) already tracks a section-counter stack (`advance_sections`, driven by `Header` blocks) and already collects every heading into `CrossrefIndex.headings` — with a comment anticipating exactly this future need ("kept for cross-file heading link fixup in book mode"). What's missing is narrow:

1. Headers are never registered as `sec`-typed crossref *targets* — `visit_header` pushes to `headings` but never calls the indexing path a figure/table/theorem goes through.
2. `CrossrefResolveTransform` has zero handling for a `sec`-typed `@ref` — `@sec-intro` doesn't resolve at all today.
3. Q2's native HTML writer has no equivalent of Q1's `sections.lua` visible-number injection ("1.2 Section Title") or the "Chapter N" presentation for a level-1 heading.
4. The section counter has no way to be *seeded* with a non-zero starting value — needed by book-projects P4 for multi-file chapter numbering.

This plan builds only what book mode (P1–P7) actually needs. **Explicitly not building** (remains `bd-5aklrxgi`'s open scope if anyone picks it up later):

- Any Pandoc-hybrid-target work. Typst has its own independent `section-numbering` metadata passthrough (already landed); LaTeX/EPUB get deferred, compiler/Lua-native resolution (design doc §6) via the already-vendored `sections.lua`/`book-numbering.lua`. This plan is Q2's *native HTML writer* only.
- `number-depth` support beyond whatever depth books actually exercise — no attempt at full parity with every Q1 `number-sections`/`number-depth` option combination.
- Any non-book single-document use case beyond "must not regress" — this plan doesn't make single-document `number-sections: true` a fully-supported, independently-tested feature; it happens to also work there as a side effect of building what books need, but that's not this plan's acceptance bar.
- Cross-reference presentation options (`title-delim`, custom prefixes, etc.) beyond "Section N.M" / "Chapter N" — tracked separately as `bd-wqdi1pd2`, unrelated to this plan.

## Decisions

- Register headers as crossref targets using a path *analogous to, not literally shared with*, `index_custom_target` — corrected by an implementability review: `index_custom_target(&mut self, node: &mut CustomNode)` operates on `CustomNode.plain_data`/`.attr`, but `Header` (`quarto-pandoc-types/src/block.rs`) is a distinct struct with no `plain_data` field at all, so `visit_header` cannot call it directly. Extract a shared private helper (duplicate-id check, `next_order` increment, `CrossrefEntry` construction, index insert) parameterized on `(identifier, ref_type, caption, source_info)` rather than on `&mut CustomNode`, and have both `index_custom_target` and the new header-registration path call it. `Order.section` (already computed) becomes the header's own number either way — no parallel numbering system, just no literal function reuse.
- `@sec-` resolution presentation: a level-1 heading resolves as "Chapter N" (bare, no numeric section address shown) when `crossref.chapters: true`; a deeper heading resolves as "Section N.M" — ported from `sections.lua`'s `numberOption`/chapter-prefix logic, verified against the `scratchpad/minibook` experiment's real output (`See <a href="chapter1.html">Chapter 1</a>`).
- **Appendix presentation is a real, previously-missing piece of this same Decision, not an extra feature.** `sections.lua` itself reads `currentFileMetadataState().appendix` (a per-chapter fact) to decide "Appendix" vs "Chapter" prefixing *and* letter-vs-numeral formatting for the section number itself (`A` not `1`) — this is baked into Q1's own generic section-numbering machinery, not layered on top of it separately. Q2 needs the equivalent: an `is_appendix: bool` fact reaching `CrossrefResolveTransform`/`CrossrefRenderTransform` via `RenderContext.chapter_seed` — **this phase builds that field and its `ChapterSeed`/`with_chapter_seed` mechanism** (see the Implementation checklist below; P4 later wires *real* per-chapter values into it via `RenderToFileOptions`/`Pass2Renderer::render_batch*`, but the `RenderContext`-level plumbing itself belongs here, since P0 runs before P4 and needs it self-contained for its own tests), plus a letter-formatting branch alongside the existing numeral formatting. Without this, the "Appendix A"-shaped `@sec-` test below has no implementation task backing it — add one explicitly, don't leave it implied.
- Visible number injection into the rendered heading itself ("1.2 A subsection") is real, new work for Q2's native HTML writer — port `sections.lua`'s content-prepend logic, not Pandoc's own (nonexistent, for Q2) internal numbering.
- `CrossrefIndex.sections` gains a seed capability (constructor parameter or a `seed_sections()` call before the transform runs), read from `RenderContext` — the field itself already exists and needs no shape change, only a way to start non-empty.
- Port `sections.lua`'s "skip `.unnumbered`" early-return: a heading carrying the `.unnumbered` class must not advance `CrossrefIndex.sections`, must not be registered as a `sec`-typed target with a numeric address, and — for a level-1 heading specifically — must not consume a book chapter-number slot. This is one rule, not book-specific logic layered on top of a separate one; P4/P5 depend on it holding for the general (non-book) case too, since a book chapter's own H1 is just an ordinary `Header` as far as this transform is concerned.
- Scope check for every item: does book-projects P2 or P4 need it? If not, defer.

## Checklist

### Baseline repair (pre-work)
- [x] Fix pre-existing `pandoc_goldens::test_docx_and_html_legs_agree_on_numbers` failure: native HTML `prefix_caption` and the uncaptioned label-only caption emitted `"Figure 1: "` with an ASCII space where Q1's `titlePrefix` (and the docx leg) use NBSP (`nbspString()`, format.lua). Fixed both sites to `{kind}\u{a0}{n}`; 1 snapshot updated (`integration__llms_txt__llms_companion_rich_content.snap`, one line). Branch baseline before: 4816 run / 4815 passed / 1 failed; after: 4816/4816. (commit `9e71d2c32`)

### Tests first
- [x] Unit test: a document with `number-sections: true` and two H1/H2 headings gets correct header-section-numbers (1, 1.1, 2) via the existing counter, once headers are registered as targets. (Task 1's `headers_register_as_sec_targets` pins exactly this: order.section [1], [1,1], [2].)
- [x] Unit test: `@sec-<id>` on a level-2 heading resolves to a link with "Section 1.2"-shaped text. **(Amendment A: in a doc whose shallowest heading is H1; no `chapters` key needed — see `maxHeading` semantics below. Pin that: non-chapters + H1-led → "Section 1.2".)** (Task 3: `sec_ref_on_level_2_heading_resolves_to_section_number` — "Section\u{a0}1.2", NBSP included.)
- [x] Unit test: `@sec-<id>` on a level-1 heading, with `crossref.chapters: true` set, resolves to "Chapter 1" (no numeric form) — mirrors the real `quarto render` output observed in the design conversation's experiment. (Task 3: `chapter_ref_resolves_when_chapters_enabled` — "Chapter\u{a0}2" for the second H1.)
- [x] Unit test: seeding `CrossrefIndex` with `sections = vec![1]` before the walk makes the first H1 become section `2` — confirms the seed contract P4 will rely on, independent of any book-specific code. (`chapter_seed_offsets_section_numbering`; seeded via `with_chapter_seed`, not by poking the field)
- [x] Unit test: a level-1 `.unnumbered` heading does not advance `sections` at all (the next real heading's number is unaffected by the unnumbered one's presence), and is not registered as a `sec` target with a numeric address. (`unnumbered_header_skips_numbering_but_is_recorded`)
- [x] Unit test: a level-1 `.unnumbered` heading **still gets pushed onto `CrossrefIndex.headings`** — this is `sections.lua`'s actual order of operations (`indexAddHeading` runs *before* its own "skip unnumbered" early-return, unconditionally for every header), and P4/P5's cross-file link fixup depends on finding *every* chapter's heading there, including an unnumbered Preface/Introduction — one of the most common real book shapes. Guard this explicitly so "skip .unnumbered" isn't misimplemented as "skip the whole visit for .unnumbered," which would silently break cross-chapter links to any unnumbered chapter.
- [x] Unit test: `@sec-<id>` on an *appendix* level-1 heading resolves to "Appendix A"-shaped text (not just "Chapter N" for an ordinary chapter) — mirrors `sections.lua`'s own appendix-title/delim handling (lines ~54-76: `appendix-title`/`appendix-delim` crossref options), not currently covered by any test here. (Task 3: `appendix_chapter_ref_resolves_to_letter` — "Appendix\u{a0}A" under an appendix seed; the visible-heading appendix-delim shape is the number-injection item below.)
- [x] Regression test: an existing single-document HTML render with no `number-sections` set is byte-identical to before (no accidental default-on behavior). (Task 4: `sectionized_no_number_sections_no_injection` in `crossref_fixtures.rs` — through the real sectionized pipeline; no `number` kv, no injected span, `@sec-` refs still resolve since registration is unconditional.)
- [x] **(Amendment A)** Pipeline-shape test including `SectionizeTransform` in the chain (the real native-HTML pipeline shape, which `run_crossref_rendered` currently omits): a number-sections doc renders headers with `data-number="1.1"` and `<span class="header-section-number">1.1</span>`, and nested `@sec-` refs resolve in document order (`sec-intro`→"Section 1", `sec-deep`→"Section 1.1"). This is the test whose absence let the plan's original premise (headers have ids at index time) survive review — in the real pipeline `SectionizeTransform` has moved the ids onto `Div.section` wrappers by the time `CrossrefIndexTransform` runs. (Task 4: `sectionized_number_sections_shape_and_nested_sec_refs`, via new helper `run_crossref_rendered_sectionized`.)
- [x] **(Amendment A)** Update `section_ref_target_is_not_float_wrapped` (crossref_render.rs:1962): its assertions stay (they now hold for a stronger reason), its comment flips — with `.section` divs excluded from float sugaring, section divs *never* become `FloatRefTarget` nodes. (Task 2: comment flipped; assertions untouched and green.)
- [x] **(Amendment A)** Unit test: an H2-led document (no H1, no `chapters`) numbers sections relative to the shallowest heading level — first H2 is "1", its first H3 child is "1.1" — port of Q1's `maxHeading` behavior (normalize/flags.lua: `crossref.maxHeading = min(maxHeading, el.level)`). (Task 3: `h2_led_document_numbers_sections_relatively` + `max_heading_tracks_shallowest_header_level` + `section_number` unit tests.)

### Implementation
- [x] Wire header registration into `CrossrefIndexTransform::visit_header` (bare-header shape landed; the sectionized-pipeline shape is the Amendment A id-recovery item below) (register as `ref_type: "sec"`, alongside the existing `headings` push — not replacing it, `headings` is still needed for P4/P5's cross-file link fixup), gated on the heading not carrying `.unnumbered`. **The `headings` push itself is not gated on `.unnumbered`** — match `sections.lua`'s real order (index unconditionally, *then* early-return on `.unnumbered` before the counter/registration logic) exactly, not "skip everything for an unnumbered heading." **(Amendment A: registration is *not* gated on `number-sections` — Q1 registers sec targets unconditionally, so `@sec-` refs resolve to numbers even in non-numbered docs. Registration *is* gated on `ctx.format.identifier.is_html_based()` — `crossref-index` also runs in the pandoc-hybrid pipeline, where registering would let resolve consume `@sec-` cites before the vendored `refs.lua` sees them.)** Also landed: the shared `index_target` helper extracted from `index_custom_target` per the Decisions section.
- [x] **(Amendment A)** Exclude `.section` divs from float sugaring: `classify_div` in `float_ref_target.rs` returns early for divs carrying the `section` class. A section is not a float; the `sec` prefix match on sectionized divs was accidental and is what produces today's bottom-up, nesting-polluted, caption-stealing `@sec-` resolution. (Bare `::: {#sec-x}` divs *without* the `section` class keep today's behavior — conservative; Q1's treatment of that shape is its own question, not P0's.) (Task 2: early-return landed; pinned by `section_div_is_not_sugared`, `floats_inside_section_divs_still_sugar`, `bare_sec_div_without_section_class_still_sugars`.)
- [x] **(Amendment A)** Section-div id recovery in the index walker: `CrossrefIndexTransform` keeps a stack of enclosing `Div.section` ids; `visit_header` (which must now take `&mut Header` for the number-attr stash) uses the header's own id when non-empty (pandoc-hybrid pipeline, where sectionize doesn't run; blockquoted headers, which sectionize doesn't descend into) and otherwise the innermost enclosing section div's id. Sound because sectionize guarantees every header it touches becomes the first child of its own section div with an empty id. (Task 2: `Walker.section_ids` stack + `visit_header` fallback landed; pinned by `sectionized_headers_register_with_section_div_ids` — intro=[1], deep=[1,1], next=[2] in document order across nesting.)
- [x] Skip `.unnumbered` headings in `advance_sections`/`visit_header`: no counter advance, no `sec` registration — but keep the unconditional `headings` push (previous bullet).
- [x] **(Amendment A)** Port Q1's `maxHeading`: before the walk, pre-scan the AST for the minimum header level (cap 7; force 1 when `crossref.chapters: true`), store on the index/transform. Port `sectionNumber`/`formatChapterIndex` as a Rust `format_section_number(path, max_heading, is_appendix)`: when `max_heading == 1` the top component is formatted (letter when appendix — via the seeded appendix-local chapter number, equivalent to Q1's `file.bookItemNumber`; `chapters-alpha` deferred), deeper components appended dot-separated, trailing zero components trimmed. When `max_heading > 1` the components above it are always zero and drop out. (Empirically verified against Q1 2026-09-23: non-chapters H1-led doc renders `data-number="1.1"` and `Section&nbsp;1.2`; the top component is *not* dropped — the `num=""` start in `sectionNumber` is compensated by `maxHeading`.) (Task 3: `compute_max_heading` pre-scan in the index transform → `CrossrefIndex.max_heading` (serde-defaulted for trace compat); `crossref/section_number.rs` ports both Lua fns literally, unnumbered headers included in the scan per flags.lua.)
- [x] Implement `sec`-ref presentation in `CrossrefResolveTransform`/`CrossrefRenderTransform`: "Section N.M" default, "Chapter N" when the target is a level-1 heading (order.section path length 1 ⟺ Q1's `isChapterRef` for header targets) and `crossref.chapters: true`. Resolve must pass the entry's `in_appendix` through `plain_data` for the render side. (Task 3: `in_appendix` joins `order` in resolved plain_data (schema artifact + conformance test updated); `sec_ref_text` in crossref_render does the ch/apx prefix swap via `terms.crossref_prefix`, hardcoded English fallback when terms is None; NBSP between kind and number throughout.)
- [x] Implement appendix presentation: read `is_appendix` from `RenderContext.chapter_seed` in the presentation code, producing "Appendix A" (letter, not numeral) for a level-1 appendix heading and letter-formatted section addresses for deeper appendix headings (`A.M`, not `N.M`) — this is what the "Appendix A"-shaped test above actually exercises; without this item it's untestable. (Task 3: via the seed → entry.in_appendix → plain_data chain; `format_section_number` letter branch pinned for "A.M" deeper addresses.)
- [x] Implement visible number injection for Q2's native HTML writer when `number-sections: true` (port of `sections.lua`'s content-prepend, not a Pandoc CLI flag — Q2's native path has no Pandoc involved). **(Amendment A, ported exactly from sections.lua: (a) at index time, stash `number` kv on the header — `visit_header` writes `header.attr.2["number"]` — gated on `is_html_based() && number-sections && level <= number-depth` (`number-depth` meta, default 6), which the writer then emits as `data-number` for free, matching Q1's markup; (b) at render time (`CrossrefRenderTransform`'s Header case, Finalization), if the kv is present prepend `Span(Str(number), class="header-section-number")` + `Space` to the header content; for a level-1 appendix heading the final content order is `[Str("Appendix"), Space, Span(number), Str(" —"), Space, ...]` (`appendix-title`/`appendix-delim` crossref options with those defaults). Injection keys off the kv's presence, so it is automatically inert wherever the stash didn't run.)** (Task 4: index-side stash in `crossref_index.rs`'s `visit_header` (`number_sections`/`number_depth` threaded into `Walker`, read once in `transform()`); render-side `inject_header_number` in `crossref_render.rs`, gated on `FloatState.appendix` (mirrors `chapters`/`max_heading`) for the appendix shape. Pinned by unit tests `number_attr_stashed_only_with_number_sections`, `number_attr_skips_unnumbered_and_non_html` (index), `header_section_number_injected_when_stashed`, `appendix_level1_header_gets_appendix_title_shape`, `no_injection_without_number_sections` (render), plus the sectionized-pipeline integration tests above.)
- [x] **This phase owns building the `ChapterSeed`/`RenderContext` seeding mechanism itself, not just consuming it** (sequencing note: P0 runs before P4 in this epic's dependency order, so P0 cannot depend on plumbing P4 introduces — P0 must be the one to introduce it, self-contained and independently testable, with P4 as its first real consumer). Concretely: a new `ChapterSeed { chapter_number: u32, is_appendix: bool }` type (the "no seed for this chapter" case is represented by `RenderContext.chapter_seed`/the caller's map lacking an entry at all — `chapter_number` itself doesn't need to be `Option`), a new `RenderContext.chapter_seed: Option<ChapterSeed>` field, and a `RenderContext::with_chapter_seed(mut self, seed: ChapterSeed) -> Self` builder (matching the existing `with_project_index`/`with_resource_resolver`/`with_options` pattern) — plus the `CrossrefIndex` seeding capability itself, reading `chapter_seed.chapter_number` to seed `sections = vec![n - 1]` before the transform's walk begins. This phase's own tests construct `RenderContext` directly via `.with_chapter_seed(...)` and need nothing from `render_document_to_file`/`Pass2Renderer` — P4's job (see its plan) is purely wiring *real* per-chapter values into this already-built mechanism via `RenderToFileOptions`/`Pass2Renderer::render_batch*`, not building the mechanism a second time. **(Amendment A: scaffolding landed in commit `615d62237` — `ChapterSeed`, the field, and the builder exist; remaining here is the `CrossrefIndex` seeding read — landed with Task 1: `transform()` seeds `sections = [n-1]` from `chapter_seed.chapter_number`, guarded on an untouched stack.)**
- [x] Wire the existing-but-currently-unused `CrossrefEntry.in_appendix: bool` field (hardcoded `false` today, comment marks it "deferred") via this phase's `is_appendix`/`ChapterSeed` mechanism, instead of leaving it dead — found by a review pass; neither this plan nor P4's originally mentioned it. (Task 1: `index_target` sets `in_appendix` from the per-file seed for *all* entry kinds, matching Q1's `currentFileMetadataState().appendix`. Passing it through resolve's `plain_data` to the render side is part of the presentation item above.)
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; phase-boundary `cargo nextest run --workspace` with delta reported against the current baseline. Per-crate: clean clippy, `quarto-core` 4835/4835/31 → 4842/4842/31 (+7: the new tests above). Phase-boundary workspace run first went red at 5,243/14,422 tests in (fail-fast): `quarto::integration conditional_content_cli::hidden_float_does_not_consume_a_crossref_number`. Per the global CLAUDE.md phase-boundary rule, attributed via full-phase context rather than bisecting blindly: reproduced at commit `9e71d2c32` (the pre-P0 NBSP baseline-repair, before Task 1 even started) with a disposable worktree, confirming it predates every P0 commit. A `--no-fail-fast` full run then surfaced two more failures of the same shape (`smoke_all` — 3 fixtures — and `render_pandoc_formats_e2e::e2e_docx_without_pandoc_on_path`), both likewise confirmed pre-existing via git ancestry. All four are stale test expectations (ASCII space vs. the NBSP `9e71d2c32` introduced; a since-renumbered `Q-18-1`→`Q-20-1` error code) — filed and fixed as `bd-hfbvnah5` (out-of-plan per the STRICT beads rule), not part of this plan's checklist. Final workspace run: **14,422/14,422 passed, 200 skipped, 0 failed.**
- [x] End-to-end: `cargo run --bin q2 -- render` a number-sections fixture (the `/tmp/p0-q2-probe/probe.qmd` doc from the investigation works) and inspect the HTML: `data-number` attrs, `header-section-number` spans, correct "Section 1"/"Section 1.1" ref text, `.unnumbered` untouched. (Task 5: rendered a fresh probe — `number-sections: true`, no `chapters` — via `cargo run --bin q2 -- render <probe.qmd>`, inspected the output HTML directly. Confirmed: `<h1 data-number="1"><span class="header-section-number">1</span> Intro</h1>`, `<h2 data-number="1.1"><span class="header-section-number">1.1</span> Details</h2>`, `@sec-intro`/`@sec-deep` resolve to `Section 1`/`Section 1.1` respectively, and the `.unnumbered` "Unnumbered" heading gets no `data-number`/span and keeps its `unnumbered` class. `@sec-unnum` correctly reports unresolved — `.unnumbered` headers are deliberately not registered as `sec` targets, per the Implementation checklist above.)

### Explicitly out of scope (found during Amendment A investigation)
- Per-chapter float counter resets in chapters mode (Q1's `indexNextChapter` resetting type counters at each level-1 heading). Q2's `next_order` never resets today; that is pre-existing behavior affecting float numbering, not `sec` targets. P4 owns chapter-aware float numbering.
- Q1's `data-anchor-id` attribute on sectionized headers (present in Q1's HTML output, origin outside the crossref filters). Not needed for numbering; separate parity question.
- Treatment of bare `::: {#sec-x}` divs without the `section` class (keeps today's float-path behavior; see implementation item above).
- `crossref.maxHeading`'s other consumer (float number truncation in `formatNumberOption`) — float display is untouched by this phase.

## Details

This phase touches only Q2's native HTML crossref pipeline. It does not touch `feature/pandoc-writer-hybrid`'s Pandoc-hybrid formats at all — confirmed in the design conversation that Typst and EPUB already have working `@sec-`/number-sections behavior via their own independent mechanisms (Typst's native `section-numbering` metadata passthrough; EPUB via the already-vendored, unsuppressed `sections.lua`). This plan exists purely to unblock P4 (multi-file HTML book mode)'s "Section N.M"/"Chapter N" presentation, which Q2's native writer cannot produce today for *any* document, book or not.

## Task 5 end-to-end verification (2026-09-23)

Invocation: `cargo run --bin q2 -- render <probe.qmd>` against:

```yaml
---
title: Probe
number-sections: true
---

# Intro {#sec-intro}

See @sec-intro and @sec-deep and @sec-unnum.

## Details {#sec-deep}

# Unnumbered {#sec-unnum .unnumbered}
```

Output HTML (`probe.html`), body content:

```html
<section id="sec-intro" class="section level1">
<h1 data-number="1"><span class="header-section-number">1</span> Intro</h1>
<p>See <a href="#sec-intro" class="quarto-xref">Section 1</a> and <a href="#sec-deep" class="quarto-xref">Section 1.1</a> and <a href="#sec-unnum" class="quarto-xref quarto-unresolved-ref">?sec-unnum?</a>.</p>
<section id="sec-deep" class="section level2">
<h2 data-number="1.1"><span class="header-section-number">1.1</span> Details</h2>
</section>
</section>
<section id="sec-unnum" class="section level1 unnumbered">
<h1 class="unnumbered">Unnumbered</h1>
</section>
```

Inspected directly: `data-number`/`header-section-number` span present on
both numbered headers with correct dotted numbering; `@sec-intro` and
`@sec-deep` resolve to "Section 1"/"Section 1.1" respectively; the
`.unnumbered` heading has neither attribute and keeps its class.
`@sec-unnum` reports unresolved (`Warning: unresolved crossref
@sec-unnum`) — expected, since `.unnumbered` headers are deliberately not
registered as `sec` targets.

## Amendment A (2026-09-23, approved by Gordon): the real pipeline shape

The plan's original premise — "headers are collected but never registered as
targets; register them in `visit_header`" — assumed headers still carry their
ids when `CrossrefIndexTransform` runs. **They don't, in the native HTML
pipeline.** Verified empirically (probe render of a number-sections doc
through the real `q2 render` pipeline) and by reading the code:

1. `SectionizeTransform` (Normalization phase) *moves* each header's id onto
   its `Div#id.section.levelN` wrapper (`pampa/src/transforms/sectionize.rs`;
   the header keeps its classes/kvs but its id becomes `String::new()` —
   pinned by `test_sectionize_id_moves_to_section`).
2. `FloatRefTargetSugarTransform::classify_div` then converts
   `Div#sec-*.section` into `CustomNode(FloatRefTarget)`, because `sec` is a
   builtin ref-type prefix and nothing excludes section divs.
3. `CrossrefIndexTransform` visits custom nodes *bottom-up* (slots first,
   then registration), so `@sec-` refs **already resolve today** — with
   wrong, document-order-inverted numbers (probe: first chapter's `@sec-`
   resolved to "Section 2", its nested H2 to "Section 1", an `.unnumbered`
   H1 to "Section 3"), and `convert_div`'s general case would steal a
   section div's trailing `Paragraph` as its "caption".

Registering in `visit_header` as originally written would have passed every
unit test (the test harness builds un-sectionized ASTs) while changing
nothing in the real render — this repo's end-to-end-verification trap in its
purest form.

**Decision (Gordon, 2026-09-23): Option A** — exclude `.section` divs from
float sugaring and recover the id in the index walker (stack of enclosing
section-div ids; `visit_header` falls back to the innermost enclosing section
div's id when its own is empty). Rejected alternatives: special-casing
sec-typed `FloatRefTarget`s in the walker (entrenches the misclassification;
same logic in a worse-shaped host); moving sectionize to Finalization
(Q1-faithful — Q1's filters number *before* pandoc's writer-side
`--section-divs` — but revealjs slides, auto-stretch, footnotes and the
page-appendix transform all consume `.section` divs mid-pipeline, so this is
a multi-day refactor P0 shouldn't carry); sectionize keeping an id copy on
the header (breaks its pinned pandoc-parity "id moves" contract, repo-wide
snapshot churn).

**Q1 numbering semantics, empirically pinned (2026-09-23, Q1 render of a
no-chapters probe):**

- `sections.lua` registers sec targets and advances the section counter
  **regardless of `number-sections`**; only the `number` attr stash and the
  visible injection are gated (`number-sections` && `level <= number-depth`,
  `number-depth` default 6). Q2 ports this: registration gated only on
  `is_html_based()`.
- The visible number and ref text include the top component **when the
  document's shallowest heading is H1** — `crossref.maxHeading` is
  `min(7, min header level)`, forced to 1 in chapters mode
  (normalize/flags.lua, options.lua). An H2-led doc numbers relative
  ("1", "1.1"). Q2 ports this as a pre-walk min-level scan.
- Appendix: the letter comes from the book item number
  (`formatChapterIndex` reads `fileMetadata.file.bookItemNumber`); with
  Q2's per-file appendix-local seeding, `section[1]` is exactly that.
  Level-1 appendix heading content ends as
  `[Str("Appendix"), Space, Span(number, header-section-number), Str(" —"), Space, ...]`.
