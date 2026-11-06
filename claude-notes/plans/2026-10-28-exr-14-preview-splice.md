---
title: 'EXR-14: preview splices records by unit id'
date: 2026-10-28
description: 'Adds the id-based, lenient preview splice for engine execution records, including inline expression spans, per-unit staleness, and masking-safe unit identity, while the legacy structural splice stays frozen for old captures.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** [EXR-13a](2026-10-28-exr-13-hub-wire.md) (the reader half: payload reaches the pipeline; not the writer half),
[EXR-2](2026-10-28-exr-02-unit-scanner.md) (unit identity),
[EXR-4](2026-10-28-exr-04-record-format.md) (hashes, the record type, and `RecordBuilder`
/ `EngineRecord`, which the fixtures use),
[EXR-1](2026-10-28-exr-01-presenter-extraction.md) (WASM-clean presenter).
Soft: [EXR-10](2026-10-28-exr-10-knitr-k0-options-map.md) (canonical recorded input for knitr labels),
[EXR-6](2026-10-28-exr-06-jupyter-flip.md) and [EXR-12](2026-10-28-exr-12-knitr-k2-inline.md)
(end-to-end checks: the flip for live-versus-preview parity, knitr spans for the span fixtures).
**Blocks:** [EXR-15](2026-10-28-exr-15-hub-integration.md), [EXR-17](2026-10-28-exr-17-legacy-removal.md)
**Lane:** D (`workspace-7`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 4 (lookup and stage: 1.5; spans: 1; masking and conformance: 1; WASM and e2e: 0.5)

> **For implementers:** TDD, task by task. Ships in the same release as EXR-13
> (the reader release).

## Goal

When `q2 preview` or the hub client renders an edited document that has a record,
show the recorded output for every unit whose identity still matches, and leave
the rest as source, exactly as the legacy splice does for cells today, but:

- matching by hash (the execution hash first, the Positron-compatible content hash as
  the fallback; decision 34), not by parsing and diffing the recorded input against the
  recorded output;
- including inline expression spans, which the legacy splice cannot represent;
- reporting per-unit staleness (fresh, stale, orphaned) for later UI, with no UI here;
- giving the same answer on native and WASM, with the nested-cell mask applied by this
  plan's own record arm (the shared pre-engine stages do not apply it).

One scope limit, pre-existing and not a regression of this plan: the preview watcher does
not follow includes. `on_file_changed` (`crates/quarto-preview/src/lib.rs:484-510`) calls
`recompute_staleness` only for the path that changed, so an edit to an included file or to
`_quarto.yml` never recomputes the includer's staleness. Everything this plan says about
staleness and re-splicing holds for edits to the document itself (and for any recompute
that something else triggers); it does not promise that an edit to an include refreshes the
preview. Fixing the watcher is outside the epic.

## Background (verified in code on 2026-10-28, workspace-1 at `2ee0bd966`)

**The legacy splice.**

- `crates/quarto-core/src/engine/capture_splice.rs` (1088 lines). It parses the
  recorded `input_qmd` (A1) and the recorded result markdown (B1), walks them in
  parallel, and builds a `CellOutputMap` (`:189`, `derive_cell_outputs` `:233`)
  keyed by `(structural_hash(cell), occurrence_index)`. `apply_capture_splice`
  (`:448`) replaces each matching engine cell in the live pre-engine AST (A2) by its
  output run; `apply_capture_splices` (`:469`) folds an engine sequence.
  `engine_cell_lang` (`:86`) identifies engine cells. It is fail-soft by design.
- `CaptureSpliceStage` (`crates/quarto-core/src/stage/stages/capture_splice.rs`):
  materializes embedded files with `materialize_capture_files` (bd-qbhp2cvv),
  folds the capture's `result.includes` into `meta.rendered.includes.*`
  (`fold_capture_includes_into_meta`, bd-5oyk1xce, the preview channel for e.g. the
  marimo header script), and calls `label_reinject::strip` on A1 (`:271`,
  bd-mu9i0bct). It reads only `result.markdown` and `result.includes` from the
  capture (verified by grep: those are the only `result.get` uses).
- The stage is inserted before `EngineExecutionStage` by `insert_capture_splice_stage_in`
  (`crates/quarto-core/src/pipeline.rs`, near `:930`), which also rebuilds the engine
  stage with `with_spliced_engines` to suppress the "(no execution)" warning.

**The mask.** `crates/quarto-core/src/engine/nested_cell_mask.rs`:
`mask(&mut Pandoc)` (`:189`) and `unmask` (`:238`) rewrite executable-fence
openers and inline expressions found inside display blocks, so no scanner claims
them. `MASK_OPENER_RE` and `MASK_INLINE_RE` (`:123`) are deliberately a union of
the separator classes of the scanners. Only two places call `mask`: `compute_input_qmd`
(`crates/quarto-core/src/engine/preview_record.rs:209`, the call is at `:245`) masks before
serializing the recorded input, and `EngineExecutionStage` masks a clone of the AST for each
engine (`crates/quarto-core/src/stage/stages/engine_execution.rs:515`) and unmasks the
result (`:623`). **Nothing in the pre-engine stages calls it**, and
`crates/quarto-core/src/stage/stages/capture_splice.rs` does not mention it. So the splice
stage sees the unmasked AST, and a display block's nested `{python}` fence or span is still
a live-looking node there. The record arm (Phase B) must mask a clone of the AST before it
scans, or it would claim units the writer never saw.

**Identity inputs.** Positron's content hash is SHA-1 first 16 hex characters over the
raw lines between the fences, including `#|` lines
(`external-sources/positron/src/vs/workbench/contrib/positronQuarto/browser/quartoDocumentModel.ts:22`);
EXR-4 adopts it as `quarto_content_hash`, the fallback key (and the only key for imported
Positron records, which have no execution hash). The primary key is `quarto_exec_hash`
(EXR-4, decision 34), which leaves out presentation options such as `echo`. The unit hash
holds no kernel (EXR-4: `quarto_kernel` is provenance only), so a viewer with no engine
loaded can recompute it from the unit and the stored hash definition. Which hash definition
applies to a document unit: the one stored in the `quarto_engines` entry whose recorded
owned-language set contains the unit's language (EXR-4 stores each engine's owned set
there), so a multi-engine document hashes each unit under its own engine's declaration.

