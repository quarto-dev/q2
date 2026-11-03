---
title: 'KNR-3: knitr record adapter, hash profiles, fallback units, cache and child handling'
date: 2026-10-30
description: 'Turns KNR-1 captured pieces into EXR-4 record units for knitr: cells and outputs, the structured hash profile beside the default one, notebook-level engine results, per-chunk fallback units, and the policies for knitr cache hits, child documents and user-overridden hooks.'
status: in progress  # re-scoped 2026-11-03 after the epic rebase; Step 0 (bd-js24dold) landed
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-3).
**Depends on:** [KNR-1](2026-10-30-knr-01-capture-spike-and-harness.md) (capture point and
mechanism, S2 mapping, S3 options, S4 cache, S5 odd chunks),
[KNR-2](2026-10-30-knr-02-knitr-presenter-layer.md) (the markdown-unit presenter that
fallback units need),
[EXR-2](2026-10-28-exr-02-unit-scanner.md) and
[EXR-4](2026-10-28-exr-04-record-format.md) (the shared unit list with ids, and the record
library with `HashDefinition` profiles),
[EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md) (the markdown-unit records this plan
extends; complete, so tests and steps that name its recorder or its chunk hook can use it), and
[EXR-7](2026-10-28-exr-07-engine-wire.md) (hard, because EXR-11 depends on it and because
the record field on `ExecuteResult` comes from it).
Cite KNR-1\'s decisions by their headings: `#### S1 decision` through `#### S9 decision`.
**Blocks:** KNR-4, KNR-5, KNR-6.
**Estimated sessions:** 3 to 4 (a guess made before KNR-1 reports; the internal keys
below were about half a session of it, an estimate from interlude I1; slice 5, the gaps the
corpus gate found, adds about one).
**Prerequisite, not part of this plan (landed 2026-11-03, `5c6885686`):** the knitr replay
claim fix (bd-js24dold), so that a knitr trace replays at all. `ReplayEngine::claims_language`
claims what knitr claims when the recorded engine is knitr (the shared `knitr_claims`), and
`execute` presents from the record whenever the capture has one; four replay tests.

> **Status of this document.** Written 2026-10-30 from code reading, before KNR-1 ran;
> revised 2026-10-31 after the paper review
> ([research note](../research/2026-10-30-knitr-epic-paper-review.md)); **re-scoped 2026-11-03**
> after the parent epic was squashed and this branch rebased onto it (see "Re-scope,
> 2026-11-03" below, which overrides any older text it contradicts). Decision 8 (the format
> in the record) was settled on 2026-11-01. Decision 2 is settled (below). Nothing waits on the
> parent session.

## Re-scope, 2026-11-03

Read against the KNR-1 corpus report, interludes I1 to I4, epic decisions 21 to 28, and the code
at `989db3fe2` (the epic squashed as `c5f8c7593`). What changed, and what that does to this plan:

1. **Decision 2 is settled.** EXR-4 is complete. `exec_hash::execution_options(def, profile,
   options)` takes the profile, so a unit's flat cell keys are its options minus **its own
   profile's** exclusions, and `RecordBuilder::push` hard-codes `profile: None`
   (`record/builder.rs:157`). The adapter therefore overwrites `quarto_hash_rule`, the exec
   hash and the flat keys after `push`, with `execution_options(def, Some("structured"), ..)`,
   and recomputes the document hash (the spike of decision 8 did exactly this). No parent
   change beyond decision 8's `matching.rs` edit.
2. **The resolved-options field is named `quarto_resolved_options` (Gordon, 2026-11-03).** A cell
   metadata key holding the KNR-1 S3 object, in knitr spelling, with `unserializable` listing the
   dropped names. The `quarto_` prefix keeps it out of `CellMetadata::options()`, the flat keys and
   the hash. It is written for structured units only. The chunk\'s current header options layer
   over it (epic decision 11); the document scope does not (decision 3).
3. **Nothing on the product path reads the knitr record keys yet (found 2026-11-03).**
   `present_document`, `record_splice::present_cell` and `ReplayEngine::present_record` all build
   options with `CellOptions::from_config`; `with_output_kinds`, `from_knitr_known`,
   `with_original_chunk`, the anchors, the holds and `knitr_visibility` are called only by the
   corpus baseline test (`knitr_corpus_baseline.rs`). KNR-2 says it "reads" the keys and KNR-2b
   Phase 2bA assumes a reading path, so it was unowned. **Gordon decided (2026-11-03) that KNR-3
   Phase 3B owns it:** one shared reader, `knitr_cell_inputs(cell, engine_entry)`, used by all
   three presenter paths, with `known_options` reaching it through a new optional field on
   `EngineEntry` and a new parameter on `present_document`. The Phase 3C shadow test must present
   from a written record, so it needs this anyway; KNR-2b 2bA then only validates it.
4. **The epic added a second live presenter path and expanded cells.** `present_document`,
   `record_splice::present_cell` and the replay path all call `CellOptions::from_config` and
   `render_expanded_unit(&CellOptions)`. Anything that changes how a cell is presented is applied
   in all of them, through the shared reader of item 3, not three copies.
5. **The join key between pieces and chunk records already exists.** `capture.R` stores the
   resolved options per chunk, and they include `quarto-unit-id` (it is not in
   `capture_option_skip`), so a captured chunk joins to its unit by that id. Pieces exist for
   every chunk knitr evaluates, including chunks without an id (children, generated chunks); only
   id-bearing chunks become units. `KnitrExecuteParams.capture` is hard-coded `false` in
   `engine/knitr/mod.rs`; this plan sets it from the route and `want_record`.
6. **`target_route` has its consumer here.** Wiring `route_for_target` removes its
   `cfg_attr(not(test), expect(dead_code))`; the build fails until the attribute is deleted.
7. **Diagnostic codes.** `docs/errors/engine/` holds Q-18-1 to Q-18-5 and Q-18-10; Q-18-6 to
   Q-18-9 are claimed by other plans (`2026-09-18-pandoc-hybrid-P5-implementation.md`,
   `2026-09-18-knitr-includes-and-engine-result-diagnostics.md`). This plan takes **Q-18-11 and up**,
   re-checking the catalog at each allocation; each code gets its docs page and sidebar entry in the
   same commit. The handoff\'s "Q-18-1 to Q-18-4" was out of date.
8. **Corpus re-measured 2026-11-03 (report r16, 221 cases):** 198 accepted, 5 allowlisted, 11
   not compared, 7 unexpected, the same as r15. The run executes (59 s) and does not skip, so the
   epic\'s R test gating (`c5f8c7593`) did not hide it. The seven: four `options/foo.unknown/*`
   and the `tbl-cap` attribute of `s2/01_kable` (this plan, through `known_options`),
   `setup-state/knit_hooks_override` (this plan, the hook-override fallback rule), `s9/01_inline`
   (KNR-4).
9. **Phase 3C runs the gate in report mode (Gordon, 2026-11-03).** This deviates from the wording
   of epic decision 26, which says the same gate runs in KNR-3 Phase 3C. `s9/01_inline` is a
   blocker until KNR-4, so the failing assertion moves to KNR-2b Phase 2bC. The deviation is
   recorded in the epic\'s decision log with this plan.

