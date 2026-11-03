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
  such a chunk also becomes a fallback unit (S7 tests it).
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
- [ ] `replay_knitr_record_runs_presenter`: replay returns the stored record and the
  presenter output equals the live output; an old markdown trace still replays unchanged,
  by returning its stored markdown with no presenter (decision 2). A scratch version of both
  is in `spike/knr-i2` (`engine/replay.rs` tests).
- [ ] A trace whose capture is loaded but never executed (an unclaimed language) fails the
  render with a diagnostic naming the engine; today it renders the code unexecuted with exit
  0 (interlude I2).
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
  names. State whether knitr\'s own copies are deleted after Rust reads them, so `_files` holds
  no duplicates.
- [ ] A counter of Rust-side fallbacks per render, by reason (unmapped output, unreadable file,
  no record). It is logged at debug level and written into the trace, never shown to users as a
  warning. It is the instrument KNR-8\'s trigger reads; a test asserts it counts the cases in
  the item above.
- [ ] Preview, hub and replay read structured knitr records. Replay: dispatch on
  `capture.record.is_some()` into the shared presenting function of decision 2; a test replays
  a knitr record and equals the live output, and the existing Jupyter replay tests stay green;
  state what `capture.result.markdown` holds after the flip (skeleton or final text). Fail when a loaded capture is
  never executed (an unused-capture check; today an unclaimed `{r}` chunk renders unexecuted
  with exit 0). Also: replaying a document that has `trace: true` overwrites the trace being
  replayed at the default path (found in I2, recorded on bd-js24dold); either the replay does
  not write a trace or the replay doc says to copy the trace aside.
- [ ] Update this epic with the knitr outcome. The parent\'s text is not edited; tell the
  parent session what changed.

### Phase 6C: deletions (separate commits, each green)

- [ ] EXR-12\'s `.QuartoInlineId` embedding, and the `spans` wire field once nothing reads it
  (keep whatever the inline fallback of decision 6 still needs). EXR-11\'s code that records
  every chunk **as a markdown unit in the record** goes, since KNR-3\'s adapter has written
  structured units since its Phase 3B; its per-chunk markdown **on the result stays** (decision
  6), and so does the chunk hook for every chunk. Narrowing them to fallback chunks is KNR-8.
  List the other deletions explicitly: `engine/knitr/chunk_record_tests.rs` for the removed
  code, `insert_unit_ids` and `SpanPlan::new` if unused, and on the R side
  `quarto_span_records` and `quarto_flush_span_records` (`execute.R`, `hooks.R`).
- [ ] Record in this epic that the knitr engine now migrates through the record (EXR-17
  trigger item 1); EXR-17 itself is not edited.
- [ ] Decide, from S7\'s verdict, whether EXR-11\'s `quarto-unit-id` option hand-off for
  structured chunks is deleted (Rust assembly, or ids ride in the unit list, S9) or kept
  (markers need it), and write the outcome here.

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
5. **Caches written before the flip** hold markdown and no markers; each such chunk is a
   fallback unit until the cache is rebuilt, which may surprise users with large caches.

## References

- [EXR-6](2026-10-28-exr-06-jupyter-flip.md), [EXR-14](2026-10-28-exr-14-preview-splice.md),
  [EXR-15](2026-10-28-exr-15-hub-integration.md), [EXR-17](2026-10-28-exr-17-legacy-removal.md)
- `crates/quarto-core/src/engine/{knitr/mod.rs,replay.rs}`; `claude-notes/instructions/replay-engine.md`