The live AST does **not** reliably hold those lines. Crossref sugaring strips the
`#| label:` line and the `<reftype>-cap` lines from the code block text and wraps the cell
in a Div (`crates/quarto-core/src/crossref/codeblock_shorthand.rs:182-237`,
`strip_consumed_lines` at `:237`); `source_info` is kept (`:247`, `:268`), but
`body_source_for` (`:669-697`) locates the body with `find(&cb.text)` and may fall back to
the whole block after a mid-body strip (reported, not verified). The knitr label
re-injection (bd-n1vmlalw) is a second rewrite, in the engine stage after the splice stage.
So the writer computes author-text hashes in the engine-execution stage from the original
text range (decision 36; EXR-4's first task picks the mechanism), and the splice must hash
**the same range the writer did**, by the same mechanism, never `CodeBlock.text` as it
stands in the AST.

**Span representation (verified 2026-10-29).** Pampa gives a span an inline `Code`
node whose text is the language in braces, a space, then the expression; the escaped
form with doubled braces stays literal text, and the classic `r` spelling is plain
`Code` text with no prefix. **Not verified, first task of Phase B:** the same shape
inside a heading, a table cell and an attribute value, and whether `ctx` at the splice
stage can see the mask's bookkeeping (it cannot reach the engine stage's clone, so the
record arm masks its own). The text-based scanner of EXR-2 and an AST walker
must agree; that agreement is the main risk of this plan.

**Language tokens (verified 2026-10-29).** `python.marimo` (no space) is one language
token in pampa, and `{python .marimo}` is language `python` plus first class `marimo`;
Positron reads both as `python`. Neither the hash nor the id contains the language, so
matching is unaffected; the unit's language matters only to the importer (EXR-16). The
token rule is EXR-2's.

