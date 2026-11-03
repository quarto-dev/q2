---
title: 'KNR-3 slice 7: generic cell options in the shared presenter'
date: 2026-11-03
description: 'Makes the shared Rust presenter honor 16 of the 17 Quarto cell options the KNR-3 option sweep found it ignoring (image attributes, figure link, cell and source classes, caption and column placement), plus the Jupyter-only gaps a new Jupyter sweep finds, for every engine, under Quarto spellings with knitr names as aliases, and leaves indent inert, with the departure documented.'
status: ready  # D1 to D9 answered 2026-11-03; D4 reopened and D10 answered 2026-11-04 (implementability review); no code written
---

**Part of:** [KNR-3](2026-10-30-knr-03-record-adapter-hash-fallback.md), slice 7 (its "Slice 6 notes"
list the gaps). Epic: [knitr execution records](2026-10-30-knitr-execution-records-epic.md).
**Depends on:** KNR-3 slices 1 to 6 (landed on `knr-2-presenter`).
**Blocks:** the KNR-3 exit criteria (the corpus gate), and so KNR-2b and KNR-6.
**Estimated sessions:** 3 (a guess: the Jupyter sweep, four small phases and one close-out).

## Goal

The Rust presenter (`crates/quarto-core/src/presenter/`) is the one nbformat-and-record-to-markdown
path for Jupyter, knitr and stored-notebook replay. Seventeen cell options reach it and change
nothing, although live Quarto honors them. Make the presenter read 16 of them, in the one place that
decides what a key means (`CellOptions::from_lookup`), so a Jupyter cell, a knitr cell and any later
engine get the same markup. The 17th, `indent`, stays inert, with a documented departure (D8). Terms are Quarto\'s
(`fig-align`, `classes`), not knitr\'s; knitr\'s dotted spellings are aliases.

**Principle (Gordon, 2026-11-04).** A knitr feature lifted into the generic presenter reaches every
engine. Where Quarto 1\'s Jupyter code differs, the plan follows the generic behavior and records
the difference as a departure (D4, D5, D6), not an engine flag. The hash follows the same rule:
every option the presenter reads is display-only for every engine (D10).

## Findings