Still open, none blocking this plan: the `wasm32` build check of the presenter commits, a KNR-2b
item that now also covers `record_splice.rs` and `expansion.rs`. (The `s8/01_setup_state`
allowlist entry is signed off, in the epic\'s decision 23, 2026-11-03.)

## Progress and slice order (2026-11-03)

Landed on `knr-2-presenter`, each gated (quarto-core nextest, only the known revealjs failure; the
Julia `j1_minimal_julia_render` flaked once under load with "Incorrect HMAC digest" and passed alone):
`5c6885686` Step 0, `fcd359b8a` this re-scope, `28474d2ec` `quarto_file_path`, `0a127e8b0` the
declaration, `8d3857d7b` profile-aware matching and `knitr::structured` (`mark_structured`,
`rehash_document`).

Remaining, in this order, each tests first (slices 1 and 2 are done, see their notes below; the `knit_meta` manifest of slice 2 waits on WGT-2\'s schema):

1. `EngineEntry.known_options` (optional field, round trip, absent means no filtering) and the
   shared reader `knitr_cell_inputs`, wired into the three presenter paths (Phase 3B, reader item).
2. The adapter from captured chunks to record units (join by `quarto-unit-id`, `quarto_output_*`,
   `quarto_resolved_options`, original chunk, `knitr_visibility`, passthrough, `knit_meta`
   manifest), calling `mark_structured`; then delete the two staged `expect(dead_code)` lines
   (`hash_rule::OUTPUT_FORMAT_OPTION` is already shared; `structured.rs` has a module-level one).
3. Capture gating from `route_for_target`; the fallback unit, the three policies and their
   Q-18-11-and-up codes. *Done (2026-11-03, slice 3; see "Slice 3 notes").*
4. Phase 3C shadow from a written record, in report mode. *Done (2026-11-03, slice 4; see "Slice 4 notes"): 16 blockers, not the 7 expected.*
5. Slice 5, the gaps slice 4 found, each tests first, in this order: (a) the `include_graphics`
   reader reads `quarto_file_path` (7 cases, a regression of `28474d2ec`), (b) knitr `#|` header
   options read as literal scalars (2 cases), (c) `present_document` presents from the author text
   and splices through `replacement` (2 cases), (d) `quarto_echoed_code` (4 cases, amends epic
   decision 21). Then re-run the report: the expected result is one blocker, `s9/01_inline`
   (KNR-4). An allowlist entry needs Gordon\'s sign-off. *Done (2026-11-03, slice 5; see "Slice 5 notes"): one blocker, `s9/01_inline`.*

### Slice 1 notes (2026-11-03)

The reader is `presenter::knitr_inputs::present_knitr_cell`; `present_document`,
`record_splice::present_cell` and `ReplayEngine::present_record` call it (the last through
`present_document`, which gained an `engine: Option<&EngineEntry>` parameter). It returns `None`
for a cell with no `quarto_resolved_options`, so every other cell presents as before.
`EngineEntry.known_options` and `EngineEntryInput.known_options` are optional (round-trip test;
`quarto_record_version` stays 1).

Decisions made while building it, for the adapter (slice 2) to follow:

- **A fourth internal cell key, `quarto_expression_starts`**: an array of line indices, one per
  source expression, for `prompt`. The three keys above cannot carry it, and the presenter cannot
  recover it from the outputs. The adapter writes it from `expression_starts(chunk)` and omits it
  when the source pieces\' lines do not add up to the code, like the anchors.
- **`quarto_resolved_options` is the chunk hook\'s options object as is.** The reader drops an
  `unserializable` key from it (the names R could not serialize are bookkeeping, not options).
- **Header layering rewrites a dashed key to its dotted name when `known_options` lists that
  name** (`class-source` to `class.source`, per KNR-1 S3), instead of copying R\'s table. Without
  a `known_options` list no key is rewritten.
- **Holds are applied by the reader** over the capture-order anchors, and the kinds follow the
  reordered outputs (held outputs are never messages or warnings).
- `knitr_visibility` moved to `presenter::visibility` (the presenter is not `wasm32`-gated;
  `engine::knitr` is) and `engine::knitr::pieces_map` re-exports it.
- The splice stage looks the engine entry up by the cell\'s `quarto_engine`, else the only entry.
- Tests: reader unit tests (one per input) in `knitr_inputs.rs`, and one shared rich fixture
  (`knitr_inputs::fixture`) driven through all three paths so they cannot drift. The Jupyter
  goldens are unchanged. Not covered: `fenced.echo`\'s original chunk through the three paths (the
  reader test covers it).

### Slice 2 notes (2026-11-03)

The adapter is `engine/knitr/adapter.rs` (`build_structured`), reached from `chunk_records::attach_records`
when the run carries `pieces`. Nothing asks R for pieces yet (`capture` is still false), so slice 3
turns it on. It joins chunks to units by `quarto-unit-id`, writes the four reader keys, calls
`mark_structured` and `rehash_document`, and builds fallback cells from the chunk hook's markdown for
a chunk the mapper refuses. The EXR-11 recorder still builds every chunk when there are no pieces.
Both staged `expect(dead_code)` attributes are gone.

- **`known_options` is the union hooks.R builds, not `names(opts_chunk$get())` alone.** The names the
  chunk hook does not forward as attributes are knitr's own plus three Quarto groups
  (`quarto_knitr_opts`, `quarto_opts`, `other_opts`; `class.source` and `attr.*` are in the last). The
  capture computes the same union (`capture_known_options` in `capture.R`, a copy kept in step with
  `hooks.R`) before any chunk runs, and ships it as `knownOptions`. A test names one option from each group.
- **Deferred: the `knit_meta` manifest (epic decision 27) and the record-level `include=FALSE`
  dependencies.** WGT-2 owns the manifest's shape and its plan is still a draft with the schema
  unwritten ("Neutral per-output manifest in the record (schema in EXR-4)" is open there). The adapter
  keeps the opaque `quarto_knitr.knit_meta` string the mapper already writes. Writing the manifest
  needs that schema first; it is left unchecked below.
- **Settled 2026-11-03 (Gordon): the manifest is not KNR-3\'s.** The widget epic (WGT-2\'s
  adapters item, which now records this) owns the conversion; KNR-6 holds the flip for knitr documents with captured widget output until it
  lands (epic decision 27, amended). To check in slice 3, before anything relies on that hold:
  whether a fallback unit (finished markdown) keeps its widget\'s dependencies, since its text
  carries no `knit_meta`.
- **Open at the call site:** `read_file` for figures reads under the knit working directory
  (`working_dir`), where knitr addresses them; slice 3's end-to-end render checks this, since
  the corpus baseline reads under the document directory.

### Slice 3 notes (2026-11-03)

Capture gating, the fallback units and the three policies. Each was written test first and seen
failing on its assertion.

- **Gating.** `target_route::plan_capture(want_record, has_units, format)` decides; `execute` sets
  `KnitrExecuteParams.capture` from it and pushes the one-per-document target note on a fallback
  route. Nothing is captured and nothing is said when no record is wanted. The `expect(dead_code)`
  on `target_route.rs` is gone. A fallback unit is what the EXR-11 recorder already wrote (finished
  markdown, default profile, no `quarto_hash_rule`).
- **Three coded notes, all `info`:** `Q-18-11` (knitr\'s cache was off for chunks that asked for it),
  `Q-18-12` (a knit hook was replaced; names the hook and the first chunk), `Q-18-13` (a `child=`
  chunk). Each has its catalog entry, docs page and sidebar entry in this commit. The target note
  stays uncoded, as before.
- **Cache.** `capture.R` records the labels of chunks whose `cache` option was truthy before the
  forcing hook turned it off (`cacheDisabled` on the result). knitr runs that option hook for every
  chunk, so the check is on the value.
- **Hook override.** The first chunk\'s `evaluate` call snapshots six hooks (`source`, `output`,
  `message`, `warning`, `error`, `plot`); the chunk hook compares them with `identical`, and the
  first difference sticks (`hook_override` on that chunk and every later one). **Scope beyond the
  handoff:** the handoff named `source` and `output`; I watched all six, since a replaced `message`
  or `plot` hook silently changes the live markdown the same way. The adapter makes a flagged chunk a
  fallback cell even when its pieces would map. An override that is reverted later stays flagged.