## Design decisions (from the epic, do not re-litigate)

1. **Lenient matching.** Preview shows any unit whose hash matches. Reuse in the
   freeze sense is out of scope; nothing here skips execution.
2. **Execution hash first (decision 34).** Match a record cell to a document cell by
   `quarto_exec_hash`. `quarto_content_hash` (Positron-compatible, includes the `#|`
   lines) is the fallback when no execution hash matches, and the only key for imported
   Positron records. Duplicates (several cells with the same hash) match by occurrence:
   the k-th document unit with hash H (counting in document order) takes the k-th record
   cell with hash H (counting in record order). The index inside `quarto_cell_id` is never
   consulted, because it shifts when a cell is added or removed. A matched record cell is presented with
   the **current** cell's options and the current document scope, not the recorded
   ones, so an `echo` edit neither re-executes nor strands the cell.
3. **Spans** are keyed by `(lang, code, occurrence)`, with `lang` lowercased (the scanner
   compares language tokens case-insensitively, and the writer lowercases it too; EXR-4):
   a new splice mode, since span outputs are inline values and not block runs. The
   occurrence number counts spans with the same lowercased language and the same code in
   the order of the shared scan, which includes any span claimed in the document's own front
   matter (it is serialized first), so the record arm must number occurrences from the same
   scan list and never from a walk of the body alone. A span's id is
   `{n}-{hash8}-inline` with one counter over all claimed spans in the document, whichever
   engine owns them (decision 38: the stage scans units once and passes one list to every
   engine), so a span never shifts a cell index; the index is never used for matching.
   A front-matter span (claimed only when the value comes from the document's own front
   matter; decision 41 and the provenance rule in EXR-2) is a unit with an id and a
   counter number, and its recorded output is kept in the record, but it is **never
   spliceable**: the body splice has no slot for it. Spans in project metadata, in
   `_metadata.yml` and in extension or format defaults are not claimed, so they take no
   number.
4. **Per-unit staleness** is `fresh` (document unit matched by either hash),
   `stale` (document unit that no record cell matches by either hash), `orphaned` (record
   cell no document unit matches). A cell is `stale` only when no hash matches. Computed
   always, displayed nowhere in this plan. It is a
   different question from **record-level staleness** (does the record still describe
   this document), which comes from the execution-input hash and belongs to EXR-15;
   this plan does not change it.
5. **Legacy path frozen.** A legacy payload goes through `apply_capture_splices`
   unchanged. A record never reaches it and a legacy payload never reaches the new
   code; the `CapturePayload` enum from EXR-13 enforces this by type.
6. **Same ids on both sites.** The splice takes its units from the shared scan (EXR-2's
   `scan_units` with EXR-4's id function, decision 38) over the masked pre-engine
   document on native and WASM, so units, ids and hashes equal the writer's; conformance
   fixtures prove it. The scan applies no ownership filter (decision 42), so the cell index
   in an id counts every braced non-raw fence in document order, whichever engine owns it
   (EXR-2, EXR-4), and the span counter is document-wide.
   Ids share Positron's format and content hash but not necessarily its label segment
   or index (decision 39: the label is the first option inside the braces, and Positron
   has no nesting rule), which is why matching uses the hash and never the id.
7. **The record path never depends on the legacy module.** New code here and in EXR-13
   and EXR-15 does not import from `capture_splice.rs`; helpers it needs
   (`engine_cell_lang`) come from the module EXR-2 moved them to. A grep-style unit
   test enforces it, so EXR-17 can delete the legacy splice without touching this
   code. Tests that exercise only the legacy path are named with the prefix
   `legacy_capture_` so EXR-17's inventory is one search.

## Out of scope

- Staleness badges, stale-cell UI, the preview toolbar (UI work).
- Replay parity for filters, metadata, `needs_postprocess` (EXR-15).
- Skipping execution because a record matches (freeze; not in this epic).
- Deleting the legacy splice (EXR-17).

## Phase A: tests first (units)

Fixtures are small records built with EXR-4's `RecordBuilder` (EXR-4 defines `RecordBuilder`
and `EngineRecord`); no kernel needed. In production the `Record` reaches the splice through
the payload EXR-13 reads, and EXR-5's task "Record handoff to the capture writers" is what
puts it there.

