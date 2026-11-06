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
extends; a draft on 2026-10-31, so tests and steps that name its recorder or its chunk hook
need it merged first), and
[EXR-7](2026-10-28-exr-07-engine-wire.md) (hard, because EXR-11 depends on it and because
the record field on `ExecuteResult` comes from it).
Cite KNR-1\'s decisions by their headings: `#### S1 decision` through `#### S9 decision`.
**Blocks:** KNR-4, KNR-5, KNR-6.
**Estimated sessions:** 3 to 4 (a guess made before KNR-1 reports).

> **Status of this document.** Written 2026-10-30 from code reading, before KNR-1 runs, and
> revised on 2026-10-31 after the paper review
> ([research note](../research/2026-10-30-knitr-epic-paper-review.md)) and the parent
> epic's hash profiles (`457229c47`). Items marked *blocked on* cannot be settled without a
> KNR-1 spike; items marked *open* need a decision before this plan runs. Two questions wait
> on the parent session: decision 8 (the format in the record) and the second item of
> decision 2 (which hash list produces the flat cell keys). Checkboxes that cannot be coded
> until decision 8 is answered are tagged *(needs decision 8)*.

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
8. **Format in the record** *(open: the parent session was asked on 2026-10-31; must be
   settled before this plan runs)*. Parent
   decision 27 says the record carries no presenter inputs and stays format-neutral: the
   output format is pipeline context at present time. But knitr pieces differ per format
   (`kable`, `df_print`, `prefer-html`), the format is in no hash, so a structured record
   built for HTML would read as fresh for a Typst render, and EXR-4 still tags rendered
   units with `quarto_format`. Options: (i) ask the parent for a knitr exception (the
   format is part of the structured profile\'s hash input and tags the unit), (ii) capture
   only format-independent pieces, which S2 tests, or (iii) one record per format. The
   recommendation is (i), because S2 is expected to show format-dependent pieces.
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

- [ ] *(needs decision 8)* Hash tests with vectors: a structured chunk with a presentation-only option changed
  has the same `quarto_exec_hash`, and one with an execution-relevant option changed
  (including `include`, `warning`, `message`, `fig.keep`) has a different one; the same
  edit to a fallback unit changes its hash; prose changes never reach either.
  `quarto_content_hash` matches Positron\'s SHA-1 vectors (EXR-4, EXR-16 S1).
- [ ] *(needs decision 8)* Declaration tests: the knitr declaration keeps the id `knitr` with a bumped version;
  the `structured` profile is stored; a unit tagged with a missing profile reads as stale;
  every `execute:` key counts in the document hash of any knitr record, mixed or not
  (decisions 3 and 9), and an edit to a fallback chunk\'s presentation option changes that
  unit\'s hash.
- [ ] *(needs decision 8)* Version test: an EXR-11-era record reads as stale and still
  presents through the pass-through presenter. The fixture is a record written by EXR-11\'s
  recorder, so this test needs EXR-11 merged; until then, hand-build one from EXR-11\'s
  format.
- [ ] The option sweep (formerly risk 2): vary every option in the `opts_chunk` defaults
  (`R/defaults.R`) and every Quarto cell option over a fixture and assert that either the hash or the presented output changes; an option that
  does neither is a silent divergence.
- [ ] *(needs decision 8, for the `quarto_format` tag)* Adapter tests over hand-built
  pieces: stream, error, display_data per KNR-1\'s table; flat option keys per decision 2; notebook-level folding of `includes`, `preserve`,
  `needs_postprocess` and `knit_meta` dependencies, unchanged from EXR-11\'s behavior.
- [ ] Every produced record passes EXR-4\'s nbformat validator.
- [ ] Alignment: unit count and order equal the shared unit list; a mismatch yields no
  record and a diagnostic, and the render succeeds.
- [ ] Table-driven fallback tests: one case per row of the table above yields exactly the
  unit kind, recorder, profile and diagnostic the table states; no unit has both outputs
  and a fallback marker.
- [ ] A deserialize test for the wire field added in KNR-1 (`KnitrExecuteResult` derives
  `Deserialize` only; add `Serialize` only if a later step needs it).

### Phase 3B: implementation

- [ ] *(needs decision 8)* The knitr declaration (same id, bumped version, the `structured`
  profile), supplied through `ExecutionEngine::hash_definition` in the knitr module.
- [ ] *(needs decision 8)* The adapter from captured pieces to record units, using KNR-1\'s mapper, replacing
  EXR-11\'s recorder for structured chunks.
- [ ] The fallback unit and its marker, built by the chunk hook from the engine\'s finished
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
  live markdown on the corpus, modulo the classified differences (this runs as KNR-2
  lands; the harness is KNR-1\'s).

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
- Decision 8 is settled with the parent\'s answer and written into this plan (decisions 3 and
  9 were settled on 2026-10-31).

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