Each is marked **run** (executed on 2026-11-03) or **read** (from code only). There is no Jupyter
kernel on the research machine, so "Jupyter" runs replay a hand-built notebook with stored outputs
(`q2 render t.ipynb`, and Quarto 1\'s `quarto render t.ipynb --no-execute`).

1. **The gap is engine-wide, not knitr-only.** *Run:* `q2 render` of a stored notebook whose cell sets
   `fig-align`, `fig-link`, `fig-scap`, `fig-env`, `fig-pos`, `classes`, `cap-location`,
   `fig-cap-location`, `code-overflow`, `fig-column` and `md-indent` yields a plain `.cell` and a bare
   `![](...png)`; no later stage adds any of them. *Read:* `from_lookup` reads `panel`, `column`,
   `layout*`, `code-fold`, `lst-*` and `keep-hidden`, and nothing else on this list. The TypeScript
   presenter (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`, used by Julia and marimo) and
   Quarto 1\'s `jupyter.ts` both implement them.
2. **Jupyter declares only 7 of the 17 presentation-only.** *Read:* `DISPLAY_ONLY_KEYS` in
   `engine/jupyter/mod.rs` excludes `fig-align`, `fig-link`, `fig-scap`, `cap-location`,
   `fig-cap-location`, `tbl-cap-location` and `code-overflow` from the hash. For those seven, an
   edit shows nothing until something else re-runs the cell (the same silent staleness the sweep
   found for knitr). The other Jupyter options (`fig-env`, `fig-pos`, `classes`, `fig-column`,
   `tbl-column`, `md-indent` and the three `out-*`) are hashed, so an edit re-runs: for them this
   plan adds a missing feature, and D10 makes them display-only. *Run (a sub-agent, stored
   notebook):* a Jupyter `fig-alt` shows nothing (`alt=""`) although it is display-only, because the
   presenter reads only `fig.alt`; *read:* `tbl-scap`, `tbl-subcap` and `tbl-colwidths` have no
   reader in `presenter/` either, and `fig-cap`, `fig-subcap` and `tbl-cap` are read dotted or not
   at all. D10 adds a Jupyter sweep to find the rest. *Run (grep):* no golden under
   `jupyter-markdown-goldens/`, `presenter-goldens/`, `jupyter-record-shadow/` or `records/` sets any
   of the options as a `"key": value`; the names appear only in hash declarations. The goldens
   should not move (the script in Phase 7f checks it).
3. **Live markup, from `hooks.R` (identical to Quarto 1\'s apart from record plumbing). Read, and
   confirmed by the corpus diffs (run):** image braces `#id`, `fig-align`, `fig-env`, `fig-pos`,
   `fig-alt`, `fig-scap`, `width`, `height`, `out.extra`; `fig-link` wraps the image as
   `[![..](..){..}](link)`; `fig.align` also writes `layout-align` on the cell div; the cell classes
   follow `.cell`, `classes`, panel, column, `fig-column`, `tbl-column`, `caption-*`,
   `fig-cap-location-*`, `tbl-cap-location-*`; `code-overflow` adds a class to the source fence.
4. **Quarto 1\'s Jupyter code differs from knitr\'s in these places. Run** (Quarto 1,
   `--no-execute`, stored notebook):
   - Image braces put `width` and `height` before the `fig-*` attributes, and `fig-alt` after
     `fig-align` (`{width=300 height=200 fig-align='left'}`), unlike knitr. The widths come from
     the output\'s metadata; `out-width` as a cell option is ignored.
   - `cap-location: top` with `fig-cap-location: margin` writes `.caption-margin`: Quarto 1 uses
     the `fig-cap-location` value. *Read:* with `cap-location` alone it writes `.caption-undefined`.
     The bug is copied into the TypeScript presenter (`to-markdown.ts:721`). It is reported upstream
     as quarto-dev/quarto-cli#15281, and for the q2 copy as bd-3uj42ytc.
   - `fig-align` is an image attribute only, with no `layout-align` on the cell.
   - An image with no attributes gets an empty `{}` (the Rust goldens have none; this plan keeps
     the Rust behavior).
5. **Record keys.** *Run (dumped records):* every chunk records the sentinels `fig.align: "default"`,
   `fig.env: "figure"`, `fig.pos: ""`. A header `fig-align` is stored as `fig.align`; a header
   `fig-column` and `fig-cap-location` are stored as written. *Read:* `layer_options` rewrites a
   header key to its dotted form when `known_options` lists that form, and `known_options` lists both
   `fig-column` and `fig.column`; so a header edit of `fig-column` lands under `fig.column` while the
   recorded `fig-column` survives, and `hooks.R` reads `fig-column` first. `fig-cap-location` has no
   twin (its dotted rewrite, `fig.cap.location`, is not a known name). D1 removes the collision by
   construction.
6. **`out.width` has a side channel. Read:** `capture.R` (lines 244 to 263) records no computed width
   for an `include_graphics` image when `out.width` is set at capture time. *Run:* the dumped record
   for `out.width: 50%` holds `out.width` and `out.width.px` and no width on the figure.
7. **`indent` is not a presentation option for knitr. Read:** knitr\'s parser sets it from the
   chunk\'s container indent (`parser.R:123`), and the splice already re-applies container prefix and
   indent (`record/place.rs`, `document.rs`). Presenting it would double-indent a list-nested chunk.
   The adapter\'s fallback is never option-driven (`adapter.rs`: unmappable pieces or an overridden
   hook), and R cannot tell a parser-set `indent` from a header-set one. Hashing it would only force
   a re-run whose presentation ignores `indent` anyway, so it buys nothing. *Run:* the corpus value
   `'"  "'` carries literal quote characters, so live writes a literal `"  "` prefix; the two corpus
   cases show that artifact, not a normal indent. Neither `indent` nor Jupyter\'s `md-indent` is
   documented or used in `docs/` or the smoke tests.
