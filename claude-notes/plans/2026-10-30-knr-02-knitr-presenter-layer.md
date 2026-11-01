---
title: 'KNR-2: the knitr presenter layer'
date: 2026-10-30
description: 'Closes the gaps KNR-1 measures between the shared Rust presenter and knitr own markdown, so structured knitr outputs present as the Quarto cell markdown that hooks.R builds today. Written before KNR-1 reports, from a reading of hooks.R and the Jupyter presenter; scope is provisional.'
status: draft  # outline written from code reading; scope to be re-set by the KNR-1 report
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-2).
**Depends on:** [KNR-1](2026-10-30-knr-01-capture-spike-and-harness.md) (the corpus, the
baseline gap, and the capture-point decision). EXR-1 must have landed (KNR-1 already
needs it); the splice support in Phase 2F is something this plan provides to
[EXR-14](2026-10-28-exr-14-preview-splice.md), not a dependency on it.
**Blocks:** KNR-6 (the flip).
**Estimated sessions:** 5 to 10. This is the epic's largest unknown; the range comes from
the size of `hooks.R`, not from a measurement.

> **Status of this document.** Written on 2026-10-30, before KNR-1 has run, by the author
> of the epic, so that reviewers and later implementers have what was learned, and revised
> on 2026-10-31 after the paper review
> ([research note](../research/2026-10-30-knitr-epic-paper-review.md)). The hooks were read
> in q2\'s `hooks.R`; the knitr line numbers it cites come from a development snapshot, not
> a release (see KNR-1 Background). Everything under "Provisional" is a prediction. KNR-1\'s
> report overrides it.

## Goal

Given structured knitr outputs (KNR-1's mapping), the resolved chunk options and the
target format, the shared Rust presenter produces the Quarto cell markdown that
`resources/rmd/hooks.R` produces today, up to the differences the epic accepts (syntax
level, semantic but looks the same, recorded intentional divergence). Zero blockers on the
corpus is the exit bar.

## Background

### What `hooks.R` does (read 2026-10-30; the inventory this plan must cover)

q2 builds its hooks on top of knitr's own: `default_hooks <- knitr::hooks_markdown()`, and
each Quarto hook first calls the default and then decorates the result (`hooks.R:178-184`).
So the presenter must reproduce **two layers**: knitr's markdown hooks and Quarto's
decoration.

**The chunk hook (`hooks.R:205-488`)** wraps everything in a cell div. It does, in order:

1. Returns the chunk unchanged for handled-language fences, `ojs`, and the `verbatim`,
   `embed` and `comment` engines (`:206-228`).
2. Computes the label (`output_label`), the `fig.cap` and `fig.subcap`, and a cell id when
   `notebook-preserve-cells` is set. Figure labels are deduplicated: a placeholder in the
   body is replaced by `<label>` or `<label>-<i>` when there are several figures (`:229-259`).
3. Synthesizes `layout` from `fig.sep` and `out.width`, or `layout-ncol` from `fig.ncol`;
   aliases `fig.align` and `fig.valign` to `layout-align` and `layout-valign` (`:261-307`).
4. Forwards `layout`, `layout-nrow`, `layout-ncol`, `layout-align` and `layout-valign` as
   attributes (`layout` JSON-encoded), then forwards **every unknown option** as
   `name='value'` (JSON-encoded if not a single string, single quotes escaped). "Unknown"
   is computed against knitr's own option tables plus lists in the hook (`:308-402`).
   This is why a new internal option must be added to `other_opts` or it leaks into HTML.
5. Builds the class list: `cell`, user `classes`, `panel-*`, `column-*`, `fig-column-*`,
   `tbl-column-*`, `caption-*`, `fig-cap-location-*`, `tbl-cap-location-*`, and `hidden`
   under `keep-hidden` (`:403-465`).
6. Decides whether a cell div is needed at all: `results: asis` with no label, no extra
   classes, no attributes and no caption is returned bare (`:466-471`).
7. Emits `::: {<id> <classes> <attrs>}` with the caption inside, indented by `options$indent`
   for list-nested chunks (`:472-487`).

**The source hook (`:490-589`)**: knitr's `hilight_source` (this is where `prompt` is
handled), then `cell-code` and `class.source`, `attr.source`, `lst-label` and `lst-cap`
(when crossref is on), `code-overflow`, `code-fold`, `code-summary`, `code-line-numbers`,
`source.hidden`; `fenced.echo` emits the original `{r ...}` header and the YAML code;
`produce-source-notebook` re-emits YAML code. The fence length comes from `create_fence`.

