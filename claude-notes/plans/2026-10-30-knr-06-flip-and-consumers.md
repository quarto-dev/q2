---
title: 'KNR-6: the knitr flip and the consumers'
date: 2026-10-30
description: 'Switches knitr from returning its own markdown to presenting from its structured record, moves replay, preview and hub consumers to structured knitr records, narrows EXR-11 recorder to fallback chunks and deletes EXR-12 inline-id code. Mirrors EXR-6 for Jupyter, with the extra problem that knitr runs the whole document in R.'
status: draft  # outline written from code reading; the skeleton design is the open question
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-6).
**Depends on:** [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md) (zero blockers; it brings [KNR-2](2026-10-30-knr-02-knitr-presenter-layer.md)),
[KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md),
[KNR-4](2026-10-30-knr-04-inline-typed-values.md),
[EXR-6](2026-10-28-exr-06-jupyter-flip.md) (the Jupyter flip, whose pattern this follows),
[EXR-14](2026-10-28-exr-14-preview-splice.md), [EXR-15](2026-10-28-exr-15-hub-integration.md).
Soft: [KNR-5](2026-10-30-knr-05-positron-interop.md).
**Blocks:** KNR-7. Once it lands, the knitr engine counts as migrated for item 1 of
[EXR-17](2026-10-28-exr-17-legacy-removal.md)\'s trigger (EXR-17 has no knitr row).
**Estimated sessions:** 3 to 4.
**Prerequisite, not part of this plan:** the knitr replay claim fix (bd-js24dold, interlude
I2); it lands before KNR-3, so replay tests here start from a working knitr replay.

> **Status of this document.** Written 2026-10-30 from code reading. The hardest design
> question (the skeleton, below) was tested by KNR-1's S7, whose verdict is markers (see its
> `#### S7 verdict`).

## Reconciliation against the tree (2026-11-05, head `f9440bb15`)

Checked by reading the code before starting; nothing here was run yet.

- **Done upstream, do not redo.** Replay dispatches on `capture.record.is_some()`
  (`engine/replay.rs`, `execute`), and `knitr_trace_with_a_structured_record_replays_through_the_presenter`
  and `a_structured_knitr_cell_reaches_the_shared_reader` cover a knitr record replay. The old-markdown-trace
  test is `replay_knitr_trace_unchanged`. The corpus gate is asserted by `corpus_report`
  (`assert_gate`, KNR-2b). Holds, `known_options`, `fenced.echo` and the fallback capture skip are KNR-3/KNR-4.
- **Still open.**
  - `present_record` is still a method on `ReplayEngine`; the shared presenting function that live
    knitr rendering also calls does not exist.
  - Nothing wires `FigureNaming::path_fallbacks()` into a render-level counter, and there is no fallback
    counter by reason at all.
  - No test pins that Jupyter figure names are unchanged by the `quarto_file_path` sink; the plan does not yet
    say whether knitr\'s own figure copies are deleted after Rust reads them (nothing deletes them today).
  - The `knitr_replay_record_runs_presenter` equality-with-live test of Phase 6A is not written under that name;
    the two replay tests above cover the replay half, not "equals the live output".

## Marker survival probe (2026-11-05)

Scratch run, not committed code: the S1 harness\'s `markers` mechanism (render mode,
`rmarkdown::render(run_pandoc = FALSE)`, knitr 1.50, R 4.3.2, html) on a fixture of cases S7
did not cover, plus a throwaway inline hook that returns `<!-- knr-span:N -->`.

- **Block markers survive** for a `results=\'asis\'` chunk, a chunk with several source groups and
  two plots, a raw-HTML result, and a chunk in an ordered-list item (hook indent `"   "`, marker
  line carries it). Hidden units leave gaps in the numbering, as S7 found. Each marker is
  followed by knitr\'s own blank line on top of the hook\'s `\\n`; blank lines are the presenter\'s
  business, as before.