8. **Header removal is stale for every presented knitr option. Run (2026-11-04, knitr 1.50; the
   [spike note](../research/2026-11-04-stale-presentation-options.md) has the cases):** a `#|`
   option deleted from a chunk keeps its recorded value, and the unit still matches as fresh. A
   deleted `fig-cap` keeps its caption and a deleted `echo: false` keeps the code hidden, while a
   fresh run shows neither. The cause: `layer_options` overlays only the current header, the record
   holds the full resolved set (with the old header value in it), and the hash leaves the option
   out. A *changed* header value shows at once. The gap is **header-only** for the options q2
   honors: a document-level `execute:` or `knitr:` change moves the knitr document hash and
   re-runs, the fence forms are rejected (Q-2-36), and `format: html:` keys do not reach R. The
   gap is knitr-only: a Jupyter cell\'s options come from the document on every run. Gordon
   decided on 2026-11-04 to pin it here and fix it in
   [KNR-3 slice 8](2026-11-04-knr-slice-8-header-removal.md), before KNR-6 flips the live path.
   Phase 7.0 writes the known-gap test.
9. **Existing consumers on Jupyter. Read:** `crossref_render` strips `fig-align` from the contained
   image (to build `quarto-figure-<align>`), so the presenter\'s emission feeds it and nothing is
   emitted twice; it also reads a `cap-location` attribute on floats. No Rust consumer of the
   `caption-*` classes. **Run (grep, recounted 2026-11-04):** 13 `classes: plain` cells in 11 files of
   `crates/quarto/tests/smoke-all/typst/margin-layout/`; 7 of the files hold Python
   (great_tables) cells and go through Jupyter, so they will change output once the presenter honors
   `classes` (the other 4, R cells, stay on the live knitr path until KNR-6). The one
   `cap-location: bottom` is in `margin-table-great-tables-caption-below.qmd` (Python). Other
   Python cells that set a newly presented option (for instance `out-width`, `fig-scap`, `fig-pos`
   in `docs/guides/authoring/figures.qmd` and `docs/guides/formats/latex/index.qmd`) change too.
   Phase 7f recounts with a grep over every option and runs them. *Read:* a Lua filter,
   `parsefiguredivs.lua`, turns a `.caption-<x>` class into a `cap-location` attribute, so
   `caption-*` has a consumer, outside Rust.
10. **`fig-pos='H'` for latex. Read:** Quarto 1 adds it when code is shown with a figure and the
    target is latex. For knitr this is moot (a latex target uses fallback cells). Jupyter latex is
    not covered by this plan.
11. **Per-figure values.** *Read:* Quarto 1 Jupyter indexes an array-valued `fig-align` and the
    other `fig-*` options by output index (`broadcastFigureOption`: `value[index]`, no recycling,
    and `index` counts every output of the cell, not only figures). *Run (knitr 1.50):* knitr
    recycles ten options by plot (`.recyle.opts`: `fig.cap`, `fig.scap`, `fig.alt`, `fig.env`,
    `fig.pos`, `fig.subcap`, `out.width`, `out.height`, `out.extra`, `fig.link`) and does not
    recycle `fig.align`: a two-plot chunk with `fig-align: [left, center]` gives every image
    `center` and the cell `layout-align="left"`. D3 states the rule the presenter follows.

## Decisions

All answered by Gordon on 2026-11-03.

- **D1. One canonical spelling per option, the Quarto one; knitr names are aliases, renamed in
  `layer_options`.** A single alias table in `presenter/options.rs` maps `fig.align` to
  `fig-align`, `fig.link`, `fig.scap`, `fig.env`, `fig.pos`, `fig.column` and the three `out.*` to
  their dashed names. `from_lookup` reads canonical names only. Existing dotted reads (`fig.cap`,
  `fig.subcap`, `fig.alt`, `prompt`, `comment`, `collapse`, `results`) stay as they are; this plan
  does not refactor them. `layer_options` then works in this fixed order:
  1. rename the recorded keys and the header keys to canonical names. For an aliased name this
     replaces today\'s dashed-to-dotted rewrite of a header key (a header `fig-align` stays
     `fig-align`, no longer `fig.align`); every other key keeps the existing rewrite;
  2. overlay the header on the record;
  3. drop knitr\'s sentinels (D2);
  4. derive `layout-align` from `fig-align` (D4).

  No twin (`fig-column` against `fig.column`) can survive step 1.
- **D2. knitr sentinels and fallbacks live in the alias layer, not in the shared reader.** The
  defaults `fig.align = "default"`, `fig.env = "figure"`, `fig.pos = ""` are dropped at step 3,
  after the header overlay and never before, so a header `default` clears a recorded value. The
  shared reader treats "empty" as "unset" for every engine, and a Jupyter `fig-env: figure` is
  written.
