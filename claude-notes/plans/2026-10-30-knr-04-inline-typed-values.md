---
title: 'KNR-4: knitr inline expressions as recorded values'
date: 2026-10-30
description: 'Records each knitr inline expression as an inline unit of the engine execution record, aligned with the Jupyter execution-of-inline-expressions conventions, replacing the id-embedding mechanism of EXR-12 once the structured capture exists.'
status: done  # 2026-11-04; the retirement of EXR-12's id statement is KNR-6's
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-4).
**Depends on:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md) (adapter and
fallback), [EXR-12](2026-10-28-exr-12-knitr-k2-inline.md) (complete: the current mechanism this plan replaces),
[EXR-2](2026-10-28-exr-02-unit-scanner.md) and [EXR-4](2026-10-28-exr-04-record-format.md)
(the stage\'s shared unit list, with the document-wide span counter and the ids). Soft:
[EXR-3](2026-10-28-exr-03-jupyter-iee.md) (the Jupyter conventions to match).
**Blocks:** KNR-6.
**Estimated sessions:** 2.

> **Status of this document.** Written 2026-10-30 from code reading, before KNR-1 runs, and
> revised on 2026-10-31 after the paper review and the parent epic\'s decisions 38 to 40.
> The knitr inline internals were read in the development snapshot of knitr; the
> value-formatting question is not verified, and it decides the design. KNR-1 S9 has reported
> (2026-11-01; see the [corpus report](../research/2026-11-01-knr-01-corpus-report.md)): the
> baseline differs from knitr on every inline span, which is this plan\'s subject.

## Goal

Every inline expression knitr evaluates becomes an inline unit of the record whose output
is the value, matched by id, so that preview and hub staleness work per span and replay
does not need R. Rendering is unchanged.

## Background

**What knitr does (read 2026-10-30 in the development snapshot, `block.R:634-665`).** For each span `inline_exec`
runs `hook_eval(code[i], envir)` (the `evaluate.inline` hook), turns `knit_asis` values into
text with `sew(res, inline = TRUE)`, coerces to character, and only `if (length(res))`
calls the `inline` hook, whose result replaces the span with `str_replace(input,
block$location, ans)`. So a `NULL` or invisible value never reaches the `inline` hook, and
the text group is rebuilt around the replaced spans.

**What q2 adds today (EXR-12 background; verify before relying).** A Rust pre-pass
(`engine/knitr/preprocess.rs`) rewrites the brace spelling `{r} expr` to
`r .QuartoInlineRender(expr)`. `.QuartoInlineRender` (`resources/rmd/execute.R:181-195`)
renders `NULL` as the text `NULL`, passes `AsIs` through, and backslash-escapes markdown
specials in character values. The classic spelling `r expr` is not wrapped, because
`I(NULL)` is an R error, so the two spellings render `NULL` differently on purpose. EXR-12
embeds a unit id as a leading statement, `.QuartoInlineId("3-1a2b3c4d-inline"); <expr>`
(the id is EXR-4\'s `{n}-{hash8}-inline`, from the stage\'s shared unit list; parent
decisions 38 and 40), and records `{id, inserted markdown}` through a wrapper on
`evaluate.inline` plus the `inline` hook.

**What the structured design changes.** EXR-12 records the *inserted markdown*. This epic's
records hold values as nbformat outputs, as Jupyter's spans do (EXR-3: a value output with a
text mime), and the Rust presenter inserts text. For R that raises the question below.

**The value-formatting question (settled by KNR-1 S9).** knitr\'s default markdown inline
hook formats numbers (rounding, scientific notation, a big mark) before insertion. The record
holds the text knitr inserts, after the R-side escaping and the `NULL` rule of both spellings,
as `text/plain`. The Rust presenter inserts it verbatim and applies no escaping of its own, so
no unit needs to know its spelling.

**Correction (2026-11-02, Gordon; review 3): S9 settled plain values only.** knitr\'s inline
path is not text-only. `inline_exec` sends a `knit_asis` or `knit_asis_url` value through
`sew(res, inline = TRUE)` (`external-sources/knitr/R/block.R:652`), so an inline htmlwidget (a
sparkline, a small plotly) comes out as HTML with its dependencies registered in `knit_meta`
(`sew.knit_asis`, `output.R:575`), and `include_graphics()` comes out as a markdown image through
the plot hook (`sew.knit_image_paths`, `output.R:679`). Quarto 1 therefore supports a chart in
prose. The final inline hook sees only a string, so a record that stores that string as
`text/plain` keeps the HTML but loses its mime, the widget dependencies and the image file.
S9\'s fixture (`s9/01_inline.Rmd`) has no rich inline case, which is why it passed. Provisional
decision 6 below carries the fix. Nothing in the widget epic (`2026-11-02-widget-epic.md`, WGT-2)
mentions inline spans, so this plan is also the place where spans join that contract.

