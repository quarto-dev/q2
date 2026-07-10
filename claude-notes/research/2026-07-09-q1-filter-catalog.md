# Quarto 1 Lua filter → Quarto 2 pipeline catalog

**Date:** 2026-07-09
**Status:** Complete — all 138 built-in filters cataloged (11 stage groups), 10% evidence spot-check passed
**Purpose:** Systematically map every built-in Quarto 1 Lua filter (~138 files across
8 stage dirs) onto its Quarto 2 (Rust) equivalent, to (a) find genuine porting gaps
for the formats Q2 emits today (HTML + revealjs), and (b) surface where built-in work
clusters into seams that might motivate **new user-filter injection points** in the Q2
filter pipeline.

This is research, not a work plan. Findings that warrant action are called out in the
synthesis; turning them into tracked work is a separate decision.

---

## The Q2 pipeline (three nested layers)

Q2 does **not** have Q1's ~7 internal filter groups + 8 user entry points. It has:

### Layer 1 — macro `PipelineStage`s (`quarto-core/src/pipeline.rs:277`, `build_html_pipeline_stages_with_options`)
```
ParseDocument → MetadataMerge → IncludeExpansion → IncludeResolve → ListingItemInfo
→ DocumentProfile → LinkResolution → UnwrapProfile → PreEngineSugaring → EngineExecution
→ CompileThemeCss → BootstrapJs → ClipboardJs → AttributionGenerate
→ [UserFiltersStage::pre]  →  AstTransformsStage  →  [UserFiltersStage::post]
→ ResourceReport → CodeHighlight → MathJs → RenderHtmlBody → ApplyTemplate
```

### Layer 2 — the two Lua user-filter stages **bracket** the transform pipeline
`UserFiltersStage::pre()` runs *before* all AST transforms, `::post()` *after*. They are
**not** interleaved with the transforms. `filter_resolve.rs` maps Q1's 8 user entry points
(`pre-ast`/`post-ast`/`pre-quarto`/`post-quarto`/`pre-render`/`post-render`/`pre-finalize`/
`post-finalize`) onto exactly these two positions via the `quarto` sentinel + explicit `at:`.
The one exception that runs Lua *inside* the transforms is `ShortcodeResolveTransform`.

### Layer 3 — 40 AST transforms in 4 declared phases (`build_transform_pipeline`, `pipeline.rs:1159`)
Ordering is insertion order; phases are declared via `fn phase()` and enforced
non-decreasing by `test_build_transform_pipeline_phase_ordering`. Contract:
`claude-notes/designs/transform-pipeline-phases.md`.

- **Normalization** — format-agnostic semantic sugar (callout, theorem, proof, float, equation, metadata-normalize, footnotes, code-block-generate, sectionize, title-block) + format scaffolding that doesn't consume crossref (reveal-slides/columns/footnotes).
- **Crossref** — `crossref-index`, `crossref-resolve` (assign numbers, rewrite `@refs` to custom nodes).
- **Navigation** — TOC / navbar / sidebar / page-nav / footer / listings (generate then render).
- **Finalization** — render custom nodes to writer-visible shapes (`crossref-render`, `code-block-render`, `example-embed-render`), then presentation that consumes them (`reveal-auto-stretch`), then resource/attribution baking.

### The reframe
The 2026-03-16 extensions plan collapsed Q1's 8 **user entry points** → 2 positions.
That was correct *for user/extension filters*. But the ~138 **built-in** filters map onto
the much richer target above. Group-level correspondence:

| Q1 group | Q2 target region |
|---|---|
| normalize | `native:parse` / `native:pampa-*` + a few Normalization transforms |
| quarto-init | macro `stage:*` (includes, metadata init, resource refs) |
| quarto-pre | Normalization-phase transforms (+ native, + customnodes) |
| crossref | Crossref-phase transforms + float/theorem/proof/equation sugar |
| layout | Navigation + Finalization; heavily format-specific |
| quarto-post | Finalization; heavily format-specific |
| quarto-finalize | tail `stage:*` (resource-report, dependencies, mediabag) |
| customnodes | `customnode:*` definitions |

