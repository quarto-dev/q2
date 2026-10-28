---
title: 'EXR-10: knitr K0, the per-cell options map and canonical recorded input'
date: 2026-10-28
description: 'Moves knitr label re-injection from the shared engine stage to the engine boundary, passing consumed cell options as a map on the execution context so the recorded input is canonical and staleness checks stop false-positiving.'
status: draft
braid:
  strand: bd-n1vmlalw
  priority: P2
  labels: [engine]
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (plan EXR-10,
knitr phase K0).
**Depends on:** nothing in this epic. It can land at any time.
**Blocks:** [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md) (the record
needs a canonical input), and bd-6sb1z0i4 (extend label naming to Jupyter and
drop the `engine.name() == "knitr"` gate), which must not land before this. Per the dependency table in the epic, soft prerequisite of EXR-14.
**Lane:** C (`workspace-6`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 1 to 2.

> **For implementers:** TDD, task by task; steps use `- [ ]` checkboxes. Tests
> that need R skip when it is missing (see Background).

## Goal

The execution stage records the **canonical** serialized document (no
knitr-transport detail) as the engine capture\'s `input_qmd`, and hands the
engine the cell options the crossref pass consumed as a structured map. The
knitr engine re-adds the chunk-header line to the text it passes to R. After
this, a freshly recorded labeled knitr document is not stale, and every
engine\'s recorded input has the same meaning.

## Background

Verified in code on 2026-10-28 (workspace-1 at `2ee0bd966`) unless marked.

- **The design flaw.** `EngineExecutionStage` calls
  `label_reinject::inject` on the per-engine masked AST clone when
  `engine.name() == "knitr"`
  (`crates/quarto-core/src/stage/stages/engine_execution.rs:526-531`), then
  serializes that AST (`:536`). The same `qmd` string is later written into the
  capture payload as `input_qmd` (`:656-660`). So a knitr-transport detail (the
  re-added `#| label:` line) becomes part of the recorded identity.
- **Consumers of `input_qmd` identity.**
  - Capture splice keys cells by a hash of their text and now calls
    `label_reinject::strip` on the recorded input to undo the injection
    (`stage/stages/capture_splice.rs:262-271`, the bd-mu9i0bct fix).
  - `ReplayEngine` byte-compares the live input with the recorded one
    (`engine/replay.rs:130`).
  - The preview staleness check compares `compute_input_qmd`
    (`engine/preview_record.rs:209`) with the recorded `input_qmd` byte for byte
    (`crates/quarto-preview/src/capture_driver.rs`, `recompute_staleness` starts at `:226`
    and the byte compare is at `:283`).
    `compute_input_qmd` stops at `PreEngineSugaringStage`, so it never has the
    injected line. The strand (bd-n1vmlalw) reports that a fresh labeled knitr
    recording therefore looks stale immediately and the strip does not fix it;
    reproducing that with a knitr-named fake engine is the first RED test below.
- **The inject and strip module.** `crates/quarto-core/src/crossref/label_reinject.rs`
  (304 lines): `inject` prepends `#| label: <id>` to a code block directly
  wrapped by a Div whose `id` classifies as a crossref label; `strip` is its
  exact inverse (`:59-69`); both walk via `for_each_wrapped_cell`.
- **`ExecutionContext`** (`crates/quarto-core/src/engine/context.rs:29`) is a
  builder-style struct (`with_handled_languages` at `:242`, and so on). It has no
  per-cell options field today.
- **Jupyter already partitions options per cell.**
  `engine/jupyter/text_execute.rs:311-322` calls `partition_cell_options` on each
  block, so it can consume a map directly later (bd-6sb1z0i4).
- **knitr never reads `ctx.source_info`**, so shifting offsets in its input is
  harmless (`engine/knitr/preprocess.rs`, "This pass shifts byte offsets").
- **R gating in tests.** `tests/integration/knitr_label_reinject.rs:37-58`
  defines `rscript_available`, `knitr_r_package_available` and
  `knitr_render_available`; each test calls `skip(name)` and returns early with a
  `SKIP:` line when `Rscript` or the knitr package is missing.
  `knitr_inline_expressions.rs` gates through
  `EngineRegistry::default().get("knitr").is_some_and(|e| e.is_available())`.
- **Open item (not verified):** how the `::: {#id}` wrapper Div serializes in every
  nesting (lists, blockquotes), which matters for the text-level re-insertion below.
  `for_each_wrapped_cell` recurses over the same containers as `desugar_blocks`, so
  the test corpus there is the starting point; the nesting corpus in Phase 10A turns
  this into a gating test.

## Design decisions

From the strand and the epic; do not re-litigate:

1. **The stage records the canonical sugared text** (no re-injection) and passes
   consumed per-cell options to the engine as a map on `ExecutionContext`, label
   first.
2. **Key labels by the wrapper Div id** (exact; confirmed by Gordon, 2026-10-30). It is
   stable under reordering and independent of scanner counting; an ordinal would couple
   this plan to the agreement risk EXR-2 exists to contain. The cell ordinal from the
   shared scanner is the stated fallback only if the nesting corpus below fails.
3. **`KnitrEngine::execute` re-adds the chunk-header line** to the text it hands
   R, producing exactly the text `inject` produces today.
4. **`label_reinject::strip` stays as the compat path** for stored captures that
   were recorded with the line present (older recordings, older hub docs).
5. **No behavior change for Jupyter or TS engines.** The map is carried for
   every engine but only knitr consumes it here; dropping the knitr-only gate is
   bd-6sb1z0i4.

## Out of scope

- Label-based output naming for Jupyter and the gate removal (bd-6sb1z0i4).
- Any record or hash work (EXR-11 consumes this plan\'s canonical input).
- Line-number mapping for knitr errors (bd-1ng2ivis; touched in EXR-11).

## Phase 10-0: baseline

- [ ] Before any change, record the live baseline: the count of labeled cells and
  wrapper Divs that `for_each_wrapped_cell` finds over the knitr fixtures and the
  `label_reinject.rs` corpus, and the pass and skip counts of
  `cargo nextest run -p quarto-core` on the unchanged tree. Phase 10C compares against
  these figures (global rule: never copy an old figure).

## Phase 10A: tests first

- [ ] **Locator stub (first).** Add the signature the nesting corpus targets, as a stub
  that panics with `unimplemented!()` so the corpus test fails for the right reason:
  `locate_cell_by_div_id(text: &str, id: &str) -> Option<CellSpan>`, where `CellSpan`
  holds the byte range of the wrapper Div and of its code fence. Phase 10B implements
  it and uses it for the re-insertion. It serves crossref-labeled cells only: an
  unlabeled chunk has no wrapper Div, so EXR-11 inserts unit ids by the unit\'s `line`
  instead.
- [ ] **Nesting corpus (gating).** `wrapper_div_id_survives_serialization_in_every_nesting`:
  a wrapped labeled cell at top level, inside a bullet list, inside an ordered list,
  inside a blockquote, inside a fenced div, and in nested combinations (start from the
  `desugar_blocks` and `for_each_wrapped_cell` corpus). For each, serialize the canonical
  AST and assert `locate_cell_by_div_id` finds exactly the wrapper Div id and its code
  fence. If any case fails, switch to the scanner-ordinal fallback in the Risks section
  before Phase 10B.
- [ ] `fresh_labeled_knitr_capture_is_not_stale` (integration, no R needed): with
  a knitr-named fake engine, a document containing `#| label: fig-x` and
  `#| fig-cap: ...`, record a capture and assert
  `compute_input_qmd(...) == capture.input_qmd`. RED today (the strand\'s repro).
- [ ] `canonical_input_has_no_reinjected_label`: stage-level, the recorded
  `input_qmd` contains no `#| label:` line the author did not write.
- [ ] `cell_options_map_carries_consumed_labels`: for a document with two labeled
  cells in different containers, the map on `ExecutionContext` has exactly the
  two entries keyed by wrapper Div id, in document order.
- [ ] `reinsert_matches_inject_on_the_existing_corpus` (unit): reuse every
  `inject` test input from `label_reinject.rs`; the new engine-boundary
  re-insertion over the *canonical* text plus the map yields byte-identical
  text to the old `inject` path.
- [ ] `reinsert_is_a_noop_without_map_entries`: an empty map returns the input
  unchanged, including unlabeled and hand-written `label:` cells.
- [ ] Real R, gated on `knitr_render_available()`: the three existing tests in
  `knitr_label_reinject.rs` stay green unchanged
  (`labelled_figure_uses_label_derived_filename`,
  `label_option_is_not_echoed_in_rendered_source`,
  `labelled_cached_chunk_uses_label_derived_cache_key`).
- [ ] `capture_splice_knitr_label.rs` stays green, plus a new case: a capture
  recorded after this plan (no injected line) splices with `strip` a no-op, and an
  old-style capture (line present) still splices.
- [ ] `labeled_jupyter_doc_is_not_stale_after_record`: written now, marked as the
  enabling test for bd-6sb1z0i4 (with the gate removed in the test setup only, a
  labeled Jupyter document records a capture that is not stale).

## Phase 10B: implementation

- [ ] Add `CellOptionsMap` (an ordered list of `{div_id, label}` entries) to
  `engine/context.rs` with a `with_cell_options` builder and a rustdoc contract.
- [ ] Add `label_reinject::collect(doc, registry) -> CellOptionsMap` using the
  existing `for_each_wrapped_cell` walk (no behavior change to `inject`/`strip`).
- [ ] In `engine_execution.rs`: build the map from `masked_ast` before
  serialization, delete the `engine.name() == "knitr"` injection block, pass the
  map via `ExecutionContext`.
- [ ] In `engine/knitr/mod.rs` `execute`: after the inline-expression step,
  re-insert chunk-header lines for map entries (implement `locate_cell_by_div_id`
  from 10A: locate the wrapper Div by its
  exact id, then its code fence, then prepend the `#| label:` line in the cell\'s
  comment syntax). Make it a pure function so the equivalence test above runs
  without R.
- [ ] Keep `inject` only if still needed by tests or tools; otherwise remove it
  in this plan and keep `strip`.

## Phase 10C: verification

- [ ] Real-R run of the knitr integration tests that exist today
  (`knitr_label_reinject`, `capture_splice_knitr_label`, `knitr_display_fence`,
  `knitr_inline_expressions`), with R present.
- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core`.
- [ ] Record the pass and skip counts, and the cell and wrapper-Div counts, against the
  Phase 10-0 baseline in the commit message.

## Exit criteria

- A freshly recorded labeled knitr document is not stale (RED test now green).
- Recorded `input_qmd` equals `compute_input_qmd` for knitr, as for other engines.
- The old injected-line captures still splice and replay (compat path intact).
- bd-6sb1z0i4 is unblocked.

## Risks and open questions

- **Text-level re-insertion is the fragile step.** Locating a Div and its fence in
  serialized text by id must handle nesting; the golden-equivalence test over
  the existing corpus is the guard. Fallback if it proves brittle: key by the
  cell ordinal found by the shared scanner ([EXR-2](2026-10-28-exr-02-unit-scanner.md))
  with a recorded code hash as an alignment assertion.
- **Hash compatibility (decided in EXR-4; affects EXR-4 and EXR-16).** Positron\'s
  `quarto_content_hash` is over the raw lines between the fences of the *source
  file*, including `#|` lines. q2\'s canonical qmd has already had `label:`
  consumed, so hashing the canonical text would not match Positron\'s hash for
  labeled cells. The record builder therefore hashes the author\'s source cell text
  for `quarto_content_hash` (re-injecting the label is not enough, because authors may
  put `#| label:` after other options), and uses the canonical text plus the options for
  `quarto_exec_hash`. Per epic decision 36 the engine-execution stage computes the
  author-text hashes from the original text range (or the pre-sugaring AST) and passes
  them to the record builder; choosing that mechanism is EXR-4\'s first task, and this plan
  does not depend on it.
- **Old hub documents** still hold injected-line captures; `strip` must stay
  until the legacy path retires ([EXR-17](2026-10-28-exr-17-legacy-removal.md)).

## References

- bd-n1vmlalw (this work), bd-6sb1z0i4 (follow-up, ordering constraint),
  bd-mu9i0bct (the strip fix), bd-2lxj10z0 (label re-injection origin,
  `claude-notes/plans/2026-09-28-knitr-label-preengine-sugaring-fix.md`)
- `crates/quarto-core/src/{crossref/label_reinject.rs,stage/stages/engine_execution.rs,engine/context.rs,engine/preview_record.rs}`
- `crates/quarto-core/tests/integration/{knitr_label_reinject,capture_splice_knitr_label}.rs`
