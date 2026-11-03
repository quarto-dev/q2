---
title: 'Epic: knitr execution records as structured ipynb (and knitr as a first-class Positron engine)'
date: 2026-10-30
description: 'Extends the execution-records epic to knitr: capture knitr evaluated pieces on the q2 side, store them as standard nbformat outputs, present them with the shared Rust presenter plus a knitr option layer, and make knitr records valid to present in Positron. Differential testing against knitr output decides how much further work is needed.'
status: in-progress  # parent epic complete (not yet merged to main); KNR-1 and the interlude done, KNR-2 closed; next KNR-3, then KNR-4, KNR-2b, KNR-6
---

**Branch:** the work runs on `knr-2-presenter` (workspace-3), rebased on 2026-11-03 onto the
squashed `feature/execution-records-epic` (`c5f8c7593`); the unsquashed history of the parent
is `history/feature/execution-records-epic`. The old `feature/knitr-execution-records-epic`
branch is superseded.
**Extends:** [the execution-records epic](2026-10-28-execution-records-epic.md). That
epic is complete on its integration branch (not yet merged to main) and unchanged in scope,
with knitr still on rendered markdown (its decision 6, reworded on 2026-10-30 to say "in
this epic").
**Origin:** the `knitr-execution-records` session of 2026-10-30, which included a
DeepWiki session on knitr hooks (`extract-dw`, search
`i-understand-knitr-supports-mu_9b46087f`) and experiments with knitr 1.50 and
rmarkdown 2.29. **Revised 2026-10-31** after a
[paper review](../research/2026-10-30-knitr-epic-paper-review.md) and a rebase onto the
parent epic\'s hash profiles. The parent was squashed on 2026-11-02 and this work was rebased
onto it on 2026-11-03, so commit hashes the plans cite from before then (for example
`457229c47`, `d0112a569`, `663a2f7a8`) name the unsquashed history.
**Terminology:** as in the parent epic. A **piece** is one element of the list
`evaluate()` returns for a chunk (source, text, message, warning, error, recorded
plot, `knit_asis`). A **fallback unit** is a knitr unit whose output is knitr's finished
markdown, used only where pieces cannot be captured.

## Goal

Make knitr a normal producer of engine execution records:

```text
chunk --> knitr evaluate() --> pieces --> (captured on the q2 side) --> nbformat outputs
      --> record --> shared Rust presenter + knitr option layer --> markdown
```

- Records hold standard nbformat outputs (`stream`, `error`, `display_data`,
  `execute_result`) that look like what a kernel would produce for the same cell, so
  Positron can present them and q2 can import a Positron cache for R cells.
- The Rust presenter, not R, builds the Quarto cell markdown, live and on replay,
  natively and in WASM. knitr's finished markdown is **not** stored beside the
  structured outputs.
- How far the Rust output is from knitr's own is **measured**, not assumed: a
  differential corpus compares the two and classifies every difference. The
  remaining work is scoped from that measurement.

## Why a separate epic

The parent epic is large and nearly drafted, and it should land with knitr working on
markdown. Everything here is additive on top of it until the flip (KNR-6). The scope
of the presenter layer (KNR-2) depends on the corpus results, so only the first plan
is written in detail. The others are outlines, to be written once KNR-1 reports.

## What this epic overrides in the parent epic

When the parent epic has landed, these change; the parent\'s text is not edited again. The
list was first agreed on 2026-10-30 and extended on 2026-10-31 after the paper review and
the parent\'s hash profiles (`457229c47`).

- **Decision 6** (knitr stores rendered chunk markdown): replaced, except for the
  fallback unit.
- **Decision 9, knitr parts** (replay and the trace for knitr): after KNR-6 the replay
  engine returns the stored record for knitr too, and the Rust presenter runs again.
- **Decision 10, knitr row** (legacy degrade): KNR-3 states that an EXR-11-era record reads
  as stale after the declaration version bump and still presents through the pass-through
  presenter until the next run; old traces replay under decision 14 here.
- **Decision 22, knitr row** (cell location by wrapper Div id): ids now come from the
  stage\'s shared unit list (parent decisions 38 and 39); KNR-3 decision 11 states what stays.
- **Decision 21** (knitr inline ids): revisited in KNR-4, because EXR-12\'s mechanism is
  replaced and parent decisions 38 and 40 changed the id source. *Not named when the
  override list was first agreed; confirmed by Gordon on 2026-10-30 (decision 15).*
- **Decision 35 and EXR-4\'s and EXR-11\'s knitr declaration** (empty exclusion lists):
  extended by a second profile for structured units, using the profile mechanism the parent
  added for this purpose.
- **Decision 27** (the record carries no presenter inputs and is format-neutral): in
  tension with format-dependent knitr pieces; KNR-3 decision 8 resolves it, and the parent
  session was asked on 2026-10-31 (open until it answers).
- **Decision 14 and EXR-16 decision 7** (importer gate, Jupyter-owned only): widened by
  KNR-5 to knitr-owned units with token `r`.
- **Decision 34** (current options layered over the record\'s): given its knitr form in
  decision 11 here; for now only the chunk\'s header options are layered, not the document
  scope (KNR-3 decision 3).
- **Decision 3, figures** (inline base64): KNR-1 S2 and KNR-5 carry knitr file paths. Settled
  (Gordon, 2026-11-01, bd-sr73): figures are inline base64.
- **EXR-10, EXR-11, EXR-12** (all complete; they were drafts on 2026-10-31): they landed as
  written first. This epic extends what they deliver. KNR-3 replaces EXR-11\'s recorder for structured chunks and KNR-6
  deletes that code and EXR-12\'s inline-id code once nothing uses it. The chunk hook stays
  because the finished markdown of a fallback chunk exists nowhere else; fallback chunks are
  its settled role. Whether it also carries final options for structured chunks (KNR-1 S3)
  or emits skeleton markers (KNR-1 S7) is decided by those spikes. EXR-10 (the options map,
  bd-n1vmlalw) stays.
- **EXR-11\'s wire field and `options_digest`:** EXR-11\'s `chunks` field
  (`{id, markdown, options_digest, files}`) carries finished markdown, so it serves fallback
  units. KNR-1\'s `pieces` is a parallel per-chunk field for structured units (KNR-1 S2 fixes
  its shape); KNR-3 merges the two per unit. EXR-11 decision 5 (`options_digest` is
  provenance, not hashed) is superseded for structured units by KNR-3 decision 2 (resolved
  options in their own metadata field); a fallback unit keeps the digest as EXR-11 defines it.
- **EXR-4\'s knitr Outputs row** (one `display_data` with `text/x-quarto-markdown`): holds
  for fallback units only; structured units hold standard nbformat outputs (decision 2).

## Settled decisions

Made with Gordon on 2026-10-30.

1. **Option A for staging, with one exception.** The parent epic lands in full, including
   EXR-10/11/12, before KNR-2 to KNR-7 start. KNR-1 (the spikes and the harness) may start
   as soon as EXR-1 and EXR-2 are available on the integration line (Gordon, 2026-10-31):
   it runs on the current markdown path, and its spikes are the cheapest way to learn
   whether the epic\'s shape holds. Until they are merged (EXR-1 Phase 3, EXR-2 Phase 2) it does
   only the work that needs neither (KNR-1, Start condition). Its chunk keys are a
   harness-local ordinal; record ids come from the stage\'s shared unit list in KNR-3.
2. **Structured outputs, no side channel.** knitr units hold nbformat outputs built
   from captured pieces. knitr's finished markdown is not stored next to them. The one
   exception is the per-chunk fallback unit (decision 8).
3. **Capture on the q2 side.** q2 already patches knitr\'s namespace
   (`resources/rmd/patch.R`, `assignInNamespace`; it already replaces `sew` and
   `add_html_caption` and defines `wrap_asis_output`, and is byte-identical to Quarto 1\'s),
   so capturing pieces needs no upstream knitr change. The capture point is before knitr\'s
   filters (decision 10). The mechanism is chosen by KNR-1 S1, tried in this order: wrap the
   `evaluate` hook (`knit_hooks$get('evaluate')`), else a replacement copy of `eng_r`;
   wrapping `sew` runs after the filters and is available only if decision 10 is overturned.
   The capture lives in its own file and never edits the existing overrides.
4. **One presenter.** The knitr output goes through the shared Rust presenter
   (EXR-1) with a small knitr-specific option layer (`collapse`, `prompt`, `comment`,
   `results`, `fig.show`, and similar). The shared presenter is how Jupyter and knitr cells
   get identical Quarto cell semantics (layout, captions, code folding).
5. **Fidelity bar.** Byte identity with knitr's own markdown is measured and reported,
   not required. Differences are classified as *syntax-level* (bytes differ, parsed AST
   equal), *semantic but looks the same* (AST differs, rendered output equal),
   *blocker* (wrong or missing output), or *intentional divergence* (a recorded
   decision). Only blockers must be fixed.
6. **Positron, two ways, "valid to present".** A Positron-written R record can fill a
   knitr unit (display-only), and a q2 knitr record is valid nbformat that Positron can
   present when placed by hand in its cache directory; KNR-5 checks both and builds no path
   that delivers a record to Positron. Equal-to-what-knitr-would-produce is not required. The
   unit hash follows Positron\'s source hash plus a **closed list of presentation-only
   options** for structured units (a named profile of the knitr `HashDefinition`, the
   parent\'s hash-profile mechanism); everything not on the list counts, including `eval`,
   `fig.width`, `dev`, `error`, `include`, `warning`, `message`, `fig.keep` and `cache`
   (decision 10). Fallback units use the default all-options profile. The declaration keeps
   the id `knitr` with a bumped version, so records written under the parent epic\'s
   all-options rule read as stale.
7. **knitr stays whole-document.** Reuse of a knitr record is whole document or
   nothing, as in the parent epic. Single-cell execution for knitr is out of scope.
8. **Per-chunk fallback.** Where pieces cannot be captured (`child=`, knitr cache hits,
   overridden hooks, LaTeX and not-yet-covered targets, and anything the harness shows is not
   capturable), that chunk becomes a fallback unit with `text/x-quarto-markdown`. That mime is
   the fallback marker (Gordon, 2026-11-02); no metadata key flags it. A record may mix structured and fallback units, but each unit uses
   exactly one mechanism, never both (parent risk 6). Fallback units are opaque to Positron
   and use the default (all-options) hash profile; structured units use the structured
   profile (decision 6). The rules per case and target are one table in KNR-3, and the chunk
   hook EXR-11 built stays permanently to write fallback units.
9. **The oracle is q2\'s current knitr output, informed by Quarto 1 (2026-10-30).** The
   differential corpus compares the new path against q2\'s current knitr output (R code
   vendored from Quarto 1, and byte-identical to it today). Quarto 1 is a second reference,
   but since the R files are identical it adds signal only from what the Rust side passes in,
   the inline preprocess and the TypeScript post-processing, especially for features q2 has
   not implemented yet: where q2\'s output lacks or differs from what Quarto 1 does, the
   difference is recorded as a finding (a q2 gap), not silently treated as the right answer.
   How Quarto 1 output is gathered for the fixtures is decided in KNR-1, Phase 1D; a Quarto 1
   run needs a Quarto 1 install, so it skips when absent, like R (decision 17). What happens
   to a gap is decision 20.
10. **Capture before knitr\'s filters (D1, 2026-10-30; refined 2026-10-31).** The capture sits
    before the filters, so the record holds what the chunk produced, independent of `echo`,
    `results` and (probably) `fig.show`, and the Rust presenter applies those filters.
    Three refinements follow from the paper review. (1) The capture does not change what R
    does: it never forces `stop_on_error`, and it does not force `keep_warning` or
    `keep_message` by default, so `warning`, `message`, `error` and `include` are
    execution-relevant (hashed); S1 may show that forcing `keep_*` is safe only with a live
    path that re-filters. (2) Plots are the exception to "the presenter filters": knitr opens
    no recording device for `fig.keep='none'`, and the default `high` merges low-level plot
    additions using its own recordings, so plot selection happens at capture time and
    `fig.keep` is execution-relevant. (3) The mechanism is chosen by S1 (decision 3). Reason:
    capture before the filters is the only option consistent with decisions 4 and 6. KNR-1 S1
    can still overturn it; it flips if no mechanism captures without changing R\'s behavior,
    or if plot capture is fragile across the knitr versions tested.
11. **Presentation options come from the record for knitr, with the current header winning
    (D2, 2026-10-30; layering added 2026-10-31).** Each knitr unit carries its resolved options
    (after `opts_chunk$set`, `opts_template`, `opts.label`, `ref.label` and `opts_hooks`), and
    the presenter reads them, with the options currently written on the chunk (`#|` lines and
    fence attributes) layered over them and winning, as parent decision 34 anticipates. A
    header edit to a presentation option therefore shows without re-running; a setup-chunk
    edit does not show until the next run (KNR-3, decision 10). This relaxes the parent EXR-6
    rule ("never from the record") for knitr only. One resolved-options type serves both
    engines: the Jupyter path fills it from the document, the knitr path from the record plus
    the current header. Reason: setup chunks are arbitrary R, so the document text cannot show
    what knitr hides. The document scope (`execute:`) is not layered over the record\'s options
    for now (Gordon, 2026-10-31): in knitr a setup chunk\'s `opts_chunk$set` outranks the
    `execute:` defaults, and the recorded resolved options already include both in knitr\'s own
    order. An `execute:` edit therefore re-executes (every `execute:` key counts in the
    document hash, KNR-3 decision 3). S8 checks where `execute.R` sets the defaults, and the
    layering is revisited with its result. KNR-1 S3 decides how function- and
    expression-valued options are serialized; S8 holds the failing fixture. The unit hash uses the structured profile\'s closed presentation-only list
    (decision 6).
12. **The Quarto cell-option layer lives in the shared presenter (D3, 2026-10-30).** `panel`,
    `column`, `layout*`, captions, labels and `code-fold` are built once in the shared
    presenter, so Jupyter gains them and the two engines cannot drift. A Jupyter cell changes
    only if it sets one of the new options (a document that already sets `panel`,
    `layout-ncol` or `code-fold` on Jupyter cells will render them, as in Quarto 1), so the
    parent epic\'s Jupyter goldens (EXR-1, EXR-6), which set none, stay byte-identical.
    Nothing needs to be communicated to the parent epic\'s owner (Gordon, 2026-10-30).
    Checked against the parent: no language forbids it. The goldens (EXR-1, EXR-6) are
    scoped to those plans\' own work, and EXR-6 itself expects this epic to change how knitr
    is presented. Two obligations stay with us: the goldens stay green unchanged, and the
    `.cell` wrapper contract holds (one `Div.cell` per executed cell, the empty string when
    echo and every output are suppressed, exactly one wrapper per cell for the preview
    splice); the layer may add classes and attributes, not change that structure.
13. **Targets: HTML, Typst and docx first; LaTeX falls back (D4, 2026-10-30).** The shared
    presenter handles HTML, Typst and docx. Q1 gives Typst and docx no special knitr handling
    (no `typst` or `docx` branches in its `rmd/*.R`), so they take the generic markdown path.
    Chunks for LaTeX targets (`latex`, `beamer`, `pdf`) become fallback units with a
    diagnostic and render exactly as today, because LaTeX is where the real branching lives
    (figure placement, size conversion, animations, table dialect). Targets the corpus does
    not cover yet (epub, pptx, context and others) also fall back until it does; KNR-3\'s
    table holds the rule per case and target, and KNR-2 routes them. Recorded as a split
    after reading Q1 on 2026-10-30; the Typst and docx half is to be confirmed against q2\'s
    `hooks.R` and the corpus (KNR-1, KNR-2 2E). What LaTeX will need is collected in
    the LaTeX epic\'s P1 plan, block "Engine output for LaTeX targets"
    (`claude-notes/plans/2026-09-23-pandoc-hybrid-latex-P1-latex-writer.md` on branch
    `explore/pandoc-hybrid-latex`); append to it whenever a plan in this epic defers LaTeX.
14. **Old knitr traces keep replaying from markdown (D5, 2026-10-30; mechanism corrected
    2026-11-01 by interlude I2, confirmed by Gordon).** After the flip, a pre-flip knitr trace
    (finished markdown, no record) still replays: `ReplayEngine` returns its stored markdown
    verbatim, as it does today, and no presenter is involved. The pass-through presenter
    (KNR-2 Phase 2F) is needed only for fallback units inside a record. New traces store a
    record and replay presents it. This differs from the parent\'s decision 9 for Jupyter
    (pre-flip Jupyter traces fail with "re-record this trace"), because keeping the verbatim
    path breaks no existing trace or fixture. Replaying a knitr trace at all needs the `{r}`
    claim fix (bd-js24dold, about 20 lines), which lands on its own before KNR-3; KNR-6 adds
    the dispatch on a record and an unused-capture check. The number of existing knitr traces
    was not counted.
15. **Decision 21 is in the overrides list (2026-10-30).** The parent's knitr inline-id
    decision is overridden by KNR-4, because EXR-12's mechanism is replaced.
16. **Corpus layout (2026-10-30).** Fixtures live in
    `crates/quarto-core/tests/fixtures/knitr-corpus/`. The differential run is an
    integration test under `tests/integration/` (registered in its `main.rs`, per the repo
    rule) that skips when R is absent, plus an `xtask` entry that produces the report.
    *As built (KNR-1, 2026-11-01): the report is produced by the integration test
    `knitr_corpus_runner::corpus_report`, which does nothing unless `KNR_REPORT=<file>` is
    set (`KNR_SAMPLE`, `KNR_ONLY`, `KNR_R_LIBS` and `KNR_Q1` narrow or pin it); there is no
    xtask entry.*
17. **R-dependent tests are CI-ready, and CI stays disabled (2026-10-30).** Every test that
    needs R or a knitr version skips cleanly when the dependency is missing, is
    deterministic, and documents the R and knitr range it needs, so enabling R in CI later is
    a workflow change and nothing else. The pattern at
    `tests/integration/knitr_label_reinject.rs:37-64` checks only that `Rscript` runs and
    that knitr is installed; it has no version gate, so a skip on a knitr version is new code
    (KNR-1). Enabling R in CI is out of scope for this epic. Consequence: until someone does,
    the corpus gates nothing automatically; KNR-2b\'s zero-blocker bar is checked by a local
    run, and its report states the pass and skip counts.
18. **No braid strand for the epic (2026-10-30).** The epic and its plans are the tracking
    surface; plan-scope work goes in the plan checklists.
19. **`knit_asis` mapping is deferred to KNR-1 spike S2 (2026-10-30).** How a captured
    `knit_asis` (kable, `df_print`, htmlwidgets, `results='asis'`) maps to nbformat, and how
    a record marks "raw markdown, not stdout", is decided in S2 against the corpus, which
    includes fixtures for each case. S2\'s brief also requires that the record carry
    `knit_meta` (htmlwidget dependencies) beside the text, that the mime choice be tested
    per target format, and (2026-10-31) that it record what q2\'s `wrap_asis_output`
    (`patch.R:99-139`) does per `knit_asis` kind, since that wrapper, not `hooks.R`, builds the
    caption and the `{=html}` fencing. Reason: the right mapping depends on which cases occur
    and on what Positron does with `text/markdown` display data, which is unverified.
20. **Quarto 1 gaps become strands, not epic work (2026-10-30).** When the Quarto 1
    comparison (decision 9) finds something q2 lacks, the plan searches the skein for an
    existing strand and files a new one only if none is present, linked to this epic by a
    `discovered-from` dependency. Closing the gap is separate work; this epic does not
    fix it unless the presenter itself needs the behavior. This is the out-of-plan case in
    the repo's tracking rule, so strands are right here even though the epic itself has none
    (decision 18).
21. **Presentation facts the presenter cannot recover live in three internal cell-metadata
    keys and one per-engine field (2026-11-01, interlude I1, signed off by Gordon; the engine
    field changed 2026-11-02; the third key added 2026-11-03, signed off by Gordon after the
    KNR-3 corpus gate).** `quarto_output_anchors` (the source lines before each output, in the
    order the capture saw them), `quarto_output_kinds` (parallel to `outputs`: `"message"`,
    `"warning"` or null) and `quarto_echoed_code` are cell keys. `quarto_echoed_code` holds the
    code knitr echoed, as an array of lines in nbformat `source` style (each with its trailing
    newline except the last), the `#|` option lines excluded. It is written only when it
    differs from the authored code with its option lines removed (a numeric `eval` masks the
    unevaluated lines as `## `, `ref.label` supplies the referenced chunk\'s code, `tidy`
    rewrites it), so an ordinary chunk carries nothing. The presenter shows it in place of the
    authored code, and the anchors and `quarto_expression_starts` index into it. The cell
    `source` stays the **authored** text, so the hash and the id stay what the stage computes
    from the document (`recompute_cell_hash` rebuilds a unit\'s hash from `source`; a `source`
    holding the echoed code would make every numeric-`eval` chunk stale on each render). This
    supersedes KNR-1\'s encoding sentence that the cell `source` is the code `evaluate()`
    received. The option names knitr knows come from
    the knitr that ran and are stored **once per record**, as the optional `known_options` list
    on the `quarto_engines` entry beside `hash_definition`; the presenter subtracts them from a
    chunk\'s current options at present time, and what is left is the unknown options. An absent
    field means no unknown-option filtering. This replaces the per-cell `quarto_unknown_options`
    key that KNR-2 was written against (KNR-3 decision 6 says so; KNR-2 reads the new field).
    The three cell keys start with `quarto_`, so `CellMetadata::options()` drops them from user
    options, and `known_options` lives on the engine entry, so it never reaches that path. None
    enters the exec hash (the hash comes from the pre-engine units). KNR-3 writes them;
    KNR-2 reads them. The option-aware reordering that knitr does (`fig.show` and `results`
    hold and hide, numeric `echo`, `prompt`) is done by the presenter at present time over the
    capture-order record, never at record time, so a header-option edit shows without
    re-running (decisions 10 and 11). A numeric `eval` is an execution option, so its masking is
    recorded: editing it changes the hash and re-executes.
22. **`text/markdown` display data is opt-in through a marker (2026-11-01, I1, signed off by
    Gordon).** The adapter sets `metadata.quarto_markdown_passthrough: true` on the output
    metadata of knitr `asis` output; the shared presenter handles `text/markdown` only then.
    The Jupyter presenter keeps dropping it, as its goldens (`unhandled-markdown-mime`,
    `markdown-beats-nothing-plain-wins`) require. Not a presenter parameter. **Amended
    2026-11-01 (Gordon):** the key is engine-neutral (any engine may set it for output that is
    document markdown to splice as-is), and flat like the other `quarto_` keys, replacing the
    nested `quarto_knitr.asis`. The separate `quarto_knitr.raw` marker is dropped: whether the
    output gets a `.cell-output-display` div, or goes out bare with no `.cell` wrapper, follows
    from the current `results: asis` cell option (and knitr\'s bare rule: no label, classes,
    attributes or caption), read by the presenter at present time (decision 21). This also
    settles the "`results='asis'` has no `.cell` wrapper" item under "New from the spikes":
    decision 12\'s one-wrapper rule has that one option-driven exception. How the preview
    splice locates a bare cell is still to be written down in KNR-2.
23. **The first allowlist of intentional divergences (2026-11-01, I1, signed off by Gordon).**
    `structure/chunk_in_blockquote` and `s7/01_structure` (the baseline keeps blockquote
    continuation lines quoted), `markdown-stress/empty_output` and `s1/03_eval_false` (an
    empty `.cell` wrapper is still a box in the page). A document that overrides the `source`
    or `output` knitr hook (`setup-state/knit_hooks_override`) becomes a fallback unit rule,
    not an allowlist entry. KNR-2 adds the allowlist reader with these entries.
    **Amended 2026-11-03 (Gordon):** `s8/01_setup_state` is signed off too, for two differences
    from knitr\'s markdown, both judged improvements and very minor. (1) The chunk `c_params`
    (`eval` false, `echo` false) renders nothing, and knitr writes an empty `.cell` wrapper for
    it; the presenter writes nothing, the same divergence as `s1/03_eval_false`. (2) knitr
    writes `.cell-code  code-fold` with two spaces and the presenter with one; HTML treats them
    alike.
24. **The flip keeps knitr\'s finished markdown as a retained fallback, and removing it is a
    deferred, trigger-gated plan (2026-11-02, Gordon; review 3 decision 1, option b).** After
    the flip, R still ships the finished markdown of every chunk next to the record. Rust
    presents from the record when it can; when it cannot (an output it does not map, an
    unreadable or out-of-directory figure, a missing record), it substitutes that chunk\'s
    shipped markdown instead of leaving the marker empty. The corpus will not run clean in
    this epic and does not cover every use of knitr, so the epic does not claim the fallback
    is unreachable. Dropping the shipped markdown for structured chunks is
    [KNR-8](2026-10-30-knr-08-fallback-removal.md), modeled on
    [EXR-17](2026-10-28-exr-17-legacy-removal.md): no date, no count, an owner-decided
    trigger. KNR-6 sets up the instrument KNR-8\'s trigger reads (a counter of Rust-side
    fallbacks).
25. **Figure files keep the producer\'s own path, through an engine-neutral key (2026-11-02,
    Gordon; review 3 B5).** An output may carry `metadata.quarto_file_path`: the
    document-relative, forward-slash path where its producer wrote (or wants) the output\'s
    file. It replaces `metadata.quarto_knitr.file` in the capture, the adapter and the
    reader; nothing has shipped, so this is a change on the integration branch, not a
    migration. The shared figure sink writes the output\'s bytes at that path when it is inside
    the document directory and does not collide with another output\'s file; otherwise it uses
    its generated `cell-<n>-output-<m>.<ext>` name and counts the fallback. Outputs without the
    key (Jupyter, imported Positron output) keep today\'s names. knitr therefore keeps its file
    names, `fig.path` and `figure-<format>` directory, and the corpus normalizer\'s
    `figure-<fmt>/<name>` rewrite is no longer hiding a divergence. `quarto_knitr.knit_meta`
    stays engine-private; the neutral dependency manifest is WGT-2\'s. The code rename
    (`pieces_map.rs:132` and its tests, the capture) is a KNR-3 Phase 3B item, tests first.
