---
title: 'KNR-3: knitr record adapter, hash profiles, fallback units, cache and child handling'
date: 2026-10-30
description: 'Turns KNR-1 captured pieces into EXR-4 record units for knitr: cells and outputs, the structured hash profile beside the default one, notebook-level engine results, per-chunk fallback units, and the policies for knitr cache hits, child documents and user-overridden hooks.'
status: draft  # outline written from code reading; blocked items named
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
**Estimated sessions:** 3 to 4 (a guess made before KNR-1 reports; the two internal keys
below are about half a session of it, an estimate from interlude I1).
**Prerequisite, not part of this plan:** the knitr replay claim fix (bd-js24dold, about 20
lines), so that a knitr trace replays at all; it lands before this plan starts.

> **Status of this document.** Written 2026-10-30 from code reading, before KNR-1 runs, and
> revised on 2026-10-31 after the paper review
> ([research note](../research/2026-10-30-knitr-epic-paper-review.md)) and the parent
> epic's hash profiles. Items marked *blocked on* cannot be settled without a
> KNR-1 spike; items marked *open* need a decision before this plan runs. Decision 8 (the format in the record) was settled on 2026-11-01: the knitr exception is
> accepted. One question still waits on the parent session: the second item of decision 2
> (which hash list produces the flat cell keys), plus the shape EXR-4 gives the exception.

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
what the presenter cannot recover from `outputs`, each starting with `quarto_` so
`CellMetadata::options()` drops it and none entering the exec hash:

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
6. **Two internal cell keys and one engine-entry field** (epic decision 21, signed off
   2026-11-01, field name settled 2026-11-02): `quarto_output_anchors` and
   `quarto_output_kinds` on cells, and `known_options` on the `quarto_engines` entry, as
   described under "What a structured knitr unit holds". The adapter writes them; KNR-2
   reads them.
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
2. **Where resolved options live** *(S3 and S8 answered 2026-10-31: the chunk hook records them, in knitr\'s spelling, with the serialization rule and the closed list in KNR-1 S3; the field name is still to be chosen here)*. EXR-4 computes the flat cell
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

## Phases (provisional)

### Phase 3A: tests first

- [ ] Key tests, each failing first (epic decision 21): `quarto_output_anchors` equals the
  capture order for a chunk with interleaved source and output, and is **not** reordered for
  `fig.show = "hold"` or `results = "hold"`; it is omitted when the source pieces\' lines do
  not add up to the code; `quarto_output_kinds` marks a message and a warning apart;
  the record\'s `known_options` contains `echo` and not `foo.unknown`, on knitr 1.44 and the
  latest; the two cell keys are dropped by `CellMetadata::options()`, `known_options` is a
  field on the engine entry, and none changes the exec hash.
- [ ] `knit_asis` output carries `quarto_markdown_passthrough: true`, and nothing else does;
  no `raw` marker is written for `results = "asis"`.
- [ ] Hash tests with vectors: a structured chunk with a presentation-only option changed
  has the same `quarto_exec_hash`, and one with an execution-relevant option changed
  (including `include`, `warning`, `message`, `fig.keep`) has a different one; the same
  edit to a fallback unit changes its hash; prose changes never reach either.
  `quarto_content_hash` matches Positron\'s SHA-1 vectors (EXR-4, EXR-16 S1).
- [ ] Splice-key tests for the parent edit (decision 8, as corrected by I3), in the parent\'s
  `record_splice.rs` tests, each failing before the edit: a structured record built for
  `typst` and rendered as html reports Stale; the same record rendered as `typst` reports
  Fresh; a presentation-only edit (`echo`) on a structured unit reports Fresh and splices with
  the current echo; an execution-relevant edit reports Stale; a unit whose id has no cell
  keeps today\'s key; the existing Jupyter and marimo-shaped tests pass unchanged. The spike
  numbers in decision 8 are the expected red state.
- [ ] The `UnitKey.alt_exec_hashes` field, the two `match_units` changes and the call in
  `splice_units` (the I3 prototype, `9a0ff2d23` on `spike/knr-i3`), small enough to read in one
  view; add a test that pass 2 skips a cell naming a profile; report the diff to Gordon before
  the commit.
- [ ] The adapter sets `quarto_hash_rule`, the exec hash and the flat keys (including
  `quarto-output-format`) after `RecordBuilder::push` and recomputes the document hash with
  `hash_document` and each cell\'s `UnitHashRef.profile`; a test writes and reads a mixed
  record and recomputes every hash (the spike\'s shape).
- [ ] Declaration tests: the knitr declaration keeps the id `knitr` with a bumped version;
  the `structured` profile is stored; a unit tagged with a missing profile reads as stale;
  every `execute:` key counts in the document hash of any knitr record, mixed or not
  (decisions 3 and 9), and an edit to a fallback chunk\'s presentation option changes that
  unit\'s hash.
- [ ] Version test: an EXR-11-era record reads as stale and still
  presents through the pass-through presenter. The fixture is a record written by EXR-11\'s
  recorder, so this test needs EXR-11 merged; until then, hand-build one from EXR-11\'s
  format.
- [ ] The option sweep (formerly risk 2): vary every option in the `opts_chunk` defaults
  (`R/defaults.R`) and every Quarto cell option over a fixture and assert that either the hash or the presented output changes; an option that
  does neither is a silent divergence.
- [ ] *(for the `quarto_format` tag)* Adapter tests over hand-built
  pieces: stream, error, display_data per KNR-1\'s table; flat option keys per decision 2; notebook-level folding of `includes`, `preserve`,
  `needs_postprocess` and `knit_meta` dependencies, unchanged from EXR-11\'s behavior; and the
  adapter converts `knit_meta` into the neutral per-output dependency manifest (epic decision
  27), tested against the four-widget fixture of KNR-1 S2 (normal, `results='hide'`,
  `include=FALSE`, `echo=FALSE`) so the rebuilt set equals the live one.
- [ ] Every produced record passes EXR-4\'s nbformat validator.
- [ ] Alignment: unit count and order equal the shared unit list; a mismatch yields no
  record and a diagnostic, and the render succeeds.
- [ ] Table-driven fallback tests: one case per row of the table above yields exactly the
  unit kind, recorder, profile and diagnostic the table states; no unit has both structured
  outputs and a `text/x-quarto-markdown` output.
- [ ] A deserialize test for the wire field added in KNR-1 (`KnitrExecuteResult` derives
  `Deserialize` only; add `Serialize` only if a later step needs it).

### Phase 3B: implementation

- [ ] Rename `metadata.quarto_knitr.file` to `quarto_file_path` (epic decision 25): `pieces_map.rs` (the
  writer at `:132` and the assertion at `:252`), the capture, and any fixture that reads the old
  key. Tests first: an image output carries `quarto_file_path` and no `quarto_knitr` `file`.

- [ ] `capture.R` computes the known option names once per record (from the knitr that ran);
  the adapter writes `known_options` on the engine entry, the two cell keys, and
  `quarto_markdown_passthrough` on `knit_asis` output (epic decision 22); KNR-2\'s
  message-versus-warning and unknown-option items (10 corpus cases) depend on this item.
- [ ] The knitr declaration (same id, bumped version, the `structured`
  profile), supplied through `ExecutionEngine::hash_definition` in the knitr module.
- [ ] The adapter from captured pieces to record units, using KNR-1\'s mapper, replacing
  EXR-11\'s recorder for structured chunks.
- [ ] The fallback unit (its `text/x-quarto-markdown` mime is the marker; no key), built by the chunk hook from the engine\'s finished
  markdown for that chunk only; the capture mechanism must leave the hook running for these
  chunks (S1 confirms).
- [ ] The chosen cache, `child=` and user-hook policies, each with a diagnostic code
  allocated from the engine range (`docs/errors/engine/` holds Q-18-1 to Q-18-3; take the
  next free numbers and add a catalog page for each, as the existing ones do).
- [ ] Both recorders write behind the same `ExecuteResult` record field (added by EXR-7;
  `engine::context::ExecuteResult` has none today), additive: the live path
  and its markdown are unchanged until KNR-6, and until then the record is shadow output
  read only by tests and the shadow comparison.

### Phase 3C: shadow

- [ ] A shadow test: with the record built, presenting it through KNR-2\'s layer equals the
  live markdown on the corpus (this runs as KNR-2 lands; the harness is KNR-1\'s). The accepted
  classes are Identical and SyntaxLevel only; Semantic and Blocker need a signed-off allowlist
  entry; non-HTML formats are compared at the AST level; the normalizer is limited to widget
  ids and figures are checked by position and extension (epic decision 26). Freeze the live
  markdown per fixture as the oracle (committed; the snapshot-reporting rule of AGENTS.md
  applies: count, summary, surprises, file list).

## Exit criteria

- A knitr run produces records whose outputs are standard nbformat except for marked
  fallback units, validated by EXR-4\'s validator, with unit counts matching the shared
  unit list on the nested and list-nested fixtures.
- Hash, declaration, version, option-sweep, alignment and table-driven fallback tests
  pass; the live output is unchanged.
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
