---
title: 'EXR-12: knitr K2, inline expressions as recorded units'
date: 2026-10-28
description: 'Records each knitr brace-spelling inline expression span as a unit of the engine execution record by embedding the unit id in the rewritten expression, without changing how any value renders; the classic spelling is still rewritten as today but gets no id and no record.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (plan EXR-12,
knitr phase K2).
**Depends on:** [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md) (chunk
record plumbing and the record adapter), [EXR-2](2026-10-28-exr-02-unit-scanner.md)
(the shared span scanner and regex), [EXR-4](2026-10-28-exr-04-record-format.md).
**Blocks:** [EXR-17](2026-10-28-exr-17-legacy-removal.md). Soft prerequisite of
[EXR-14](2026-10-28-exr-14-preview-splice.md)\'s knitr-span end-to-end fixtures (the span
splice itself works on any record with span units). Per the dependency table in the epic, it also blocks EXR-18.
**Lane:** A (`workspace-4`; re-assigned from C by Gordon, 2026-11-01); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2.

> **For implementers:** TDD, task by task; steps use `- [ ]` checkboxes. Tests
> that need R skip when it is missing (pattern below).

## Goal

Every brace-spelling inline expression (`{r} expr`) knitr evaluates becomes a **unit**
of the record, with the markdown text knitr inserted for it, matched by id. The classic
spelling (`r expr`) stays knitr\'s private business (EXR-2 design decision 2: the scanner
claims the brace spelling only), so it is rewritten as today but gets no id and no
record unit. Rendering is unchanged for both: the same bytes come out of a document with
and without recording.

## Background

Verified in code on 2026-10-28 unless marked.

- **Two spellings, one rewrite, one unit spelling.** `crates/quarto-core/src/engine/knitr/preprocess.rs`
  rewrites the brace spelling `{r} expr` to `r .QuartoInlineRender(expr)`
  (`resolve_inline_r_expressions`, `:257-297`) and re-emits the classic spelling
  `r expr` trimmed with a single space. The pattern is `INLINE_R_PATTERN`
  (`:202-211`); `has_inline_r_expressions` (`:315`) skips the pass when there is
  no match. The call site is step 2 of `KnitrEngine::execute`
  (`engine/knitr/mod.rs:157-175`). The pattern matches both spellings
  (`(\{r\}|r)`, `:210`), but only the brace spelling is a unit: the shared scanner
  (EXR-2) never returns a classic span, so there is no id for one.
- **The wrapper escapes, the classic form does not.** `.QuartoInlineRender` is
  defined in `resources/rmd/execute.R:181-195`: `NULL` becomes the text `NULL`,
  `AsIs` passes through, character values get markdown specials backslash-escaped.
  The classic spelling is deliberately *not* wrapped, because `I(NULL)` is an R
  error and the two spellings render `NULL` differently on purpose (empty versus
  the text `NULL`; module docs at `preprocess.rs:36-60`, pinned by
  `null_values_render_per_spelling` in `tests/integration/knitr_inline_expressions.rs`).
- **No inline hook is set.** `hooks.R` defines no `inline` hook (searched on
  2026-10-28); inline values go through knitr\'s default markdown inline hook, which
  receives only the evaluated value, not the expression or any id.
- **The `inline` hook is not called for every span (verified 2026-10-30, knitr 1.50
  installed locally; the version q2 pins is unknown).** In `knitr:::inline_exec`,
  `if (length(res)) ans[i] = paste(hook(res))`, so for a `NULL` or invisible value the
  `inline` hook never runs. The default `evaluate.inline` hook
  (`withVisible(eval(parse_only(code)))`) runs for every span. Anything that records a
  span through the `inline` hook alone loses the spans whose value is empty, and a
  pending id would stay set and be taken by the next span.
- **Known divergence between the pass and any scanner.** Two adjacent spans such
  as a brace span immediately followed by another evaluate only the first, because
  the pattern consumes the guard character before a match (`preprocess.rs:62-70`
  docs). A scanner that disagrees with this pass would produce phantom units.
  For the brace spelling, EXR-2 shares one span regex between the mask, the resolver and
  the scanner; this plan must use that same function. The classic spelling is outside
  that regex and keeps its own pattern here.
- **Offsets shift.** The pass runs after `serialize_ast_to_qmd`, so byte offsets
  in its output no longer match the `SourceInfo` built from the pre-pass string
  (`preprocess.rs` docs, "This pass shifts byte offsets"). That is why ids are
  embedded in the text instead of matched by offset.
- **Spans appear in more places than prose.** The integration tests evaluate them
  in prose, a fenced-div attribute value and a link title
  (`brace_spelling_is_evaluated_in_prose_and_attributes`,
  `knitr_inline_expressions.rs:82-125`); a span in a nested display fence is
  intentionally left alone (`nested_executable_fence_is_not_eaten_by_the_inline_pass`).