26. **The flip gate accepts Identical and SyntaxLevel only (2026-11-02, Gordon; review 3
    decision 3, option a).** A Semantic difference (different AST, same HTML) needs a signed-off
    allowlist entry (decision 23), and so does every Blocker. Non-HTML fixture formats (typst,
    docx, gfm) are compared at the AST level too, because the classifier\'s rendered-HTML level
    cannot see a difference that shows only there. The comparison normalizer is limited to
    htmlwidget ids; figures are checked by position and extension, not rewritten to a
    placeholder. The same gate runs in KNR-3 Phase 3C and, promoted, in KNR-6 Phase 6A (one
    harness, not two), and a frozen oracle per fixture is committed before any deletion.
    **Amended 2026-11-03 (Gordon):** KNR-3 Phase 3C runs the gate in report mode, and the
    failing assertion moves to KNR-2b Phase 2bC, because `s9/01_inline` stays a blocker until
    KNR-4. KNR-6 Phase 6A still promotes the one harness.
27. **Widget dependencies from a knitr record are rebuilt in Rust (2026-11-02, Gordon).** The
    knitr adapter converts the recorded `knit_meta` into the neutral per-output dependency
    manifest of WGT-2 when it writes the record, and the Rust presenter reads the manifest at
    present time, so replay and hub guests need no R. The serialized R object stays as an opaque
    engine-private field beside the manifest (`quarto_knitr.knit_meta`). This applies to
    brace-span outputs as to chunk outputs. WGT-2 owns the manifest\'s shape; KNR-3 Phase 3B
    writes it.
    **Amended 2026-11-03 (Gordon):** KNR-3 does not write the manifest, because WGT-2 has not
    written its schema. The conversion belongs to the widget epic\'s adapters item (WGT-2 lists a
    knitr `knit_meta` adapter, and records the dependency on this epic\'s flip). Until it exists, the record keeps the opaque `quarto_knitr.knit_meta` string, and
    KNR-6\'s flip holds back any knitr document with captured widget output (the live path keeps
    serving it), so no widget loses its scripts and styles. The conversion is a prerequisite of
    flipping those documents, not of the flip as a whole.
