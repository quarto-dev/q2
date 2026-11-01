---
title: 'KNR-2: the knitr presenter layer'
date: 2026-10-30
description: 'Closes the gaps KNR-1 measures between the shared Rust presenter and knitr own markdown, so structured knitr outputs present as the Quarto cell markdown that hooks.R builds today. Written before KNR-1 reports, from a reading of hooks.R and the Jupyter presenter; scope is provisional.'
status: closed 2026-11-02  # the part that waits on KNR-3 and KNR-4 moved to KNR-2b
---

**Part of:** [the epic](2026-10-30-knitr-execution-records-epic.md) (plan KNR-2).
**Depends on:** [KNR-1](2026-10-30-knr-01-capture-spike-and-harness.md) (the corpus, the
baseline gap, and the capture-point decision). EXR-1 must have landed (KNR-1 already
needs it); the splice support in Phase 2F is something this plan provides to
[EXR-14](2026-10-28-exr-14-preview-splice.md), not a dependency on it.
**Blocks:** KNR-3 (the fallback-unit presenter, done) and, through [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), KNR-6 (the flip).
**Split (2026-11-02):** this plan is closed for the scope that does not wait on another plan. What remains (the options and fences that need KNR-3\'s record fields, the holds, and the zero-blocker bar) is [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), sequenced after KNR-4.
**Estimated sessions:** 4 to 7 (revised 2026-11-01 from 5 to 10). Interlude I1 prototyped
the presenter fixes against the corpus: five fixes closed 97 of the 154 blockers, about 330
lines, opening none ([findings](../research/2026-11-01-knr-i1-presenter-gap-sizing.md)). The
range is a judgment: the prototype has no tests, no record schema and none of Phases 2E and 2F.

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
8. **`text/markdown` is opt-in through `metadata.quarto_markdown_passthrough: true`** (epic
   decision 22, amended 2026-11-01). The key is flat and engine-neutral. The shared
   presenter handles `text/markdown` only for outputs that carry it, and writes it in a
   `.cell-output-display` div. Whether the output goes out bare instead follows from the
   current `results: asis` cell option at present time (Phase 2D), not from a second
   marker. The Jupyter goldens stay unchanged. Interlude I1 prototyped this branch and the
   two goldens failed until it was gated.
9. **The record holds capture order; the presenter applies knitr\'s reordering** (epic
   decision 21). The presenter reads `quarto_output_anchors` (source lines before each
   output), and `quarto_output_kinds`, cell keys that KNR-3 writes. It also reads `known_options`, an
   optional list on the record\'s `quarto_engines` entry (settled 2026-11-02): subtracted from
   a chunk\'s current options at present time, what is left are the unknown options. It then
   applies `fig.show` and `results` hold and hide, numeric `echo` and `prompt` at present
   time. Interlude I1 showed why the order matters: without applying `fig.show = "hold"`, the
   anchors put each plot after the `plot()` call that drew it, while knitr puts every plot at
   the end of the chunk (two cases opened until it was applied).
10. **The first allowlist** is the four cases in epic decision 23; `knit_hooks_override` is a
    fallback rule (KNR-3), not an allowlist entry.

## Provisional: decisions this plan must make

1. **How the cell-option layer reaches Jupyter** (settled, decision 12). The layer is in
   the shared presenter and this plan lands it, with the options inert unless a cell sets
   them. Obligations: the EXR-1 goldens stay green unchanged, and the `.cell` wrapper
   contract holds (see the decision). Add a test with a Jupyter cell that sets `panel` or
   `layout-ncol` (its output changes by design, as in Quarto 1), and one asserting that a
   cell without them is byte-identical. Documents that already set these options on
   Jupyter cells will see new output; KNR-7 documents that.