**Output hooks (`:591-595`)**: `output`, `warning`, `message` and `error` become
`.cell-output .cell-output-stdout|stderr|error` divs unless `results: asis` or `collapse`;
`.hidden` under `keep-hidden`.

**The plot hook (`:603-770`)** is format-aware: `fig-align`, `fig-env`, `fig-pos` (an
automatic `H` for LaTeX with echo and no layout), `fig-alt`, `fig-scap`, `resize.width`,
`resize.height`, `width`, `height`, `out.extra`, `fig.link`; animations (`fig.show:
animate`) via gifski or the LaTeX `animate` package (and a `knit_meta_add` dependency);
`results: asis` returns the bare image. Output is a `.cell-output-display` div.

**What else the presenter must reproduce (read 2026-10-31).** The R files under
`resources/rmd/` (`hooks.R`, `execute.R`, `patch.R`, `rmd.R`, `ojs.R`) are byte-identical to
Quarto 1\'s, so the R-side oracle is this code; a difference from Quarto 1 can only come from
the inputs the Rust side passes in (`format.execute`, `format.render`, handled languages),
the inline preprocess and the TypeScript post-processing (`external-sources/quarto-cli/src/
execute/rmd.ts`).
- **`patch.R`** (`patch.R:99-213`): `wrap_asis_output` builds the `cell-output-display`
  wrapper for `knit_asis` output (caption after a widget, `{=html}` fencing of table output,
  the `results='asis'` bare rule, the hidden class), and q2 replaces knitr\'s `sew` with a
  manual dispatcher that special-cases `knit_asis`, `knit_image_paths` and `rgl`. This is a
  presentation layer outside `hooks.R`; KNR-1 S2 records what it does per `knit_asis` kind.
- **knitr\'s own message formatting in `sew`** (`output.R:614-654`): the `Warning in <call>:`
  prefix, wrapping at the R `width` option (state the record does not hold unless S2 carries
  the formatted text), the `knitr.sanitize.*` options, `comment_out` and `strip.white`.
- **State shared between hooks:** `lastYamlCode` links the options hook to the source hook
  (`hooks.R:40-47`), which needs `yaml.code` and `original.params.src` in the resolved
  options.
- **Option rewriting hooks** (`hooks.R:50-114`): `execute: enabled: false` forces
  `eval=FALSE`, `echo: fenced` forces `fig.show` and `results` to `hold`, and `output: false`
  rewrites `results`, `fig.show`, `message` and `warning`; a line annotation in the code
  forces `warning=FALSE` and `results='hold'` (`hooks.R:812-823`). These run before the
  chunk, so the resolved options already include their effect.
- **Global setup** (`execute.R:290-361`): `comment = NA`, `message = isTRUE(warning)`,
  `fig.retina` for `dev=retina`, pdf `dev.args` and the `crop` hook, `ft.shadow`,
  `screenshot.force`, the `format$metadata$knitr` merge; also the dashboard `DT.options`
  (`execute.R:101-111`), the htmlwidgets `resolveSizing` patch (`:733-761`) and
  `df_print: paged` mapped to kable (`:142-149`).
- **An existing Rust-adjacent reference for the cell-option layer:**
  `ts-packages/quarto-api/src/jupyter/to-markdown.ts` (Quarto 1\'s `jupyterToMarkdown`:
  `panel`, `layout`, captions, `code-fold`; EXR-1 mentions it). It is a better oracle for
  decision 12 than `hooks.R`, because it already builds those options for Jupyter cells.
### What the Jupyter presenter does today (read 2026-10-30)