28. **A block-level span output inside a paragraph keeps knitr\'s behavior for now (2026-11-02,
    Gordon).** An inline htmlwidget div is inserted into the paragraph as knitr does. KNR-4 adds
    an observation fixture (an inline plotly div); if it causes trouble in practice, promoting
    such a span to a block is a later change, not part of this epic.

## Findings that shaped this epic

Read on 2026-10-30 in `external-sources/knitr`, a **development snapshot** (`DESCRIPTION`
Version 1.52.5, `NEWS.md` headed 1.53), so its line numbers match no release: `evaluate(` is
at `block.R:261` in v1.44, `:262` in v1.50 and `:313` in the snapshot. Installed locally:
knitr 1.50, rmarkdown 2.29; the experiments in the research note ran on 1.50. q2\'s R code has
version branches at `patch.R:6`, `:152` and `:300` (`hooks.R` has none; its 1.44 mentions at
`:332`, `:793` and `:842` are comments); `rmd.R:8` stops below knitr 1.44 but only for `.R`
(spin) input, and `execute.R:774` picks `has_crop_tools`. No hard pin was found.

- **The document is the join of per-group results.** `knit()` collects one result per
  group (text group or chunk) and joins them with `"\n"` (`block.R:2-8`; the group loop is
  `output.R:346-370`, the join `one_string` at `utils.R:1130`, applied at `output.R:295`).