2. **How much of knitr\'s filtering the presenter reimplements** (settled by interlude I1,
   2026-11-01). All of the list in (b) above, at present time, over the capture-order record
   (decision 9), minus plot selection (done at capture) and the execution-relevant options.
   Interlude I1 prototyped it: five fixes (interleaving by anchors, `text/markdown`, options
   as attributes, the text hooks, error text and the widget class) took the corpus from 154
   blockers to 57 in about 330 lines and opened none; the rest is a list of small items of the
   same kind (Phases 2B to 2D). Two points from the prototype shape the work. (i) For knitr the
   presenter must not drop stderr for a logical `warning` or `message` value: R already
   enforced those at capture (see Phase 2D), and the baseline\'s single `warning` flag, which
   drops all stderr, is what made `warning = FALSE` a blocker (inferred from the oracle, which
   shows the message the baseline dropped; verify first, test first). (ii) `class.message`,
   `class.warning` and numeric `warning` and `message` need `quarto_output_kinds` (KNR-3).
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
   fallback units and KNR-6\'s old-trace replay depend on it. KNR-3 decision 8
   (settled 2026-11-01) carries the stored format as a flat cell option, proposed as
   `quarto-output-format` (the name awaits Gordon\'s confirmation); the rule for a stored
   format that differs from the render\'s is the Phase 2F item below. The step is for fallback
   units only: an old knitr trace (finished markdown, no record) replays by returning that
   markdown verbatim with no presenter (epic decision 14, interlude I2), so this plan owes
   old-trace replay nothing.

## Out of scope

- The record format, hashing and fallback unit handling (KNR-3), inline expressions (KNR-4).
- Changing how any existing document renders; the flip is KNR-6.
- Quarto 1 behavior beyond what q2's current hooks reproduce.

## Phases (provisional)

### Phase 2A: triage

- [x] Read the KNR-1 report. Turn each blocker class into a work item; list the semantic
  and syntax-level classes as candidates for recorded intentional divergence. Done: the
  slices of Phases 2B to 2D are the work items, and the divergences became the signed-off
  allowlist (Phase 2G, epic decision 23).
- [x] Settle the decisions above (1 and 2 first), and re-estimate this plan. Done: decisions
  1 to 4, 6 and 7 are settled (marked above); decision 5 (user hooks) is KNR-3\'s by design;
  the estimate was revised on 2026-11-01.

### Phase 2B: tests first

- Replaced, not done as written: the corpus is a classified report, not one failing test per blocker, and the gate (`knitr_corpus_gate`) judges every case. [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bC, makes the gate an assertion.
- [x] Anchor tests over hand-built records: source, output, source, output presents
  alternating fences; outputs with equal anchors form one group; a record without anchors
  presents all the source first (today\'s Jupyter shape); `fig.show = "hold"` and
  `results = "hold"` move plots and printed output to the end while the stored anchors stay
  in capture order (decision 9).
- [x] The Jupyter goldens `unhandled-markdown-mime` and `markdown-beats-nothing-plain-wins`
  stay unchanged, and a `text/markdown` output with `quarto_markdown_passthrough: true` is
  written as a `.cell-output-display` div. Only that flat key opts in: a false value, a
  missing key and the old nested `quarto_knitr.asis` all keep the Jupyter shape. The mapper
  sets the key on every `asis` piece.
- [x] For a knitr cell the presenter does not drop stderr for logical `warning = FALSE` or
  `message = FALSE` (R enforced them at capture); the Jupyter `warning: false` behavior is
  unchanged. Expected to close `options/warning/*/01_false` first; verify. Done (slice 2; test `logical_warning_and_message_select_nothing_in_the_presenter`; all four `options/warning` and `options/message` `01_false` cases are syntax-level in r15).
- [x] Trailing whitespace of output lines is kept for knitr cells (`markdown-stress/whitespace`
  and `long_line`); the Jupyter presenter\'s `trim_end()` stays. Done (slice 5, `CellOptions::trim_output`; `markdown-stress/whitespace` and `long_line` are syntax-level in r15).
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bB: unit tests of each hook feature over hand-built outputs (layout synthesis is not built yet).

### Phase 2C: the Quarto cell-option layer

- [x] Slice 1 (commit `66f3c37b8`): `CellOptions` in `presenter/options.rs`, with two sources
  (`from_config` for Jupyter, `from_knitr` for a record) behind one key lookup, covering
  `class.*`, `attr.*`, `column`, `keep-hidden`, `code-fold`, `code-summary`,
  `code-line-numbers`, `lst-cap` and `lst-label`. `render_cell_with` and
  `render_cell_interleaved` take it. Corpus blockers 101 to 79 (`quarto-cell-options` single-group
  blockers 10 to 0, `options` 36 to 24); clippy clean, `quarto-core` nextest 7185 of 7186
  (the known revealjs failure).
- [x] Slice 2 (commit `147b1286f`): `panel` and `layout`, `layout-nrow`, `layout-ncol`,
  `layout-align`, `layout-valign` on the cell div, shared by Jupyter and knitr records (a Jupyter
  document test covers `panel: tabset` plus `layout-ncol: 2`). Corpus blockers 79 to 76;
  `quarto-cell-options` has none left. Clippy clean, `quarto-core` nextest 7191 of 7192 (the
  known revealjs failure), no snapshot changes.
- [x] Output groups (refactor, no format change; requested by the q2-da session 2026-11-02): the
  interleaving runs over `OutputGroup { code, outputs, anchors }` (`interleave_group` in
  `presenter/cell.rs`). Today a cell is one implicit group over all its outputs, so
  `quarto_output_anchors` and `quarto_output_kinds` are unchanged. Sub-units (knitr `child=`,
  `knit_child()`) are a possible future producer of groups, with anchors relative to the
  sub-unit\'s own source; nothing here depends on that.
- [x] Slice 3 (figure captions and alt text): `fig.cap` and `fig.subcap` become the image alt
  text (subcaptions win, each recycled per figure, the count carried across interleaved runs;
  `fig.subcap: true` is empty subcaptions), and `fig.alt` becomes `{fig-alt='...'}` on the image.
  Read under knitr\'s dotted names only, so Jupyter cells are untouched. Corpus blockers 76 to
  66 (`fig.cap` 4, `fig.subcap` 2, `fig.alt` 2, `s1/15_fig_cap_expr`, and one more). Not done, and
  deliberately left for later: the `#fig-` label and cell-level caption when `fig.cap` and
  `fig.subcap` are both set, `fig.scap`, `fig.link`, image `width`/`height`, `fig.align`, the
  `fig.sep` layout synthesis, `fig-column`, `tbl-column` and the `cap-location` family. No
  corpus case needs them.
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md): what is left of 2C. Done here: the layout keys (slice 2), captions and alt text (slice 3), the `fig-` and `tbl-` labels, and the presenter side of unknown options (2D slice 5). Left: unknown options over a real record (Phase 2bA, waits on KNR-3\'s `known_options`) and the options slice 3 left unbuilt (Phase 2bB).
- [x] Jupyter goldens stay byte-identical; the new options are inert for a cell that sets
  none, and the one test with a Jupyter cell that sets `panel` or `layout-ncol` changes by
  design (provisional decision 1). Verified 2026-11-02 at `6465d4130`: the four Jupyter golden
  sets (`jupyter-markdown-goldens`, `presenter-goldens`, `jupyter-inline`,
  `julia-presenter-parity`) were all committed before KNR-2\'s first presenter change
  (`ff771d2b9`, checked with `merge-base --is-ancestor`), and no golden or snapshot file was
  modified or deleted anywhere on the branch (`git diff --diff-filter=MDR` shows only
  `echo-engine.ts`, an `import-recordings` README and `ts_wire_parity.json`, none a golden).
  The 53 golden-named tests pass in the quarto-core run. The by-design exception never
  occurred: no golden changed.

### Phase 2D: the knitr layer

- [x] Slice 1 (`prompt` and `comment`): `prompt` prefixes each expression `> ` and its
  continuation lines `+ `; `comment` prefixes every output line (stdout, stderr, errors,
  `text/plain` results), including empty interior lines, as `knitr:::comment_out` does. The
  presenter learns expression boundaries from the mapper: `expression_starts(chunk)` returns the
  line where each source piece starts, and `render_cell_interleaved` takes it as `starts`
  (empty means unknown: the code is one expression). Jupyter cells and cells that set neither
  option are unchanged. Corpus: `options/prompt` (4) and `options/comment` (6) go from
  blockers to syntax-level only.
- [x] Slice 2 (2026-11-02): `collapse` (output fences merge into the source fence; `comment`
  defaults to `##`), `results = hide` (drops printed text and `knit_asis` text, keeps messages,
  plots and widgets; the mapper marks a widget with `quarto_knitr.widget`), `results = asis`
  (bare text, no `.cell` wrapper unless attributes need it), `knit_asis` HTML as a display div
  with a five-tick `{=html}` fence, source runs with no output between them share one fence,
  and `fenced.echo` (the chunk as written; the mapper supplies the info string and `#|` lines
  through `CellOptions::with_original_chunk`, decided with Gordon). Corpus blockers 59 to 45.
- [x] Slice 3 (2026-11-02): numeric `echo` (`select_expressions`: selected expressions stay,
  anchors remap), error text without an `ename:` header (the mapper emits the printed text as
  the traceback; the presenter writes no header for an empty `ename`, strips ANSI from the
  value), `knitr_visibility` (logical `warning` and `message` never drop stderr: R filtered at
  capture), and message versus warning: `OutputKind`, `CellOptions::with_output_kinds`
  (the record\'s `quarto_output_kinds`; the mapper\'s `output_kinds(chunk)` supplies it),
  `class.message` / `class.warning` on their own outputs, and numeric `warning` / `message`
  filters (`select_by_kind`). Corpus blockers 45 to 26. No snapshot changes.
- [x] Slice 4 (2026-11-02): figures. `include_graphics` keeps the user\'s path and writes a
  width (capture.R computes `raster_dpi_width` per path into `widths`; the mapper marks the
  output `quarto_knitr.include_graphics`), `fig.show = "hide"` writes nothing but still
  splits the source, and an SVG with an `<?xml` prolog (R\'s `svg()`) is written as text.
  Corpus blockers 26 to 18. No snapshot changes. `fig.show = "hold"` (`apply_holds`) is
  still not wired into the presenter; no corpus case needs it yet.
- [x] Slice 5 (2026-11-02): knitr cells keep trailing spaces of output (`CellOptions::trim_output`;
  Jupyter unchanged); a `tbl-` label is the cell id and a `fig-` label the widget output
  div\'s id, with `fig.cap` after the widget; an htmlwidget\'s raw fence stays three ticks;
  `results = "hide"` hides a widget too (Q1, `s2/09_widget_filters`; this reverses slice 2\'s
  "keeps widgets"); `CellOptions::from_knitr_known(options, known_options)` writes unknown
  options as cell attributes (hand-built records only: waits on KNR-3 writing
  `known_options`; attributes come in key order, since a JSON object loses R\'s order).
  Corpus blockers 18 to 12. No snapshot changes.
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md): what is left of the text hooks. Done here: `prompt`, `comment`, `collapse`, the `results` modes, numeric `echo`, the error text, and the presenter side of `fenced.echo` (slices 1 to 3). Left: `fenced.echo` over a real record (Phase 2bA) and language fences and the `knit_asis` fence length (Phase 2bB).
- [x] Figures: `include_graphics` keeps the user\'s path and writes its width, `fig.show = "hide"` drops the plots, and `dev = "svg"` figures are written. Done in 2D slice 4; the corpus\'s `include_graphics` cases are syntax-level in r15. The `quarto_file_path` rename and the sink that honors it are decision 25\'s item below.
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bA: the shared figure sink writing at `metadata.quarto_file_path` (epic decision 25).
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bC: the hidden-widget dependencies hand-off to KNR-3 and KNR-6. The presenter side (the wrapper, and `results = "hide"` dropping a widget) is done here, in slice 5.
- [x] Message versus warning (`class.message`, `class.warning`, numeric filters): done in
  slice 3 over hand-built kinds; KNR-3 must write `quarto_output_kinds` into the record.
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bB: the option-aware reordering of decision 10 that is not yet built (`fig.show` and `results` holds, `animate`, `fig.beforecode`). Done here: logical and numeric `echo`, `results`, numeric `warning` and `message`, and the message and warning classes.

### Phase 2E: format-aware parts

- [x] Per decision 13, first half: Typst and docx need nothing beyond the generic path.
  Evidence (corpus report r14, slice 6): all 13 non-HTML cases (typst 4, docx 4, gfm 5) are
  syntax-level, none a blocker, with no format-specific presenter code.
- [x] Per decision 13, Rust half (slice 6): `engine/knitr/target_route.rs` decides
  `route_for_target(format)`: structured for html, html4, html5, revealjs, epub, typst, docx and gfm
  (the corpus-covered set; one list to extend), fallback for `latex`/`beamer`/`pdf`
  (`FallbackReason::Latex`) and for every other target (`UncoveredTarget`: pptx, context and the like), with `fallback_diagnostic` as the
  one-per-document info message (no catalog code, so no docs page). Four unit tests. The
  module is unconsumed: its dead-code `expect` fails the build once KNR-3 calls it, which is
  the cue to delete that attribute.
- [x] Per decision 13, revealjs and epub (slice 7): the four multi-format rich-output corpus
  documents (kable, df_print_default, knit_asis, include_graphics) now also run under
  `revealjs` and `epub`. knitr treats both as HTML output (`is_html_output`). All 8 new cases
  are syntax-level, and each diff is byte-identical to the same document\'s html diff, so
  they were added to `STRUCTURED_TARGETS` with no presenter change. The router test was
  changed first and failed on its assertion before the list changed.
- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bC: the R half of decision 13 (KNR-3\'s capture call).

### Phase 2F: the markdown-unit (pass-through) presenter

- [x] Tests first, over hand-built records (KNR-3\'s adapter is not needed): a record of
  only fallback units presents the same markdown as today\'s live path on the corpus; a
  mixed record (hand-built from a structured unit and a fallback unit) keeps unit order. (An
  old knitr trace needs no presenter: replay returns its stored markdown verbatim, tested
  in KNR-6.)
- [x] The stored-format mismatch rule (a fallback unit stored for one format, presented
  for another) *(needs KNR-3 decision 8, owned by the parent session)*: this is the only
  Phase 2F item that waits on it. KNR-3 depends on the presenter step, not on this rule, so
  there is no cycle. Done (slice 6) over the `quarto_format` output metadata that
  `record::place` already reads: a mismatched unit stays as written and
  `present_document` returns `PresentNote::MarkdownFormatMismatch`. Moving the stored format
  to KNR-3\'s flat cell option, if decision 8 settles on one, is a change to the one reader.
- [x] Implement the presenter step for `text/x-quarto-markdown` units in the shared
  presenter, WASM-clean (no new trait is proposed; any async trait would use
  `#[async_trait(?Send)]`), and the splice support that EXR-14 needs to show these units.
  Done (slice 6): `present_document` takes the target `format` and splices a stored-markdown
  cell with `record::replacement`; five tests in `presenter/document.rs`, written first.

### Phase 2G: re-measure

- Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bC: re-run the corpus to zero blockers. Measured here on 2026-11-02 (report r15, 221 cases): 198 accepted, 5 allowlisted, 11 not compared, 7 blockers that wait on KNR-3 and KNR-4.
- [x] Add the allowlist reader to the runner. `tests/integration/knitr_corpus_gate.rs` reads
  `tests/fixtures/knitr-corpus/allowlist.toml` (name, optional formats, pinned class, reason)
  and judges the records per epic decision 26: `identical` and `syntax-level` pass; a
  `semantic` or `blocker` needs an entry; an entry whose case became accepted, or moved to
  another class, is reported as stale or for re-review. Cases never compared (fallback,
  aborted, unavailable) are counted, not judged. Ten tests, written first and seen failing on
  their assertions. `corpus_report` appends the verdict to its summary file but does not
  assert it, because the 7 residual cases above wait on KNR-3 and KNR-4; the assertion
  moves to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), Phase 2bC (KNR-3 Phase 3C runs
  the same harness in report mode). Five entries: the four of epic decision
  23 and `s8/01_setup_state` (Gordon, 2026-11-02: the same divergence, an empty `.cell` div
  for a chunk that renders nothing, here `c_params` with `eval` false; Q1\'s
  `.cell-code  code-fold` double space is cosmetic). That settles the earlier open question
  about its `params:` front matter: it is an entry, not a gap.

## Exit criteria

Met by this plan (2026-11-02, at `6465d4130`):

- The pass-through presenter passes its Phase 2F tests.
- `cargo clippy -p quarto-core --all-targets -- -D warnings` is clean, and
  `cargo nextest run -p quarto-core` passes 7274 of 7275 (the one failure is the known
  `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`).
- Jupyter output is unchanged on its goldens (Phase 2C, last item); the by-design exception
  never occurred.
- The Rust half of target routing: `latex`, `beamer`, `pdf` and uncovered targets route to
  fallback with a diagnostic (`target_route.rs`), and revealjs and epub are structured.

Moved to [KNR-2b](2026-11-02-knr-02b-presenter-remainder.md), which owns them:

- Zero blockers on the corpus, on the minimum and the latest supported knitr, with the pass
  and skip counts and the R and knitr versions reported (epic decision 17).
- The R half of routing: fallback units with one diagnostic, written by KNR-3\'s capture call.
- The presenter changes build for `wasm32` and add no dependency on the engine.

## Risks

1. **Size.** Interlude I1 measured it: the prototype closed 97 of 154 blockers in one
   session, the rest is small items of the same kind, so the plan is not split (4 to 7
   sessions). The one cross-plan dependency is KNR-3\'s two keys, for 10 corpus cases.
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