- **D3. Image attribute order follows knitr for every engine:** `fig-align`, `fig-env`, `fig-pos`,
  `fig-alt`, `fig-scap`, `width`, `height`, then the extra text. The corpus is the only oracle for
  it. Order shows only as the order of `<img>` attributes in the HTML (run) and has no rendering
  effect. **Per-figure rule, for every engine:** figure *i* (counting images only, as `figure_cap` already
  does) gets `value[i % len]` of a list-valued option, and a scalar applies to every figure. This is
  knitr\'s rule for its ten recycled options, extended to `fig-align`; it departs from Quarto 1
  Jupyter (no recycling, output index) on purpose, each pinned by a test.
- **D4. `fig-align` also writes `layout-align` on the cell, for every engine** (reopened and
  answered 2026-11-04; Quarto 1\'s Jupyter code writes the image attribute only, so this is a
  departure for Jupyter). Step 4 derives it from the first element of the layered `fig-align`
  (never the compact-JSON form of a list, which would break the attribute), after the sentinel drop,
  so a `default` yields none; an explicit `layout-align` loses to it, as in `hooks.R`. The derived
  key lives only in the layered options the presenter reads; the hash and the sweep read the
  author\'s options, so it moves no hash (a Phase 7b test pins that).
- **D5. `cap-location` writes `caption-<cap-location value>`,** not Quarto 1\'s
  `caption-<fig-cap-location value>`. A deliberate departure that fixes a Quarto 1 bug (for knitr it is
  no departure: `hooks.R` already writes it), pinned by a test whose input is `cap-location: top`
  with `fig-cap-location: margin`: the presenter writes `.caption-top`, Quarto 1 writes
  `.caption-margin`. The class options (`cap-location`, `fig-cap-location`, `tbl-cap-location`,
  `fig-column`, `tbl-column`) write a class only for a scalar string, for every engine (answered
  2026-11-04; Quarto 1 Jupyter\'s `typeof === "string"` guard). A bool, number or list writes
  nothing; the test inputs are `cap-location: true` and `cap-location: [top, bottom]`. The knitr
  list case is bd-s344ebc0.
- **D6. `out-width`, `out-height` and `out-extra` are generic cell options for every engine,** with
  no engine flag, and a cell option wins over output metadata. The presenter also reads an image\'s
  `width` and `height` from the output metadata (Quarto 1 parity; the Rust Jupyter path reads
  none today, and no golden carries one, by grep), into the same attribute slot. Quarto 1 ignores
  `out-width` as a Jupyter cell option (run), so this is a deliberate extension for Jupyter. The
  three names are display-only for Jupyter (D10).
- **D7. `out-width` is presented; the gap in finding 6 is accepted and pinned with a test, and
  header removal (finding 8) is pinned the same way for every presented knitr option.** Adding or
  changing `out-width` shows at once; deleting it from an `include_graphics` cell shows the computed
  width only after a re-run. The fix for header removal is [slice 8 of KNR-3](2026-11-04-knr-slice-8-header-removal.md).
- **D8. `indent` is inert, not presented and not hashed; its two corpus cases are allowlisted.**
  The presenter ignores it with or without a re-run, so hashing would only force a pointless run.
  The sweep row moves from `Gap` to `Inert` with an `INERT_REASONS` entry that states the
  departure. `indent` stays in `STRUCTURED_PRESENTATION_OPTIONS`, so no hash moves and
  `KNITR_HASH_VERSION` stays 2. Whether a header `indent` still takes effect after KNR-6 depends on how
  the skeleton marker\'s indent (KNR-6 plan, "indented by `options$indent`") survives the splice; the
  allowlist reason says KNR-6 decides, with Gordon\'s sign-off for the divergence (`indent` is
  undocumented and unused in `docs/` and the smoke tests). No fallback unit. Jupyter\'s `md-indent` stays out of this plan.
- **D9. The TypeScript presenter is not changed here.** EXR-8 owns parity. This plan adds parity
  fixtures for the options the harness can compare (`classes`, `code-overflow`, `fig-column`; the
  harness checks block kinds, div and code classes and figure names, not image attributes). A
  `cap-location` fixture needs an `ACCEPTED` entry naming D5. The TS copy of the bug is bd-3uj42ytc;
  the Quarto 1 original is quarto-dev/quarto-cli#15281.