- **The chunk hook is not the placed value.** For `include=FALSE` the chunk hook still
  runs and returns the cell markdown (`block.R:396`), and the chunk then yields `''` (or,
  for a chunk nested in a list, just its indent string) instead (`block.R:425`; `:214` for
  non-R engines). A chunk with `cache=TRUE` (level 3) returns the cached string before
  the hook runs (`block.R:100-101`). So recording at the chunk hook cannot reproduce the
  output; recording at the group level can.
- **Pieces exist before formatting.** `eng_r` calls `evaluate()` and then `sew(res,
  options)` (`block.R:313-392`). `sew` is an S3 generic with methods for `character`,
  `message`, `error`, `recordedplot`, `knit_asis`, `list` and others (`NAMESPACE:11-22`),
  and `engine_output(options, out = LIST)` routes engines through `sew` too
  (`engine.R:94`); q2 replaces `sew` itself in `patch.R`. Most other engines call
  `engine_output(options, code, out)` with a string (19 call lines) and have no pieces; a
  source plus stdout can be synthesized for them.
- **Some output is random between runs.** In stock knitr `htmltools::extractPreserveChunks`
  gave the same content different ids within one document; q2\'s `patch.R` sets
  `htmltools.preserve.raw`, so those markers may not appear in q2\'s markdown at all (not
  established). `htmlwidget-<hex>` ids and `plotly` `cur_data` ids differ between runs
  (reported by review, not re-run). Comparison needs a normalizer built from a control run (KNR-1).