---

## Status vocabulary

- **ported** — Q2 has a working equivalent (evidence = Q2 `file:line`)
- **partial** — some coverage; specifics missing
- **not-ported** — Q1 does this for a format Q2 **emits** (html/revealjs) but Q2 lacks it — *the seam-motivating set*
- **format-not-in-q2** — targets a format Q2 doesn't emit yet (latex/typst/docx/pptx/epub/jats/asciidoc/confluence/hugo/…) — a *writer* gap, not a filter-pipeline gap
- **obsolete** — Q1-engine machinery with no Q2 analog by design (emulated-filter scaffolding, filter-chain wiring)

`render.rs:626-633` hard-fails any `--to` target other than HTML/revealjs today, so
"format-not-in-q2" is a large and expected bucket.

---
## Catalog

Each row: **file** · **format scope** · **status** · **Q2 location (ported) / recommended landing (gap)**. `⚑` = new-seam signal (would need a user-filter injection point Q2 lacks, or a whole missing subsystem). Evidence `file:line` anchors are in the per-agent notes; representative ones kept inline.

### normalize/ (7)

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| astpipeline.lua | agnostic (raw-HTML) | partial | list-table→Table native (`postprocess.rs:325`); float/filename ported. **Raw-HTML `<table>`/`<pre>`→AST-Table (a11y roles) not-ported**; `forward_cell_subcaps` not-ported |
| capturereaderstate.lua | agnostic | obsolete | Q1 Meta round-trip for reader opts; Q2 passes opts as args |
| draft.lua | html | partial | only bare `draft:` bool captured (`document_profile.rs:313`, used for sidebar filtering). **`draft-mode: gone` doc-hiding + `<meta quarto:status>` not-ported**; `drafts:` path-list unread |
| extractquartodom.lua | agnostic / latex | obsolete / format-not-in-q2 | qmd-in-HTML embedding needless (real AST + structured CustomNodes); latex-vault is writer gap |
| fixupdatauri.lua | agnostic | obsolete | patches a Pandoc-reader bug Q2's native reader doesn't have |
| flags.lua | agnostic | obsolete | Q1 skip-flag precompute; Q2's 40 named transforms self-gate |
| normalize.lua | agnostic (+jats) | partial | `pagetitle` only (`metadata_normalize.rs:59`). **`ensureMetaInlines` block→inline coercion not-ported → live nested-`<p>` subtitle bug** (`template.rs:220,677`). Author/license/citation model deferred (own epic). Shortcode-in-metadata not resolved (`shortcode_resolve.rs:891` walks blocks only) |

### quarto-init/ (6)

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| configurefilters.lua | agnostic | obsolete | runtime-toggleable filter chains; Q2 pipeline is fixed per format |
| includes.lua | agnostic | **ported** | `stage:IncludeResolveStage` (`pipeline.rs:220,286`) |
| knitr-fixup.lua | agnostic | not-ported | `knitsql-table`→`cell-output-display` fixup; → post-`EngineExecutionStage`. Low priority (SQL chunks only) |
| metainit.lua | agnostic | partial | includes+custom-crossref ported; general crossref option defaults hardcoded (`crossref_render.rs:27`) |
| quarto-init.lua | agnostic | partial | includes ported; resourceRefs half not-ported |
| resourcerefs.lua | agnostic | not-ported | per-included-file `Image.src`/raw-HTML path rebasing → extend `stage:IncludeExpansionStage`. **Real bug**: relative paths in `{{< include >}}` content resolve against wrong dir |