- [x] Lookup: a record with cells A, B, C and a document with A, B, C splices all
  three; each output run equals what the presenter produces for that cell.
- [x] Edit one cell: A, B2, C (B2 is B after an edit). A and C splice, B2 is `stale`
  and stays as source, and the old B record cell is `orphaned`.
- [x] Reorder: C, A, B splices all three (position is not part of the key).
- [x] Duplicates: two identical cells (hash H) splice in occurrence order: the first takes
  the first record cell with H and the second takes the second. Deleting the first document
  cell leaves one cell, now the first occurrence of H, and it takes the **first** record cell
  with H (the second record cell is `orphaned`). Cell indexes inside the ids shifted; the
  test asserts they are not consulted.
- [x] Option sensitivity, converse pair. An `echo` (presentation-only) edit to a `#|`
  line keeps the cell matched through `quarto_exec_hash` and the record fresh, and the
  cell is presented with the new `echo` value. An execution-relevant `#|` edit (for
  example `fig-width`) changes the execution hash, matches nothing, and the cell is
  `stale`, stays as source, and the next execution re-runs it.
- [x] Fallback keys: a record cell with only `quarto_content_hash` (an imported Positron
  record) matches by content hash, so an `echo` edit there makes the cell `stale` (the
  content hash includes the `#|` lines); a record cell with both hashes prefers the
  execution hash when the two disagree.
- [x] Presentation uses the current cell: a matched cell whose recorded `echo` was true
  and whose current `echo` is false splices without the source block, and the reverse.
- [x] Spans: a document with `` `{python} a` `` twice and `` `{python} b` `` once
  splices three inline values; editing the second `a` leaves the first matched and
  makes the edited one `stale`; a span of a language the record has no unit for stays
  as source. Language case: `` `{Python} a` `` and `` `{python} a` `` are the same key.
- [x] Front-matter span: a document whose own `title:` holds `` `{python} a` `` and whose
  body holds `` `{python} a` `` has two units with the same key; the record has two. The
  body span is occurrence 1 (the front-matter span is occurrence 0 and is never spliced), so
  it takes the second record unit's value, and the front-matter span's value is not placed
  in the body. A span with the same text in `_quarto.yml` (project metadata) is not a unit
  and takes no occurrence number, so the body span stays occurrence 0 when the span lives
  only in the project metadata.
- [x] Span value kinds: plain text is escaped as text; markdown-mime values are
  parsed as inline markdown; an HTML value becomes a raw inline. These mirror the
  rules EXR-3 defines for live execution, and the test asserts the same expected
  strings for the same inputs (shared table).
- [x] Files and includes: a record with embedded files materializes them through
  `ctx.runtime` before splicing; a record whose notebook-level metadata has
  `include-in-header` content folds it into `meta.rendered.includes.header`
  (parity with `fold_capture_includes_into_meta`).
- [x] Fail-soft: a record cell with an unparseable output, or a record whose units
  are all unmatched, leaves the AST unchanged and never errors.
- [x] Ids: a span added before a cell leaves that cell's id unchanged (span counter is
  separate); a ceded braced cell and a `{python.marimo}` cell count in the cell index;
  a raw `{=html}` block does not. A span in prose in a language no engine owns still
  takes the next number of the document-wide span counter (the scan applies no ownership
  filter); text inside any braced fence and inside an HTML comment holds no spans (decision
  41; EXR-3 divergences 8 and 10), so it takes none. A span in the document's own front
  matter takes a number; a span from project metadata does not.
- [x] Labeled cells hash the writer's range (decision 36): a Jupyter cell with a
  `#| label: fig-x` line and a `#| fig-cap:` line (crossref sugaring strips both from the
  AST text) gets the same `quarto_content_hash` at splice time as the writer recorded, and
  splices. The knitr-labeled counterpart is the ignored test in Phase C.