- **R gating in the tests.** `knitr_inline_expressions.rs:33-37` defines
  `knitr_available()` as `EngineRegistry::default().get("knitr").is_some_and(|e|
  e.is_available())`; each test begins with
  `if !knitr_available() { eprintln!("SKIP: ..."); return; }`. Helpers
  `render_html(tmp, name, content)` (`:39`), `read_html` and `doc` (`:62-68`) build a
  document and render it through `render_document_to_file`.
- **Not verified (open items, confirmed by spike S1 and S3):** the knitr version q2
  pins; that `eval` of two expressions (`.QuartoInlineId("3-1a2b3c4d-inline"); expr`) gives the last
  expression\'s visibility; that `evaluate.inline` runs in document order for every span;
  `execute: enabled: false` behavior for inline spans.

## Design decisions

From the epic; do not re-litigate:

1. A span is a unit; its recorded output is the markdown text knitr inserted for
   it, as a `display_data` output with `text/x-quarto-markdown`, tagged with the
   format. Spans are matched in preview by `(lang, code, occurrence)` and in the
   record by id.
2. Additive and render-neutral: no output byte may change.
3. Only brace spans are units and embed an id (decided with Gordon, 2026-10-31: the
   classic `r expr` spelling stays knitr\'s private business). Classic spans are still
   rewritten exactly as today, carry no id and produce no record unit. The mechanism
   for embedding an id in a brace span must leave the value alone (the `NULL` rule in
   Background).

Ids and the unit list (epic decisions 38 and 40): the span id is EXR-4\'s
`{n}-{hash8}-inline`, filled by the stage\'s one scan, whose span counter runs
document-wide over all spans (owned or not), like cells. knitr receives the same unit
list as every other engine, takes the ids of its owned spans from it, and validates
counts for its owned spans only. Spans are counted once, document-wide, in that scan; the
rewrite never numbers spans itself, and for the brace spelling the shared span regex
(widened in EXR-2 to the shared token class, decision 40) is the only definition of a
span. The classic spelling has no shared definition and no unit.

Decision made in this plan (replacing the handoff\'s counter proposal; accepted by
Gordon, 2026-10-30):

4. **Embed the id as a leading statement, recorded through knitr\'s `evaluate.inline`
   hook.** Rewrite each brace span so the evaluated text is
   `.QuartoInlineId("3-1a2b3c4d-inline"); <expr>` (with `.QuartoInlineRender(...)` kept
   around the expression). Classic spans are rewritten as today, with no id statement.
   `.QuartoInlineId` records the
   current id in the knit environment and returns an invisible `NULL`; the value
   of the whole expression is the last statement\'s value, so rendering is
   unchanged, `NULL` included. The recording does **not** hang off a wrapper on the
   `inline` hook, because that hook is not called for `NULL` or invisible values (see
   Background). Instead:
   - a wrapper around `evaluate.inline`, which runs for every span, sets the pending
     id (taking it from the span\'s code; a classic span has no id statement, so it sets
     none and is not recorded);
   - the `inline` hook records `{id, inserted markdown}` and clears the pending id;
   - at the next `evaluate.inline` call, or at the end of knitting, any pending id the
     `inline` hook did not serve is recorded as an empty string (the `NULL` and
     invisible cases).
   Do **not** fall back to a bare evaluation-order counter: the adjacent-span
   limitation (`preprocess.rs:62-70`) would misalign it silently. If spike S1 shows the
   `evaluate.inline` hook cannot be wrapped with the pinned knitr version, stop and
   raise it with Gordon instead of switching mechanism.

## Out of scope

- Jupyter spans ([EXR-3](2026-10-28-exr-03-jupyter-iee.md)) and Julia spans
  ([EXR-8](2026-10-28-exr-08-julia.md)); recording classic `r expr` spans.
- Changing escaping or `NULL` rendering; fixing the adjacent-span limitation.
- Preview splicing of spans (EXR-14) beyond emitting the right record.

## Phase 12S: spikes first (decisions written into this plan)

- [x] **S1.** In a scratch R session with the pinned knitr (first record which
  version that is): confirm that `` `r .QuartoInlineId("1-9f8e7d6c-inline"); 1+1` `` renders `2`,
  that `` `r .QuartoInlineId("1-9f8e7d6c-inline"); NULL` `` renders empty, that a wrapped
  `evaluate.inline` hook runs for every span including the `NULL` one while the
  `inline` hook does not, that the pending id set by the `evaluate.inline` wrapper
  reaches the `inline` hook of the same span, and that evaluating two expressions
  gives the last one\'s visibility. Pin the minimum knitr version these rely on.
- [x] **S2.** Set equality: list the brace spans knitr evaluates for the existing test
  documents (prose, attribute values, link titles, adjacent spans, display
  fences, spans inside `eval: false` chunks\' surrounding prose) against the EXR-2
  scanner\'s units. Record every difference; classic spans are an expected,
  by-design difference and are listed separately.
- [x] **S3.** `execute: enabled: false` and `error: true`: what knitr does with
  inline spans, and what Quarto 1 does.

## Phase 12A: tests first

No R needed:

- [x] `rewrite_embeds_ids_in_brace_spans`: golden strings for `{r} x`, a tab-separated
  brace span, and brace spans in attribute values.
- [x] `rewrite_leaves_classic_spans_without_ids`: golden strings for `r x` and a
  tab-separated classic span come out exactly as today, with no id statement.
- [x] `rewrite_leaves_empty_classic_span_alone`: loud failure preserved
  (existing behavior at `preprocess.rs:285-290`).
- [x] `ids_come_from_the_shared_unit_list`: the rewrite takes the ids of its owned brace
  spans from the stage\'s shared unit list (matched by `line` in the text knitr receives,
  in order within a line, under a count check; core re-locates each unit\'s `line`, EXR-2
  `locate_units` carried by EXR-7), never generates its own; a span another engine owns,
  or a classic span, is skipped and does not shift the ids of the owned ones.
- [x] `resolver_and_scanner_agree_on_the_corpus`: property test over every
  fixture string in `knitr_inline_expressions.rs` and the mask tests; the set of brace
  spans from the rewrite equals the scanner\'s inline units for language `r` (this is the
  guard for phantom units; classic spans are excluded from both sides by design).