### quarto-pre/ (36)

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| bibliography-formats.lua | bibtex/biblatex/csljson | format-not-in-q2 | — |
| book-links.lua | single-file-book | format-not-in-q2 | no chapter-merge mode in Q2 |
| book-numbering.lua | typst/latex/epub + book | format-not-in-q2 / not-ported ⚑ | needs per-chapter "book item" context Q2 lacks |
| code-annotation.lua | agnostic (+ presentation) | not-ported | → `code-block-generate` (parse) + new Finalization render (DL) |
| code-filename.lua | agnostic | partial | ported via `code_block_decorations` sideband (`code_block_render.rs:187`). Missing Div-wraps-CodeBlock rule |
| contentsshortcode.lua | agnostic | not-ported ⚑ | `{{< contents id >}}` needs two-pass doc-wide shortcode resolution; current `ShortcodeHandler` is single-pass |
| engine-escape.lua | agnostic | not-ported | backtick-escaped engine fences → likely `native:parse` (grammar); unconfirmed |
| figures.lua | html / latex | not-ported / format-not-in-q2 | `fig-alt`→`alt` propagation missing → `crossref-render`/`float-ref-target` |
| hidden.lua | agnostic | not-ported | `keep/remove/clear-hidden` class strip + note-strip → early Normalization |
| include-paths.lua | agnostic | not-ported | (same gap as resourcerefs) rebase spliced-content links → `IncludeExpansionStage` |
| input-traits.lua | agnostic | not-ported | `positioned-refs` flag; `appendix.rs:204` relocates `#refs` unconditionally → duplicate-bib risk |
| line-numbers.lua | agnostic (+revealjs) | not-ported | `code_block_generate.rs:210` comment already earmarks it |
| llms-code-annotations.lua | llms-txt | format-not-in-q2 | — |
| llms-conditional-content.lua | llms-txt | format-not-in-q2 | also blocked on content-hidden |
| meta.lua | pdf/latex | format-not-in-q2 | callout/tikz/bookmark preamble |
| options.lua | agnostic | partial | generic dotted-path reader obsolete; **`cap-location` per-scope feature not-ported** (blocks parsefiguredivs cap-location) |
| output-location.lua | revealjs | not-ported | per-cell code/output split → new reveal Normalization transform (`revealjs/columns.rs` precedent) |
| outputs.lua | pptx/gfm (output-divs:false) | format-not-in-q2 | inert on html/revealjs by default |
| panel-input.lua | html+bootstrap | not-ported | `.panel-input`→card classes → new Normalization transform |
| panel-layout.lua | html+bootstrap | not-ported | `.panel-fill/-center`→`panel-grid` bootstrap grid |
| panel-sidebar.lua | html+revealjs | not-ported | `.panel-sidebar` pairing; also needs Tabset CustomNode |
| parseblockreftargets.lua | agnostic | partial | theorem ported; **proof only `.proof`, missing `.remark`/`.solution`** (`proof.rs:8`) |
| parsefiguredivs.lua | agnostic (+latex) | partial | core Div/Figure/Table ported (`float_ref_target.rs`). **Missing: subfloat/subcap splitting, cap-location forwarding, `lst-cap` listings** (all TODO in source) |
| project-paths.lua | agnostic | not-ported | `/`-root-relative `Image.src`/`Link` rewrite → extend `link_rewrite.rs` (whose comment wrongly says Q1 doesn't rewrite images) |
| quarto-pre.lua | agnostic | obsolete | filter-list wiring = `build_transform_pipeline` |
| resolvescopedelements.lua | agnostic | not-ported | `tbl-colwidths` scoped resolution → new Normalization transform (only `ColWidth` type exists) |
| resourcefiles.lua | agnostic | **ported** | `transform:resource-collector` (`resource_collector.rs:57`); Q1 records, Q2 copies |
| results.lua | agnostic | obsolete | JSON side-channel for Q1's 2-process split; Q2 shares `RenderContext` in-memory |
| shiny.lua | html (python-shiny) | not-ported ⚑ | needs a Python-Shiny **engine**, not a filter seam |
| shortcodes-handlers.lua | agnostic | partial | only `meta` handler exists (`shortcode_resolve.rs:120`). **`var`/`env`/`pagebreak`/`brand`/`contents` missing** → add `ShortcodeHandler`s |
| table-captions.lua | agnostic (+latex) | not-ported ⚑ | `tbl-cap`/`tbl-subcap` cell-output propagation needs which-cell-produced-this context |
| table-classes.lua | agnostic | partial | caption-attr merge native; base bootstrap classing ported (`table_bootstrap_class.rs`). Missing short-name normalize (`sm`→`table-sm`) + float→table forwarding |
| table-colwidth.lua | n/a | obsolete | 100% commented-out dead code (logic moved to modules/) |
| table-rawhtml.lua | html | not-ported | flextable raw-HTML merge + gt-CSS `:where()` respecify → Normalization; gated on R engine emitting such output |
| tableattributes.lua | agnostic | **ported** | `native:parse` caption-attr extraction (`caption.rs:16`, `postprocess.rs:1556`) |
| theorems.lua | agnostic | **ported** | header consumed into `customnode:Theorem` title (no unnumbered marker needed) |

### crossref/ (15)

Architecturally **done** at the pipeline-shape level (`crossref-index`→`crossref-resolve`→`crossref-render` + Normalization sugar). Gaps are missing *logic within* existing transforms, not missing seams — **no `⚑` in this group**.

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| crossref-standalone.lua | agnostic | obsolete | filter-chain wiring |
| crossref.lua | agnostic | obsolete | IDE autocomplete chain wiring |
| custom.lua | agnostic / latex | partial | category registration ported (`metadata.rs:96`, `registry.rs:167`); `caption-prefix`/`caption-location`/`space-before-numbering` unread; latex-inject format-not-in-q2 |
| equations.lua | agnostic | partial | `equation-label`→index→render ported (`equation_label.rs`, `crossref_render.rs:605`); non-mathjax `(N)` fallback + latex/typst missing |
| figures.lua | agnostic | partial | `crossref_index.rs:250` order assign ported; **subfloat/`nextSubrefOrder` missing** (`parent:None`, :304) |
| format.lua | agnostic | partial | **biggest "wired but shallow" gap**: numbering machinery ported, but styles/custom titles/prefixes/delims/chapter-numbers all absent — `crossref_render.rs` hardcodes English/arabic |
| index.lua | agnostic | partial | `CrossrefIndex` ported (`index.rs:29`); `writeIndex` external-file (multi-file) deferred; `number-offset` seeding absent |
| meta.lua | pdf/latex | format-not-in-q2 | LaTeX preamble |
| options.lua | agnostic | partial | no generic option-bag; `chapters`/`ref-hyperlink`/`title-delim`/… unread |
| preprocess.lua | agnostic | not-ported | `crossref_mark_subfloats` → extend `float-ref-target`; root cause of subfloat gaps |
| qmd.lua | agnostic | **ported** | `crossref/codeblock_shorthand.rs:8` (AST rewrite pre-engine, cleaner than Q1 regex) |
| refs.lua | agnostic | partial | `Cite`→resolved-ref ported (`crossref_resolve.rs:184`, `crossref_render.rs:661`); multi-crossref cite drops all but first; no `ref-hyperlink`/`sec`/chapter prefixes; latex/asciidoc/typst format-not-in-q2 |
| sections.lua | agnostic (+epub) | partial | section-counter ported/tested (`crossref_index.rs:212,336`). **`@sec-` refs + `number-sections` numbering not-ported** (user-visible) |
| tables.lua | agnostic / html+latex | partial | native-Table path ported (`float_ref_target.rs:235`); raw-HTML/raw-LaTeX table caption extraction missing |
| theorems.lua | agnostic | **ported** | `theorem`/`proof` sugar + render ported (`crossref_render.rs:325,538`); latex/jats format-not-in-q2 |

### layout/ (24)

Dominated by the format axis — **13 files target formats Q2 doesn't emit** (`render.rs:626` hard-fails non-html/revealjs). The real HTML surface is the **panel-layout subsystem** (`layout.lua`+`width.lua`+`html.lua`), entirely unported.

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| asciidoc.lua | asciidoc | format-not-in-q2 | — |
| cites.lua | pdf/latex | format-not-in-q2 | margin-citations |
| columns-preprocess.lua | agnostic | not-ported | scoped column/caption class resolution → extend `float-ref-target` |
| columns.lua | html / typst / latex | not-ported / format-not-in-q2 | HTML `.aside`→`.column-margin`/`margin-aside` missing (CSS vendored but never triggered — a testing trap) |
| confluence.lua | confluence | format-not-in-q2 | — |
| docx.lua | docx/odt | format-not-in-q2 | — |
| epub.lua | — | obsolete | 0-byte file |
| figures.lua | agnostic | not-ported | extended-figure decision; no consumer exists |
| html.lua | html | partial | basic Figure/caption via `crossref_render.rs:220`. **No multi-cell `PanelLayout` at all**; no `quarto-figure`/align/`cap-location` |
| hugo.lua | hugo/gfm | format-not-in-q2 | — |
| ipynb.lua | ipynb (output) | format-not-in-q2 | notebook-as-output not a Q2 concept |
| jats.lua | jats | format-not-in-q2 | — |
| latex.lua | pdf/latex/beamer | format-not-in-q2 | — |
| layout.lua | agnostic | not-ported | **core panel-layout engine** (`layout-ncol/-nrow/[[…]]`) → new `panel-layout-render` Finalization transform (after crossref-render). Biggest HTML gap |
| lightbox.lua | html | not-ported | glightbox wrap + JS dep → Finalization transform + JS macro stage |
| manuscript.lua | agnostic(jats-excl) / wp+latex | not-ported / format-not-in-q2 | `manuscriptUnroll` blocked on manuscript-project notebook-embed |
| meta.lua | pdf/latex, typst | format-not-in-q2 | preamble/geometry |
| odt.lua | odt | format-not-in-q2 | — |
| pandoc3_figure.lua | html / latex / typst | partial | HTML core covered natively (`html.rs:1530`); linked-figure div, class-forward, reveal-fragment edge cases missing |
| pptx.lua | pptx | format-not-in-q2 | (also dead: no caller in Q1) |
| table.lua | docx/odt/wp | format-not-in-q2 | word-processor panel table (not shared w/ HTML) |
| typst.lua | typst | format-not-in-q2 | — |
| width.lua | agnostic | not-ported | width math for panel-layout; lands with `layout.lua` |
| wp.lua | docx/odt | format-not-in-q2 | — |

### quarto-finalize/ (8)

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| book-cleanup.lua | agnostic(+fmt) | not-ported ⚑ | blocked on multi-chapter book-merge feature (doesn't exist) |
| coalesceraw.lua | pdf/latex | format-not-in-q2 | raw-node merge (latex only) |
| dependencies.lua | agnostic | **ported** | drained per-filter (`user_filters.rs:196`, `dependency.rs:34`) instead of end-of-chain file |
| descaffold.lua | agnostic | obsolete | Q2 CustomNodes are real types; no scaffold round-trip |
| finalize-combined-1.lua | agnostic | obsolete | perf-merge of coalesceraw+descaffold |
| mediabag.lua | agnostic | not-ported | mediabag API exists (`lua/mediabag.rs`) but nothing drains it to disk + rewrites `Image.src` → extend `UserFiltersStage-post` drain |
| meta-cleanup.lua | native/json | format-not-in-q2 | Pandoc AST-dump target |
| typst.lua | typst | format-not-in-q2 | (also unfinished in Q1) |

### customnodes/ (12)

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| callout.lua | agnostic(+fmt) | **ported** | `customnode:Callout`, `callout.rs`/`callout_resolve.rs`. (revealjs/epub DOM variant not branched — fidelity check) |
| content-hidden.lua | agnostic | not-ported | `content-visible/-hidden` + `when/unless-format/meta/profile` — **entirely absent**; needs profile plumbing → Normalization transform |
| decoratedcodeblock.lua | agnostic(+fmt) | **ported** | sideband map, deliberate (`render.rs:350`) |
| floatreftarget.lua | agnostic(+fmt) | **ported** | `customnode:Float`, full crossref pipeline; ~9/11 format branches format-not-in-q2 |
| htmltag.lua | html | not-ported | leaf helper for tabset/panel; obviated by direct RawBlock construction |
| latexcmd.lua | pdf/latex | format-not-in-q2 | — |
| latexenv.lua | pdf/latex | format-not-in-q2 | — |
| panel-tabset.lua | html(+fmt) | not-ported | `.panel-tabset`→bootstrap nav-tabs → sugar+render pair (common feature!) |
| panellayout.lua | agnostic(+latex) | not-ported | multi-cell grid CustomNode (= layout.lua feature) |
| proof.lua | agnostic(+fmt) | **ported** | `customnode:Proof`, `proof.rs`, `crossref_render.rs:555` |
| theorem.lua | agnostic(+fmt) | partial | ported; **missing `algorithm`/`alg` class** (one-line add, `theorem.rs:61`) |
| shortcodes.lua | agnostic | **ported** | native `Inline::Shortcode` grammar variant + `shortcode_resolve.rs` (cleanest port) |

### quarto-post/ (30)

**17 of 30 are `format-not-in-q2`** (each a single top-level `is_format` guard). The 13 in-scope files split into ported-but-relocated, staged-unfinished, and absent.

| file | scope | status | Q2 location / recommendation |
|---|---|---|---|
| bibliography.lua | pdf/latex | format-not-in-q2 | bib-loc marker |
| book.lua | latex + agnostic | partial | title-block exists (`title_block.rs`); per-chapter title-block-with-license + `\markboth` not-ported |
| cell-renderings.lua | html/revealjs/typst | not-ported | `renderings:[light,dark]` selection → `code-block-render`; needs `brand-mode` param |
| cellcleanup.lua | agnostic | not-ported | strip empty `Div.cell` → early Normalization |
| cites.lua | agnostic | not-ported | book cross-chapter cite index → `ResourceReportStage` |
| code.lua | agnostic | not-ported | `clear-cell-options`; largely superseded by `cell_options/mod.rs:71` upstream stripping |
| dashboard.lua | dashboard | format-not-in-q2 | — |
| delink.lua | html | not-ported | `.delink` Link→Span → `listing-render` (only producer today) |
| docx.lua | docx | format-not-in-q2 | OpenXML callouts |
| email.lua | email | format-not-in-q2 | — |
| fig-cleanup.lua | agnostic | not-ported | strip synthetic `fig-anonymous-N` ids (may be obsolete if Q2 never assigns them) |
| foldcode.lua | html/revealjs | not-ported | **explicitly staged**: `code_block_render.rs:182` `// TODO` Phase 3 `<details>` fold |
| gfm.lua | gfm | format-not-in-q2 | — |
| html.lua | html/revealjs | partial | odd/even rows ported (native writer `html.rs:1501`), caption-top ported (`table_bootstrap_class.rs`). **fig-align/fig-alt/figure-wrap not-ported** (Q1's 1 filter → 2 Q2 layers + 1 gap) |
| ipynb.lua | ipynb | format-not-in-q2 | — |
| jats.lua | jats | format-not-in-q2 | — |
| landscape.lua | docx/latex/typst | format-not-in-q2 | (no html branch in Q1 either) |
| latex.lua | pdf/latex | format-not-in-q2 | tcolorbox/margin/float |
| latexdiv.lua | pdf/latex | format-not-in-q2 | (leaks stray `data-latex` attr to html — cosmetic) |
| meta.lua | latex + agnostic | obsolete | `quarto-filters` bookkeeping; Q2 doesn't stash filter list in Meta |
| ojs.lua | html (ojs) | not-ported ⚑ | needs OJS cell-exec + client-runtime **subsystem** (P3, deferred) |
| pdf-images.lua | pdf/latex | format-not-in-q2 | svg→pdf |
| pptx.lua | pptx | format-not-in-q2 | — |
| render-asciidoc.lua | asciidoc | format-not-in-q2 | — |
| responsive.lua | html/revealjs | not-ported | `img-fluid` + `table-responsive*` → Normalization; already flagged P0 in html-parity plan |
| reveal.lua | revealjs | not-ported | `.absolute`→CSS `style`, `show-notes` quote, blockquote-fragment fix → Finalization reveal transforms |
| tikz.lua | pdf/latex | format-not-in-q2 | — |
| typst-brand-yaml.lua | typst | format-not-in-q2 | — |
| typst-css-property-processing.lua | typst | format-not-in-q2 | — |
| typst.lua | typst | format-not-in-q2 | — |

---

## Synthesis

### Verification

The catalog was produced by 11 parallel agents classifying against a shared taxonomy, each required to cite Q2 `file:line` evidence for any "ported"/"partial" claim. A 10% spot-check of ported/evidence anchors (`theorem.rs:61`, `shortcode_resolve.rs:120`/287, `template.rs:221`/679, `pipeline.rs:296`, `resource_collector.rs:57`, `code_block_render.rs:182`, `caption.rs:16`, `crossref_resolve.rs:487`) confirmed every sampled anchor exists and says what was claimed. Findings are trustworthy.

### Distribution (138 files)

| status | count | % | meaning |
|---|---:|---:|---|
| **format-not-in-q2** | 44 | 32% | blocked on a *writer* (latex/typst/docx/pptx/epub/jats/asciidoc/hugo/gfm/ipynb/dashboard/email/llms/bibtex), not on filter work |
| **not-ported** | 43 | 31% | real gap for a format Q2 emits (html/revealjs) — *the actionable set* |
| **partial** | 25 | 18% | some Q2 coverage, specific behavior missing |
| **obsolete** | 14 | 10% | Q1-engine machinery Q2 needs by design (filter-chain wiring, skip-flags, JSON side-channels, scaffold round-trips, reader-state smuggling) |
| **ported** | 12 | 9% | working equivalent with cited evidence |

So **~59%** of built-in Q1 filters (ported + partial + not-ported − the writer-blocked ones) are *about* the formats Q2 ships; **~32%** are simply waiting on writers; **~10%** will never be needed because Q2's architecture (native tree-sitter reader, real Rust CustomNodes, single-process `RenderContext`, small named-transform pipeline) dissolves the problem they solved.

### The headline answer: do the built-in filters motivate new Lua *stages*?

**Overwhelmingly, no.** Of 43 not-ported + 25 partial files, all but a handful land inside an **existing** phase/stage — the recommendation column is almost always "extend transform X" or "new transform in phase Y," never "we need a new pipeline position." Q2's four-phase transform structure (Normalization → Crossref → Navigation → Finalization) plus its surrounding macro stages already have the right slots; they are just **unoccupied**. The gaps are *unwritten transforms* (features), not *missing seams*.

Only **6 files** raised a genuine `⚑` signal, and every one is a **missing subsystem or data source**, not a request for an intermediate user-filter injection point:

| ⚑ file | what it actually needs |
|---|---|
| `shiny.lua` | a Python-Shiny **engine** (subprocess cell→app), alongside `EngineExecutionStage` |
| `ojs.lua` | the whole OJS reactive-runtime subsystem (already triaged P3/deferred) |
| `book-cleanup.lua`, `book-numbering.lua` | a multi-chapter **book-merge** pipeline + per-chapter "book item" context object |
| `table-captions.lua` | which-executed-cell-produced-this-table provenance, available only right after `EngineExecutionStage` |
| `contentsshortcode.lua` | **two-pass** document-wide shortcode resolution (current `ShortcodeHandler` is single-pass) |

The one finding that genuinely touches the *filter-extension* seam question is **`contentsshortcode` + shortcode-in-metadata**: the `ShortcodeResolveTransform` resolves inline, single-pass, and only over `ast.blocks` (`shortcode_resolve.rs:891`). A `{{< contents >}}`-style relocation shortcode and `{{< meta … >}}` inside metadata both need capabilities the current resolver lacks. That's an argument for evolving the **shortcode resolver**, not for adding user-filter positions.

**Implication for the extensions plan.** The 2026-03-16 plan's 8→2 collapse for *user* filters remains sound: the built-in catalog gives no evidence that authors of built-in work needed intermediate positions, so it's weak evidence that *user* filters need them either. If a case for a mid-pipeline user-filter seam is ever made, it should come from a concrete extension use-case (e.g. "run after crossref numbering but before navigation"), which `bd-0fd0` already gestures at — not from this catalog.

### Where the actionable HTML/revealjs work clusters

The 43 not-ported + partial-gap items concentrate into a few coherent feature areas, most fitting one or two existing transforms:

1. **Panel layout** — the single biggest gap. `layout.lua`+`width.lua`+`layout/html.lua`+`customnodes/panellayout.lua`+`panel-tabset.lua`+`quarto-pre/panel-*.lua` together implement `layout-ncol`/`layout=[[…]]`/subfigure grids/tabsets/bootstrap panels — **zero** Q2 implementation, no `PanelLayout` concept at all. Lands as a new Finalization `panel-layout-render` transform (after crossref-render, since subfigure captions need numbers) + Normalization sugar. Common, user-visible.
2. **Crossref display-formatting** — numbering machinery is ported; the *presentation* layer isn't. `format.lua`/`options.lua`/`sections.lua`/`refs.lua`: custom prefixes/titles/`title-delim`, number styles, `chapters`, **`@sec-` refs, `number-sections` numbering**, multi-cite ref lists, `ref-hyperlink`. All concentrated in `crossref_render.rs` (which hardcodes English/arabic) + one missing metadata-options reader.
3. **Code cell features** — `line-numbers`, `code-annotation`, `foldcode` (`<details>`, TODO-staged), `cell-renderings` (light/dark). All fit the `code-block-generate`/`code-block-render` pair; two are already earmarked in source comments.
4. **Figure/image polish** — `fig-alt`→`alt`, `fig-align`/`quarto-figure` classing, `figure`-wrapping bare `Para>Image`, `columns`/`.aside`→`.column-margin` (CSS vendored but never triggered — a test trap), `lightbox`, `pandoc3_figure` edge cases, subfloat numbering.
5. **Content gating** — `content-visible`/`content-hidden` with `when/unless-format/meta/profile` is **entirely absent** (needs profile plumbing); `hidden`-class handling; `responsive` classes (P0-flagged); revealjs `.absolute`/`output-location`.
6. **Include/project path correctness** — see bugs below.

### Bugs & inaccuracies surfaced (independent of porting decisions)

1. **Live HTML-validity bug:** a `PandocBlocks`-valued `subtitle`/`title`/`abstract` renders as `<p>…</p>` inside `<p class="subtitle">` — nested `<p>` — because Q2 has no `ensureMetaInlines` block→inline coercion (`template.rs:221` + `titleblock_field_to_html`→`write_blocks_to` at :679). Q1's `normalize.lua` exists specifically to prevent this.
2. **Include path correctness gap:** relative `Image`/`Link` paths inside `{{< include child/sub.qmd >}}` content are not rebased when spliced into the root document (`IncludeExpansionStage` resolves the include's *own* path but not paths *within* it) — `resourcerefs.lua` / `include-paths.lua` / `project-paths.lua` all cover this; `![](img.png)` in an included file will resolve against the wrong directory.
3. **Stale/incorrect comment:** `link_rewrite.rs:29` states "images point at static resources … Q1 doesn't rewrite them either" — but Q1's `project-paths.lua` *does* rewrite `/`-root-relative image `src`. Worth correcting.
4. **Silent crossref drop:** a `Cite` bundling multiple crossref ids (`[@fig-a; @fig-b]`) resolves only the first and drops the rest (`crossref_resolve.rs:487`); Q1 joins all with `refDelim`.

### Suggested next steps (for the user to weigh — not opened as work)

- The two **bugs** (nested-`<p>`, include-path rebasing) are small, high-value, and independent of any porting roadmap.
- **Panel layout** and **crossref display-formatting** are the two largest coherent feature areas; each is a natural epic that fits existing phases.
- The `content-hidden`/profile subsystem and the shortcode-resolver evolution (two-pass + metadata) are the two items with architectural weight worth designing before implementing.
- `format-not-in-q2` items (44) should be treated as *writer* backlog, not filter backlog — they'll come "for free" (as porting targets) when/if each writer lands.