## Design decisions (from the epic; do not re-litigate)

1. A brace-spelled span `{r} expr` is a unit, `quarto_kind: inline`, with the id format and
   domain-separated content hash of EXR-4 (`{n}-{hash8}-inline`, hash input
   `inline:<lang>\n<code>`). The span counter runs document-wide over all brace spans, owned or
   not (parent decision 38), and the shared scanner is the only definition of a unit
   (decision 40; it emits brace-spelled spans only), so a span another engine owns never
   shifts the ids of knitr\'s. A classic span `` `r expr` `` is not a unit: it stays on the
   live R path and gets no record (Gordon, 2026-11-02).
2. Statements and errors in spans are fatal and `error: true` is ignored, as in q1; `None`
   and `NULL` follow EXR-12 and EXR-3 (parent decisions 17 and 21).
3. Additive until KNR-6: the live output does not change.
4. **Flag:** the [units-only engines epic](2026-11-02-units-only-engines-epic.md) (draft) follows
   decision 1 for `` `r expr` `` (its open question 2) and may supersede KNR-3\'s `child=` fallback
   with sub-units. Do not extend the `child=` fallback beyond what KNR-3\'s table needs.

## Provisional: decisions this plan must make

1. **Capture mechanism** *(answered by S1 and S9, 2026-10-31: wrap `evaluate.inline` and `inline`, one entry per span by position, text as knitr inserts it; see KNR-1 S9)*: the wrapper
   records one entry per `evaluate.inline` call. Entries carry the spelling (the
   `.QuartoInlineRender` wrapper is present for a brace span and absent for a classic one), and
   only wrapped entries are matched by position to the shared unit list; classic spans are
   evaluated and skipped. The id rides in the unit list the Rust side hands to R, as chunk ids
   do in EXR-10, not in the expression. The new wire field (its Rust type and JSON key) is
   defined in this plan\'s Mechanism item.
2. **What is recorded**: the text knitr inserts (see above). Substitution at render time is
   owned by KNR-6 Phase 6B: classic spans are substituted by R on the live path, and 6B says
   whether brace spans are substituted at present time from the record or carried by markers.
   This plan builds the record side and calls the existing `presenter/inline.rs`
   (`classify_inline_result`, `InlineTarget`); its tests do not assume the venue.
3. **The two spellings\' `NULL` rule** is applied in R, before the text is recorded (empty
   for a classic span, the text `NULL` for a brace span). No spelling key is needed: the
   record holds the final text, and a classic span has no unit.
4. **Empty and invisible values** (EXR-12 decision 4 behavior: an unserved pending id is
   recorded as empty): carry over, with no fallback to a bare counter (adjacent spans are
   evaluated once; `preprocess.rs:71`), and with ids taken from the shared list, never
   generated here.
5. **Spans in unusual places** (attribute values, link titles, nested display fences): the
   shared scanner and the mask decide; carry over EXR-12's corpus.
6. **Rich values in brace spans (added 2026-11-02, Gordon).** A span\'s value is recorded as an
   nbformat output with the mime knitr produced, not only `text/plain`: `text/html` for an
   htmlwidget or other `knit_asis` HTML, an image output for `include_graphics` and a plot,
   `text/plain` for everything else. The capture therefore wraps `sew` for the inline path (or
   reads the value before the inline hook flattens it), and the output takes the same
   metadata as a chunk output: `quarto_file_path` for an image, `quarto_knitr.knit_meta` for
   widget dependencies, and files in the `files` map. The presenter inserts the output through
   the same output handling as a chunk, inline: raw HTML as an inline `{=html}` span, an image
   as an inline image. Dependencies follow the widget epic\'s per-output placement (a manifest
   on the output that needs it; WGT-2, decided 2026-11-01), so a span needs no special rule;
   the knitr adapter owns knitr\'s hidden-output quirk as elsewhere (WGT-2). `knit_meta` is
   converted to the neutral manifest by the adapter and read in Rust at present time (epic
   decision 27), so a span needs no R at present time. A block-level output (a plotly div)
   inside a paragraph keeps knitr\'s behavior for now (epic decision 28); an observation
   fixture (below) shows whether it causes trouble. Classic spans
   are unaffected: R substitutes them live, and knitr already handles `knit_asis` there.

## Out of scope

- Jupyter spans (EXR-3), Julia spans (EXR-8); changing escaping rules or the adjacent-span
  limitation; Positron's lack of execution of inline expressions (EXR-16 S3).

## Phases