- **Inline markers survive** in a paragraph, inside emphasis, inside link text, in a table cell
  and in a heading, byte for byte.
- **Consequence for brace spans (provisional decision 1):** carrying brace spans by markers is
  viable on the evidence. I lean to it, since it reuses the chunk mechanism and the shipped
  fallback is already keyed by id. Still to decide there: the marker needs a per-run nonce so
  user text that looks like a marker cannot be mistaken for one (risk 1).
- **`indent`:** the hook returns `options$indent` plus the marker, and Rust reads the prefix off
  the marker line (S7). So nesting is not carried by the `indent` presenter option, and an explicit
  header `indent` would come through the marker line\'s prefix for free. Whether to honor it or keep
  it inert (KNR-3 decision D8) is a product choice and is independent of marker survival.
- **Decisions (Gordon, 2026-11-05):** brace spans are carried by markers (option B), and an explicit
  header `indent` stays inert (D8 stands). See "If `indent` is ever needed" below.
- **Not covered:** non-html targets, knitr 1.44 and 1.52 for the inline case, and the real
  engine path (the probe used the harness, not `KnitrEngine`).

## If `indent` is ever needed

Not part of this plan\'s work; a record for whoever needs it. The presenter ignores a chunk\'s
explicit `indent` header option (KNR-3 slice 7, D8), so live knitr and the presenter differ on the
two corpus cases `options/indent*` (allowlisted). To honor it:

1. In `presenter/document.rs`, at the comment marked there, prefix every line of the rendered cell
   text with the option\'s resolved value (from the record\'s resolved options, header winning)
   before calling `replacement`. Container prefix and the unit\'s own indent are added by
   `replacement`; do not add them twice.
2. Flip the `indent` row of `record_splice/tests/option_sweep.rs` from `Inert` to `Presents` and
   delete its reason; remove the two allowlist entries.
3. Check the marker line: the hook returns `options$indent` plus the marker (S7), so Rust reading the
   prefix off the marker line would also carry an explicit `indent`; pick one mechanism, not both.
4. Update the user-facing page `docs/guides/authoring/migrating-markdown.qmd` if it lists the difference.

## Replay never writes a trace

Done 2026-11-05 (Gordon chose to skip the write, and to suppress it in the Pass-1 pass).
A document that still says `trace: true` used to rewrite the recording being replayed. Two
writers did it: the real render (its registry holds the replay engines) and the Pass-1 profile
pass (a separate pipeline run before it, with a registry that knows nothing of replay).
`activate_trace_from_metadata` now installs no JSON writer when `EngineRegistry::is_replaying()`
or when `RenderContext::suppress_trace_output` is set, which `pass1_profile_single_file_live`
sets. Tests: `trace_true_installs_the_json_trace_observer` (control),
`trace_true_is_ignored_while_replaying`, `trace_true_is_ignored_when_trace_output_is_suppressed`.
End to end, through `target/debug/q2`: render a `trace: true` knitr document (a 70,463-byte
`latest.json.gz`), then `render --replay` that file twice; the SHA-1 of the trace is identical
before and after, and the replayed output holds `[1] 2` (inspected). One behavior change to know:
a document that fails in Pass 1 no longer leaves a partial trace. `trace: "summary"` is unchanged.

## Settled design of the flip (Gordon, 2026-11-05)

Decisions 1 to 6 of the handoff stand; these settle the rest of 6B.

1. **Marker.** Rust makes a random nonce per run and passes it to R in the execute params. A
   marker is `<!-- q2:<nonce>:<unit-id> -->`, for chunks and for brace spans. Only a marker
   carrying this run\'s nonce counts, so user text that looks like one is left alone. A marker
   whose unit has no usable record cell renders that unit\'s shipped markdown (decision 6).
2. **What R emits.** A structured chunk\'s hook returns `options$indent` plus the marker, and
   still computes the finished markdown and ships it keyed by unit id. A chunk on the fallback
   table (`child=`, cache hit, overridden hook) returns its real markdown, as today. The inline
   hook does the same for a brace span; a classic span stays substituted by R.
