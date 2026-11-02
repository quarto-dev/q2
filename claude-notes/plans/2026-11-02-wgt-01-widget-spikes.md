---
title: 'WGT-1: widget and dependency spikes'
date: 2026-11-02
description: 'A time-boxed set of spikes across Python, Julia, R and marimo widget packages that decides whether the engine-neutral dependency contract (option B) can be designed well. Defines the catalog, the three references (browser, Quarto 1, q2), the AMD and cross-engine experiments, and the gate for option B.'
status: draft  # not started; environments built 2026-11-02, no spike has run
---

**Part of:** [the widget epic](2026-11-02-widget-epic.md) (WGT-1, the gate for the contract in
[WGT-2](2026-11-02-wgt-02-dependency-contract.md); it was EXR-19 phase 0 until 2026-11-02).
Findings and proposals so far are in
[the design review](../research/2026-11-01-exr-19-design-review.md).

## Goal and the gate

**We want option B**: one engine-neutral dependency contract that accommodates every
strategy a widget or engine uses, enables an AMD loader (requirejs) only when something needs
it, and interoperates with other systems (Positron and Ark, the Jupyter ecosystem, marimo).

**We build it only if these spikes teach us enough to make it good.** A contract designed from
one derivation path (Quarto 1\'s Jupyter code) and a few plans would be a guess. So this phase
produces evidence, not a design, and it ends at a gate:

- **Pass:** the strategy table (below) is filled for every Tier 1 family; the AMD and
  cross-engine matrices are measured, not argued; a candidate AMD detector has been scored
  against the corpus; and each field proposed for B is needed by at least one measured row.
  Then B is designed, in a note that cites the table, and phase 1 of WGT-2 builds against it.
- **Fail:** if the rows are too heterogeneous to share a type, or the cross-engine collisions
  cannot be handled at the merge point, we say so, ship option A (derive-in-core `includes`
  text, plus a file manifest for knitr) and keep the table as the input for a later attempt.

Non-goals: designing B in this phase, fixing every inconsistency we find, or being foolproof.
The aim is to learn what the problems are, including the ones we cannot fix.

## Decisions already made (Gordon, 2026-11-02)

- Match Quarto 1 for Jupyter and Julia visibility: document-wide, hidden outputs still count
  (this is the baseline the spikes measure, not a design recommendation).
- Dependency bytes, where files exist, go in the record\'s `quarto_files`, because a frozen
  record must render without the original library (design review, finding 5 and "Decisions and
  proposals after discussion"); derived text is recomputed and never stored.
- Phase 1 ships as option A, narrowly, with a seam; option B is gated on this plan.
- Quarto 1 is one strategy, not the reference answer. Anywidget, htmlwidgets, marimo and
  others are strategies the system has to accommodate.

## Why multi-engine makes this harder

q2 documents can mix engines, and each engine has its own page strategy:

- **Jupyter and Julia** outputs are presented by core. Their dependencies are derived text
  (Quarto 1\'s rules) that become `includes`.
- **knitr** returns markdown plus `knit_meta` (HTML dependency files in the R library).
- **marimo** (the `quarto-marimo` extension, `external-sources/quarto-marimo`) compiles the
  whole document into one app, emits `marimo-quarto-island` custom elements with a base64url
  payload in a `data-` attribute, and injects its browser adapter once through
  `include-in-header` (`src/engine/projection.ts`, `AGENTS.md` "Runtime invariants"). It does
  not use ipywidgets. Third-party widgets enter through `mo.ui.anywidget`
  (`docs/tutorials/external_dependencies.qmd`). q2 treats it as a markdown-returning engine
  (`engine/replay.rs`, "knitr, marimo, …").

All of them meet in `EngineExecutionStage`, which appends each engine\'s includes and stores
each engine\'s dependency files (`stage/stages/engine_execution.rs:770-800`). Nothing there
knows that two engines may need incompatible things on one page. Gordon has seen this: a
document with marimo and Julia, where the Julia side did not want requirejs on the page.
The mechanism is not yet known; reproducing it is spike S4.

The collision classes we expect to find (hypotheses, to be confirmed or discarded):

1. **AMD versus global.** A UMD bundle with an AMD loader present registers as a module
   instead of setting a global, so a later plain script cannot find it.
2. **Same library, different versions.** jQuery, plotly.js, DataTables and Leaflet arrive from
   several engines at once (Python plotly and R plotly; `itables` and R `DT`; folium,
   ipyleaflet and R `leaflet`).
3. **Same custom element or global name** registered twice.
4. **ES-module scope** versus classic scripts that expect globals.
5. **Load order and timing**, for example a widget that runs before its loader.

## Environments (built 2026-11-02)

All three live under one directory and touch nothing global (no `~/.julia`, no site R library,
no system Python):

```text
/private/tmp/claude-502/-Users-gordon-src-q2/d0cb25c2-370c-435f-b0da-155c15136d28/scratchpad/widget-spikes/
  py/.venv           Python 3.12.12 (uv)
  r-lib              R 4.3.2 library (arm64 macOS binaries from CRAN)
  jl, julia-depot    Julia 1.12.7 project and its own depot
  logs               install logs
```

That directory is session scratch space and may be removed. The install steps are short; to
rebuild, repeat them with the package lists below.

```bash
S=<some directory>/widget-spikes
# Python: uv venv --python 3.12 $S/py/.venv, then install the Python list below
VIRTUAL_ENV=$S/py/.venv uv pip install -r requirements.txt six ipython_genutils 'marimo>=0.23.16'
# R: install into a private library with binaries only
R_LIBS_USER=$S/r-lib Rscript install.R        # options(pkgType = "binary")
# Julia: own depot, trailing colon keeps the system stdlib path
JULIA_DEPOT_PATH="$S/julia-depot:" julia --project=$S/jl -e 'using Pkg; Pkg.instantiate()'
```

To run a spike against an environment:

- Python: `QUARTO_PYTHON=$S/py/.venv/bin/python`.
- R: `R_LIBS_USER=$S/r-lib` (and `QUARTO_R` if a specific R is wanted).
- Julia: `JULIA_DEPOT_PATH="$S/julia-depot:"`. How the QuartoNotebookRunner worker picks the
  notebook\'s project is **unverified** (`QUARTO_JULIA_PROJECT` in `julia-engine.ts:453`
  selects the project that holds the runner, not the notebook\'s). Settling it is S0.

### The catalog

Eras are coarse and from memory; they are not checked against release dates. Tier 1 families
get the full treatment (S1 to S5); Tier 2 get capture only (S1 and S3, no browser matrix).

**Python (installed, imports checked):**

| Package | Version | Era | Strategy to test | Tier |
|---|---|---|---|---|
| ipywidgets | 8.1.9 | 2015+ | widget protocol, widget state, html-manager | 1 |
| tqdm (notebook) | 4.70.1 | 2015+ | ipywidgets user, tiny | 2 |
| bqplot | 0.13.1 | 2015+ | widget protocol, own bundle | 2 |
| ipyleaflet | 0.20.0 | 2017+ | widget protocol, Leaflet | 1 |
| ipympl | 0.10.0 | 2018+ | widget protocol, canvas | 2 |
| ipydatagrid | 1.4.0 | 2020+ | widget protocol, large | 2 |
| ipysheet | 0.7.0 | 2017+ | widget protocol (needs `six`) | 2 |
| ipycytoscape | 1.3.3 | 2019+ | widget protocol | 2 |
| ipyvolume | 0.6.3 | 2016+ | old widget protocol (needs `ipython_genutils`) | 2 |
| pythreejs | 2.4.2 | 2015+ | old widget protocol, WebGL | 2 |
| anywidget | 0.11.0 | 2023+ | ES module `_esm` in widget model | 1 |
| drawdata | 0.5.2 | 2023+ | anywidget user | 2 |
| jupyter-scatter | 1.0.1 | 2022+ | anywidget user, WebGL | 2 |
| lonboard | 0.17.0 | 2023+ | anywidget user, deck.gl | 2 |
| plotly | 7.1.0 | 2012+ | inline bundle or CDN, MIME types, AMD history | 1 |
| bokeh | 3.10.1 | 2013+ | own loader, MIME types | 1 |
| altair | 6.3.0 | 2016+ | Vega-Lite MIME, vega-embed from CDN | 1 |
| folium | 0.20.0 | 2013+ | CDN, iframe or srcdoc | 1 |
| itables | 2.9.1 | 2019+ | DataTables, loader changes by version | 1 |
| pydeck | 0.9.3 | 2018+ | deck.gl, CDN | 2 |
| py3Dmol | 2.5.5 | 2014+ | CDN with AMD fallback | 1 |
| mpld3 | 0.5.12 | 2013+ | d3, requirejs-era | 2 |
| pyvis | 0.3.2 | 2018+ | vis-network, iframe | 2 |
| holoviews | 1.23.2 | 2014+ | framework MIME types and loaders | 1 |
| hvplot | 0.12.2 | 2018+ | holoviews and bokeh user | 2 |
| panel | 1.10.0 | 2018+ | own comm and extension loader | 1 |
| pandas | 3.1.0 | any | plain `text/html` tables with inline CSS | 1 |
| great-tables | 1.1.0 | 2023+ | plain HTML tables | 2 |
| matplotlib | 3.11.2 | any | static control | 2 |
| marimo | 0.25.2 | 2023+ | engine-declared `include-in-header`, custom elements, `mo.ui.anywidget` | 1 |

Baselines with no package, one fixture each: raw `IPython.display.HTML`, `Javascript` and
`IFrame` (Python); `htmltools::tags` (R); `HTML("...")` and `HypertextLiteral` (Julia). They
show the least a producer can emit.

**R (installed under `r-lib`, versions as resolved 2026-11-02):**

| Package | Version | Era | Strategy to test | Tier |
|---|---|---|---|---|
| htmlwidgets | 1.6.4 | 2014+ | the HTML dependency model | 1 |
| plotly | 4.11.0 | 2014+ | htmlwidgets, large bundle, AMD guard needs | 1 |
| leaflet | 2.2.2 | 2015+ | htmlwidgets, file dependencies | 1 |
| DT | 0.33 | 2014+ | htmlwidgets, jQuery and DataTables | 1 |
| dygraphs | 1.1.1.6 | 2015+ | htmlwidgets, small | 2 |
| visNetwork | 2.1.2 | 2015+ | htmlwidgets | 2 |
| networkD3 | 0.4.1 | 2015+ | htmlwidgets, d3 | 2 |
| threejs | 0.3.4 | 2015+ | htmlwidgets, WebGL | 2 |
| highcharter | 0.9.4 | 2016+ | htmlwidgets, Highcharts | 2 |
| echarts4r | 0.4.5 | 2018+ | htmlwidgets, ECharts | 2 |
| reactable | 0.4.4 | 2019+ | htmlwidgets with reactR | 1 |
| reactR | 0.6.1 | 2018+ | React dependency | 2 |
| gt | 1.0.0 | 2018+ | HTML table with inline CSS | 1 |
| kableExtra | 1.4.0 | 2017+ | HTML table with dependencies | 2 |
| formattable, htmlTable, sparkline | 0.2.1, 2.4.3, 2.0 | mixed | HTML and widget dependencies | 2 |
| DiagrammeR | 1.0.11 | 2015+ | graphviz and mermaid | 2 |
| ggiraph | 0.8.13 | 2016+ | SVG plus script | 2 |
| timevis, scatterD3, r2d3 | 2.1.0, 1.0.1, 0.2.6 | mixed | htmlwidgets | 2 |
| rgl | 1.3.24 | 2000s+ | `rgl::rglwidget()`; the separate `rglwidget` package is gone from CRAN | 2 |
| crosstalk, bslib | 1.2.1, 0.9.0 | 2016+ | shared dependencies | 2 |

**Julia (own depot; all 16 load, resolved 2026-11-02):**

| Package | Version | Era | Strategy to test | Tier |
|---|---|---|---|---|
| Plots (plotly backend), PlotlyBase, PlotlyJS | 1.41.7, 0.8.23, 0.18.18 | 2015+ | AMD loader cell plus plot cell (QuartoNotebookRunner extension); inline or CDN | 1 |
| WGLMakie, Bonito | 0.13.16, 4.2.0 | 2020+ | WebGL, its own JS and websocket layer | 1 |
| CairoMakie | 0.15.16 | any | static control | 2 |
| Gadfly | 1.4.1 | 2012+ | d3, old JS output | 2 |
| VegaLite, VegaDatasets | 3.3.1, 2.1.1 | 2018+ | Vega-Lite MIME | 1 |
| WebIO, Interact | 0.8.21, 0.10.5 | 2017+ | WebIO loader (requirejs-era) | 2 |
| StatsPlots | 0.15.8 | 2018+ | Plots user | 2 |
| DataFrames, PrettyTables | 1.8.2, 3.5.0 | any | plain HTML tables | 1 |
| HypertextLiteral, Observables | 1.0.0, 0.5.5 | 2020+ | hand-written HTML, observables | 2 |

Left out on purpose: ipywidgets 7 (conflicts with 8, an optional second environment);
keplergl, cufflinks, pygwalker, solara (broken, heavy or separate runtimes); d3heatmap and
rbokeh (archived); server modes of Bokeh and Panel (static notebook output only).

### Strategies the catalog is meant to cover

These are hypotheses until the spikes fill the table:

1. Inline bundle in the output (self-contained).
2. CDN loader tags.
3. AMD or requirejs.
4. Widget protocol: `widget-view+json`, notebook-level widget state, a manager script.
5. anywidget ES modules.
6. htmlwidgets: dependency files copied to `libs/`, a `staticRender` call.
7. Framework MIME types and loaders (HoloViews, Bokeh, Panel; exact MIME names to be read, not
   assumed).
8. Custom elements with an engine-declared adapter (marimo).
9. Iframe or `srcdoc`.
10. A static fallback for non-HTML formats.

## Methodology: three references

Quarto 1 is one strategy, so it is graded, not trusted.

1. **Ground truth: a browser.** Headless Chrome (Quarto 1 installed one; `quarto check` prints
   its path) loads the rendered page. Record whether the widget\'s DOM appears (svg, canvas,
   iframe, an expected element) and the console errors. File access flags may be needed for
   local `libs/` (`--allow-file-access-from-files`); if serving over HTTP is easier, do that.
2. **Quarto 1** (`quarto` on PATH is the 99.9.9 dev build): what it puts on the page.
3. **q2 today:** the baseline for the regression list. The `q2` on PATH is
   `0.36.0-nightly.20261027` and may predate the Julia flip. The flip is merged into
   `feature/execution-records-epic` (EXR-8 is complete), so for S4 and S5 build q2 from that tip or
   later. Check `q2 --version` against the flip commit first.

Positron and Ark are a fourth reference. EXR-16\'s S7 has run for plotly and leaflet
([the findings](../research/2026-11-02-exr-16-s7-widget-outputs-findings.md)); other families
would need the GUI again.

## Fixture layout

One fixture per family and language: a small `.qmd` with a single canonical widget cell.
Keep the fixtures (text, small) in `claude-notes/research/2026-11-02-exr-19-widget-spikes/`;
keep captures that contain library bytes outside git. A fixture directory has:

```text
<lang>-<family>/
  doc.qmd            the minimal document
  q1/                Quarto 1 render: page head, body slice, libs listing with sizes, kept ipynb or knit.md
  q2/                q2 render and its execution record (.quarto/execution-records/...)
  raw/               the engine\'s raw output: MIME bundle, or knit_meta and knit output
  browser/           console log and DOM probe results for each AMD variant
  notes.md           one paragraph: what surprised us
```

Visibility fixtures (S5): a widget with `echo: false`; a widget cell with `output: false` next
to a visible table; an R widget in an `include: false` chunk; a Julia Plotly loader cell that
is hidden while the plot is shown.

## The spikes

Time-box: one session per language, stop when the table is filled. The output of every spike
is rows in the tables, not code.

### S0. Harness and prerequisites

- [ ] Confirm each environment runs a one-cell document through Quarto 1: Python
  (`QUARTO_PYTHON`), R (`R_LIBS_USER`), Julia (how the worker selects the project; try a
  `Project.toml` beside the document and the `exeflags` front matter option, and record what
  works).
- [ ] Install the `marimo` Quarto extension from `external-sources/quarto-marimo/_extensions`
  into a scratch project. Note that its loader caches release artifacts and may need network.
- [ ] Script the browser probe: load a page in headless Chrome, report console errors and
  whether a given selector exists. Run it on a trivial page first.
- [ ] Check the available q2 build against the flip commit; record which fixtures can run
  through q2.

### S1. Quarto 1 baseline capture (all tiers)

- [ ] For every fixture, render to HTML with Quarto 1, with `keep-ipynb: true` (Jupyter and
  Julia) or the knit markdown (R), and save the page head, the widget region, the `libs/`
  listing with sizes, and the executed notebook.
- [ ] Record the dependency traits per row: loads an AMD loader, ships an ES module, uses a
  CDN, self-contained, size.
- [ ] For the widget-protocol rows (ipywidgets, bqplot, ipyleaflet, an anywidget fixture), save the
  notebook-level widget state JSON and measure its size and binary buffers. This is the input to
  [WGT-3](2026-11-02-wgt-03-ipywidgets.md).

### S2. Raw output capture (what a core presenter would see)

- [ ] For Jupyter and Julia fixtures, extract each output\'s MIME bundle (types, sizes, and
  whether the payload contains `require(`, `define(`, a CDN URL, an ES import, or widget
  state). For knitr fixtures, extract `knit_meta`.
- [ ] Confirm or correct the design review\'s Plotly claims (loader cell, the three
  `isPlotlyLibrary` patterns) against a real runner output, and check whether
  `nb.metadata.widgets` reaches the record.