`render_cell` (`engine/jupyter/text_execute.rs:562-585`, moving to the shared presenter
in EXR-1) applies only `echo`, `output`, `warning` and `include` (`CellVisibility`) and
wraps the result in a bare `::: {.cell}`. It already matches knitr's **output** divs on
purpose: comments in `text_execute.rs` cite "q2-knitr" for the `.cell-output-*` classes
and the figure div, and `tests/integration/engine_output_parity.rs` compares block shape
between the two engines. It does **not** do layout, panel, column, captions, labels, code
folding, extra classes or forwarded attributes. (A grep of `text_execute.rs` found none of
those; Jupyter label naming is the open strand bd-6sb1z0i4.)

### Consequence: the layer is two layers

(a) **The Quarto cell-option layer** (items 2-7 of the chunk hook, the source-hook Quarto
options, and the plot hook's attributes) is not specific to knitr. Quarto 1 applies it to
Jupyter cells too. If it is built in the shared presenter, Jupyter benefits and the two
engines cannot drift; but it changes Jupyter behavior, which EXR-1 and EXR-6 freeze with
goldens. (b) **The knitr-specific layer** reproduces knitr's own hooks and filters: `prompt`
and `comment`, `collapse` merging, `results` modes, language fences from `hooks_markdown()`,
and, because the capture is before the filters (epic decision 10), knitr's filters
(`echo` vectors, `results: hold`, numeric `warning` and `message`, `fig.show`,
`fig.beforecode`, merging of adjacent pieces). Plot selection by `fig.keep` is done at
capture time, and logical `warning`, `message`, `error` and `include` are execution-relevant,
so the presenter does not filter those.

## Design decisions (from the epic; do not re-litigate)

1. One shared presenter plus a knitr option layer (epic decision 4).
2. Byte identity is measured, not required; only blockers must be fixed (decision 5).
3. knitr's finished markdown is not stored; only fallback units pass through (decision 8).
4. Capture is before knitr's filters, so the presenter applies them (decision 10; KNR-1 S1
   can still overturn it).
5. Presentation options come from the resolved options carried per unit, behind one
   resolved-options type shared with the Jupyter path, and the chunk\'s current header
   options win over the recorded ones (decision 11, with parent decision 34).
6. The Quarto cell-option layer lives in the shared presenter, with new options inert
   unless a cell sets them (decision 12).
7. HTML, Typst and docx go through the shared code; LaTeX targets (`latex`, `beamer`,
   `pdf`) become fallback units with a diagnostic (decision 13).

## Provisional: decisions this plan must make

1. **How the cell-option layer reaches Jupyter** (settled, decision 12). The layer is in
   the shared presenter and this plan lands it, with the options inert unless a cell sets
   them. Obligations: the EXR-1 goldens stay green unchanged, and the `.cell` wrapper
   contract holds (see the decision). Add a test with a Jupyter cell that sets `panel` or
   `layout-ncol` (its output changes by design, as in Quarto 1), and one asserting that a
   cell without them is byte-identical. Documents that already set these options on
   Jupyter cells will see new output; KNR-7 documents that.
2. **How much of knitr's filtering the presenter reimplements.** The list in (b) above,
   since the capture is before the filters (decision 10), minus plot selection (done at
   capture) and the execution-relevant options; that is most of the effort in this plan. If
   S1 overturns decision 10, almost none of it.
3. **The Typst and docx half of decision 13.** Q1 gives them no special knitr handling, so
   they should take the generic path; confirm against q2's `hooks.R` and the corpus, and
   record what LaTeX will need in the LaTeX epic's P1 block "Engine output for LaTeX
   targets" (branch `explore/pandoc-hybrid-latex`).
4. **Figures.** knitr's markdown embeds its own file paths. Decide whether the presenter
   keeps those paths (the files map restores them) or renames files. A rename is a
   classified difference, not a blocker, but it changes URLs and caching.
5. **User hooks.** A user's `knit_hooks$set(...)` is ignored by the presenter. The policy
   (detect and fall back) is made in KNR-3; this plan only needs the detector's result as
   an input.