- **Flag: the [units-only engines epic](2026-11-02-units-only-engines-epic.md) (draft, 2026-11-02)
  may supersede the `child=` policy below** (its proposed decision 5 records generated chunks as
  sub-units, spike S5). Build no more of the `child=` fallback than the policy table needs; the
  wrapper below is the option for now, chosen knowingly. `quarto_echoed_code` (slice 5) does not
  depend on that epic: it is written only when knitr\'s echoed code differs from the authored code,
  so if core later resolves `ref.label` itself (that epic\'s open question 1) the key is simply not
  written for it, and a numeric `eval` or `tidy` still needs it.
- **`child=`: Gordon chose a real fallback unit (2026-11-03).** knitr calls no hook for the parent
  chunk (`call_block` returns the child\'s markdown straight into the output). `hooks.R` now wraps
  `knitr::knit_child` with `assignInNamespace` (precedent: `patch.R`), installed only when a
  `child=` chunk that carries a unit id is about to run. For those calls it writes the chunk record
  (finished markdown, joined with a newline across several children as knitr does) and, on a captured
  run, a captured record whose one piece is `other` with class `child`, which the existing adapter
  turns into a fallback cell. **A finding that explains the earlier "labels disagree" oddity:**
  `knit_child` copies the parent chunk\'s options into `opts_chunk`, so the child\'s own chunks
  inherited the parent\'s `quarto-unit-id` and recorded themselves under it. The wrapper strips the id
  for the duration of the child. The wrapper returns what `knit_child` returned, and a test pins that
  the markdown is unchanged. In a real qmd the chunk is written with `#| child: path` (the header
  form is a parse error).
- **Upstream idea (not proposed yet).** A `child` knit hook, called with the child\'s markdown and the
  parent\'s options, defaulting to identity, would replace the wrapper: about three lines in
  `R/block.R` (`call_block`, `out = one_string(unlist(cmds))`). The id inheritance would then be
  handled by skipping chunks while `knitr::opts_knit$get("child")` is true. For a future tree of
  child units, knitr already exposes the current child file: `knitr::current_input()` (basename, or
  the full path with `dir = TRUE`, which needs `output.dir`) is correct at every depth and restored
  after each child (run on a parent, child and grandchild). The tree itself (scanner following
  includes, the presenter reading the child\'s prose, matching across files) is our work, not
  knitr\'s.
- **Widget dependencies (the check the handoff asked for first).** Run with a real
  `htmlwidgets` widget on `html` and `pdf`: the widget\'s JS and CSS come only through the live
  result\'s `header_includes`; the record entry has `includes: null` and no `html_dependencies` in
  both the structured and the fallback path, and a fallback unit holds only the widget\'s div and
  JSON. So a fallback unit does not keep its dependencies, but neither does any knitr record, and
  the KNR-6 hold on documents with widgets stands for both.
- **`read_file` base directory (the end-to-end check).** `q2 render` of a project with the document in
  `sub/`: knitr\'s working directory is the project directory, but it writes figures under the
  document\'s directory (`sub/doc_files/figure-html/`), so reading only under the working directory
  made a plain plot chunk a fallback cell. `read_file` now tries the working directory first (right
  for an author\'s `include_graphics` path) and then the document directory. Test:
  `a_figure_in_a_project_subdirectory_is_found_and_stays_structured`. **Separate, existing
  behavior, not changed:** a relative `child:` path resolves against that same working directory (the
  project root), so `child: kid.Rmd` beside `sub/doc.qmd` fails; the older `q2` build fails the same
  way. Whether Quarto 1 resolves it relative to the document is not checked here.
- **End-to-end run (inspected).** Invocation: `cargo run --bin q2 -- render <scratch>/proj/sub/doc.qmd`
  with `execute: {record: true}`, a plain chunk, a plot, a `#| child: sub/kid.Rmd` chunk, a chunk that
  replaces the `output` hook, and one after it. The record at `.quarto/execution-records/sub/doc.ipynb`
  has cells: plain `STRUCTURED` (stream), plot `STRUCTURED` (`image/png`), child `FALLBACK`, override
  `FALLBACK`, after `FALLBACK` (each `text/x-quarto-markdown`); the render printed `Info [Q-18-13]` and
  `Info [Q-18-12]`. I read the cells with `jq`; the cache note was exercised by a test, not this run.
- **Existing tests changed, and why.** Four EXR-11 integration tests (`knitr_chunk_records`: one per
  owned chunk, files per chunk, stage shadow equality; `knitr_inline_records`: id embedding does not
  change output) asserted the one-markdown-block-per-chunk shape through `record::place`, which has no
  caller outside tests. On `html` those chunks are now structured cells, so the tests are pinned to
  `pdf`, a fallback target, where that shape still holds; a new test
  (`knitr_html_records_are_structured_cells`) pins the structured shape through the same stage. Equality
  for structured cells is the presenter path, which is slice 4\'s corpus gate.

### Slice 4 notes (2026-11-03)

The shadow, in report mode. `corpus_report` runs the real `KnitrEngine` twice per (fixture, format): once with no record wanted (the live markdown, the oracle) and once with a record wanted. It coalesces the record as the stage does, writes it, reads it back, and presents it with `present_document` and the shared reader (`tests/integration/knitr_corpus_record.rs`). `KNR_DIRECT=1` keeps the KNR-1 direct path for comparison; the command is unchanged: `KNR_REPORT=<file.json> KNR_ONLY=<fixture> cargo nextest run -p quarto-core -E \'test(corpus_report)\'`. `KNR_Q1=1` adds Quarto 1\'s output for the html cases.

Result, 221 cases: 193 accepted (Identical or SyntaxLevel), 5 allowlisted, 16 blockers with no entry, 7 not compared (4 knitr aborts, 3 missing packages, as before). **The handoff expected seven residual blockers; there are sixteen.** Against the direct path (14 blockers) the product path closes six: the four `options/foo.unknown/*` (the `known_options` work), `setup-state/knit_hooks_override` (the hook-override fallback), `s2/01_kable` and its `tbl-cap`. It opens eight. The sixteen fall into five groups by root cause. Quarto 1 agrees with the live oracle on every one but the caption, where it also keeps the markup, so the candidate is the side that is wrong in each. Decisions are Gordon\'s, 2026-11-03.

- **A. Echoed code (4):** `options/eval/{header,yaml}/02_1`, `s1/13_numeric_eval`, `setup-state/ref_label`. knitr echoes code that differs from the authored text: a numeric `eval` masks the unevaluated lines as `## x`, and `ref.label` supplies the referenced chunk\'s code. The presenter showed the authored text. KNR-1 had written that the cell `source` is the code `evaluate()` received, but the builder sets `source` to the authored text, and `recompute_cell_hash` rebuilds a unit\'s hash from `source`, so the earlier text cannot be followed literally (it would make a numeric-`eval` chunk stale on every render). **Decision:** the third cell key `quarto_echoed_code` (epic decision 21, amended; shape under "What a structured knitr unit holds"). The S8 silent staleness of a `ref.label` chunk (editing the referenced chunk does not change its hash) stays open.
- **B. Header options read as markdown (2):** `options/comment/yaml/02_empty` and `options/fig.cap/yaml/01_a__marked_up__caption`. The failing comment value is `"##"`, the third sweep value (`""` and `"#>"` pass). `options_to_config` reads the `#|` lines with `InterpretationContext::DocumentMetadata`, so a YAML string is parsed as markdown and flattened with `as_plain_text`: `"##"` is an empty heading, and `*marked up*` loses its markup. R\'s `yaml.load` and Quarto 1 keep the scalar. The header form (brace options) never goes through this path. **Decision:** `knitr_header_options` in `knitr_inputs.rs` reads the knitr `#|` header with the literal context (`ProjectConfig`), used at the two call sites (`present_document`, `record_splice`). A `#| fig-cap: !expr ...` test comes first: the capture evaluates `!expr` in R, and the layering would override the recorded value with the expression text; if confirmed, the layering skips `!expr` entries.
- **C. List-nested chunks (2):** `structure/chunk_in_list`, `s1/02b_include_false_in_list`. `present_document` partitions the raw `u.code` (container prefix and indent kept) and splices without `record::place::replacement`, so only the first line is indented and the fences land at column 0. `record_splice` already presents from the author text and splices through `replacement`, as the old baseline did. **Decision:** the same in `present_document`. The Jupyter engines send only unindented, unprefixed cells through it (`runs_in_legacy_executor`), and the Jupyter goldens have no such cell, so only knitr changes. The allowlisted blockquote blank-line case is unaffected.
- **D. `include_graphics` (7):** `rich-output/include_graphics` in six formats and `s2/06_include_graphics`. A regression of `28474d2ec`, which moved the writer from `metadata.quarto_knitr.file` to `metadata.quarto_file_path`: `included_graphic` (`presenter/cell.rs`) still reads `quarto_knitr.file`, so the image is written to `_files` (`![](FIG1)`) instead of keeping the user\'s path (`![](made.png){width=200}`). The presenter test hand-builds the old key, so it kept passing; KNR-2 had these cases syntax-level. **Decision:** read the new key, with the test built from the mapper\'s output, not a hand-built key.
- **E. `s9/01_inline` (1):** inline spans, KNR-4.

**Finding, live path (not changed here):** with a record wanted, the engine injects `#| quarto-unit-id: ...` into every chunk (EXR-11), and the ids reach the live markdown: `fenced.echo` prints the line, and a numeric `eval` counts it. The first oracle run, which used the record-wanted run\'s markdown, showed both. A render without a record is unaffected, and KNR-6 removes the path, so this is noted, not fixed. The oracle is therefore the no-record run.

The gate also found one bug in slice 3: `capture_known_options` ran before the format\'s `opts_chunk` was applied, so `ft.shadow` read as an unknown option and became a `.cell` attribute. It now includes the format\'s `opts_chunk` names (test `capture_reports_knitrs_known_option_names_once`, red first).

**Frozen oracle:** `KNR_FREEZE=1` writes each compared case\'s normalized live markdown to `tests/fixtures/knitr-corpus/live/<name>.<format>.md` (214 files, 856 KB); without it the report counts match, differ and missing (`214 match, 0 differ, 0 missing` on a second run, so the oracle is stable).

### Slice 5 notes (2026-11-03)

Four commits, each tests first and seen failing: `285e2e8b6` (a, `included_graphic` reads
`quarto_file_path`; the test builds the cell with the mapper), `be2270f85` (b, `knitr_header_options`
reads the `#|` header with `ProjectConfig`; the `!expr` hypothesis was confirmed, so the layering
skips `Expr` entries), `1d718db57` (c, `present_document` partitions the author text and splices
through `replacement`, keeping the rendered text\'s own trailing newlines), `6c65dfe33` (d,
`quarto_echoed_code`, written by the adapter only when the echoed code differs from the authored
code, read by `present_knitr_cell`; the hash test passed before the change and guards it).

Corpus after slice 5: 208 accepted, 5 allowlisted, 7 not compared, **1 blocker, `s9/01_inline`
(KNR-4)**, as expected. No allowlist entry was added, and no snapshot or frozen-oracle file changed.

## Goal

For each knitr-owned unit, produce an EXR-4 record unit whose outputs are standard nbformat
(from KNR-1's mapping), whose hashes follow the knitr declaration below, and which degrades
to a fallback unit exactly where pieces cannot be captured. Everything document-level that
the engine result carries today is preserved at notebook level.

## Background

**What the parent specifies for knitr today (the all-options rule this plan changes).**
EXR-11 gives knitr units one `display_data` with `text/x-quarto-markdown`, and a
`HashDefinition` (parent decision 35, EXR-4) with EMPTY exclusion lists and `document_keys:
["knitr"]`: the unit hash covers the language, **all** chunk options and the code, "because
presentation is baked into the rendered output". The record stores the full declaration in
`quarto_engines[].hash_definition`, and the declaration\'s `id` and `version` prefix every
hash, so a changed rule never silently matches. A `HashDefinition` may also list named
`profiles` (each with `presentation_cell_options` and `presentation_execute_keys`); a unit
names one in its cell metadata as `quarto_hash_rule`, and an absent key means the default
profile, the top-level lists. A unit naming a profile the stored declaration lacks reads as
stale. The document hash drops an `execute:` key only if every profile used in that
engine\'s record excludes it. Cells carry Positron\'s keys (`quarto_cell_id`,
`quarto_content_hash` over the author\'s source) and q2\'s (`quarto_kind`, `quarto_lang`,
`quarto_engine`, `quarto_exec_hash`, and the cell\'s options minus the exclusion list as
flat keys). Ids are `{index}-{hash8}-{label|unlabeled}`, filled by the stage and passed to
every engine in the shared unit list (parent decisions 38 and 39).

**The unit set per engine result.** The result carries `includes`, `filters`, `preserve`,
`needs_postprocess` (and in knitr `engineDependencies` / `knit_meta`), plus supporting
files; EXR-4 keeps them at notebook level in `quarto_engines[]` and `quarto_files`. That
does not change here.

**How knitr\'s cache interacts with pieces** (`block.R`, line numbers from the development
snapshot; KNR-1 S4 re-reads them in released versions).
- `cache=TRUE` is level 3. A hit returns the cached **output string** before the chunk runs
  (`block.R:100-101`), so no pieces exist on a level-3 hit.
- Levels 1 and 2 cache the **piece list** (`res.orig`, `:337-340`), but a hit reads it back
  inside the expression that otherwise calls `evaluate()` (`:308-311`), so a capture at the
  `evaluate` hook never sees it. Level 2 stores `remove_plot(res, ...)` (`:339`), so a
  level-2 hit holds no renderable plots.
- `cache.rebuild`, `cache.lazy`, and `dependson` also matter; not examined.

**Format dependence of execution (expected, verify in the corpus).** The target format
reaches R through `opts_knit` (`rmarkdown.pandoc.to`, `out.format`) and through q2\'s
`opts_chunk` (`dev` from `fig-format`, `dpi`, `fig.width`, and so on; `execute.R`
`knitr_options`). kable and `df_print` choose markdown, LaTeX or HTML by format, so the
*pieces themselves* differ per format.

**What a structured knitr unit holds (settled 2026-11-01 from interlude I1 and I2).** One
cell per chunk. Its `outputs` come from the captured pieces: stdout and stderr streams, the
error, images as `display_data` (with knitr\'s own path in `metadata.quarto_file_path`, the engine-neutral key of epic decision 25), and
`knit_asis` output as `display_data` with the mime knitr gave it (`text/html` or
`text/markdown`), marked `metadata.quarto_markdown_passthrough: true` (the bare, no-wrapper
case of `results = "asis"` is not recorded: the presenter reads that option at present time).
Three internal cell-metadata keys carry
what the presenter cannot recover from `outputs` or from the authored `source`, each starting with
`quarto_` so `CellMetadata::options()` drops it and none entering the exec hash:

- `quarto_output_anchors`: an array parallel to `outputs`, each entry the number of source
  lines before that output, **in capture order**. It is never reordered at record time:
  `fig.show = "hold"`, `results = "hold"`, `results = "hide"` and numeric `echo` are applied by
  the presenter (KNR-2) so a header-option edit shows without re-running. The key is omitted
  when the source pieces\' lines do not add up to the chunk\'s code (the presenter then writes
  all the source first); the prototype did this and the corpus did not measure how often it
  happens (interlude I3 does).
- `quarto_output_kinds`: an array parallel to `outputs`, `"message"`, `"warning"` or null,
  meaningful on stderr streams. Set from the piece classes, which the capture already tells
  apart. Without the key the presenter treats stderr as a warning, as for Jupyter.
- `quarto_echoed_code` *(decided 2026-11-03, Gordon; slice 5)*: the code knitr echoed, as an
  array of lines in nbformat `source` style, the `#|` lines excluded. Written only when it
  differs from the authored code (a numeric `eval` masks the unevaluated lines as `## `,
  `ref.label` supplies the referenced chunk\'s code, `tidy` rewrites it); the adapter already
  receives it from `pieces_to_cell_anchored` and drops it. The cell `source` stays the authored
  text, because `recompute_cell_hash` rebuilds a unit\'s hash from `source`. The presenter shows
  it in place of the authored code; the anchors and expression starts index into it.
- `known_options` (a field on the `quarto_engines` entry, not a cell key): the option names
  knitr knows, from the knitr that ran (`names(knitr::opts_chunk$get())`, so the answer
  follows the version). The capture ships them once per record and the adapter stores them.
  *Decided (Gordon, 2026-11-01 after I3; name settled 2026-11-02):* the presenter subtracts
  the list from each chunk\'s current options at present time, so a header edit that adds a
  new unknown option (knitr writes these as cell attributes, `foo.unknown='1'`) shows without a
  re-run. This replaces the per-cell `quarto_unknown_options` key that KNR-2 is written
  against. An absent field means no filtering, as for Jupyter and EXR-11 records. The field is
  optional on the engine entry, and the knitr declaration bump of decision 3 already restarts
  the hashes, so `quarto_record_version` stays 1.

## Design decisions (from the epic; do not re-litigate)

1. **One knitr declaration with two profiles.** The knitr `HashDefinition` keeps the id
   `knitr` and gets a bumped version. Its default profile is EXR-11\'s all-options rule
   (empty exclusion lists), used by fallback units, which carry no `quarto_hash_rule`.
   A second profile (proposed name `structured`) lists the presentation-only cell options,
   and structured units carry `quarto_hash_rule: "structured"`. Because the id and version
   prefix every hash, the bump restarts every unit\'s hash, fallback units included; an
   EXR-11-era record therefore reads as stale after the bump and still presents through the
   pass-through presenter (KNR-2) until the next run (this is the knitr row of parent
   decision 10).
2. Per-chunk fallback; one mechanism per unit (epic decision 8).
3. knitr is whole-document: reuse is whole-document or nothing (epic decision 7). A stale
   unit means the next run re-executes the whole document, which refreshes every unit\'s
   resolved options.
4. A count mismatch between units core sent and units knitr returned means no record plus a
   diagnostic, and the render succeeds (parent decision 19).
5. The exec hash is computed by core from the masked pre-engine units (EXR-4), never by the
   knitr module, and never from the text R receives (which has labels re-injected,
   EXR-10); author-text hashes come from the stage (parent decision 36).
6. **Three internal cell keys and one engine-entry field** (epic decision 21, signed off
   2026-11-01, field name settled 2026-11-02, third key added 2026-11-03): `quarto_output_anchors`,
   `quarto_output_kinds` and `quarto_echoed_code` on cells, and `known_options` on the
   `quarto_engines` entry, as described under "What a structured knitr unit holds". The adapter
   writes them; KNR-2 reads them.
7. **`text/markdown` is marked, not special-cased** (epic decision 22): the adapter sets
   `metadata.quarto_markdown_passthrough: true` on `knit_asis` output so the shared presenter
   can opt in without changing what Jupyter does. There is no `raw` marker: `results: asis`
   is a cell option the presenter reads at present time.

## Provisional: decisions this plan must make

1. **The structured profile\'s list.** `HashProfile` holds exact option names, unknown
   options count (parent decision 35), and EXR-4 has no pattern syntax. So the list is a
   closed list of **presentation-only** options, written out by name: `echo`, `output`,
   `results`, `collapse`, `prompt`, `comment`, `fig.cap`, `fig.scap`, `fig.subcap`,
   `fig.alt`, the layout, caption and code options, and every documented Quarto cell
   option that only affects presentation. This plan cannot be coded until the list is closed:
   S3 produces it as one list with one spelling per name, starting from the `opts_chunk`
   defaults in `external-sources/knitr/R/defaults.R` (the snapshot) plus the Quarto cell
   options documented under `docs/`, and sorts each name into presentation-only or
   execution-relevant. S1 decides whether `fig.show` joins it.
   **Execution-relevant, therefore not on it:** `eval`, `engine`, `error`, `include`,
   `warning`, `message`, `fig.keep` (plot selection happens at capture, epic decision 10),
   `dev`, `dpi`, `fig.width`, `fig.height`, `fig.asp`, `fig.ext`, `dev.args`,
   `fig.retina`, `fig.process`, `render`, `df_print`, `cache`, `R.options`, `ref.label`,
   `child`, `file`, `code`, `opts.label`, and `params`-dependent values. Options that
   cannot be listed by name (arbitrary `attr.*`, `class.*`, `layout*` extensions and unknown
   options forwarded as attributes) count in the hash, so editing one re-executes the
   chunk; this plan documents that limit. A pattern form for exclusions would need a change
   in EXR-4; Gordon decided on 2026-10-31 not to ask for one unless the corpus shows the
   limit is hit. The structured profile\'s `presentation_execute_keys` is empty (decisions 3
   and 9): every `execute:` key counts. S3 produces the real list and decides one
   spelling for names (knitr\'s `fig.width` or Quarto\'s `fig-width`, which EXR-4 uses for
   flat keys); this plan states it.
2. **Where resolved options live** *(settled 2026-11-03, see "Re-scope" items 1 and 2: flat keys follow the unit\'s own profile; the resolved options are `quarto_resolved_options`. Older text follows. S3 and S8 answered 2026-10-31: the chunk hook records them, in knitr\'s spelling, with the serialization rule and the closed list in KNR-1 S3; the field name is still to be chosen here)*. EXR-4 computes the flat cell
   keys as the cell\'s options minus the exclusion list, so they never disagree with the
   hash; EXR-4 does not say which list applies when a unit names a profile. This plan
   applies the unit\'s own profile (*open*, a second question for the parent session, listed in
   the epic\'s open questions), so structured units
   carry only execution-relevant options as flat keys. The resolved options the presenter
   reads (epic decision 11) live in their own cell metadata field, not in flat keys; its
   name is decided here after S3. They include state the document text does not show (a
   setup chunk\'s `opts_chunk$set`, `opts_template`, `opts.label`, `ref.label`,
   `opts_hooks`) and need a rule for values that are functions, environments or R
   expressions.
3. **Layering of current options over the record\'s** *(settled for now, 2026-10-31;
   revisit after S8; shared with KNR-2 and EXR-14)*. The chunk\'s current header options win
   over the recorded resolved options (epic decision 11); the document scope is **not**
   layered. In knitr a setup chunk\'s `opts_chunk$set` outranks the `execute:` defaults, and
   the resolved options already include both in knitr\'s own order, so layering the scope over
   them could reverse that precedence. Consequence: an `execute:` presentation edit would not
   show without a run, so the structured profile excludes no `execute:` key and every
   `execute:` key counts in the document hash; an `execute:` edit re-executes, as today. S8
   verifies where q2\'s `execute.R` sets the defaults; the layering is revisited with its
   result.
4. **Fallback and target rules, one table** (below), for every case and every target.
5. **Cache policy** *(answered by KNR-1 S4, 2026-10-31: option (a), with an unconditional forcing hook)*: (a) disable knitr\'s cache while recording,
   (b) record pieces for levels 1-2 and fallback for level 3, (c) store pieces in q2\'s own
   cache. The expected decision is (a), because the `evaluate`-hook mechanism never sees a
   hit and a level-2 hit has no plots; (a) changes documents\' runtime and may surprise.
6. **`child=`** *(S5 answered 2026-10-31: the parent chunk fires no hook, the child\'s chunks look like ordinary chunks, and the count-mismatch rule catches it; default stands)*: the child is knitted recursively and returned as one
   string. Default: the parent chunk becomes one fallback unit; the child\'s own chunks are
   not units. Whether the scanner (EXR-2) can tell a `child=` chunk is settled by S5 reading
   EXR-2\'s scanner; if it cannot, the count-mismatch rule (decision 4) covers it.
7. **User-overridden hooks** *(S5 answered 2026-10-31: detectable by comparing hooks with `identical`; flagged from the first overriding chunk on, so every later chunk is a fallback unit)*: a `knit_hooks$set(...)` in a setup chunk
   changes today\'s markdown and would be ignored by the presenter. Proposal: compare the
   active hook functions with q2\'s own at chunk time; an override makes that chunk (or the
   rest of the document) a fallback unit with a diagnostic naming the hook. Decide the
   granularity once S5 shows when hooks can change (a setup chunk runs mid-document).
   Settled for the corpus (2026-11-01, epic decision 23): the corpus case is
   `setup-state/knit_hooks_override`, where a `source` hook rewrites the chunk text; a
   document that overrides the `source` or `output` hook becomes a fallback unit from the
   first overriding chunk on (S5\'s finding), and the case is a fallback rule, not an
   allowlist entry.
8. **Format in the record** *(settled 2026-11-01: Gordon accepted option (i) and chose way 1
   below, the flat option key; the code consequences are listed under "Decided")*. Parent
   decision 27 says the record carries no presenter inputs and stays format-neutral: the
   output format is pipeline context at present time. But knitr pieces differ per format
   (`kable`, `df_print`, `prefer-html`), the format is in no hash, so a structured record
   built for HTML would read as fresh for a Typst render, and EXR-4 still tags rendered
   units with `quarto_format`. Options: (i) ask the parent for a knitr exception (the
   format is part of the structured profile\'s hash input and tags the unit), (ii) capture
   only format-independent pieces, which S2 tests, or (iii) one record per format. The
   recommendation is (i), because S2 is expected to show format-dependent pieces.
   **As built in EXR-4 (read 2026-11-01 from the parent branch, tip `a2a9d715d`; EXR-4\'s
   tests not re-run).** (a) The tag half exists: rendered units carry `quarto_format`, and
   `place` (`record/place.rs`) leaves a unit verbatim and reports a mismatch when the tag
   differs from the render format or is absent. (b) The hash half does not: `hash_unit`
   (`record/exec_hash.rs:210`) hashes `id`, `version`, `unit`, kind, language, code and the
   options minus the profile\'s exclusions, takes no format, and a profile is only two
   exclusion lists, so no profile can make the format an input. (c) The profile mechanism
   (`quarto_hash_rule`, `UnitHashInput.profile`) does exist, so structured units can leave
   presentation options out. The parent plan names this epic as the owner of that second
   profile (EXR-4, the profile paragraph). Ways to make the format count, for Gordon to
   choose: (1) the adapter stores the format as a flat option key on structured units,
   counted by the structured profile, so `recompute_cell_hash` sees it from the cell alone
   and nothing in EXR-4 changes (the caller that hashes a scanned unit must supply it, which
   needs checking against the caller); (2) tag and check only, as `place` does: no hash
   change, a record built for HTML is fresh for Typst, and the check at present time
   re-executes or falls back; (3) a field on `UnitHashInput`, an EXR-4 change.
   **Decided (Gordon, 2026-11-01): way 1, the flat option key.** A structured unit carries
   the output format as a flat cell option under the reserved key
   `quarto-output-format` (confirmed by Gordon 2026-11-01). It must not start with
   `quarto_`: `CellMetadata::options()` (`record/types.rs:188`) drops every `quarto_` key, so
   such a key would never reach `recompute_cell_hash` or the hash. The hyphen keeps it out
   of that filter, and no knitr or Quarto chunk option has the name. The structured profile does not exclude it, so it is in the unit hash and in
   `recompute_cell_hash`, which reads the cell\'s flat keys alone. Fallback units carry no
   such key (they hold baked markdown and keep the default profile). Checked against the
   code (read 2026-11-01, not re-run): the document-side hashing does not yet supply it.
   `UnitKey::from_author_text` (`record/matching.rs:35-60`) hashes a scanned unit with
   `profile: None` and options parsed from the author text only, and
   `ExecutionEngine::hash_definition`\'s default path (`engine/traits.rs:436`) does the same.

   **Spikes (2026-11-01, scratch tests in `record_splice.rs`, reverted; the patch is not
   kept in the repo).** Run through the real `splice_units`:
   - *Writing and reading need no parent change.* An adapter that uses only the public API
     (`hash_unit` with `profile: Some("structured")`, `execution_options`, `hash_document` with
     `UnitHashRef.profile`, then `write_record` and `read_record`) builds a mixed record, one
     structured unit and one fallback unit. After the round trip `recompute_cell_hash` agrees
     with the stored hash for both, and the profile name survives. `RecordBuilder::push` itself
     cannot take a profile, so the adapter sets `quarto_hash_rule`, the exec hash and the
     flat keys on the cells after `push`, and recomputes the document hash.
   - *The splice ignores the profile and the format.* A structured record built for `typst`,
     rendered as html, reports **Fresh** and splices. It matched on the content hash, because
     the text is unchanged. An edit of a presentation-only option (`echo: true` to `false`) on
     a structured unit reports **Stale** and shows the source, because the author text changed
     (content hash) and the unit\'s default-profile key (`echo` counts) differs from the
     profile hash (`echo` excluded, the format counted). Both break the exception: the
     structured unit is meant to ignore a presentation edit, and meant to go stale on a
     different format.

   **Decided (Gordon, 2026-11-01): an additive parent edit.** Add one function in
   `record/matching.rs`, called by `splice_units`: for a document unit it finds the record
   cell with the unit\'s id, takes that cell\'s `quarto_hash_rule` as the profile, and, when the
   cell has a `quarto-output-format` flat key, adds the current format under that key before
   hashing. A unit with no cell by that id keeps today\'s key. No wire or schema change; the
   Jupyter, marimo and Julia paths do not change, because their cells name no profile and
   record no format key. The edit comes with parent-side tests (below), and the parent plans
   are not edited; this plan and the epic record it.

   **Corrected by interlude I3 (2026-11-01, prototype on `spike/knr-i3`; the edit as decided
   does not suffice).** Run with tests first, the edit above left the red tests red, for two
   reasons: (1) the unit id embeds the content-hash prefix (`{index}-{hash8}-{label}`), so it
   changes on any edit and a lookup by id misses exactly when a presentation edit needs the
   profile; (2) `match_units` pass 2 matches on the content hash, so a format change with
   unchanged text is Fresh whatever the exec hash says. The variant that passes: `UnitKey` gains
   `alt_exec_hashes` (one per profile in the engine\'s stored declaration, each with the current
   format under `quarto-output-format`); pass 1 tries the default hash, then the alternatives;
   pass 2 skips cells that name a profile; no lookup by id. About 65 lines in `matching.rs` plus a
   call in `splice_units`; still no wire or schema change, and cells naming no profile behave as
   before (349 record, splice, replay and presenter tests pass). The format value is the base
   format identifier (`html` for `q2-preview`). Findings:
   [I3](../research/2026-11-01-knr-i3-records-through-preview.md). **Confirmed by Gordon
   (2026-11-01)** as the shape of the parent edit.
9. **The document hash of a mixed record** *(settled 2026-10-31: accepted)*. The document
   hash drops an `execute:` key only if every profile used in the record excludes it. The
   structured profile excludes none (decision 3), so the default profile of a fallback unit
   changes nothing at document level: every `execute:` key counts in every knitr record. The
   cost of a fallback unit is per unit: its hash counts every option, so an edit to a
   presentation option of that chunk re-executes. The corpus reports how often a document has
   a fallback unit; revisit if it is common.
10. **Setup-chunk edits.** Editing a setup chunk makes only its own unit stale (the hash
    sees only that unit), so downstream chunks present with the resolved options recorded at
    the last run until the next execution. Accepted and documented; S8 asserts it.
11. **Cell location for knitr** (parent decision 22: wrapper Div id with a nesting corpus
    and an ordinal fallback). Ids now come from the stage\'s shared unit list (parent
    decisions 38 and 39) and are handed to R as in EXR-10, so the capture does not invent
    its own. Keep the wrapper-Div lookup only where the nested-cell corpus needs it.

### Chunk kinds, recorders and hash profiles

| Case | Unit | Written by | Hash profile | Diagnostic |
|---|---|---|---|---|
| Plain R chunk, HTML, Typst or docx target | structured | the capture adapter (this plan) | `structured` | none |
| `eval=FALSE`, or blank code | structured, source only | the capture adapter, from the document | `structured` | none |
| `child=` chunk | fallback | the chunk hook | default | one per chunk |
| Non-R engine chunk (bash, SQL, reticulate Python, custom engines) | fallback (accepted; reticulate Python verified 2026-11-03: no evaluate-hook call, finished markdown only) | the chunk hook | default | none today (accepted rate, reported) |
| Level-3 cache hit | fallback | the chunk hook | default | one per chunk |
| Level-1 or 2 hit with knitr\'s cache left on | fallback (expected decision (a) removes this row) | the chunk hook | default | one per chunk |
| Overridden `knit_hooks` | fallback | the chunk hook | default | names the hook |
| `latex`, `beamer`, `pdf` target | fallback | the chunk hook | default | one per document |
| Any other target (epub, pptx, context and so on; gfm once the corpus covers it, which KNR-1 does) | fallback until the corpus covers it | the chunk hook | default | one per document |

Who decides the target rows: Rust knows the target (the stage passes it to the engine), so
KNR-2\'s routing (2E) is a Rust-side decision that tells R to skip capture for the whole
document; the chunk hook then writes the fallback units. The fallback table is what KNR-2
routes by.

The chunk hook that EXR-11 built stays permanently, and its role is: write the finished
markdown for fallback chunks (the rows above, settled). Whether it also carries the
structured chunks\' final options (KNR-1 S3: `fig.cap` and the `eval.after` set exist only
at the chunk hook or later) and whether it emits skeleton markers (KNR-1 S7, KNR-6) are
decided by those spikes, not here; the fallback role is the only one this plan relies on.
What the adapter replaces is EXR-11\'s recorder for structured chunks; KNR-6 deletes that
code once nothing reads it, and keeps the chunk hook (see KNR-6).

## Out of scope

- The presenter (KNR-2), inline units (KNR-4), Positron (KNR-5), the flip (KNR-6).
- Changing the hub transport or MIME (EXR-13 carries any record).

## Phases

### Step 0: prerequisite

- [x] bd-js24dold, the knitr replay claim fix (`5c6885686`; quarto-core nextest 7451 of 7452, the
  known revealjs failure; strand closed).

### Phase 3A: tests first

- [x] Key tests, each failing first (epic decision 21): `quarto_output_anchors` equals the
  capture order for a chunk with interleaved source and output, and is **not** reordered for
  `fig.show = "hold"` or `results = "hold"`; it is omitted when the source pieces\' lines do
  not add up to the code; `quarto_output_kinds` marks a message and a warning apart;
  the record\'s `known_options` contains `echo` and not `foo.unknown`, on knitr 1.44 and the
  latest; the three cell keys are dropped by `CellMetadata::options()`, `known_options` is a
  field on the engine entry, and none changes the exec hash.
- [x] `knit_asis` output carries `quarto_markdown_passthrough: true`, and nothing else does;
  no `raw` marker is written for `results = "asis"`.
- [x] Hash tests with vectors: a structured chunk with a presentation-only option changed
  has the same `quarto_exec_hash`, and one with an execution-relevant option changed
  (including `include`, `warning`, `message`, `fig.keep`) has a different one; the same
  edit to a fallback unit changes its hash; prose changes never reach either.
  `quarto_content_hash` matches Positron\'s SHA-1 vectors (EXR-4, EXR-16 S1).
- [x] Splice-key tests for the parent edit (decision 8, as corrected by I3), in the parent\'s
  `record_splice.rs` tests, each failing before the edit: a structured record built for
  `typst` and rendered as html reports Stale; the same record rendered as `typst` reports
  Fresh; a presentation-only edit (`echo`) on a structured unit reports Fresh and splices with
  the current echo; an execution-relevant edit reports Stale; a unit whose id has no cell
  keeps today\'s key; the existing Jupyter and marimo-shaped tests pass unchanged. The spike
  numbers in decision 8 are the expected red state.
  *Done (2026-11-03): 7 tests in `record_splice.rs`, built on `knitr::structured`; 3 were red before the edit (another format, presentation edit, cell inserted above), the rest pin the unchanged cases.*
- [x] The `UnitKey.alt_exec_hashes` field, the two `match_units` changes and the call in
  `splice_units` (the I3 prototype, `9a0ff2d23` on `spike/knr-i3`), small enough to read in one
  view; add a test that pass 2 skips a cell naming a profile; report the diff to Gordon before
  the commit.
  *Done (2026-11-03): ported from the I3 prototype to the current `matching.rs` (it gained `attr_tail` since), with 4 matching tests including pass 2 skipping a profiled cell. `OUTPUT_FORMAT_OPTION` is defined in `record::matching` and re-exported by `knitr::hash_rule`. **Diff for Gordon\'s review before any push:** `git show` of the KNR-3 `matching.rs` commit (about 70 lines in `matching.rs`, 2 in `record_splice.rs`).*
- [x] The adapter sets `quarto_hash_rule`, the exec hash and the flat keys (including
  `quarto-output-format`) after `RecordBuilder::push` and recomputes the document hash with
  `hash_document` and each cell\'s `UnitHashRef.profile`; a test writes and reads a mixed
  record and recomputes every hash (the spike\'s shape).
- [x] Declaration tests: the knitr declaration keeps the id `knitr` with a bumped version;
  the `structured` profile is stored; a unit tagged with a missing profile reads as stale;
  every `execute:` key counts in the document hash of any knitr record, mixed or not
  (decisions 3 and 9), and an edit to a fallback chunk\'s presentation option changes that
  unit\'s hash.
  *Done (2026-11-03, `hash_rule.rs`; the missing-profile read was already EXR-4 behavior).*
- [x] Version test: an EXR-11-era record reads as stale and still
  presents through the pass-through presenter. The fixture is a record written by EXR-11\'s
  recorder, so this test needs EXR-11 merged; until then, hand-build one from EXR-11\'s
  format.
  *Done (2026-11-03; EXR-11 is merged, the record is hand-built in `record_splice.rs`\'s `exr11_record`). **Finding:** the record does not read as stale. The splice judges a record by the declaration it stores (version 1), so it is fresh and its pass-through markdown is placed; no code compares the stored declaration with the current one. The version bump does what decision 1 needs in practice: the current declaration never reproduces a version 1 hash, so a new run's record shares no hash with the old. The test asserts those two facts; decision 1's "reads as stale" wording is loose.*
- [ ] The option sweep (formerly risk 2): vary every option in the `opts_chunk` defaults
  (`R/defaults.R`) and every Quarto cell option over a fixture and assert that either the hash or the presented output changes; an option that
  does neither is a silent divergence.
- [x] *(for the `quarto_format` tag)* Adapter tests over hand-built
  pieces: stream, error, display_data per KNR-1\'s table; flat option keys per decision 2; notebook-level folding of `includes`, `preserve`,
  `needs_postprocess` and `knit_meta` dependencies, unchanged from EXR-11\'s behavior; and the
  adapter converts `knit_meta` into the neutral per-output dependency manifest (epic decision
  27), tested against the four-widget fixture of KNR-1 S2 (normal, `results='hide'`,
  `include=FALSE`, `echo=FALSE`) so the rebuilt set equals the live one.
  *Done (2026-11-03) for everything that is KNR-3\'s: stream, error, display_data and flat keys were covered; `notebook_level_results_stay_out_of_the_knitr_record` (adapter) and `chunk_records_leave_notebook_level_meta_unchanged` (R-gated) pin that includes, preserve, `needs_postprocess` and dependencies stay on the `ExecuteResult` as in EXR-11. The `knit_meta` manifest and the four-widget test are not KNR-3\'s (settled 2026-11-03, see slice 2 notes): they belong to WGT-2\'s adapters item.*
- [x] Every produced record passes EXR-4\'s nbformat validator.
  *Done (2026-11-03): `KNR_DUMP_RECORDS=<dir>` on the corpus report keeps every written record; `uv run scripts/exr-record/nbformat_facts.py <dir>/*.ipynb` accepted all 214 with no warnings (exit 0). Run on demand because it needs `uv` and Python.*
- [x] Alignment: unit count and order equal the shared unit list; a mismatch yields no
  record and a diagnostic, and the render succeeds.
- [x] Table-driven fallback tests: one case per row of the table above yields exactly the
  unit kind, recorder, profile and diagnostic the table states; no unit has both structured
  outputs and a `text/x-quarto-markdown` output.
- [x] A deserialize test for the wire field added in KNR-1 (`KnitrExecuteResult` derives
  `Deserialize` only; add `Serialize` only if a later step needs it).
  *Reconciled 2026-11-03 against the tree: `capture_tests.rs` (anchors, holds, kinds, `known_options` without `foo.unknown`) and `pieces_map`/adapter tests; the knitr 1.44 versus latest comparison was not run.*
  *Reconciled 2026-11-03 against the tree: `asis_pieces_carry_the_markdown_passthrough_marker`.*
  *Reconciled 2026-11-03 against the tree: `hash_rule.rs`, `a_presentation_only_option_does_not_move_a_structured_hash`, `content_hash_matches_every_phase_0_vector`.*
  *Reconciled 2026-11-03 against the tree: adapter tests `the_document_hash_follows_the_structured_cells` and the rehash round trip.*
  *Reconciled 2026-11-03 against the tree: `fill_ids_names_both_lengths_and_fills_nothing_on_a_mismatch`, `a_unit_with_no_captured_chunk_is_a_count_mismatch`.*
  *Reconciled 2026-11-03 against the tree: `execute_result_with_chunks_field_deserializes_records` and the `cacheDisabled`/`hook_override` defaults.*
  *Reconciled 2026-11-03: open. No test builds an EXR-11-era record; the version-mismatch rejection is covered by the reader only.*
  *Reconciled 2026-11-03: open. Only the corpus generator (`generate_sweep`) exists, not the assert-hash-or-output-changes test.*
  *Reconciled 2026-11-03: stream, error, display_data and flat option keys are covered; the neutral `knit_meta` manifest and the four-widget test are open, waiting on WGT-2\'s schema (slice 2 note).*
  *Reconciled 2026-11-03: open as a dedicated test. The reader rejects invalid structure and every read-back in slices 3 and 4 goes through it, but nothing runs EXR-4\'s validator over each produced record.*

- [x] Slice 5 tests, each failing first (see "Slice 4 notes"): (a) an `include_graphics` image
  built by the mapper presents as `![](made.png){width=200}`, not `FIG1`; (b) a `#|` header
  `comment: "##"` keeps the prefix, a `fig-cap` with `*emphasis*` keeps its markup, and a
  `!expr` header value does not replace the recorded one; (c) a list-nested and a blockquote
  chunk present with every line in its container, through `present_document`; (d) a numeric
  `eval` and a `ref.label` chunk present the echoed code, `quarto_echoed_code` is absent for an
  ordinary chunk, and the exec hash and `recompute_cell_hash` are unchanged by the key.
  *Done (2026-11-03, slice 5; see "Slice 5 notes"). (c) tested a list-nested chunk through
  `present_document`; the blockquote case stays the allowlisted one.*

### Phase 3B: implementation

- [x] Rename `metadata.quarto_knitr.file` to `quarto_file_path` (epic decision 25): `pieces_map.rs` (the
  writer in `image()` and the assertion in its tests), the capture, and any fixture that reads the old
  key. Tests first: an image output carries `quarto_file_path` and no `quarto_knitr` `file`.
  *Done (2026-11-03, `28474d2ec`).*
- [x] **The shared record-to-presenter reader (re-scope item 3).** *Done (2026-11-03, slice 1; see "Slice 1 notes" under the progress section).* `knitr_cell_inputs(cell,
  engine_entry)` turns a knitr record cell into the presenter inputs: `quarto_output_kinds`,
  `quarto_output_anchors`, `quarto_resolved_options` layered under the chunk\'s current header
  options, the original info string and `#|` lines, `expression_starts` for `prompt`, the
  holds, `knitr_visibility`, and `known_options` from the entry. Wire it into `present_document`,
  `record_splice::present_cell` and `ReplayEngine::present_record`; add an optional
  `known_options` field to `EngineEntry` (absent means no filtering; `quarto_record_version`
  stays 1) and the parameter `present_document` needs to receive it. Tests first, one per
  input, each driven through all three paths over a hand-built record; Jupyter goldens stay
  identical, and a cell with none of the knitr keys presents as before.
- [x] Consume `target_route`: call `route_for_target(&ctx.format)` in `KnitrEngine::execute`, set
  `KnitrExecuteParams.capture` from it and `want_record`, surface `fallback_diagnostic` once per
  document, and delete the `cfg_attr(not(test), expect(dead_code))` on the module.
- [x] Join captured chunks to units by `options["quarto-unit-id"]`; a captured chunk without an
  id is ignored, and a unit with no captured chunk is a count mismatch (decision 4).

- [x] `capture.R` computes the known option names once per record (from the knitr that ran);
  the adapter writes `known_options` on the engine entry, the first two cell keys, and
  `quarto_markdown_passthrough` on `knit_asis` output (epic decision 22); KNR-2\'s
  message-versus-warning and unknown-option items (10 corpus cases) depend on this item.
- [x] The knitr declaration (same id, bumped version, the `structured`
  profile), supplied through `ExecutionEngine::hash_definition` in the knitr module.
  *Done (2026-11-03, `0a127e8b0`; version 2).*
- [x] The adapter from captured pieces to record units, using KNR-1\'s mapper, replacing
  EXR-11\'s recorder for structured chunks.
- [x] The fallback unit (its `text/x-quarto-markdown` mime is the marker; no key), built by the chunk hook from the engine\'s finished
  markdown for that chunk only; the capture mechanism must leave the hook running for these
  chunks (S1 confirms).
- [x] The chosen cache, `child=` and user-hook policies, each with a diagnostic code
  allocated from the engine range (`docs/errors/engine/` holds Q-18-1 to Q-18-5 and Q-18-10, and
  Q-18-6 to Q-18-9 are claimed elsewhere; take Q-18-11 and up, re-checking the catalog, with a docs
  page and a sidebar entry in the same commit as each code).
- [x] Both recorders write behind the same `ExecuteResult` record field (added by EXR-7;
  `engine::context::ExecuteResult` has none today), additive: the live path
  and its markdown are unchanged until KNR-6, and until then the record is shadow output
  read only by tests and the shadow comparison.
  *Reconciled 2026-11-03 against the tree: `ExecuteResult.record` exists in `engine::context`; the structured adapter and the fallback cells write through it.*

- [x] Slice 5 implementation, in the order of the progress list: the `included_graphic` key;
  `knitr_header_options` at both call sites; the author text and `replacement` in
  `present_document`; `quarto_echoed_code` (the adapter keeps the `code` that
  `pieces_to_cell_anchored` returns, `present_knitr_cell` reads it). Epic decision 21 is
  already amended (2026-11-03). *Done (2026-11-03, slice 5; see "Slice 5 notes").*

### Phase 3C: shadow

- [x] A shadow test, **run in report mode** (Gordon, 2026-11-03; the assertion moves to KNR-2b
  Phase 2bC): with the record built and **written then read back**, presenting it through the
  product path (`present_document` and the reader above), not the runner\'s direct path, equals the
  live markdown on the corpus (this runs as KNR-2 lands; the harness is KNR-1\'s). The accepted
  classes are Identical and SyntaxLevel only; Semantic and Blocker need a signed-off allowlist
  entry; non-HTML formats are compared at the AST level; the normalizer is limited to widget
  ids and figures are checked by position and extension (epic decision 26). Freeze the live
  markdown per fixture as the oracle (committed; the snapshot-reporting rule of AGENTS.md
  applies: count, summary, surprises, file list).
  *Slice 4 done (2026-11-03): report mode, written and read back, through `present_document`; the live markdown is frozen (214 files). 16 blockers, all closed by slice 5 except `s9/01_inline` (KNR-4); see "Slice 4 notes".*

## Exit criteria

- A knitr run produces records whose outputs are standard nbformat except for marked
  fallback units, validated by EXR-4\'s validator, with unit counts matching the shared
  unit list on the nested and list-nested fixtures.
- Hash, declaration, version, option-sweep, alignment and table-driven fallback tests
  pass; the live output is unchanged.
- The corpus report, through the written record, has one blocker, `s9/01_inline` (KNR-4), and
  every other difference is Identical, SyntaxLevel or a signed-off allowlist entry.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` green; counts against the live baseline.
- The fallback rate on the corpus is reported per fixture. The acceptable rate is set by
  Gordon from the first report; until then the criterion is that it is reported.
  *Settled (Gordon, 2026-11-01, after I3):* no numeric threshold. Hook override, non-R engine,
  `dev="pdf"` and `child=` are accepted fallbacks; the rate is reported on any new corpus.
  *Reported by I3 (2026-11-01):* 8 of 278 cells (2.9%) in the 206 documents that build a record
  (hook override 2, non-R engine 4, `dev="pdf"` 2), plus one document of 9 cells with no record
  (`child=` count mismatch); 17 of 287 (5.9%) counting that one. The corpus is a one-construct
  sweep, so the figure names the constructs that fall back, not a rate for real documents.
  Anchors key omitted: 0 of 244 structured chunks with output. Record sizes: median 2.4 KB,
  figure records median 46 KB, largest 163 KB, inline figures 1.31 times the files (bd-sr73:
  inline recommended). [Findings](../research/2026-11-01-knr-i3-records-through-preview.md).
- Decision 8 is settled (2026-11-01) and written into this plan (decisions 3 and 9 were
  settled on 2026-10-31).

## Risks

1. **Hash correctness depends on decision 10.** If KNR-1 S1 overturns the capture point,
   the structured profile\'s list changes. Do not finalize it before S1 reports.
2. **Silent divergence between hash and capture.** If an option affects what is captured
   but is not execution-relevant in the list, a stale record looks fresh. Mitigation: the
   option sweep in Phase 3A.
3. **Setup-chunk hook changes mid-document** make "per document" detection unsound.
4. **Names that cannot be listed.** Forwarded attribute options count in the hash (decision
   1), so editing `class.output` re-executes a chunk.

## References

- [EXR-4](2026-10-28-exr-04-record-format.md), [EXR-10](2026-10-28-exr-10-knitr-k0-options-map.md),
  [EXR-11](2026-10-28-exr-11-knitr-k1-chunk-records.md)
- `external-sources/knitr/R/block.R` lines 100-101 and 300-340 (development snapshot, see
  KNR-1 Background)
- `crates/quarto-core/src/engine/knitr/{types.rs,mod.rs}`