3. **What Rust does.** It finds the marker ranges in the skeleton, re-ranges the document\'s
   units onto them (a unit with no marker is dropped, so it presents nothing, which is what
   `include=FALSE` needs), takes each marker line\'s prefix as the container prefix, and calls the
   shared presenting function. `present_document` already splices over a base text by unit
   start and end, so the base text is the skeleton and the ranges are the markers\'; the Jupyter
   path still passes the input text and the scanned ranges.
4. **Where the skeleton lives.** `ExecuteResult` gains an optional `skeleton { text, nonce }`
   (serde default). `result.markdown` holds the **final presented text**, as Jupyter\'s does after
   EXR-6, so the legacy capture reader and the preview splice keep working. Replay presents over
   `skeleton.text`; a trace without the field replays its markdown verbatim, as today. (This
   corrects the earlier note that `result.markdown` would hold the skeleton.)
5. **Knitr\'s own figure files are kept.** A fallback chunk\'s shipped markdown still points at
   them, so deleting is unsafe. With `quarto_file_path` honored Rust normally writes the same path,
   so there are no duplicates; a path fallback leaves one. A test pins that Jupyter figure names
   are unchanged.
6. **6C shrinks.** The `.QuartoInlineId` hand-off stays (brace-span markers need it to reach R\'s
   inline hook; ordinal alignment is unreliable because hidden units leave gaps), and so do the span
   fallback records, until KNR-8. 6C deletes only what nothing reads after the flip; the list is
   decided there from what is unreferenced.
7. **Probe before the flip.** Marker survival on docx, typst and revealjs, and inline markers on
   knitr 1.44 and 1.52, are red-first tests in 6A.
8. **User-facing.** `cache: true` chunks stop caching on structured targets (Q-18-11 says so);
   state it in the docs page.

## Goal

After this plan, the knitr engine runs R, builds a record, and returns
`present(record, document)` as its markdown, like Jupyter after EXR-6. Replay, preview and
hub consume structured knitr records. **R keeps shipping the finished markdown of every chunk
next to the record** (epic decision 24), so a chunk Rust cannot present still renders; the
EXR-11 recorder and the chunk hook therefore stay as they are, and dropping that markdown for
structured chunks is the deferred [KNR-8](2026-10-30-knr-08-fallback-removal.md). EXR-12\'s
inline-id embedding is deleted once nothing reads it (see Phase 6C for what the spans keep).
Output on the corpus is within the classified differences.

## Background

**Jupyter drives the loop; knitr cannot be.** After EXR-6, core steps cells and splices
presenter output between the gaps of the input text (`execute_blocks_inner`; EXR-6
background). knitr is whole-document: `rmarkdown::render` runs every group and returns the
finished document (`resources/rmd/execute.R`; `KnitrEngine::execute`, `engine/knitr/mod.rs`).
Prose, list nesting, `include=FALSE`, inline substitution, `child=` and the `"\n"` joins
are all done by knitr (`external-sources/knitr/R/{block.R,output.R}`; epic, Findings).
Rust assembling the document itself would have to reproduce knitr's group splitting.

**The skeleton problem (design, untested).** To present chunks in Rust while letting knitr
build the rest, knitr could return a *skeleton*: the chunk hook returns a marker (for
example an HTML comment carrying the unit id, indented by `options$indent`) instead of the
formatted chunk, so knitr's own join builds the document around the markers, and Rust
replaces each marker with the presenter's output. (KNR-3 slice 7 leaves `indent` inert in the
presenter, so whether a header `indent` still takes effect after the flip depends on how this
marker's indent survives the replacement; its allowlist entry says KNR-6 decides.) What reading
the code says about it:
- `include=FALSE` returns `''` or the indent after the hook, so the marker disappears and
  the unit presents nothing, as today (`block.R:425`).