- **Presentation is large and R-side, in more than `hooks.R`.** `resources/rmd/hooks.R` is
  1211 lines, built as two layers: knitr\'s own `hooks_markdown()` first, then Quarto\'s
  decoration (cell div, classes, forwarded layout and unknown options, captions, code
  options, format-aware plot hook). `patch.R` adds a third layer: it replaces knitr\'s `sew`
  and `add_html_caption` and defines `wrap_asis_output` (the `knit_asis` wrapper, caption and
  `{=html}` fencing). All of q2\'s R files are byte-identical to Quarto 1\'s. The Rust
  presenter has to reproduce what matters of all three. This is the main unknown in the
  epic\'s size. The Jupyter presenter already matches knitr\'s *output* divs but has none of
  the cell-option layer (layout, panel, column, captions, labels, code-fold); that layer is
  not knitr-specific (KNR-2), and Quarto 1\'s TypeScript `jupyterToMarkdown` already builds
  it.
- **Filters sit between `evaluate()` and `sew`.** knitr filters pieces by `echo`,
  `results`, `warning`, `message`, `fig.keep` and `fig.show` before formatting
  (`block.R:352-371`), and `sew.recordedplot` is where plot files are written
  (`output.R:657-677`). Where the capture sits therefore decides whether the record
  depends on presentation options, and so what the unit hash may leave out (settled as
  decision 10). Three details found in review: `fig.keep='none'` opens no recording device
  (`block.R:259`) and the default `high` merges low-level plot additions, so plot selection
  cannot wait for the presenter; `stop_on_error` is derived from `error && include`
  (`:317-319`), so `include` changes what runs; and `evaluate()` is the only place logical
  `warning` and `message` are enforced (reported by review, to confirm in S1).