### S3. AMD experiment (Tier 1)

- [ ] Render each Tier 1 page three ways: as Quarto 1 emits it (AMD loader present), with no AMD
  loader, and with the loader forced on. Run the browser probe on each.
- [ ] Classify every family as needs-AMD, indifferent, or breaks-with-AMD, with the console
  error that shows it.
- [ ] Score candidate detection rules for "emit requirejs only when needed" against the whole
  corpus, as a confusion table (false positives and false negatives per rule). Candidates to
  start with: any output containing `require(` or `define(`; the Quarto 1 rule (any
  `text/html`); the Plotly patterns; presence of a widget-view MIME type.

### S4. Cross-engine coexistence (Tier 1 pairs and one triple)

- [ ] Reproduce Gordon\'s marimo and Julia observation first. Record the page head, the
  console error, and which side failed.
- [ ] Render documents that combine families across engines and read which collision class
  occurs: Python plotly with R plotly; `itables` with R `DT`; folium or ipyleaflet with R
  `leaflet`; Julia Plotly with Python ipywidgets; marimo with each of the above; one triple.
- [ ] For each collision, note whether Quarto 1\'s name-only first-wins dedupe changed the
  result, and whether any rule at the merge point (`EngineExecutionStage`) could have
  prevented it.

### S5. Visibility and q2 comparison