6. **Where presentation options come from** (settled, decision 11 with parent decision 34;
   the interface remains to be designed). The Jupyter presenter reads them from the
   document at present time. For knitr the presenter reads the **resolved** options carried
   per unit (KNR-1 S8, KNR-6), because a setup chunk\'s `opts_chunk$set`, `opts_template`
   and `opts.label` are invisible to the document text, and the chunk\'s current header
   options (`#|` lines and fence attributes) win over them. Design the option-reading
   interface so the Jupyter path (document) and the knitr path (record plus current header)
   are two sources behind one resolved-options type. The current *document scope*
   (`execute:`) is **not** layered over the record\'s options for now (Gordon, 2026-10-31;
   KNR-3, decision 3): in knitr a setup chunk\'s `opts_chunk$set` outranks the `execute:`
   defaults and the resolved options already include both, and an `execute:` edit
   re-executes. S8 verifies where `execute.R` sets the defaults, and the layering is
   revisited with its result.
7. **The markdown-unit (pass-through) presenter.** Fallback units (decision 8) and old
   knitr traces (decision 14) hold finished markdown as `text/x-quarto-markdown`. No plan
   built a presenter step that inserts such a unit verbatim inside a record that may also
   hold structured units (parent EXR-6 keeps knitr on today\'s whole-document markdown
   replay path, which is a different thing). This plan owns it (Phase 2F): KNR-3\'s
   fallback units and KNR-6\'s old-trace replay depend on it. What it does when the stored
   format differs from the render\'s format is open (KNR-3, decision 8).

## Out of scope

- The record format, hashing and fallback unit handling (KNR-3), inline expressions (KNR-4).
- Changing how any existing document renders; the flip is KNR-6.
- Quarto 1 behavior beyond what q2's current hooks reproduce.

## Phases (provisional)

### Phase 2A: triage

- [ ] Read the KNR-1 report. Turn each blocker class into a work item; list the semantic
  and syntax-level classes as candidates for recorded intentional divergence.
- [ ] Settle the decisions above (1 and 2 first), and re-estimate this plan.

### Phase 2B: tests first

- [ ] Each blocker becomes a failing corpus test with a minimal fixture.
- [ ] Unit tests of each hook feature against hand-built outputs: caption and label
  dedupe, layout synthesis, class assembly, the `results: asis` bare rule, unknown-option
  forwarding (JSON, escaping), indent for list-nested chunks, fence length for outputs
  containing backtick runs.

### Phase 2C: the Quarto cell-option layer

- [ ] Implement items 2-7 of the chunk hook, the source-hook options and the plot
  attributes in the shared presenter, per decision 12 (there is no runtime switch).
- [ ] Jupyter goldens stay byte-identical; the new options are inert for a cell that sets
  none, and the one test with a Jupyter cell that sets `panel` or `layout-ncol` changes by
  design (provisional decision 1).

### Phase 2D: the knitr layer

- [ ] `prompt`, `comment`, `collapse`, `results` modes, `fenced.echo`, language fences.
- [ ] Per decision 10 (capture before the filters, with plot selection at capture):
  `echo` (logical and numeric), `results` (`markup`, `asis`, `hold`, `hide`), numeric
  `warning` and `message` (the presenter filters these; logical `warning` and `message` are
  enforced by R at capture because they are execution-relevant, so the presenter does not
  filter them), `fig.show`
  (`asis`, `hold`, `animate`, `hide`), `fig.beforecode`, source-piece positions
  (KNR-1 S2), and the merging of adjacent pieces; each with a test against the knitr
  behavior in `external-sources/knitr/R/block.R` lines 352-384 of the snapshot, re-read in
  the released versions KNR-1 tests. Also knitr\'s message formatting (`sew.message`,
  `sew.warning`), as the Background lists it.

### Phase 2E: format-aware parts