- [x] Nesting corpus: the corpus the splice tests use includes a cell inside a blockquote
  and a cell inside a list item (Positron's parser, which has no nesting rule, indexes
  these differently; decision 39), and the splice matches them by hash regardless of the
  index.
- [x] Dispatch: a legacy payload runs the existing splice (all existing
  `capture_splice` tests stay green and unmodified; tests added here for the legacy
  path alone carry the `legacy_capture_` prefix); a record payload cannot reach
  `apply_capture_splices` (compile-time, plus a test that the stage rejects an
  attempted mixed configuration).
- [x] Independence: a grep-style test fails if any file of the record arm imports from
  `engine/capture_splice.rs`.

## Phase B: implementation

- [x] First, write the characterization test for how spans appear in the pre-engine AST
  in a heading, a table cell and an attribute value (a paragraph and the plain `Code`
  shape are already verified). Record the result in this plan.
  **Result (2026-11-01):** the scan is textual over the serialized qmd, so a span in a heading,
  a table cell and an attribute value (`title="..."`) is each one unit and each splices in
  place (an attribute value is substituted as raw text, so a value with a double quote would
  break the attribute; EXR-3's value rules do not special-case it). Test:
  `spans_in_a_heading_a_table_cell_and_an_attribute_value_splice`.
- [x] Write the tests first, then make the record arm mask a clone of the AST before it
  scans: tests that a `{python}` fence and a span nested inside a display block yield no
  units (the same corpus as Phase C), and that the arm's unit list equals the writer's unit
  list on the Phase C fixtures. Then call `nested_cell_mask::mask` on a clone (as
  `EngineExecutionStage` does at `engine_execution.rs:515`) and scan the clone. Nothing in
  the pre-engine stages masks, so without this task the arm would claim units the writer
  never saw.
- [x] Verify and wire (UNVERIFIED until checked): that the splice presenter receives the
  **current** cell's options and the current document scope (`execute:` scope with
  cell-over-scope precedence, as `resolve_cell_options` does for the live path) layered
  over any options the record stores, with a current option winning over the record's
  value. Jupyter's record stores no presentation options, so for it the current options
  apply as is; the exact layering for knitr's resolved options (setup-chunk state such as
  `opts_chunk$set`) is settled with the knitr follow-on epic. Check what `ctx` holds at the splice stage and what the
  EXR-1 presenter takes; if the current options do not reach it, add the plumbing and make
  the "presentation uses the current cell" test pass.
- [x] Unit enumeration from the shared scan (EXR-2's `scan_units` plus EXR-4's id
  function, decision 38) over the masked clone of the pre-engine document (previous task),
  the same list the writer
  used (same ids, same document order, document-wide span counter), then mapped onto the
  AST cells. The hash for each unit is computed over the writer's text range by the
  mechanism EXR-4's first task chose, never from the sugared `CodeBlock.text`. If a
  separate AST walker is kept for the mapping, it must agree with the scan on every
  conformance fixture.
- [x] A `RecordSpliceStage` or a record arm inside `CaptureSpliceStage` (decide from
  the code: the stage already owns files, includes and fail-soft logging, so an arm
  is likely smaller). It runs the lookup, calls the EXR-1 presenter per matched
  cell, parses the presenter markdown with the same `pampa` reader the legacy path
  uses, and replaces the cell. Only the `Legacy` arm of `CapturePayload` calls the
  legacy splice, `label_reinject::strip` or `parse_capture_gz`.
- [x] Span splice mode, per the span decision above.
- [x] Implement the matching order of decision 34: execution hash, then content hash,
  duplicates by k-th occurrence in record order (design decision 2); matched cells are
  presented with the current options. A document unit is hashed under the `HashDefinition`
  of the `quarto_engines` entry whose recorded owned-language set contains its language.
- [x] Emit the per-unit staleness result as stage auxiliary data (kind constant next
  to `ENGINE_CAPTURE_KIND`), and into `ctx` for EXR-15. No UI.
