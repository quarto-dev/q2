---
title: 'EXR-11: knitr K1, chunk records recorded additively'
date: 2026-10-28
description: 'Makes the knitr chunk hook record each chunk as a unit of the engine execution record (id, finished markdown, resolved-options digest, files) while the live render path stays byte-identical.'
status: draft
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md) (plan EXR-11,
knitr phase K1).
**Depends on:** [EXR-4](2026-10-28-exr-04-record-format.md) (record format),
[EXR-10](2026-10-28-exr-10-knitr-k0-options-map.md) (canonical input and the
engine-boundary label header edit), [EXR-2](2026-10-28-exr-02-unit-scanner.md)
(unit ids), [EXR-7](2026-10-28-exr-07-engine-wire.md) (it defines the Rust-side
optional unit-output field on `ExecuteResult`).
**Blocks:** [EXR-12](2026-10-28-exr-12-knitr-k2-inline.md),
[EXR-17](2026-10-28-exr-17-legacy-removal.md). It does not block
[EXR-13](2026-10-28-exr-13-hub-wire.md) (the hub carries any record); it is a soft
prerequisite of [EXR-15](2026-10-28-exr-15-hub-integration.md), since knitr records
appear on the hub once this plan lands.
**Lane:** C (`workspace-6`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3 (one is the spikes).

> **For implementers:** TDD, task by task; steps use `- [ ]` checkboxes. Tests
> that need R skip when it is missing, using the helpers in
> `tests/integration/knitr_label_reinject.rs:37-58`.

## Goal

For every knitr-owned chunk, the engine returns `{id, markdown, options digest,
files}` next to the unchanged `markdown`, and the core builds knitr units of the
record from them. The live path does not change: the record is a side product,
and a shadow test asserts that placing the recorded chunk markdown reproduces the
live output.

## Background

Verified in code on 2026-10-28 unless marked.

- **The chunk hook receives finished markdown.** `knit_hooks$chunk` is
  `delegating_hook("chunk", ...)` (`crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R:205`),
  where `delegating_hook` first applies knitr\'s default markdown hook and then
  Quarto\'s (`hooks.R:178-184`). So `x` is the chunk\'s markdown after knitr has
  formatted the pieces, and `options` is the resolved chunk options. Early
  returns leave handled-language fences, `ojs`, and `verbatim`/`embed`/`comment`
  engines untouched (`hooks.R:206-222`); those must not become units.
- **Presentation is baked in R.** The remainder of the hook builds the cell
  `div` with classes, captions, layout attributes, and so on (`hooks.R:228+`).
  This is why the epic stores rendered markdown for knitr (decision 6).
- **Unknown options reach the hook, and are forwarded as attributes.** Options
  outside the known lists are collected as `unknown_opts` and emitted as
  `name='value'` attributes on the cell div (`hooks.R:372-404`). The known list
  includes `other_opts` (`hooks.R:361-370`, with `eval`, `yaml.code`, `code`,
  `file`, `params.src`, `original.params.src`, `hash`, and others). Consequence:
  an id option **must be added to `other_opts`**, or it would leak into the HTML.
  `original.params.src` is set by q2\'s patched parser (`resources/rmd/patch.R:37`),
  so it is not evidence that arbitrary custom options survive; the forwarding
  loop is.
- **Document-level pieces are extracted after the whole knit.**
  `execute.R:205-254`: `knit_meta` is an attribute of the render output (one per
  document, not per chunk), and `extract_preserve_chunks(output_file, format)`
  (`execute.R:211`, defined at `:611-624`) runs on the finished output file using
  `htmltools::extractPreserveChunks` for HTML formats. So the chunk hook sees the
  raw preserve markers and any placeholder map is built later.
- **The Rust result type.** `KnitrExecuteResult` (`engine/knitr/types.rs:97`) has
  `markdown`, `supporting`, `filters`, `includes`, `preserve`, `post_process`;
  adding a field with `#[serde(default)]` is wire-additive.
- **Capture today.** The stage emits an `EngineCapture` aux event with the
  engine\'s whole `ExecuteResult` JSON and optional embedded file bytes
  (`stage/stages/engine_execution.rs:640-680`; file bytes only when the observer
  asks, via `collect_capture_files`).
- **knitr errors.** `engine/knitr/error_parser.rs` already extracts "Quitting
  from lines X-Y" into `source_lines`; knitr names the chunk label and reports
  line numbers in the `.rmarkdown` intermediate, which is bd-1ng2ivis.
- **Cached chunks may skip the chunk hook.** In the knitr checkout
  (`external-sources/knitr/R/block.R:94-101`), a cache hit on an `include: false` chunk
  returns `''` before any hook runs, and with `cache: 3` it returns `cache$output(hash)`
  without calling the hook, so the hook never sees those chunks. That is spike S4\'s
  starting evidence.
- **Not verified:** whether `htmltools::extractPreserveChunks` works on a single
  string with stable per-content placeholder ids (decides spike S3); how `child=`
  documents invoke the chunk hook, and how far the cache-hit cases above reach (spike S4).

## Design decisions (from the epic; do not re-litigate)

1. **Record rendered chunk markdown** as one `display_data` output with MIME
   `text/x-quarto-markdown`, tagged with the output format in its metadata. It is
   not `text/markdown` (which means something else for spans) and other viewers
   ignore unknown MIME types.
2. **Additive.** The live path is unchanged; the record is a side product with a
   test-only shadow equality.
3. **Identity.** Rust hands each chunk an explicit id (the id from EXR-4\'s id function,
   taken from the stage\'s one shared unit list, epic decision 38) through a chunk-header
   edit at the engine boundary, placed by the unit\'s `line` in the text knitr receives
   (Phase 11C; EXR-10\'s `Div`-id locator serves crossref labels only); the hook echoes it
   back. The knitr unit
   hash covers all chunk options (fence info string, `#|` lines, code), because
   presentation is baked in, and the document-level hash uses the knitr declaration
   (decision 6 below; EXR-4 implements that declaration). The Positron-style `quarto_content_hash` is computed over the
   *source* cell text by the stage (epic decision 36), not the canonical pipeline text
   (see EXR-10 risks).
4. **Notebook-level data.** `knit_meta`, `preserve`, `needs_postprocess`,
   includes and filters are notebook level, not per cell. Files go in per-cell
   metadata; nbformat `attachments` exist only on markdown cells, so they are not
   used.
5. **Resolved-options digest.** R also returns a digest of the *resolved* options
   the chunk ran with (excluding the id and code). It captures presentation
   inputs the source text does not show: document `execute:` scope, project
   defaults, and `opts_chunk` defaults. It is recorded as provenance, not matched
   on, and it is not part of the hash.
6. **The knitr hash declaration (epic decision 35).** The knitr engine supplies its
   `HashDefinition` from `ExecutionEngine::hash_definition()` (EXR-4 owns the knitr
   override, `exr-04` Phase 2, and this plan only tests the resulting unit hash),
   core owns the algorithm
   and no engine name appears in core hashing code. The declaration has EMPTY exclusion
   lists (`presentation_cell_options` and `presentation_execute_keys` are empty, because
   presentation is baked into the rendered markdown, so every chunk option and every
   `execute:` key counts) and `document_keys: ["knitr"]`. The unit hash therefore covers
   all chunk options and the document-level hash covers the whole `execute:` map plus
   the `knitr` key; the record stores the full declaration in
   `quarto_engines[].hash_definition`. This is the knitr declaration\'s only profile, the
   default, and its units carry no `quarto_hash_rule`. The knitr follow-on epic adds a
   second profile for its structured units (presentation options excluded) and tags its
   fallback units (baked markdown, every option counts) with the profile that keeps
   today\'s rule, as epic decision 35 allows.

## Out of scope

- Structured knitr outputs (not in this epic; the follow-on knitr epic revisits them,
  and extends this plan's records with a per-chunk fallback to the markdown unit).
- Inline spans (EXR-12).
- Presenting records in the Rust presenter beyond placing the stored markdown.
- Executing a single chunk.

## Phase 11S: spikes first (each ends in a written decision in this plan)

- [ ] **S1, id round trip.** In a throwaway R script and then a Rust test: give a
  chunk an extra `#|` option and print `names(options)` from a test chunk hook.
  Decide the option name (proposal: `quarto-unit-id`) and confirm it is returned
  verbatim and added to `other_opts` so it is not emitted as a div attribute.
- [ ] **S2, files per chunk.** Compare (a) a before/after listing diff of the
  figures directory around each chunk, (b) parsing file references from the chunk
  markdown, (c) knitr\'s own plot counter and `fig.path`. Pick one; record the
  failure modes (`fig.show: hold`, multiple devices, `results: asis`).
- [ ] **S3, preserve.** Check whether `htmltools::extractPreserveChunks` applied
  to one chunk string yields a placeholder map that can be unioned across chunks
  and equals the whole-document extraction. If yes, record placeholders per chunk
  and keep the map at notebook level; if no, record raw markers and apply
  extraction in Rust (only possible for HTML preserve; document the gap).
- [ ] **S4, odd chunks.** Start from the cache evidence in Background
  (`block.R:94-101`: a cache hit on an `include: false` chunk and `cache: 3` return
  before the chunk hook). Observe the hook for `cache: true`, `eval: false`,
  `include: false`, `engine: bash`, and `child=`. Decide: record all knitr-owned
  chunks (including `eval: false` and `include: false` with empty markdown) and
  record only the outermost chunk for `child=`.

## Phase 11A: tests first

- [ ] `knitr_chunk_records_one_per_owned_chunk` (real R): three chunks, one with
  `eval: false`, one with `include: false`; three units in document order; ids
  match the stage\'s shared unit list.
- [ ] `knitr_chunk_id_is_not_emitted_as_an_attribute` (real R): the rendered HTML
  has no trace of the id option.
- [ ] `knitr_shadow_equality` (real R): placing the recorded chunk markdown into
  the document reproduces `result.markdown` byte for byte on the fixtures from
  `knitr_label_reinject.rs`, `knitr_display_fence.rs`, and a labeled-figure
  document.
- [ ] `knitr_unit_hash_covers_all_options` (no R): changing `echo` in the brace
  options or in a `#|` line changes the unit\'s execution hash; changing prose
  does not.
- [ ] `knitr_record_files_per_chunk` (real R): a two-figure document attributes
  each file to its chunk; bytes arrive through the existing
  `collect_capture_files` path.
- [ ] `knitr_record_has_notebook_level_meta` (real R, an htmlwidget fixture such
  as the one in `knitr_html_dependency.rs`): `preserve`, `needs_postprocess` and
  dependencies are at notebook level and unchanged.
- [ ] Round-trip serde tests (no R): `KnitrExecuteResult` with and without the new
  `chunks` field.
- [ ] Skip behavior: every R test starts with `if skip("<name>") { return; }`.

## Phase 11B: R side

- [ ] Add `quarto-unit-id` (final name from S1) to `other_opts` in `hooks.R`.
- [ ] In `knit_hooks$chunk`, after the early returns and the final wrapping,
  append `{id, markdown, options_digest, files}` to a record list held in the
  hooks closure; return it from `execute.R` as `chunks` next to `markdown`. The
  markdown returned to knitr is unchanged.
- [ ] Digest function over resolved options, stable across runs (sorted, id and
  code removed).
- [ ] Files per chunk per the S2 decision.

## Phase 11C: Rust side

- [ ] `KnitrExecuteResult.chunks: Vec<KnitrChunkRecord>` with `#[serde(default)]`.
- [ ] `KnitrEngine::hash_definition()` is EXR-4\'s task (the knitr override: empty
  exclusion lists, `document_keys: ["knitr"]`, with its own document-hash test
  `exr-04` Phase 2). Nothing to implement here; the first Rust task that needs the
  declaration waits for EXR-4 Phase 2.
- [ ] Engine-boundary id insertion, by the unit\'s `line` in the text knitr receives:
  core re-locates each unit\'s `line` into that text (EXR-2 `locate_units`, carried by
  EXR-7), and the header edit inserts the unit\'s id (the id EXR-4\'s function filled in
  the shared list) at that line, under the count check below. An unlabeled chunk has no
  wrapper `Div` to key on (`label_reinject.rs:35-45` walks `for_each_wrapped_cell`, which
  visits only code blocks wrapped by a `Div` whose id classifies as a crossref label), so
  EXR-10\'s `Div`-id locator serves crossref labels only and is not used for ids.
- [ ] Adapter into the record library (EXR-4): units with rendered outputs, format
  tag, hash definition version, notebook-level folded results.
- [ ] Alignment assertion: the number and order of returned chunk records equals
  the count of knitr-owned chunk units in the shared list. A mismatch yields no record
  plus a diagnostic naming both counts and the render succeeds, because the record is
  additive (epic decision 19, the alignment contract, and EXR-12 follow the same rule);
  it is never a silent drop and never a failed render.
- [ ] Test-only shadow equality helper, using `place(record)` for rendered units, which
  EXR-4 owns (marimo, [EXR-9](2026-10-28-exr-09-marimo.md), reuses the same function).
  If EXR-4\'s `place` has not landed, this task waits.

## Phase 11D: optional bonus, error line mapping (bd-1ng2ivis)

- [ ] Because q2 builds the text handed to R (preprocessing, header edits), keep a
  line map from that text to units. Map the "Quitting from lines X-Y" range from
  `error_parser.rs` back to the unit\'s source location instead of the
  `.rmarkdown` intermediate. Do not use the unit id as the knitr label: labels
  drive output filenames. Ship only if Phase 11C is green and the map costs under
  a session; otherwise leave the strand open.

## Exit criteria

- Shadow equality holds on all knitr fixtures with R present; live output is
  byte-identical to before this plan.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` green; pass and skip counts reported against
  the live baseline.
- Spike decisions written into this plan.

## Risks and open questions

- **Id echo fragility** (epic risk 1): the count assertion and S1 are the guards.
- **Per-chunk preserve** may not be possible for every format (S3). RTF uses a
  different extractor (`knitr::extract_raw_output`, `execute.R:611-624`).
- **Format dependence.** A record is valid only for the format it was produced
  for; the tag makes a mismatch a visible miss, not wrong output.
- **Record size** with embedded figures (bd-sr73) is measured in EXR-4.

## References

- `crates/quarto-core/src/engine/knitr/{mod.rs,types.rs,error_parser.rs,resources/rmd/{hooks.R,execute.R,patch.R}}`
- `crates/quarto-core/src/stage/stages/engine_execution.rs`
- `crates/quarto-core/tests/integration/{knitr_label_reinject,knitr_display_fence,knitr_html_dependency}.rs`
- bd-1ng2ivis, bd-sr73, bd-n1vmlalw