- A level-3 cache hit returns the cached string without running the hook (`block.R:100-101`);
  the cached string is the *previous hook output*, so a marker would be cached and replayed,
  but there are no pieces for it: that chunk is a fallback unit (KNR-3). A cache written
  *before* the flip holds full markdown, not markers, so there is no marker to substitute:
  such a chunk also becomes a fallback unit (S7 tests it). *Corrected 2026-11-05: capture turns the
  cache off on a structured route, so neither case arises there; see risk 5.*
- Fallback chunks must still return real markdown, while structured chunks return a marker,
  so the hook behaves two ways per chunk (KNR-3\'s table decides which); S7 tests the mix.
- The marker must survive the rest of the R side: `extract_preserve_chunks`, the `.rmarkdown`
  rename (`postprocess_markdown`), and `rmarkdown::render` steps. An HTML comment is plausible;
  not tested.
- The preview and hub consumers (EXR-14, EXR-15) work on the pre-engine AST cells, not on a
  skeleton, so they do not need this.
The alternative is Rust assembling the document from the source (as Jupyter does), which
needs agreement with knitr's `split_file` on every chunk boundary (list-nested, blockquote,
adjacent fences, chunks at file ends). Prefer the skeleton if S7 shows it works.

**EXR-6's rules that carry over.** The flip is one reviewed commit; rollback is a revert;
**no runtime switch**, because a switch is two mechanisms alive at once (the bd-mu9i0bct
class). The error policy stays in the engine. Blank-line discipline lives in the presenter.
`ReplayEngine`, `--replay` and the trace's `engine_captures` stay; replay returns the stored
record and runs the presenter again (decision 9 of the parent epic).

**A rule that is relaxed for knitr.** EXR-6 says presentation reads options from the
*document* at present time, never from the record. For knitr that is insufficient: chunk
options also come from R-side state the document does not show, such as
`knitr::opts_chunk$set(echo = FALSE)` in a setup chunk, `opts_template`, `opts.label` and
`ref.label`, and `opts_hooks` (`block.R:9-92`, `call_block`). The record carries the
**resolved** presentation options per unit (KNR-1 S3) and the code knitr echoed when it
differs from the authored code (`quarto_echoed_code`, epic decision 21), and the presenter
reads those, with the chunk\'s current header options winning (epic decision 11). Parent decision 34 now
anticipates this layering and leaves knitr\'s exact layering to this epic (EXR-14 defers it);
the document scope is not layered for now, and an `execute:` edit re-executes (KNR-3
decision 3, settled 2026-10-31; revisited after S8).

**What the consumers do with knitr units today.** EXR-14 splices recorded unit outputs into
the live pre-engine AST by unit id, matching on `quarto_exec_hash` first (parent decision
34); for knitr (parent epic) the output is stored markdown. After this plan the per-unit
output is produced by the presenter with the unit\'s resolved options, the current header
options and the target format. EXR-15\'s staleness compares the execution-input hash;
EXR-11-era records carry the old declaration version and read as stale (KNR-3).

## Design decisions (from the epic; do not re-litigate)

1. One flip, no runtime switch; shadow equivalence on the corpus before it (the
   [parent pattern](2026-10-28-exr-06-jupyter-flip.md)).
2. knitr stays whole-document (decision 7); fallback units keep passing through the
   pass-through presenter, which KNR-2 builds (Phase 2F; decision 8). Old knitr traces do not
   use it (see provisional decision 2).
3. `ReplayEngine` and the trace stay (parent decision 9).

## Provisional: decisions this plan must make

1. **Skeleton (markers) versus Rust assembly** *(S7, settled: markers)*. Chunks are markers.
   Inline substitution is owned here: classic spans stay on the live R path and R substitutes
   them (Gordon, 2026-11-02; they are not units). This phase decides, and writes down, whether
   brace spans are substituted at present time from the record or carried by markers too.
   KNR-4 builds the record side and calls `presenter/inline.rs`.