- [x] Suppress the "(no execution)" warning for engines the record served, by
  reusing `with_spliced_engines` with the engine list from the record
  (`CapturePayload::served_engine_names`, read from `quarto_engines`). Pinned by
  `editing_one_cell_leaves_n_minus_1_outputs_and_one_source_cell`: with an empty registry an
  edited cell left as source errors out ("jupyter is registered but its runtime is not
  available") unless the record's engine is in the spliced set.

## Phase C: masking and conformance

- [x] Fixtures with display blocks (a four-backtick fence containing a `{python}`
  cell and a span, a blockquote containing a fence, a `{{python}}` doubled brace
  that must not be claimed): the unit list with the mask applied contains no
  phantom units, on native and in a WASM test. Include a ceded cell and the dotted
  `{python.marimo}` form so the document-wide cell index is exercised.
  Native: `splice_ids_equal_the_writers_with_and_without_the_mask` (five fixtures) and the
  Phase B unit test. WASM: `recordPayload.wasm.test.ts`, "a cell shown inside a display
  block is not a unit" (a same-text cell *before* the real one would steal the output if
  claimed).
- [x] Cross-site property test: for each conformance fixture (the same files EXR-2
  and EXR-16 use), unit ids computed by the native record writer equal those
  computed by the splice at read time, with the mask on and with it off.
  `splice_ids_equal_the_writers_with_and_without_the_mask` runs the real writer
  (`record_capture_run`) and the real preview pipeline, and compares the staleness event.
  "Mask off" is the control documents with nothing to mask; the mask itself is never
  disabled. The fixtures are this plan\'s own, not the EXR-2/EXR-16 conformance files.
- [x] `MASK_INLINE_RE` and the scanner's span regex are one definition (EXR-2);
  add a test that fails if the two diverge. Already in place from EXR-2: both regexes are
  built in `engine/patterns.rs`, and `mask_inline_pattern_covers_every_scanned_span`
  (`engine/units.rs`) fails if the mask stops covering a scanned span.
- [x] Canonical recorded input: a knitr labeled cell hashes the same at record time
  and at splice time (the label re-injection must not leak into the id). If EXR-10
  has not landed, mark this test ignored with the strand reference, and leave it as
  the acceptance test for EXR-10. It is not the only labeled-cell test: the Jupyter
  `fig-` label plus `fig-cap` test in Phase A runs without EXR-10.
  EXR-10 has landed, so this is a live test, not ignored:
  `a_knitr_labeled_cell_hashes_the_same_at_record_and_splice_time`. The writer is a stub
  named `knitr`, which makes the real engine stage re-inject the label after hashing; no
  real knitr record exists until EXR-11.

## Phase D: pipeline, WASM, end to end

- [x] Wire the record arm through `insert_capture_splice_stage_in` and the three
  pipeline builders that take captures (`pipeline.rs` near `:504` to `:666`).
  Done by replacing `Vec<EngineCapture>` with `CapturePayload` in the builders,
  `PartialKind`, `PandocRequestInput`, `BookDownloadParams`, the preview render helper and
  the WASM entry points (a91d287d4). `insert_capture_splice_stage_in` attaches a record with
  `CaptureSpliceStage::with_record`; `HtmlRenderConfig::with_payload` does the same for the
  HTML path. Left as-is on purpose: `capture_driver.rs`\'s record staleness arm and
  `cache.rs` `read_cached` treat a record as a miss; record-level staleness is EXR-15\'s
  (design decision 4) and nothing writes records to that cache yet.
- [x] WASM tests: `render_page_for_preview` with a record payload shows the recorded
  output; an edited cell falls back to source; an inline expression shows its value.
  `recordPayload.wasm.test.ts` covers presented, edited-to-source and display block for both
  entry points. **Not covered in WASM: the inline expression** (the checked-in fixture has
  no span and hashing one by hand in TS is not worth it); spans are covered natively by the
  `record_splice` unit tests.