- [ ] Run the visibility fixtures under Quarto 1 and record what each hidden case contributes.
- [ ] Where a runnable q2 build exists, render the Tier 1 fixtures and diff the page and the
  record against S1. This is the regression list for phase 1 and the parity row WGT-2 needs.

### S6. Synthesis and the option B gate

- [ ] One table: family by language, with the strategy, the page needs, the collision class and
  the AMD verdict.
- [ ] For each candidate field of a neutral dependency type (head text, CDN reference with
  attributes, inline text, local file, ES module, custom-element adapter, loader requirement,
  ordering, version or identity), name the rows that need it. A field with no row is cut.
- [ ] Record what cannot be known from this survey (future widget frameworks, other output
  formats, extension authors) and which decisions stay out of the type so it can change.
- [ ] Apply the gate in the section above, and write the result into the design review note
  as a decision for Gordon.

## What this plan does not settle

- Positron and Ark behavior beyond what S7 covered (plotly and leaflet only; no Python widgets, no
  other packages, no hand-seeded q2 record).
- How output formats other than HTML (revealjs, dashboards, Typst) consume dependencies.
- How a record-based freeze stores the bytes. Decided (Gordon, 2026-11-02): a frozen record
  must render a widget document without the original environment, so file-backed dependency
  bytes go in `quarto_files`; Quarto 1 freeze parity is not a requirement.
- Which widget frameworks will matter in five years. The catalog covers what exists now.

## Checklist

- [x] Build Python, R and Julia environments and smoke-test imports (Python 33 of 33 after adding `six` and `ipython_genutils`; R 29 of 30, the missing one is `rglwidget`, no longer on CRAN; Julia 16 of 16; about 3.9 GB total).
- [x] Add marimo to the catalog and the Python environment.
- [ ] S0 harness and prerequisites.
- [ ] S1 Quarto 1 baseline capture.
- [ ] S2 raw output capture.
- [ ] S3 AMD experiment and detector scoring.
- [ ] S4 cross-engine coexistence.
- [ ] S5 visibility and q2 comparison.
- [ ] S6 synthesis and the option B gate decision.