- **D10. Jupyter gets its own option sweep, and hash and presenter agree for every engine**
  (answered 2026-11-04). Slice 6\'s sweep covers knitr only; Jupyter declares about 30 keys
  display-only (`DISPLAY_ONLY_KEYS`) and nothing checks that the presenter reads them. Phase 7.1
  builds the check first. `fig-alt`, `tbl-colwidths`, `tbl-scap` and `tbl-subcap` are in scope now,
  and so is whatever else the sweep flags (candidates read only, not run: `fig-cap`, `fig-subcap`,
  `tbl-cap`, `code-copy`); the plan\'s counts are rewritten from its result. Every option the
  presenter reads becomes display-only for Jupyter (`fig-env`, `fig-pos`, `classes`, `fig-column`,
  `tbl-column`, the `out-*` names), so the first run re-runs a stored Jupyter cell that sets one;
  the goldens set none.

## Phases

TDD throughout: in each phase write the tests first and watch them fail. Each phase ends with
`cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p quarto-core`
(expected known failure: `revealjs::assemble::tests::vendored_reveal_assets_match_npm_package`).

### Phase 7.0: confirm the claims (no code)

- [x] The crossref layer and later stages: no double emission (finding 9; read, plus the run in
  finding 1).
- [x] Image attribute order is observable only as `<img>` attribute order (D3; run).
- [x] Quarto 1 Jupyter behavior for `fig-align`, `fig-link`, `cap-location`, `out-width` and
  metadata widths (finding 4; run with `--no-execute`; the `.caption-undefined` case is read).
- [x] Record keys for a header `fig-column`, `fig-cap-location` and `fig-align` (finding 5; run).
- [x] No golden sets any of the options as a value (finding 2; run as grep).
- [ ] **Header removal (finding 8, knitr only; a known-gap test, not a fix):** present a record
  built with a header option (`fig.cap`, `echo: false`, and later `fig-align` and `out-width`),
  then present the same record against a header with the option deleted, matched by id as the
  spike harness did. The test asserts today\'s *wrong* result (the old caption or hidden code
  survives) under a name and comment that say it is the slice 8 gap. Slice 8 (design (c)) flips it: the
  deletion then marks the unit stale, so the presenter never sees the old record for that unit. Also
  pin that a changed header value shows at once (the control). A header `fig-align: default`
  clears a recorded `right` only once step 3 follows step 2; Phase 7b asserts that. Document-level
  options, fence parameters and `opts_chunk` need no test here: the spike showed they re-run or
  never reach R.
- [x] **Per-figure vectors (finding 11):** knitr\'s semantics run (a sub-agent, knitr 1.50) and the
  rule decided by Gordon on 2026-11-04 (D3): recycle by figure index for every engine.

### Phase 7.1: the Jupyter sweep (D10; tests only, before any presenter change)

Model it on `record_splice/tests/option_sweep.rs`, in the same module or a sibling (unit tests, not
an integration binary).

- [ ] A test that, for every `DISPLAY_ONLY_KEYS` entry, builds a stored-notebook cell with the key
  set and without it, runs the Jupyter presenter path (`CellOptions::from_config`), and requires the
  markdown to differ or the key to carry an `INERT_REASONS` entry; plus the reverse check: every
  option the presenter reads is in `DISPLAY_ONLY_KEYS` (D10). It starts red.
- [ ] Record the red rows in this plan (rewrite this item with the final list), and rewrite the
  Findings and the phases below from them. Rows already expected: `fig-alt`, `tbl-colwidths`,
  `tbl-scap`, `tbl-subcap`, and the seven options of the 17 that Jupyter hashes or excludes.

### Phase 7a: canonical reader and alias layer

Implements steps 1 to 3 of D1\'s order in `layer_options` (step 4 comes with Phase 7b). The
alias table also lists `fig.alt` (to `fig-alt`, D10). The reader fields arrive in 7b, so the tests
here assert on the merged options `Value` that `layer_options` returns.

Tests first (in `presenter/knitr_inputs.rs` for the layering; `presenter/options.rs` for the table):

- [ ] The alias table maps each dotted name to its canonical one; a record with `fig.align: center`
  and one with `fig-align: center` give the same merged `Value`.
- [ ] Sentinels (`"default"`, `"figure"`, `""`) are dropped at step 3 of `layer_options` (D2); a
  header `default` clears a recorded `center`.