- [x] **Tests first.** EXR-12\'s tests already cover the ported list and stay green against this
  work: one unit per brace span in document order, a classic span yields no unit and is
  untouched (`span_records::ids_come_from_the_shared_unit_list`, `preprocess` classic-spelling
  tests), attribute and display-fence spans, count mismatch yields no record plus a diagnostic
  (`chunk_records::inline_count_mismatch_skips_record_with_diagnostic`), ids stable beside another
  engine\'s spans and mixed spellings (`MIXED` in `span_records`). New and red first: the
  deserialize test for the new `piece` field, the five `span_output` tests, the three R-gated
  engine tests (`gating_tests`, "KNR-4 decision 6"), and `a_verbatim_inline_unit_keeps_its_trailing_newlines`
  (mutation-checked). Live equality with recording on and off is the corpus shadow.
- [x] **Mechanism.** *Decided 2026-11-04: keep EXR-12\'s embedded `.QuartoInlineId` statement
  until KNR-6 retires it.* It already aligns spans by the shared unit list and the plan puts the
  retirement in KNR-6 (last item below); moving the id to a Rust-side hand-off now would change
  the live-path rewrite for no capability gain. The new capture rides the same wrapper on
  `evaluate.inline`.
- [x] Fixtures: `s9/02_inline_rich` (raw html from `asis_output`, an `include_graphics` svg, a plain
  number) and `s9/dot.svg`; the widget case is the R-gated engine test
  `a_widget_span_records_html_and_its_dependencies` (plotly; `sparkline` is not installed here and a
  small corpus widget would need a package the CI image may lack). Corpus: `s9/02_inline_rich` is
  `identical` to the live output. `s9/01_inline` was rewritten to brace spans (see Notes).
- [x] An observation fixture, not a gate: `s9/03_inline_block_widget`. A block-level widget in a
  brace span inserts knitr\'s `{=html}` fence in mid-paragraph: the paragraph splits into two
  with a `RawBlock` between, and the text after the closing fence starts a new paragraph. Live and
  record agree (`identical`), so nothing breaks; it informs epic decision 28 (keep knitr\'s
  behavior for now).
- [x] **Adapter.** `KnitrSpanRecord.piece` (wire field) and `span_output`: one `display_data`
  output whose bundle holds `text/x-quarto-markdown` (the text knitr inserted) and, for
  `knit_asis` or exactly one image path, the value as knitr produced it (`text/html` with
  `quarto_knitr.knit_meta` and `widget`, or `image/*` with `quarto_file_path`).
- [x] **Presenter.** The record\'s markdown is inserted verbatim: a span output carries
  `quarto_verbatim: true`, and `record::placed_replacement` (shared by `place` and
  `present_document`) skips the inline trailing-newline trim for it. Shadow equality holds on the
  corpus.
- [ ] **Retire EXR-12\'s mechanism** in KNR-6, once nothing reads the old `spans` field.

## Notes (2026-11-04)

- **Why `s9/01_inline` was the blocker, and what cleared it.** The fixture hand-wrote
  `.QuartoInlineRender(...)` inside *classic* spans. Classic spans are not units (decision 1), so
  the record path left them as written while the live path evaluated them: the record
  presentation cannot match live for a classic span until KNR-6 6B decides the venue. The fixture
  now uses the brace spelling `{r} expr`, whose behavior (the `NULL` rule, markdown escaping,
  `asis`) the live path produces through the same `.QuartoInlineRender`. Classic-span behavior is
  covered by the EXR-12 and `preprocess` tests, not by the shadow. The live oracle for the
  fixture was refrozen (1 file).
- **Scientific-notation numbers left the fixture.** knitr formats `12345` as `1.2345\times
  10^{4}` with no surrounding `$`, which q2 does not parse in either path; both the live and the
  record presentation show the same parse errors, but the rendered HTML then differs between
  them and the case classes as a blocker. It is a live-path quirk outside this plan.
- **Rich values.** `.QuartoInlineRender` escapes any character value (it drops the
  `knit_asis` class), exactly as in Quarto 1, so a rich value in a brace span is written
  `I(...)`. A widget in a span is recognized by its `knit_meta` attribute, because only a chunk\'s
  auto-print adds knitr\'s `knit_asis_htmlwidget` class. Several image paths in one span keep
  only the markdown (one output holds one image).
- **Gate counts:** see the final report in the commit that closes this plan.

## Exit criteria

- Every span in the corpus has a unit; shadow equality with the live path; EXR-12's tests
  pass against the new mechanism; clippy and nextest green with counts reported.

## Risks

- **A rich value rendered as plain text** if the capture records only the flattened string; the
  fixtures above fail for exactly that reason before the implementation.
- **Number formatting drift** between R's formatting and any Rust re-implementation: avoid
  by recording R's formatted text.
- **Hook order.** User-defined `inline` or `evaluate.inline` hooks (same class of problem as
  user hooks in KNR-3): detect and fall back.

## References

- `external-sources/knitr/R/block.R:634-665`; EXR-12; EXR-3; EXR-4 (inline units);
  `crates/quarto-core/src/engine/knitr/{preprocess.rs,resources/rmd/execute.R}`