- [x] Hub-client integration test (existing `Preview.capture.integration.test.tsx`
  style) for a record payload. The TS layer is shape-agnostic, so the test pins that a
  record-typed binary doc\'s bytes reach the render call untouched.
- [x] End to end on native: record a Jupyter document (after EXR-5), edit one cell,
  preview shows N-1 outputs and one source cell. Then change only an `echo` option on a
  cell: preview shows all N outputs, the changed cell with its new presentation.
  `tests/integration/record_preview_splice.rs`, library-level through
  `render_qmd_to_preview_ast`, with a stub engine standing in for Jupyter. Not run through
  the `q2 preview` binary or a browser.
- [x] Scope note test: an edit to an included file does not recompute the includer in
  the watcher (pre-existing limitation, `lib.rs:484-510`); state it in the test name so
  nobody reads it as a regression.
  `editing_an_included_file_does_not_recompute_the_includer_known_watcher_limit`
  (`quarto-preview/src/capture_driver.rs`).

## Exit criteria

- Phase A to D green; clippy and nextest on `quarto-core` and the WASM crate;
  `npm run build:wasm`; hub-client tests.
- Legacy preview behavior is unchanged: the existing splice tests pass untouched.
- Unit ids from the masked AST equal EXR-2's text-scanner ids on every conformance fixture.

## Risks and open questions

- **Front matter spans** are evaluated (EXR-3 divergence 10) and are units in the record,
  but they are metadata, not body blocks, so the body splice cannot place them. First check:
  how a span in `title:` looks in the pre-engine metadata, and whether preview shows
  its value or its source. Until checked, preview may show the source for these. Only
  spans from the document's own front matter are units (decision 41; EXR-2's provenance
  rule); project metadata spans are not, so they never reach the record or the splice.
- **Span representation in other contexts** (heading, table cell, attribute value)
  may force a text-level scan after all (serialize the masked AST, scan, map back). If
  so, record that decision here and share the code with the engine stage, which
  already serializes.
- **Presenter output re-parsed to blocks** can diverge from the legacy block shape
  (the `.cell` wrapper, echoed source block). Keep the wrapper shape until EXR-17
  (`2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`); a parity test compares the
  record splice with the legacy splice on the same Jupyter fixture.
  Done: `record_splice_matches_legacy_splice_on_the_jupyter_fixtures`, kernel-free over the
  EXR-6 goldens. Nine shadow cases render byte-identical HTML; the one divergence is
  `include-false`, where the legacy splice leaves the hidden cell as source (an empty output
  run is indistinguishable from "no match") and the record correctly hides it. Figure
  cases are not compared (they need materialized files).
- **Duplicate cells** match by k-th occurrence in record order (design decision 2). After
  deleting one of several identical cells, the later record cell is the one reported
  `orphaned`, whichever one the author meant; the cells share a hash, so the display differs
  only when their recorded outputs differed (a non-deterministic cell).
- **Stale cells shown as source.** With the execution hash first, a cell is `stale` only
  when no hash matches: an edit to code or to an execution-relevant option. Such a cell is
  shown as source until the next execution, as the legacy splice does for any edited
  cell; the staleness data exists so a later UI can say why. Presentation-only edits no
  longer strand a cell. Imported Positron records match by content hash only, so a `#|`
  edit of any kind still strands those.
- **The preview watcher does not follow includes** (pre-existing, not introduced here):
  `crates/quarto-preview/src/lib.rs:484-510` recomputes staleness only for the changed path.
  Include and `_quarto.yml` edits reach the preview only through whatever re-renders the
  includer.

## References

- `crates/quarto-core/src/engine/capture_splice.rs`, `.../stage/stages/capture_splice.rs`,
  `.../engine/nested_cell_mask.rs`, `.../engine/preview_record.rs`
- [Preview AST splice design](2026-05-18-q2-preview-project-replay-engine.md)
- [Jupyter cell wrapper](2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md)
- `external-sources/positron/src/vs/workbench/contrib/positronQuarto/browser/quartoDocumentModel.ts`