- **There is no hook exactly "after `evaluate()`" except one.** The `evaluate` hook
  (`knit_hooks$get('evaluate')`, `block.R:303`) is the unpatched seam, and it is bypassed for
  `eval=FALSE`, blank code and cache hits. The alternatives are a replacement copy of
  `eng_r` (about 190 lines, a size from review that is not measured; version-fragile) or wrapping `sew`, which runs after the filters
  (decision 3).
- **Cache levels differ.** `cache=TRUE` is level 3: a hit returns the cached output string
  and the chunk hook never runs (`block.R:100-101`). Levels 1 and 2 cache the piece list
  (`block.R:337-340`), but a hit reads it inside the expression that otherwise calls
  `evaluate()` (`:308-311`), so a capture at the `evaluate` hook never sees it; level 2 stores
  `remove_plot(...)` (`:339`), so a level-2 hit has no renderable plots.
- **knitr options include R-side state.** A setup chunk\'s `opts_chunk$set`,
  `opts_template`, `opts.label`, `ref.label` and `opts_hooks` shape presentation and are not
  in the document text (`block.R:9-92`), which breaks the parent epic\'s rule that
  presentation reads options from the document (settled as decision 11). `fig.cap`,
  `fig.scap` and `fig.alt` are resolved only after the chunk runs (`eval.after`,
  `block.R:343-345`), so final options exist at the chunk hook, not at the `evaluate` hook.
- **No R in CI.** A grep of `.github/workflows/*.yml` for `setup-r`, `r-lib`, `Rscript`
  and `install.packages` found nothing, so R tests skip there (pattern at
  `tests/integration/knitr_label_reinject.rs:37-64`).
- **Not verified:** `eng_python` and reticulate\'s engine; which user `knit_hooks`
  changes the Rust presenter would silently ignore; behavior on knitr 1.44 to 1.49 (history
  read, nothing run); forced `warning=NA` and `message=NA` under `options(warn=2)`; whether
  Positron presents `text/markdown` display data or `include=FALSE` outputs; whether Ark
  sends htmlwidget dependencies.

## Plans and DAG

**Status (2026-11-03).** KNR-1 and the interlude are complete. KNR-2 is closed (its remainder is
KNR-2b). KNR-3 is next, after the replay claim fix (bd-js24dold); then KNR-4, KNR-2b and KNR-6,
with KNR-5 in parallel after KNR-3. KNR-7 follows KNR-6; KNR-8 waits on its trigger. The parent
epic is complete but not yet merged to main, so this work stays on top of it until it is.

| ID | Plan | Sessions (guesses) | Notes |
|---|---|---|---|
| KNR-1 | [Capture spike and differential harness](2026-10-30-knr-01-capture-spike-and-harness.md) | 4-6 | detailed; spikes S1-S9; the gate for everything below |
| KNR-2 | [knitr presenter layer](2026-10-30-knr-02-knitr-presenter-layer.md) | 4-7 (used) | also builds the pass-through presenter. KNR-1\'s report ([corpus report](../research/2026-11-01-knr-01-corpus-report.md)) sets the scope: of 202 compared cases, 154 are blockers (78 from source/output interleaving) and none is identical; interleaving anchors and `text/markdown` display data are the two largest pieces. Interlude I1 (2026-11-01) narrowed the estimate: five prototype fixes close 97 of 154 blockers, 57 remain (48 small gaps, 2 whitespace, 4 divergence candidates, 3 other); see [the findings](../research/2026-11-01-knr-i1-presenter-gap-sizing.md) Closed 2026-11-02 for the scope that does not wait on another plan: the presenter, the fallback-unit presenter, the target router (revealjs and epub are structured) and the corpus gate with its five-entry allowlist. The corpus measures 198 accepted, 5 allowlisted, 7 blockers; all 7 wait on KNR-3 or KNR-4. The rest moved to KNR-2b. |
| KNR-3 | [Record adapter, hash profiles, fallback units, cache and `child=`](2026-10-30-knr-03-record-adapter-hash-fallback.md) | 3-4 | KNR-1 S1 to S5, S8 and S9 answered its option, cache, `child=` and hook decisions; decision 8 (the format in the record) settled 2026-11-01: the exception is accepted |
| KNR-4 | [Inline expressions as recorded values](2026-10-30-knr-04-inline-typed-values.md) | 2 | replaces EXR-12\'s mechanism; mechanism answered by S1 and S9 |
| KNR-2b | [knitr presenter layer, remainder](2026-11-02-knr-02b-presenter-remainder.md) | 1-2 | split from KNR-2 on 2026-11-02; sequenced after KNR-4. The options and fences that need KNR-3\'s record fields, the hold wiring, the shared figure sink, and the corpus bar of zero blockers on the minimum and latest knitr |
| KNR-5 | [Positron interop for knitr](2026-10-30-knr-05-positron-interop.md) | 2-3 | widens EXR-16\'s importer gate; spikes in the Positron fork |
| KNR-6 | [The knitr flip and the consumers](2026-10-30-knr-06-flip-and-consumers.md) | 3-4 | skeleton design settled: markers (S7 verdict) |
| KNR-7 | [Docs and end-to-end](2026-10-30-knr-07-docs-e2e.md) | 1-2 | written as the others land |
| KNR-8 | [Fallback removal (trigger-gated)](2026-10-30-knr-08-fallback-removal.md) | 1-2, plus whatever time passes before the trigger holds | deferred by design (decision 24); not part of this epic\'s exit; mirrors EXR-17 |

**Interlude (2026-11-01).** Between KNR-1 and KNR-2, four spikes answer the open questions of
the later plans against complete parent code, our own harness and Positron:
[the interlude plan](2026-11-01-knr-interlude-spikes.md) (I1 sizing the presenter gaps, I2 knitr
replay and trace, I3 structured records through preview with real sizes and the fallback rate,
I4 Positron and knitr records). It builds nothing for the product and writes findings into the
plans it informs.