- [ ] `layer_options`: a header edit of `fig-column` replaces a recorded `fig-column` (no stale
  twin), and likewise for `fig-align` against `fig.align`; a non-aliased dashed header key
  (`class-source`) still gets the existing dotted rewrite; a layer holding both `fig-column` and
  `fig.column` keeps the dashed one (as `hooks.R:601-605` reads it first).
- [ ] **No leaked attribute:** with the shipped `known_options` (dotted names only, from
  `capture.R:372-400`), a header `fig-align`, `fig-scap`, `fig-link`, `fig-env`, `fig-pos`,
  `out-width`, `out-height` or `out-extra` produces no `key='value'` on the `.cell` div.
  `from_knitr_known` must treat both spellings of every alias as known; without that, step 1 of D1
  forwards the renamed keys as unknown attributes and the sweep rows flip for the wrong reason.
- [ ] A header or record `out-width: 0.5` (a float) survives `layer_options` (`config_to_json`
  has no `Yaml::Real` arm today and writes null, which would clear the recorded value).

Then implement the table, the known-name check and the layering. The unit tests here must pass with
the output unchanged for every existing test. Callers that skip `layer_options` (the `KNR_DIRECT=1`
path in `knitr_corpus_runner.rs`, `knitr_corpus_baseline.rs`, and unit tests that call
`CellOptions::from_knitr` with dotted keys and `"default"` sentinels) must apply the same alias and
sentinel step: put the alias rename and sentinel drop in one function that `layer_options` and
`from_knitr` both call.

### Phase 7b: image attributes

Covers `fig-align`, `out-width`, `out-height`, `out-extra`, `fig-scap`, `fig-env`, `fig-pos`.

Tests first:

- [ ] `image_attrs_with` exact strings: each option alone, all together (order of D3), defaults
  absent, an apostrophe in `fig-scap` escaped as `\'`. All together, for example:
  `{fig-align='center' fig-env='figure*' fig-pos='h' fig-alt='a' fig-scap='short' width=50% height=50% style="border: 1px"}`
  (`out-extra` is `style="border: 1px"`). `image_attrs_with` takes a numeric width today
  (`options.rs:303`); the new `out-width` is text (`50%`), so its signature changes.
- [ ] Multi-figure cell with `fig-cap` and `fig-subcap`: every image carries the attributes; a
  list-valued option recycles by figure index (D3), including `fig-align` (a two-value list on
  three figures gives `left`, `center`, `left`).
- [ ] `fig-align` also writes `layout-align` on the cell for knitr and for Jupyter (D4), from the
  first element of a list; an explicit `layout-align` loses to it; a `default` `fig-align` writes
  neither; the derived `layout-align` moves no unit hash and no sweep row (D4). This is where step 4
  of `layer_options` lands, and where the derived key is added.
- [ ] `include_graphics` image with `out-width`: the cell option wins over the recorded width, and
  the limitation of D7 has its own pinned test.
- [ ] Jupyter: a cell with `fig-align: center` yields `{fig-align='center'}` on its image and
  `layout-align="center"` on its cell; a stored image with output metadata `{"width": 300}` gets
  `width=300` (D6, new reader); a cell with `out-width: 50%` over that metadata gets `width=50%`
  (the cell option wins); a Jupyter `fig-env: figure` is written (D2); a Jupyter `fig-alt: 'alt text'`
  gives `alt text` on the image (D10).
- [ ] The sweep rows `fig.align`, `fig-align`, `fig.scap`, `fig.env`, `fig.pos`, `out.width`,
  `out.height`, `out.extra` move from Gap to Presents
  (`stage/stages/record_splice/tests/option_sweep.rs`).
- [ ] Corpus: the `options/*` and `quarto-cell-options/fig-align` blockers for these options become
  accepted.

Implement: new fields on `CellOptions`, read in `from_lookup`; `image_attrs_with` and the layout
vector in `cell_div_attrs`.

### Phase 7c: figure link

Tests first:

- [ ] A plot image and an `include_graphics` image with `fig-link` are written
  `[![alt](path){attrs}](link)`, with the link verbatim.
- [ ] `results: asis` returns the bare link markdown, as `hooks.R` does.
- [ ] A Jupyter cell with `fig-link` wraps its image.
- [ ] Sweep row `fig.link`: Gap to Presents. Corpus: the `options/fig.link` blockers accepted; add a
  case with an unquoted URL (the current corpus value carries literal quote characters, which live
  copies into the link target, so it does not show a normal URL).