2. **Old knitr traces** (settled, epic decision 14, corrected 2026-11-01 by interlude I2 and
   confirmed by Gordon). A trace recorded before the flip holds a markdown result and no
   record. It keeps replaying, unlike Jupyter (parent decision 9), and it does so with no
   presenter: `ReplayEngine::execute` returns the stored markdown verbatim, as it does today.
   The pass-through presenter (KNR-2 Phase 2F) matters only for fallback units inside a
   record. What this plan adds is one shared presenting function (Gordon, 2026-11-02): it
   generalizes `present_record`, which today is Jupyter-specific (`engine/replay.rs:113-157`,
   `execute` gated on `engine_name == JUPYTER` at line 228), and both live knitr rendering and
   replay call it. Engine differences (where options come from, the error text) are inputs,
   not a branch on engine name. It runs when the capture has a record, and the replay of an old
   trace keeps its test. The I2 prototype (`spike/knr-i2`) is a hint only; it predates KNR-2\'s
   `CellOptions`. The `{r}` claim fix that makes any knitr replay work
   (about 20 lines, `ReplayEngine::claims_language` claims what knitr claims through a shared
   `knitr_claims` function) is bd-js24dold and lands before KNR-3, not with the flip.
3. **Target format and execute scope at replay.** Parent decision 27 is settled: the record
   carries no presenter inputs and stays format-neutral, and replay supplies the format and
   scope again (in the trace if `ExecutionContext` does not reach replay). For knitr the
   format also shaped the *pieces* (kable and `df_print`), which conflicts with that rule;
   KNR-3 decision 8 owns the resolution, and this plan implements what it decides.
4. **What happens at the first flipped release to existing records.** EXR-11-era records
   read as stale; consumers re-record on the next preview. Phase 6B checks, by one test per consumer (preview, hub, replay), that each reads a
   stale record without an error.
5. **The corpus bar for flipping**: zero blockers (KNR-2b) *and* a named list of accepted
   intentional divergences, signed off, because users will see them.