- [ ] Per decision 13: confirm Typst and docx need nothing beyond the generic path, and
  route `latex`, `beamer`, `pdf` and any other target the corpus does not yet cover to
  fallback units with a diagnostic (KNR-3\'s table lists the rule). The routing is decided
  in Rust, which knows the target, and tells R to skip capture for the whole document; the
  chunk hook then writes the fallback units. LaTeX figure attributes
  (`fig-pos`) and animations are the LaTeX epic\'s concern.

### Phase 2F: the markdown-unit (pass-through) presenter

- [ ] Tests first, over hand-built records (KNR-3\'s adapter is not needed): a record of
  only fallback units presents the same markdown as today\'s live path on the corpus; a
  mixed record (hand-built from a structured unit and a fallback unit) keeps unit order; an
  old knitr trace (finished markdown, no record) replays unchanged.
- [ ] The stored-format mismatch rule (a fallback unit stored for one format, presented
  for another) *(needs KNR-3 decision 8, owned by the parent session)*: this is the only
  Phase 2F item that waits on it. KNR-3 depends on the presenter step, not on this rule, so
  there is no cycle.
- [ ] Implement the presenter step for `text/x-quarto-markdown` units in the shared
  presenter, WASM-clean (no new trait is proposed; any async trait would use
  `#[async_trait(?Send)]`), and the splice support that EXR-14 needs to show these units.

### Phase 2G: re-measure

- [ ] Re-run the corpus. Every difference is classified; zero blockers; the intentional
  divergences are written down with one-line reasons.

## Exit criteria

- Zero blockers on the corpus, on the minimum and the latest supported knitr. Per epic
  decision 17 this is a local run, so the report states the pass and skip counts and the R
  and knitr versions it ran on.
- `latex`, `beamer`, `pdf` and uncovered targets route to fallback units with a diagnostic,
  and the pass-through presenter passes its Phase 2F tests.
- The presenter changes build for `wasm32` (EXR-1\'s availability check) and add no
  dependency on the engine.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` are green; counts reported against the live baseline.
- Jupyter output is unchanged on its goldens, except the one test whose cell sets a new
  option (provisional decision 1).

## Risks

1. **Size.** If the corpus shows many blockers, split this plan (cell-option layer, knitr
   filters, format-aware parts).
2. **Jupyter drift.** Building the shared layer touches Jupyter output; mitigate with the
   goldens (no runtime switch, decision 12).
3. **Format dependence.** Some presentation depends on the target format at present time,
   so replay needs it. Parent decision 27 settles that the record carries no presenter
   inputs and stays format-neutral, which conflicts with format-dependent knitr pieces;
   KNR-3 (decision 8) owns the resolution.
4. **The long tail.** Option combinations nobody wrote a fixture for. Mitigation: the
   corpus is generated from the option matrix; unknown options are forwarded as
   attributes by rule, not by enumeration.

## References

- `crates/quarto-core/src/engine/knitr/resources/rmd/{hooks.R,patch.R,execute.R}`
  (`hooks.R` is 1211 lines)
- `crates/quarto-core/src/engine/jupyter/text_execute.rs`; EXR-1;
  `ts-packages/quarto-api/src/jupyter/to-markdown.ts`
- `external-sources/knitr/R/{block.R,output.R,hooks-md.R}` (development snapshot)
- bd-6sb1z0i4 (label naming for Jupyter), bd-gthycd33 (the `.cell` wrapper, see
  `2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`)

## From the KNR-1 report (2026-11-01)

Measured against the unmodified presenter on 213 cases (`claude-notes/research/2026-11-01-knr-01-corpus-report.md`):
202 compared, 0 identical, 48 syntax-level, 154 blockers (78 of them have a chunk with
two or more source groups); 4 fallbacks, 4 aborts, 3 unavailable. The report\'s list of causes
is this plan\'s starting scope: interleaving anchors, options as attributes (cell, fence
and image), hook-equivalent text (numeric `echo`, `prompt`, `comment`, `fenced.echo`,
`results`), `message` versus `warning`, `text/markdown` display data (dropped today), error
text, and the widget wrapper class. The class of every case is the same on knitr 1.44, 1.50
and 1.52, so the zero-blocker bar is checkable on one version plus a pinned minimum. The
runner is `knitr_corpus_runner::corpus_report`; the bar is "no blocker outside an
allowlisted intentional divergence", and the allowlist reader does not exist yet (add it
with the first entry). A separate Quarto 1 gap, figure `{width=N}` and widget sizing, is
bd-x8hqtvm0 and is not part of this plan.