Implement at the two image call sites in `presenter/cell.rs`.

### Phase 7d: cell and source classes

Covers `classes`, `cap-location`, `fig-cap-location`, `tbl-cap-location`, `fig-column`,
`tbl-column`, `code-overflow`.

Tests first:

- [ ] `cell_div_attrs` class order: `classes` (a list, a string, a leading dot kept, not split on
  spaces), panel, column, `fig-column`, `tbl-column`, `caption-*`, `fig-cap-location-*`,
  `tbl-cap-location-*`. These readers need a typed read: `from_lookup` flattens everything to
  strings, so add a scalar-string accessor and use it for the five class options (D5): the inputs
  `cap-location: true` and `cap-location: [top, bottom]` write no `caption-` class.
- [ ] `cap-location: top` with `fig-cap-location: margin` writes `.caption-top` (D5); Quarto 1
  writes `.caption-margin`. `cap-location` alone does not write `caption-undefined`.
- [ ] Regression test: `needs_cell_div` already keeps the div (it tests `cell_div_attrs().is_empty()`),
  so a `results: asis` cell with any of these classes, or a derived `layout-align`, keeps its div.
- [ ] `code-overflow: wrap` and `scroll` add the class to the source fence in its own slot between
  `.cell-code` and the attributes (`echoed_source_fence_with`); `hooks.R` puts it before
  `attr.source` classes. The presenter has no `source.hidden`, so nothing pins `.hidden`.
- [ ] Jupyter cells with each option.
- [ ] Sweep rows for the seven options: Gap to Presents. Corpus: the `quarto-cell-options/*`
  blockers accepted.

### Phase 7e: indent (D8)

- [ ] Sweep row `indent`: `Gap` to `Inert`, with an `INERT_REASONS` entry that states the departure
  (the presenter ignores `indent`; the splice owns container indentation). A test pins that the
  structured hash does not move for an `indent` edit and that `KNITR_HASH_VERSION` stays 2.
- [ ] Change the corpus `indent` values to a normal one (a YAML string of two spaces, no literal
  quotes), so the case shows a real indent; this keeps the case names
  (`options/indent/header/00_empty`, `options/indent/yaml/00_empty`).
- [ ] Run the corpus report first and confirm both cases classify as a Blocker (a sub-agent saw the
  live output indent code and output two spaces). Then add the two allowlist entries by exact name
  (the allowlist matches names exactly, with no globs, and a test fails an entry that names no
  fixture or that classifies as accepted), `class = "blocker"`, with a reason that states the D8
  departure and Gordon\'s sign-off (epic rule: a Blocker needs a signed-off entry).

### Phase 7f: close-out