Real R, gated like the existing file:

- [x] `inline_records_one_unit_per_span_in_document_order`.
- [x] `inline_records_brace_spans_and_not_classic_spans`: a brace span records the
  escaped text and a `NULL` brace span records `NULL`; a classic span in the same document
  produces no unit (its rendered text still matches `null_values_render_per_spelling`).
- [x] `inline_null_and_invisible_spans_record_an_empty_unit_and_do_not_shift_ids`: a
  `NULL` span between two valued spans records an empty unit under its own id, and
  the following span keeps its id (the case a recording on the `inline` hook alone
  gets wrong).
- [x] `inline_id_embedding_does_not_change_output` (shadow equality): the HTML of
  every existing fixture in `knitr_inline_expressions.rs` is byte-identical with
  embedding on and off.
- [x] `inline_in_attribute_and_link_title_are_units`.
- [x] `inline_in_nested_display_fence_is_not_a_unit`: masked fences yield no unit
  and no recorded value.
- [x] `inline_count_mismatch_skips_record_with_diagnostic`: if knitr evaluates fewer
  spans than the scanner found (the adjacent-span case until fixed), no record is
  written for the document and a diagnostic names the span\'s source location; the
  render succeeds (the epic\'s count-mismatch rule while records are additive; whether
  a mismatch may fail the render after knitr flips is decided in that flip task).
- [x] `inline_error_names_the_unit_location` (bonus, with bd-1ng2ivis): an inline
  expression that errors reports `file.qmd:line:col` of the span.

## Phase 12B: R side

- [x] Define `.QuartoInlineId` next to `.QuartoInlineRender` in `execute.R`; wrap the
  `evaluate.inline` hook (set the pending id for every span), add the `inline` hook
  that records `{id, text}` and clears the pending id, and flush an unserved pending id
  as an empty string at the next span and at the end of knitting; return the records as
  `spans` next to `chunks`.

## Phase 12C: Rust side

- [x] Extend the rewrite in `preprocess.rs` to take the shared unit list\'s ids and emit the
  leading statement for brace spans only; keep the old signature for callers that record
  nothing, and leave classic spans rewritten as today.
- [x] `KnitrExecuteResult.spans` with `#[serde(default)]`; adapter into the record
  (span units with rendered outputs), joined by id.
- [x] The owned-span count is restricted to language `r`: knitr\'s view marks
  `python`, `sql`, `bash` and `sh` owned through its interop claims
  (`engine/knitr/mod.rs:257-266`), but the pass evaluates only `r` spans
  (`preprocess.rs:202-211`), so a span in any other language knitr owns stays literal
  with no id and no unit, and does not enter the count.
- [x] Alignment check: every scanner span for language `r` has a record (or the engine reported it
  as not evaluated); otherwise no record plus a diagnostic with the span\'s location,
  and the render succeeds.

## Results (2026-11-01)

Spike findings, all on knitr 1.50 (the version installed locally; the version q2 pins is
still unknown, and this plan adds no new minimum beyond the `evaluate.inline` and `inline`
hooks, both long-standing `knit_hooks` entries):