6. **The retained fallback (settled by Gordon, 2026-11-02; epic decision 24).** Structured
   chunks return a marker in the skeleton, and R also ships each chunk\'s finished markdown
   in the result, keyed by unit id (EXR-11\'s `chunks[].markdown`, kept). Rust substitutes
   each marker with the presenter\'s output, and with that chunk\'s shipped markdown when
   the presenter cannot produce it: `pieces_to_cell` returns `None` (an `Other` piece, an
   image that is not png, jpg or svg, an unreadable file, a file outside the document
   directory), or there is no record at all (a count mismatch or a malformed one). The
   render succeeds in each case. Consequences this plan writes down: a missing record is no
   longer a marker-only document (this answers whether the render fails; it does not, it
   falls back for the whole document); the legacy capture path that reads `result.markdown`
   keeps working only if that field holds final text, so 6B states which it holds while the
   record-writing gate exists; and each Rust-side fallback is counted (6B), which is the
   evidence KNR-8\'s trigger reads. Brace spans keep the same fallback (Gordon, 2026-11-02):
   R also ships each span\'s finished inserted text or fragment, keyed by unit id. It is a
   fragment, not plain text, because a span can be rich (an htmlwidget or an image; KNR-4
   decision 6), so the substitution is an inline fragment, and an unmappable span output is
   counted like a chunk\'s. KNR-8 drops it with the chunk markdown.

## Out of scope

- Single-cell execution; freeze; removing `ReplayEngine` or the trace (the epic's
  out-of-scope list); the legacy capture reader (EXR-17).

## Phases (provisional)

### Phase 6A: tests first

- [ ] The corpus shadow test promoted to a gate: presenting the record equals the live
  markdown (the frozen oracle KNR-3 3C committed) under epic decision 26: Identical and
  SyntaxLevel pass, anything else needs a signed-off entry, non-HTML formats are compared at
  the AST level, on the minimum and the latest supported knitr. It is the KNR-3 3C test
  promoted, not a second harness.
- [ ] Replay equals live, through `ReplayEngine` (`tests/integration/knitr_replay_equals_live.rs`,
  classified by `knitr_corpus_classify` under decision 26). Half done 2026-11-05:
  `a_replayed_knitr_record_presents_what_the_live_engine_produced` (chunks only) passes today, and
  the old markdown trace's unchanged replay is `replay_knitr_trace_unchanged`. The other half,
  `a_replayed_knitr_record_keeps_classic_span_values`, is red and committed `#[ignore]`d: replay
  presents over the *input* text, so a classic `` `r 1 + 1` `` span comes back unevaluated
  (live: `After 2 inline.`; replay: ``After `r 1 + 1` inline.``). **Consequence for 6B**
  (settled above, point 4): replay presents over `skeleton.text`, a new result field, and
  `result.markdown` holds the final text. The flip commit removes the `ignore`.
- [x] A trace whose capture is loaded but never executed (an unclaimed language) fails the
  render with a diagnostic naming the engine; today it renders the code unexecuted with exit
  0 (interlude I2). Done 2026-11-05 (Gordon chose error over warning):
  `test_replay_capture_that_never_runs_fails_the_stage` (red first, then green);
  `ExecutionEngine::is_unused_replay`, `EngineRegistry::unused_replay_engines`, and
  `fail_on_unused_replay` in `EngineExecutionStage` on the empty-sequence and normal exits. The
  `.md` and execution-policy skips do not trip it. Replay only; ordinary renders never reach it.
- [ ] Marker survival tests (settled design, point 7): a block marker on docx, typst and revealjs,
  and an inline marker on knitr 1.44 and 1.52, survive `rmarkdown::render` byte for byte. The
  markers are exercised through the real engine once the flip emits them, so these start as
  `#[ignore]`d red tests like the replay span test.
- [ ] Consumer tests: preview splice of a knitr unit by id; per-unit staleness; hub guest
  shows the host's knitr record; hub admin inventories it.
- [ ] A document with a setup chunk setting `opts_chunk` presents the same as before (the
  resolved-options layering), and the layering tests of KNR-7: a header-option edit shows
  without re-running, a setup-chunk edit does not show until the next run.
- [ ] A header option deleted from a knitr chunk marks that unit stale and re-runs ([KNR-3 slice 8](2026-11-04-knr-slice-8-header-removal.md)); a setup-chunk edit re-runs the whole document only under `EnginePolicy::Auto` in the preview, and otherwise leaves its neighbors showing the old record (state this in the user-facing behavior). Prerequisite found in slice 8: `record_is_stale` hashes every unit under the default profile, so a real knitr record reads stale even unchanged; hash profiled cells the way the writer did (their `quarto_hash_rule` and the format) before this item can pass. Done in [KNR-3 slice 9](2026-10-30-knr-03-record-adapter-hash-fallback.md) (2026-11-04): `record_is_stale` hashes a profiled cell under its recorded profile, so a real knitr record reads fresh when unchanged.
- [ ] Fallback chunks (`child=`, cache hit, overridden hook) in a mixed document present
  exactly as before.

### Phase 6B: the flip (one commit)

- [ ] The R side returns the skeleton (or units, per decision 1), the record, and the finished
  markdown of every chunk (decision 6); the Rust engine presents and substitutes that markdown
  for any chunk it cannot present. Tests first: a gif, an unreadable image, an out-of-directory
  `include_graphics`, an `Other` piece and a count mismatch each render the chunk\'s markdown,
  not an empty marker, and the render succeeds.
- [ ] The shared figure sink honors `quarto_file_path` (epic decision 25). Tests first: a knitr
  figure keeps its own name, directory and `fig.path`; a colliding path and a path outside the
  document directory fall back to the generated name and are counted; Jupyter output keeps its
  names. Knitr\'s own copies are kept (settled design, point 5).
- [ ] A counter of Rust-side fallbacks per render, by reason (unmapped output, unreadable file,
  no record). It is logged at debug level and written into the trace, never shown to users as a
  warning. It is the instrument KNR-8\'s trigger reads; a test asserts it counts the cases in
  the item above.
- [ ] Preview, hub and replay read structured knitr records. Replay: dispatch on
  `capture.record.is_some()` into the shared presenting function of decision 2; a test replays
  a knitr record and equals the live output, and the existing Jupyter replay tests stay green;
  `capture.result.markdown` holds the final text and the skeleton rides in `result.skeleton` (settled design, point 4). The unused-capture check
  (6A) and "replay never writes a trace" (below) are done.
- [ ] Update this epic with the knitr outcome. The parent\'s text is not edited; tell the
  parent session what changed.

### Phase 6C: deletions (separate commits, each green)

- [ ] (Narrowed, settled design point 6: the `.QuartoInlineId` hand-off and the span fallback
  records stay until KNR-8.) EXR-11\'s code that records
  every chunk **as a markdown unit in the record** goes, since KNR-3\'s adapter has written
  structured units since its Phase 3B; its per-chunk markdown **on the result stays** (decision
  6), and so does the chunk hook for every chunk. Narrowing them to fallback chunks is KNR-8.
  List the deletions explicitly at that point, from what is unreferenced: `engine/knitr/chunk_record_tests.rs`
  for the removed code, and any helper nothing calls once the flip has landed. `insert_unit_ids`,
  `SpanPlan::new`, `quarto_span_records` and `quarto_flush_span_records` stay while the hand-off does.
- [ ] Record in this epic that the knitr engine now migrates through the record (EXR-17
  trigger item 1); EXR-17 itself is not edited.
- [x] The `quarto-unit-id` option hand-off for structured chunks is **kept** (markers need it);
  settled 2026-11-05.

## Exit criteria

- The flip gate passes on the corpus; the sign-off list of divergences is committed.
- No consumer reads the deleted code; `ReplayEngine` and the trace still work.
- A chunk Rust cannot present still renders through the retained markdown, and the fallback
  counter records it.
- clippy and nextest green; counts against the live baseline; the workspace-wide run is
  recommended in the pre-push summary because `quarto-core` has dependents.

## Risks

1. **Skeleton markers collide** with user content or are rewritten by a later R step.
2. **R-side state invisible to the document** (the exception above) is the likeliest source
   of subtle regressions; the corpus must include `opts_chunk$set` in setup chunks.
3. **Users with custom hooks** see changed output; the diagnostic and the fallback are the
   mitigation.
4. **Two mechanisms alive** is now deliberate for the shipped chunk markdown (decision 6),
   and bounded: it is a fallback read only when Rust cannot present a chunk, never a second
   presentation of a chunk that did present. A test must show a presented chunk never reads
   its shipped markdown. Removal is [KNR-8](2026-10-30-knr-08-fallback-removal.md).
5. **Caches and the flip (corrected 2026-11-05).** There is no pre-flip-cache problem: capture turns
   knitr\'s cache off for every chunk on a structured route (`capture_install`, S4), so an old
   markdown cache is never replayed there; a fallback target (latex) has no markers and keeps its
   cache. The real user-facing cost is that `cache: true` chunks stop caching on structured targets
   once recording is the default (info diagnostic Q-18-11 already says so). Pinned by
   `a_cache_written_without_capture_is_not_replayed_by_a_recorded_run`.

## References

- [EXR-6](2026-10-28-exr-06-jupyter-flip.md), [EXR-14](2026-10-28-exr-14-preview-splice.md),
  [EXR-15](2026-10-28-exr-15-hub-integration.md), [EXR-17](2026-10-28-exr-17-legacy-removal.md)
- `crates/quarto-core/src/engine/{knitr/mod.rs,replay.rs}`; `claude-notes/instructions/replay-engine.md`