- [ ] Re-run the corpus report. Baseline at `fd0ab4d1f`: 227 accepted, 5 allowlisted, 7 not
  compared, 31 unexpected (30 gaps, `s9/01_inline`). Expected after Phases 7b to 7e, before the
  cases this plan adds: 255 accepted (227 plus the 28 non-`indent` gaps), 7 allowlisted (5 plus the
  two `indent` cases), 7 not compared, 1 unexpected (`s9/01_inline`, KNR-4\'s). The command is
  `KNR_REPORT=<file> cargo nextest run -p quarto-core -E 'binary(integration) & test(knitr_corpus_runner::corpus_report)'`
  (needs R with knitr). The gate is report-only, so read the summary. The unquoted `fig.link` URL
  adds two cases (header and yaml forms), and the `indent` value change adds none; recompute and
  record the final numbers here.
- [ ] Freeze the new corpus cases (49, plus 2 for the unquoted URL) in the oracle: edit the manifest
  values first, then run the report with `KNR_FREEZE=1` and `KNR_REPORT` set, with the same pinned
  knitr (`KNR_R_LIBS`) as the 214 frozen files. The oracle is live knitr output and does not depend
  on the presenter, so freezing need not wait for the gaps to close. Report the count of added
  `live/*.md` files, what they show and anything surprising, as AGENTS.md asks for snapshots.
- [ ] `scripts/check-presenter-goldens-unchanged.sh` covers only `presenter-goldens`. Also run
  `jupyter_markdown_goldens::{jupyter_markdown_matches_goldens, stored_records_present_to_the_goldens,
  legacy_capture_payload_is_byte_identical_after_flip}` and say which need a kernel: the Jupyter and
  presenter goldens must be byte-identical. A changed golden is a behavior change; stop and ask
  (epic decision 28).
- [ ] Run the typst margin-layout smoke tests that set `classes: plain` and `cap-location`, and
  any other Python-cell document that sets a newly presented option (recount by grep; finding 9), and review what changed. They need a Jupyter kernel, which the research machine did
  not have: run them where one exists (or in CI) and say which; if neither is possible, record them
  as not verified.
- [ ] Add parity fixtures for `classes`, `code-overflow` and `fig-column` to the Julia presenter
  parity set (`CASES` in `tests/integration/julia_presenter_parity.rs`; each needs an `.ipynb` and a
  `.ts.md` made by `scripts/exr-julia-parity/generate-ts-goldens.sh`, which needs TS Quarto on
  `PATH`), and an `ACCEPTED` entry for a `cap-location` fixture (D9).
- [ ] End to end through the binary, per AGENTS.md, on a rebuilt `q2`: `cargo run --bin q2 --
  render` on a `.qmd` with R chunks, and on a stored `.ipynb` (no kernel; options as `#|` lines in
  the code cells) that sets each option; read the HTML and record the invocation and the markup
  seen in this plan.
- [ ] Exit-criteria walk from KNR-3: workspace `cargo nextest run --workspace` against the live
  baseline, the wasm32 check, `cargo xtask lint`, `cargo xtask verify` (quarto-core has downstream
  dependents), `cargo clippy -p quarto-core --all-targets --target x86_64-pc-windows-msvc -- -D
  warnings`, and a `q2 render --strict` of this note. New integration tests live under
  `tests/integration/`.
- [x] Update the neighbors (done 2026-11-04, uncommitted at the time of writing; re-check at close-out): KNR-2b\'s open list (it still lists the image and caption options this
  slice builds; its counts of 221 cases and 16 blockers predate the 270), KNR-3\'s "Slice 6 notes"
  (the freeze ordering), and a note in KNR-6 that D8\'s `indent` outcome depends on its marker.
- [ ] Decide whether D4, D5, D6 and D8 (departures from Quarto 1) belong in
  `docs/guides/authoring/migrating-markdown.qmd`, which AGENTS.md says to keep current.
- [ ] Reconcile this checklist and the KNR-3 plan\'s slice list; add a "Slice 7 notes" section to
  KNR-3 and tick its progress list.

## Out of scope

- The TypeScript presenter, except the parity fixtures (EXR-8 owns it; bd-3uj42ytc holds its
  `cap-location` bug).
- `md-indent` (Jupyter) and `resize.width` / `resize.height` (latex-only; the sweep does not cover
  them).
- Refactoring the existing dotted reads onto the alias table.
- The `fig-pos='H'` latex default for Jupyter (finding 10).
- Silent staleness through `ref.label` (S8), the stored-versus-current declaration check, and
  KNR-4\'s inline typed values. Header removal (finding 8) is in scope only as the Phase 7.0 known-gap test;
  its fix is [KNR-3 slice 8](2026-11-04-knr-slice-8-header-removal.md) (decided 2026-11-04).
- List-valued class options for knitr (bd-s344ebc0), and `fig.sep`, `fig.ncol` and `fig.valign`
  layout synthesis (KNR-2b).

## Risks

- Jupyter output changes for the first time since the goldens froze it. Mitigation: the goldens set
  none of these options (finding 2), and the goldens script runs in Phase 7f.
- The typst smoke tests change output (finding 9). Mitigation: Phase 7f runs them.
- Departures from Quarto 1 on Jupyter are decisions, not accidents, and each is tested: D3 (knitr\'s
  attribute order, recycling by figure), D4 (`layout-align`), D5 (`cap-location`, string-only), D6
  (`out-*` cell options) and D10 (hash).
- Header removal is stale for every presented knitr option (finding 8); it is pinned, not fixed.
- The first run after upgrade re-runs a stored Jupyter cell that sets an option D10 moves out of the
  hash.
- D1\'s layering order is the part most likely to hide a mistake (it replaces an existing rewrite
  for the aliased names); Phase 7a tests each step separately.