- **I1 (2026-11-01):** the presenter gaps are sized. Five fixes close 97 of the 154 blockers and open none; KNR-2 is 4 to 7 sessions. Two gaps need a record or capture change first (message versus warning, knitr\'s known option names); `text/markdown` needs a marker or a presenter parameter, since the Jupyter goldens drop it on purpose. [Findings](../research/2026-11-01-knr-i1-presenter-gap-sizing.md).
- **I2 (2026-11-01):** the knitr replay claim fix is about 20 lines and lands on its own before KNR-3 (bd-js24dold); an old knitr trace needs no presenter (the stored markdown is returned verbatim), a refinement of KNR-6 decision 2. Replay also needs an unused-capture check. [Findings](../research/2026-11-01-knr-i2-replay-and-trace.md).
- **I3 (2026-11-01):** structured and mixed records splice through the real preview pipeline, but KNR-3 decision 8\'s additive edit does not suffice (the unit id changes with the text, and the content-hash pass defeats a format change); a profile-aware variant passes. Median record 2.4 KB, largest 163 KB with figures inline (inline recommended for bd-sr73); fallback 8 of 278 cells (2.9%) on a one-construct-per-document corpus. [Findings](../research/2026-11-01-knr-i3-records-through-preview.md).
- **I4 (2026-11-01):** Positron loads and presents a q2 knitr record and consumes it on the first matching hash, rewriting it without the q2 keys; the fallback MIME is harmless (raw text). An Ark cache imports into knitr units through a one-condition gate widening and presents through the shared presenter (errors need ANSI and empty-`ename` handling). KNR-5 should wait for EXR-16 and shrinks to about one session. [Findings](../research/2026-11-01-knr-i4-positron-and-knitr-records.md).

Total roughly 19 to 28 sessions, which is the sum of the per-plan guesses (it was 20 to 31 before I1 narrowed KNR-2), dominated
by KNR-2. KNR-1 is written to be executed; KNR-2 to KNR-7 were written on 2026-10-30, before
KNR-1 reports, from a reading of the code and revised on 2026-10-31 after the paper review,
so each marks what is *blocked on* a KNR-1 spike and is expected to be revised once it
reports. The [research note](../research/2026-10-30-knitr-capture-findings.md) holds the
experiments and line references behind the findings, and the
[paper review](../research/2026-10-30-knitr-epic-paper-review.md) records what was checked
and what was found wrong.

| Plan | Hard dependencies | Soft |
|---|---|---|
| KNR-1 | EXR-1 Phase 3 (the extracted presenter), EXR-2 Phase 2 (the shared unit scanner); both landed | none (EXR-10, EXR-11 and EXR-12 were not needed) |
| KNR-2 | KNR-1 (which brings EXR-1 and EXR-2) | |
| KNR-3 | KNR-1, KNR-2 (the pass-through presenter), EXR-2, EXR-4, EXR-11, EXR-7 (EXR-11 depends on it, and it adds the record field) | |
| KNR-4 | KNR-3, EXR-12, EXR-2, EXR-4 | EXR-3 (Jupyter inline conventions) |
| KNR-2b | KNR-2, KNR-3 (the record fields), KNR-4 (inline capture) | |
| KNR-5 | KNR-3, KNR-2, EXR-16 (its importer and validator are the base) | EXR-14 (to show imported output in preview) |
| KNR-6 | KNR-2b (which brings KNR-2), KNR-3, KNR-4, EXR-6, EXR-14, EXR-15 | KNR-5 |
| KNR-7 | KNR-6 | every other plan |
| KNR-8 | KNR-6 (and the trigger in its own plan) | blocks nothing |

KNR-1 ran entirely on the then-current (legacy markdown) path and the extracted presenter
(decision 1). The parent is now complete, so every hard dependency on a parent plan is met.
KNR-5 blocks nothing hard: KNR-6 and KNR-7 take it as soft.

## Risks

1. **Presenter scope.** If the corpus shows many blockers, KNR-2 is large and may
   split. Mitigation: the harness is the first plan and reports before anything else
   is committed to.
2. **Dependence on knitr internals.** Capturing at the `evaluate` hook or a replacement
   `eng_r` relies on non-exported behavior, and q2 supports a range of knitr versions (S1
   measures the minimum; 1.44 is the working assumption). Line numbers and some behavior
   differ between 1.44, 1.50 and the latest. Mitigation: capture tests run against the
   minimum and the latest version, and an unrecognized shape degrades to fallback units,
   never to wrong output.
3. **Silent loss of user hooks.** A user's `knit_hooks$set(...)` changes markdown today;
   the Rust presenter would ignore it. Mitigation: detect overridden hooks and fall
   back for the affected chunks, with a diagnostic (decided in KNR-3).
4. **Two mechanisms on one record** (parent risk 6). Mitigation: decision 8, one
   mechanism per unit, tested.
5. **Corpus not in CI.** No R in CI means the differential run is local or scheduled
   until R is enabled in CI (decision 17).
6. **Nondeterminism in the oracle.** Random ids, temp paths and figure directories differ
   between runs; the normalizer list is built from a control run in KNR-1, and a rule
   missing from it shows as a blocker, not as noise.
7. **A format-neutral record against format-dependent pieces** (parent decision 27). Settled
   2026-11-01 by Gordon: a knitr exception (KNR-3, decision 8). EXR-4 is complete without a
   hash input for the format; the flat option key carries it (KNR-3, decision 8).

## Out of scope

- Single-cell execution for knitr; freeze.
- Upstream knitr changes (a `sew` hook, structured results).
- Presenting knitr output by Positron's own code (this epic makes the record valid to
  present; Positron-side work stays in the parent epic's EXR-16 spikes).
- Replacing knitr as the engine for `{r}` (running R through a Jupyter kernel, which the
  owned-languages epic permits, is a different feature).
- Matching Quarto 1's knitr output beyond what q2's current hooks already do, except that
  gaps the Quarto 1 comparison finds are filed as strands (decision 20).

## Open questions

### Design decisions

All five design questions (D1 to D5) are settled: decisions 10 to 14 above.

### Other decisions needed from Gordon

Open: none that block KNR-3 except the one below.

Also waiting on the parent session (not on Gordon): **which hash list produces the flat cell
keys of a structured unit** (KNR-3, decision 2). EXR-4 computes them from the engine\'s default
exclusion list while the unit hash uses the unit\'s profile; KNR-3 applies the unit\'s own
profile and asks the parent to confirm.

Settled by Gordon on 2026-11-01:
1. **The format in the record (KNR-3, decision 8): the exception is accepted.** The output
   format becomes an input to the structured profile\'s hash and tags the unit. The parent
   epic is not edited. EXR-4 is complete (2026-11-01) and
   supplies the tag and the per-unit profile but no format input to the hash. Gordon chose
   (2026-11-01) that structured units store the format as a flat option key the structured
   profile counts; no EXR-4 change. Spikes on 2026-11-01 showed the parent\'s splice ignores the profile and the format, so Gordon authorized one additive edit to the parent\'s `record/matching.rs` and `splice_units` (KNR-3, decision 8); the parent plans are not edited.

Settled by Gordon on 2026-10-31, each as recommended:
2. **The document hash of a mixed record (KNR-3, decision 9): accepted.** Because the
   structured profile excludes no `execute:` key (item 3), a fallback unit changes nothing at
   document level; its cost is per unit, and the corpus reports how often a document has one.
3. **Layering of the document scope (KNR-3, decision 3): not layered for now.** Only the
   chunk\'s header options win over the recorded options; an `execute:` edit re-executes.
   Revisited after S8.
4. **A pattern form for `HashProfile` exclusions: not requested.** The limit is documented
   in KNR-3 (forwarded options such as `class.*`, `attr.*` and `layout*` count in the hash);
   asked of the parent only if the corpus shows it is hit.
5. **When KNR-1 may start: once EXR-1 and EXR-2 are available** (decision 1).

### Markers with an owner and an unblocker

- **bd-sr73 (figures: inline base64 or knitr file paths): settled.** Gordon signed off
  inline base64 on 2026-11-01, after the I3 measurement (inline figure records are 1.31 times
  the size of the files). KNR-1 S2 recorded file paths and stated what changes under base64.
- **Whether the scanner can tell a `child=` chunk (KNR-3 decision 6):** KNR-1 S5 reads EXR-2.
- **The spelling key for inline units:** dropped (S9 settled that the record holds knitr\'s inserted text, so no unit needs a spelling).
- **`execution_count` (KNR-5 decision 4):** KNR-5, from KNR-1\'s S2 table and K-S1.
- **Quarto 1 fixture gathering (KNR-1 Phase 1D):** the executor, by the default written
  there.
- **The count of existing knitr traces (decision 14):** KNR-6 counts them before the flip.
- **Whether a consumer hard-fails on a stale record (KNR-6 decision 4):** KNR-6 Phase 6B tests
  each consumer.
- **The acceptable fallback rate (KNR-3 exit) and the KNR-6 corpus bar and sign-off list:**
  Gordon, from the first report.

Unverified facts that gate work (updated 2026-10-31 after KNR-1 spikes S1 to S5, S8 and S9;
each answer is in the named `#### Sn decision` subsection of KNR-1):

- **Answered:** the capture mechanism and its behavior on knitr 1.44, 1.50 and 1.52 (S1: the
  `evaluate` hook plus capture-time plot selection, minimum 1.44); the cache rule (S4: off
  while recording); the mapping, wire shape and `knit_meta` carriage (S2); option capture,
  spelling and the closed presentation list (S3, S8); odd chunks and hook-override detection
  (S5); inline capture and formatting (S9).
- **Answered 2026-11-03 (S5, reticulate Python):** the evaluate hook is not called for a Python
  chunk and the chunk hook holds one finished string with source and output interleaved, so the
  verdict is fallback unit (the existing non-R rule). Recovering the pieces later is possible
  through reticulate\'s `reticulate.engine.wrap` option; not run.
  *Settled since:* figures are inline base64 (bd-sr73, 2026-11-01), and the format in the record
  is an input to the structured profile\'s hash (KNR-3 decision 8, 2026-11-01).
- **New from the spikes, to settle in the plans named:** `results='asis'` chunks have no
  `.cell` wrapper in knitr\'s markdown, against the wrapper contract (S2, KNR-2); a
  `ref.label` chunk with an empty body can go stale silently (S8, KNR-3); forcing
  `keep_warning` or `keep_message` changes the live markdown, so it is ruled out until
  KNR-6 (S1); the three "newer than 1.50" behaviors in KNR-1 Background are newer than 1.52,
  and `eng_r` is 165 to 180 lines in releases, not 190 (S1).

## Conventions for the plans in this epic

As in the parent epic: TDD with tests first; each plan names its exit criteria;
plan-scope work goes in the plan checklist, not in strands. Facts marked *verified* were
checked by reading or running on 2026-10-30; *reported by review, not re-run* means a
reviewer reported it and nobody repeated it (the paper review records which); anything else
must be re-verified before relying on it.

## For reviewers

This epic was drafted in one session on 2026-10-30 by an author who will not be available
afterwards; the plans are meant to stand on their own. A short guide to reviewing them.

**What is solid.** Claims marked *verified* cite a file and line, or an experiment that is
in the [research note](../research/2026-10-30-knitr-capture-findings.md) with a script to
repeat it. They concern knitr's own behavior (how the document is assembled, where filters
and cache short-circuits sit, preserve ids) and the structure of `hooks.R` and the Jupyter
presenter. Re-check a few line numbers against the released knitr you test before relying
on the rest: the cited lines are from a development snapshot and shift between releases.

**What is a prediction.** Session estimates (all of them are guesses made before any spike
ran); that the shared presenter plus a knitr layer reaches zero blockers; that markers can
carry the skeleton (S7); that capture before the filters is workable across the knitr
versions S1 tests, and that the `evaluate` hook is a sufficient mechanism (S1); that
fallback units stay rare enough for the structured profile to pay off (KNR-3 decision 9);
that the Jupyter presenter matches knitr\'s output divs beyond the block shape the parity
test checks; anything about Positron or Ark beyond what the parent EXR-16 verified by
reading.

**Questions worth a reviewer's time.**
1. Is decision 10 (formerly D1) right? Capture before the filters buys a Positron-like record and a cleaner hash
   but makes the presenter much larger, and the review showed that the capture seam, plot
   selection and the forced `evaluate()` arguments each need care (decision 10, KNR-1 S1).
   Would capture after the filters, with a bigger hash exclusion list and honest "this record
   depends on presentation options" semantics, be the better trade? Challenge this hardest,
   because it fixes KNR-2\'s size and KNR-3\'s hash.
2. Is a separate epic right, or is this a long plan after all? The argument for an epic is
   that most pieces are additive and KNR-1 decides the rest.
3. Does the per-chunk fallback undercut the "no side channel" decision? It is the one place
   knitr markdown stays in the record; is that acceptable, and is the list of fallback
   cases (`child=`, level-3 cache hits, overridden hooks) complete?
4. Staging *(answered 2026-10-30: Option A, and the parent did land first; the cost below was
   paid)*: the epic assumes the parent lands first (Option A). EXR-11 and EXR-12 build a
   chunk-hook recorder and an inline-id mechanism that this epic later deletes, roughly five
   sessions of work that will be thrown away. Is that the right cost to pay for an
   independently landing parent?
5. KNR-1's differential harness compares against q2's current knitr output, which is itself
   vendored from Quarto 1, and (decision 9) is informed by Quarto 1 where q2 lacks a
   feature. Is that the right oracle, and is "record q2 gaps as findings" the right
   handling?
6. The settled decisions (1-20) were checked once against the parent epic\'s decisions; the
   paper review found overrides the first list missed (now named above). Are there more?

**What the author would do first.** Run KNR-1's spikes S1, S7 and S8 in that order. They
are the cheapest experiments (an R session each) that can overturn the shape of the epic.