- **S1.** Confirmed: the `evaluate.inline` hook runs for every span, `NULL`, invisible and
  zero-length ones included, while the `inline` hook does not (`knitr:::inline_exec` skips
  it when `length(res)` is 0); `.QuartoInlineId("..."); expr` keeps the last expression\'s
  value and visibility; a comment after the leading statement is harmless. Two facts the
  plan did not have:
  - A hook set through rmarkdown\'s `knit_hooks` replaces knitr\'s default `inline` hook,
    which drops its numeric rounding (`3.14159265` instead of `3.141593`). The recording
    hook is therefore installed lazily from the `evaluate.inline` wrapper, wrapping the hook
    that is current at that moment (`quarto_ensure_inline_hook` in `hooks.R`); `inline_exec`
    reads its `hook` argument lazily, after the first `evaluate.inline` call. Output is
    byte-identical to a run with no recording (checked in R and by
    `inline_id_embedding_does_not_change_output`).
  - `knitr_hooks()` can run more than once in a process; the wrapper unwraps its own earlier
    install (`quarto_base` attribute) or it calls itself and fails with "evaluation nested
    too deeply".
- **S2.** The brace spans the rewrite evaluates equal the scanner\'s `r` units on every
  fixture in `resolver_and_scanner_agree_on_the_corpus` (prose, attributes, link titles,
  adjacent spans, multi-line, tab, front matter, lists and tables, after a chunk). The
  adjacent-span limitation is shared: the scanner and the rewrite both consume the guard
  character, so both see only the first span and the units stay aligned (no mismatch).
  By-design differences: classic spans (no unit); `{R}`, `{python}` and other languages
  (no unit, not counted); a span inside an HTML comment, which the scanner skips and the
  rewrite evaluates (it gets no id and no record, test
  `a_span_in_an_html_comment_is_evaluated_but_not_a_unit`). A span inside a display fence
  is masked upstream, so neither side sees it.
- **S3.** `execute: enabled: false` does not stop knitr evaluating a brace span in a
  document that has an `r` chunk (existing behavior, unchanged; the span is recorded). A
  document with no `r` chunk does not reach knitr, so its spans are neither evaluated nor
  recorded. `error: true` governs chunks only: a failing inline expression fails the render
  either way. Quarto 1 was not run for any of this. Errors from an inline expression used
  to be reported at a line of the text handed to R (`lines 139-139`); they now name the
  line in the author\'s file (`map_span_line_to_engine_line`, `inline_error_names_the_unit_location`).

Notes on the tests: the count-mismatch case is a unit test
(`inline_count_mismatch_skips_record_with_diagnostic` in `chunk_records.rs`): because the
scanner and the rewrite agree even on adjacent spans, no real-R document produces a
mismatch today. The shadow-equality test also runs `place(record)` against the live
document, except for documents with a classic span (no unit, so `place` leaves it literal)
and for the masked display fence. A numeric inline value such as `` `{r} 1e6` `` (`1e+06`)
made the live render fail to parse in the shadow fixture, with or without recording; it was
left out of the fixture and is not part of this plan.

## Exit criteria

- Shadow equality: embedding does not change the rendered HTML on any fixture.
- The resolver and the scanner agree on the corpus.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` green, with R present; pass and skip counts
  reported against the live baseline.

## Risks and open questions

- **Leading statement edge cases.** An expression that is only a comment, or that
  uses constructs sensitive to a preceding statement, may differ; S1 and a
  dedicated test pin them.
- **knitr versions.** The hook contract is knitr behavior, not q2 code; pin the
  minimum version the existing hooks already require (`hooks.R` checks
  `knitr_has_yaml_chunk_options`). **Open:** which knitr version q2 pins (the finding
  about `inline_exec` was made against knitr 1.50), and whether the `evaluate.inline`
  hook is as stable across versions as the `inline` hook; both are answered in S1.
- **The adjacent-span limitation** is a Quarto 1 inheritance. Either fix it in the
  shared regex (a behavior change that needs sign-off) or make the no-record
  diagnostic the documented behavior.
- **The leading statement adds an `R` call to every recorded span\'s text.** Accepted
  (Gordon, 2026-10-30) in place of the counter; S1 and the dedicated edge-case test pin
  the behavior.

## References

- `crates/quarto-core/src/engine/knitr/{preprocess.rs,mod.rs,resources/rmd/{execute.R,hooks.R}}`
- `crates/quarto-core/tests/integration/knitr_inline_expressions.rs`
- `claude-notes/plans/2026-10-28-exr-02-unit-scanner.md` (shared span regex, written alongside this plan)
- bd-knitr-inline-r-eats-fence-2ofk91x1 (the fence-eating bug that motivated the guard)
