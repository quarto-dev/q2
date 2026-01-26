# Lua Filter Chain Analysis for Rust Quarto

**Parent Plan**: `2026-01-24-html-rendering-parity.md`
**Parent Epic**: kyoto-6jv
**Beads Issue**: kyoto-dy3
**Created**: 2026-01-24
**Status**: In Progress

---

## Purpose

This document analyzes the TypeScript Quarto Lua filter chain to:
1. Understand the complete filter pipeline architecture
2. Classify each filter as format-agnostic or format-specific
3. Identify which filters affect HTML output
4. Guide the Rust Quarto implementation

## Pipeline Architecture

The Quarto Lua filter chain follows a conceptual model:

```
qmd ──pandoc_reader──> AST ──quarto-pre──> AST ──quarto-post──> AST ──pandoc_writer──> output ──postprocessors──> output
```

The key insight from the Quarto author:
- **Pre-filters** (quarto-pre) _should_ be format-agnostic
- **Post-filters** (quarto-post) are explicitly format-specific
- Some pre-filters may contain format-specific code (opportunity for improvement)

## Filter Execution Order

From `main.lua`, the actual execution order is:

```
1. pre-ast         (user entry point)
2. quarto_init_filters
3. quarto_normalize_filters
4. post-ast        (user entry point)
5. pre-quarto      (user entry point)
6. quarto_pre_filters
7. quarto_crossref_filters  (if enabled)
8. post-quarto     (user entry point)
9. pre-render      (user entry point)
10. quarto_layout_filters
11. quarto_post_filters
12. post-render    (user entry point)
13. pre-finalize   (user entry point)
14. quarto_finalize_filters
15. post-finalize  (user entry point)
```

User filters can be injected at any entry point.

---

## Filter Group 1: quarto_init_filters

**Purpose**: Initialize Quarto metadata and state before normalization.

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `init-quarto-meta-init` | `quarto-init/metainit.lua` | ? | ? | Initialize Quarto metadata structures |
| `init-quarto-custom-meta-init` | inline | ? | ? | Initialize content_hidden metadata |
| `init-metadata-resource-refs` | combined: `filemetadata.lua`, `resourcerefs.lua` | ? | ? | File metadata and resource references |
| `init-knitr-syntax-fixup` | `quarto-init/knitr-fixup.lua` | Yes | No | Fix knitr output syntax (conditional on engine=knitr) |

### Analysis Needed
- [ ] Read `quarto-init/metainit.lua` to understand meta initialization
- [ ] Read `quarto-init/resourcerefs.lua` to understand resource reference handling
- [ ] Read `quarto-init/knitr-fixup.lua` to understand knitr fixups

---

## Filter Group 2: quarto_normalize_filters

**Purpose**: Normalize the document into a "Quarto AST" ready for processing. User filters after this point see the normalized Quarto AST (e.g., Figure nodes become FloatRefTarget custom nodes).

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `normalize-draft` | `normalize/draft.lua` | ? | ? | Handle draft document processing |
| `normalize` | `normalize/normalize.lua` | ? | ? | Main normalization filter |
| `normalize-capture-reader-state` | `normalize/capturereaderstate.lua` | ? | ? | Capture Pandoc reader state |
| (plus `quarto_ast_pipeline()`) | `normalize/astpipeline.lua` | ? | ? | AST pipeline transformations |

### Analysis Needed
- [ ] Read `normalize/normalize.lua` - this is likely critical
- [ ] Read `normalize/astpipeline.lua` - understand custom AST nodes
- [ ] Read `normalize/draft.lua` - draft document handling
- [ ] Understand FloatRefTarget and other custom nodes

---

## Filter Group 3: quarto_pre_filters

**Purpose**: Format-agnostic pre-processing. Should apply equally to all formats.

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `flags` | `normalize/flags.lua` | Yes | Yes | Compute document flags |
| `pre-server-shiny` | `quarto-pre/shiny.lua` | ? | ? | Shiny server setup |
| `pre-read-options-again` | `quarto-pre/options.lua` | Yes | Yes | Re-read options after user filters |
| `pre-bibliography-formats` | `quarto-pre/bibliography-formats.lua` | ? | ? | Bibliography format handling |
| `pre-shortcodes-filter` | `quarto-pre/shortcodes-handlers.lua` | Yes | Yes | Process shortcodes ({{< ... >}}) |
| `pre-contents-shortcode-filter` | `quarto-pre/contentsshortcode.lua` | Yes | Yes | Process contents shortcode |
| `strip-notes-from-hidden` | inline | Yes | Yes | Remove notes from hidden content |
| `pre-combined-hidden` | combined: `hidden.lua`, `content-hidden.lua` | **MAYBE** | Yes | Hidden/conditional content |
| `pre-table-captions` | `quarto-pre/table-captions.lua` | Yes | Yes | Table caption processing |
| `pre-code-annotations` | `quarto-pre/code-annotation.lua` | **MAYBE** | Yes | Code annotations (complex!) |
| `pre-code-annotations-meta` | `quarto-pre/code-annotation.lua` | Yes | Yes | Code annotation metadata |
| `pre-unroll-cell-outputs` | `quarto-pre/outputs.lua` | Yes | Yes | Unroll cell outputs |
| `pre-output-location` | `quarto-pre/output-location.lua` | Yes | Yes | Output location (column-*) |
| `pre-scope-resolution` | `quarto-pre/resolvescopedelements.lua` | Yes | Yes | Resolve scoped elements |
| `pre-combined-figures-theorems-etc` | (combined, see below) | **MIXED** | Yes | Large combined filter |
| `pre-quarto-pre-meta-inject` | `quarto-pre/meta.lua` | Yes | Yes | Inject Quarto metadata |
| `pre-write-results` | `quarto-pre/results.lua` | Yes | Yes | Write results to metadata |

### pre-combined-figures-theorems-etc (expanded)

This is a combined filter with multiple components:

| Component | Source File | Format-Agnostic? | HTML Impact | Description |
|-----------|-------------|------------------|-------------|-------------|
| `file_metadata()` | `common/filemetadata.lua` | Yes | Yes | File metadata handling |
| `index_book_file_targets()` | `quarto-pre/book-links.lua` | Yes | Yes | Book file target indexing |
| `book_numbering()` | `quarto-pre/book-numbering.lua` | Yes | Yes | Book numbering |
| `include_paths()` | `quarto-pre/include-paths.lua` | Yes | Yes | Include path resolution |
| `resource_files()` | `quarto-pre/resourcefiles.lua` | Yes | Yes | Resource file handling |
| `quarto_pre_figures()` | `quarto-pre/figures.lua` | **MAYBE** | Yes | Figure processing |
| `quarto_pre_theorems()` | `quarto-pre/theorems.lua` | **MAYBE** | Yes | Theorem processing |
| `docx_callout_and_table_fixup()` | ? | **NO** (docx) | No | DOCX-specific fixup (format-specific!) |
| `engine_escape()` | `quarto-pre/engine-escape.lua` | Yes | No | Engine escape sequences |
| `line_numbers()` | `quarto-pre/line-numbers.lua` | **MAYBE** | Yes | Line number handling |
| `bootstrap_panel_input()` | `quarto-pre/panel-input.lua` | **MAYBE** | Yes | Bootstrap panel input |
| `bootstrap_panel_layout()` | `quarto-pre/panel-layout.lua` | **MAYBE** | Yes | Bootstrap panel layout |
| `bootstrap_panel_sidebar()` | `quarto-pre/panel-sidebar.lua` | **MAYBE** | Yes | Bootstrap panel sidebar |
| `table_respecify_gt_css()` | ? | **MAYBE** | Yes | GT table CSS handling |
| `table_classes()` | `quarto-pre/table-classes.lua` | Yes | Yes | Table class handling |
| `input_traits()` | `quarto-pre/input-traits.lua` | Yes | Yes | Input traits |
| `resolve_book_file_targets()` | `quarto-pre/book-links.lua` | Yes | Yes | Resolve book targets |
| `project_paths()` | `quarto-pre/project-paths.lua` | Yes | Yes | Project path handling |

### Filters Potentially Format-Specific in Pre-Stage

Several "pre" filters may have format-specific code:
1. `docx_callout_and_table_fixup()` - Clearly DOCX-specific
2. `bootstrap_panel_*` - Bootstrap implies HTML
3. `line_numbers()` - Rendering differs by format
4. `code-annotation.lua` - Complex, has format branches

### Detailed Analysis: code-annotation.lua

**File**: `quarto-pre/code-annotation.lua` (~600 lines)

**Purpose**: Process code annotations like `# <1>` in code blocks and convert to definition lists.

**Format-Agnostic Parts**:
- Parsing annotations from code comments
- Matching annotation numbers to ordered list items
- Creating definition list structure

**Format-Specific Parts**:
- `processLaTeXAnnotation()` - Creates LaTeX-specific placeholders
- `processAsciidocAnnotation()` - Creates AsciiDoc callout markers
- `processAnnotation()` (generic) - Just strips annotation markers
- HTML output uses `<span>` with `data-code-cell-*` attributes

**Rust Strategy**:
- AST transform for annotation parsing (format-agnostic)
- HTML postprocessor for `<span>` decoration (format-specific)

### Analysis Needed
- [x] Read `quarto-pre/code-annotation.lua` - complex filter (DONE - see above)
- [ ] Read `quarto-pre/parsefiguredivs.lua` - figure parsing
- [ ] Read `quarto-pre/hidden.lua` - hidden content
- [ ] Read `quarto-pre/panel-*.lua` - panel handling
- [ ] Verify which are truly format-agnostic

---

## Filter Group 4: quarto_crossref_filters

**Purpose**: Cross-reference processing. Should be format-agnostic conceptually, but rendering may differ.

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `crossref-preprocess-floats` | `crossref/preprocess.lua` | Yes | Yes | Mark subfloats for crossref |
| `crossref-preprocessTheorems` | `crossref/theorems.lua` | Yes | Yes | Preprocess theorems |
| `crossref-combineFilters` | combined (see below) | **MIXED** | Yes | Combined crossref processing |
| `crossref-resolveRefs` | `crossref/refs.lua` | Yes | Yes | Resolve references |
| `crossref-crossrefMetaInject` | `crossref/meta.lua` | Yes | Yes | Inject crossref metadata |
| `crossref-writeIndex` | `crossref/index.lua` | Yes | No | Write crossref index |

### crossref-combineFilters (expanded)

| Component | Source File | Format-Agnostic? | HTML Impact | Description |
|-----------|-------------|------------------|-------------|-------------|
| `file_metadata()` | `common/filemetadata.lua` | Yes | Yes | File metadata |
| `qmd()` | `crossref/qmd.lua` | Yes | Yes | QMD crossref handling |
| `sections()` | `crossref/sections.lua` | Yes | Yes | Section numbering |
| `crossref_figures()` | `crossref/figures.lua` | **MAYBE** | Yes | Figure crossrefs |
| `equations()` | `crossref/equations.lua` | **MAYBE** | Yes | Equation crossrefs |
| `crossref_theorems()` | `crossref/theorems.lua` | **MAYBE** | Yes | Theorem crossrefs |
| `crossref_callouts()` | ? | Yes | Yes | Callout crossrefs |

### Analysis Needed
- [ ] Read `crossref/refs.lua` - reference resolution
- [ ] Read `crossref/format.lua` - check for format-specific code
- [ ] Read `crossref/figures.lua` - figure reference handling
- [ ] Read `crossref/equations.lua` - equation handling (LaTeX vs HTML)

---

## Filter Group 5: quarto_layout_filters

**Purpose**: Layout processing. This is where column layouts, panels, and positioning happen.

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `manuscript` | `layout/manuscript.lua` | ? | ? | Manuscript layout |
| `manuscriptUnroll` | `layout/manuscript.lua` | ? | ? | Unroll manuscript |
| `layout-lightbox` | `layout/lightbox.lua` | **NO** (HTML) | Yes | Lightbox for images (HTML-specific) |
| `layout-columns-preprocess` | `layout/columns-preprocess.lua` | **MAYBE** | Yes | Preprocess columns |
| `layout-columns` | `layout/columns.lua` | **MAYBE** | Yes | Column layout |
| `layout-cites-preprocess` | `layout/cites.lua` | Yes | Yes | Preprocess citations |
| `layout-cites` | `layout/cites.lua` | Yes | Yes | Citation layout |
| `layout-panels` | `layout/layout.lua` | **MAYBE** | Yes | Panel layout |
| `post-fold-code-and-lift-codeblocks-from-floats` | `quarto-post/foldcode.lua` | **MAYBE** | Yes | Code folding |

### Analysis Needed
- [ ] Read `layout/columns.lua` - column layout implementation
- [ ] Read `layout/layout.lua` - panel layout
- [ ] Read `layout/lightbox.lua` - lightbox (HTML-specific?)
- [ ] Check if layout filters have format branches

---

## Filter Group 6: quarto_post_filters

**Purpose**: Format-specific post-processing. These are explicitly tied to output formats.

### Format-Agnostic Post Filters

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `post-cell-cleanup` | `quarto-post/cellcleanup.lua` | Yes | Yes | Clean up cell outputs |
| `post-combined-cites-bibliography` | combined | Yes | Yes | Citations and bibliography |
| `post-choose-cell_renderings` | `quarto-post/cell-renderings.lua` | Yes | Yes | Choose cell rendering |
| `post-landscape-div` | `quarto-post/landscape.lua` | Yes | ? | Landscape div handling |

### HTML-Specific Post Filters

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `post-figureCleanupCombined` | combined | **MIXED** | Yes | Figure cleanup |
| → `responsive()` | `quarto-post/responsive.lua` | **NO** (HTML) | Yes | Responsive HTML |
| → `responsive_table()` | `quarto-post/responsive.lua` | **NO** (HTML) | Yes | Responsive tables |
| → `figCleanup()` | `quarto-post/fig-cleanup.lua` | Yes | Yes | Figure cleanup |
| → `delink()` | `quarto-post/delink.lua` | Yes | Yes | Remove links |
| `post-postMetaInject` | `quarto-post/meta.lua` | Yes | Yes | Inject post metadata |
| `post-render-html-fixups` | `quarto-post/html.lua` | **NO** (HTML) | Yes | HTML-specific fixups |
| `post-ojs` | `quarto-post/ojs.lua` | **NO** (HTML) | Yes | Observable JS (HTML only) |
| `post-render-dashboard` | `quarto-post/dashboard.lua` | **NO** (HTML) | Yes | Dashboard (HTML only) |

### Other Format-Specific Post Filters

| Filter Name | Source File | Format | HTML Impact | Description |
|-------------|-------------|--------|-------------|-------------|
| `post-ipynb` | `quarto-post/ipynb.lua` | ipynb | No | Jupyter notebook output |
| `post-render-jats` | `quarto-post/jats.lua` | JATS | No | JATS XML output |
| `post-render-asciidoc` | `quarto-post/render-asciidoc.lua` | AsciiDoc | No | AsciiDoc output |
| `post-render-latex` | `quarto-post/latex.lua` | LaTeX | No | LaTeX output |
| `post-render-latex-fixups` | `quarto-post/latex.lua` | LaTeX | No | LaTeX fixups |
| `post-render-typst` | `quarto-post/typst.lua` | Typst | No | Typst output |
| `post-render-typst-fixups` | `quarto-post/typst.lua` | Typst | No | Typst fixups |
| `post-render-typst-css-to-props` | `quarto-post/typst-css-property-processing.lua` | Typst | No | Typst CSS props |
| `post-render-typst-brand-yaml` | `quarto-post/typst-brand-yaml.lua` | Typst | No | Typst brand YAML |
| `post-render-gfm-fixups` | `quarto-post/gfm.lua` | GFM | No | GFM fixups |
| `post-render-hugo-fixups` | ? | Hugo | No | Hugo fixups |
| `post-render-email` | `quarto-post/email.lua` | Email | No | Email output |
| `post-render-pptx-fixups` | `quarto-post/pptx.lua` | PPTX | No | PowerPoint fixups |
| `post-render-revealjs-fixups` | `quarto-post/reveal.lua` | Reveal | No | Reveal.js fixups |

### Post Filters with Format Branches

| Filter Name | Components | Notes |
|-------------|------------|-------|
| `post-figureCleanupCombined` | latexDiv, responsive, quartoBook, reveal, tikz, pdfImages, delink, figCleanup, responsive_table | Mixed format code |

### Detailed Analysis: quarto-post/html.lua

**File**: `quarto-post/html.lua` (~138 lines)

**Purpose**: HTML-specific fixups for tables, figures, images, and paragraphs.

**Transformations**:
1. **Table** - Add `odd`/`even`/`header` classes to rows (Pandoc 3.2.1 removed these)
2. **Table** - Add `caption-top` class when caption is at top
3. **Figure** - Forward `fig-align` attribute to figure element as `quarto-figure-*` class
4. **Image** - Move `fig-align` to class, move `fig-alt` to `alt` attribute
5. **Para** - Wrap standalone images in `<figure>` elements with alignment classes
6. **Div** - Handle `.cell-output-display` divs with images (knitr compatibility)

**Rust Implementation Notes**:
- All transformations are HTML postprocessor operations
- Require DOM manipulation (add classes, wrap elements)
- Should be straightforward to port

### Detailed Analysis: quarto-post/responsive.lua

**File**: `quarto-post/responsive.lua` (~62 lines)

**Purpose**: Make HTML output responsive.

**Transformations**:
1. **Image** - Add `img-fluid` class (Bootstrap) if `fig-responsive: true` and no explicit height
2. **Table** - Wrap tables with `responsive*` class in `<div class="table-responsive*">`

**Rust Implementation Notes**:
- Pure HTML postprocessor
- Bootstrap-specific classes
- Simple class manipulation

### Detailed Analysis: quarto-post/foldcode.lua

**File**: `quarto-post/foldcode.lua` (~165 lines)

**Purpose**: Implement code folding using HTML `<details>` elements.

**Transformations**:
1. Wrap `.cell-code` blocks with `code-fold` attribute in `<details><summary>...</summary>...</details>`
2. Lift code blocks from floats (prevents code from being inside figure captions)
3. Handle `code-summary` attribute for custom fold labels

**Format Specificity**: HTML-only (uses `<details>` and `<summary>` elements)

**Rust Implementation Notes**:
- HTML postprocessor for the `<details>` wrapping
- May need AST transform for the code block lifting logic
- Integrates with DecoratedCodeBlock custom node

### Detailed Analysis: quarto-finalize/dependencies.lua

**File**: `quarto-finalize/dependencies.lua` (~16 lines)

**Purpose**: Process final dependencies into metadata.

**Implementation**: Just calls `_quarto.processDependencies(meta)` - the real work is in the Quarto runtime, not this filter.

**Rust Implementation Notes**:
- Need to understand where `_quarto.processDependencies` is defined
- Likely involves injecting JS/CSS into document
- Part of template/finalization stage

### Analysis Needed
- [x] Read `quarto-post/html.lua` - HTML-specific fixups (DONE - see above)
- [x] Read `quarto-post/responsive.lua` - responsive HTML (DONE - see above)
- [ ] Read `quarto-post/dashboard.lua` - dashboard layout
- [ ] Read `quarto-post/ojs.lua` - Observable JS integration
- [x] Read `quarto-post/foldcode.lua` - code folding (DONE - see above)

---

## Filter Group 7: quarto_finalize_filters

**Purpose**: Final cleanup and dependency injection.

| Filter Name | Source File | Format-Agnostic? | HTML Impact | Description |
|-------------|-------------|------------------|-------------|-------------|
| `finalize-combined` | combined | **MIXED** | Yes | Combined finalization |
| → `file_metadata()` | `common/filemetadata.lua` | Yes | Yes | File metadata |
| → `mediabag_filter()` | `quarto-finalize/mediabag.lua` | Yes | Yes | Media bag handling |
| → `inject_vault_content_into_rawlatex()` | ? | **NO** (LaTeX) | No | LaTeX vault content |
| `finalize-bookCleanup` | `quarto-finalize/book-cleanup.lua` | Yes | Yes | Book cleanup |
| `finalize-cites` | `quarto-post/cites.lua` | Yes | Yes | Write citations |
| `finalize-metaCleanup` | `quarto-finalize/meta-cleanup.lua` | Yes | Yes | Clean up metadata |
| `finalize-dependencies` | `quarto-finalize/dependencies.lua` | **MAYBE** | Yes | Dependency injection |
| `finalize-combined-1` | `quarto-finalize/finalize-combined-1.lua` | ? | ? | Additional finalization |
| `finalize-wrapped-writer` | `ast/wrappedwriter.lua` | Yes | Yes | Wrapped writer |

### Analysis Needed
- [ ] Read `quarto-finalize/dependencies.lua` - dependency injection (critical for JS/CSS)
- [ ] Read `quarto-finalize/meta-cleanup.lua` - metadata cleanup
- [ ] Read `quarto-finalize/finalize-combined-1.lua` - understand what's combined

---

## HTML-Specific Filter Summary

Filters that are **explicitly HTML-specific** or have significant HTML impact:

### Must Implement for HTML Parity

1. **quarto-post/html.lua** - `render_html_fixups()` - HTML-specific fixups
2. **quarto-post/responsive.lua** - `responsive()`, `responsive_table()` - Responsive HTML
3. **quarto-post/dashboard.lua** - `render_dashboard()` - Dashboard layouts
4. **quarto-post/ojs.lua** - `ojs()` - Observable JS integration
5. **quarto-post/foldcode.lua** - `fold_code_and_lift_codeblocks()` - Code folding
6. **layout/lightbox.lua** - `lightbox()` - Image lightbox
7. **quarto-finalize/dependencies.lua** - `dependencies()` - JS/CSS dependency injection

### Pre Filters with HTML Impact (but should be format-agnostic)

1. **quarto-pre/code-annotation.lua** - Code annotations
2. **quarto-pre/panel-*.lua** - Bootstrap panels (if HTML-specific)
3. **quarto-pre/parsefiguredivs.lua** - Figure parsing
4. **quarto-pre/table-captions.lua** - Table captions
5. **quarto-pre/shortcodes-handlers.lua** - Shortcodes
6. **quarto-pre/hidden.lua** - Hidden content

### Crossref Filters with HTML Impact

1. **crossref/refs.lua** - Reference resolution
2. **crossref/figures.lua** - Figure references
3. **crossref/equations.lua** - Equation references

### Layout Filters with HTML Impact

1. **layout/columns.lua** - Column layout
2. **layout/layout.lua** - Panel layout

---

## Custom AST Nodes

The Quarto filter chain uses custom AST nodes beyond Pandoc's standard types. These are created during normalization:

| Node Type | Source File | Description | HTML Rendering |
|-----------|-------------|-------------|----------------|
| `FloatRefTarget` | `customnodes/floatreftarget.lua` | Cross-referenceable float | Figure/table with ID |
| `Callout` | `customnodes/callout.lua` | Callout blocks | Styled div with icon |
| `PanelLayout` | `customnodes/panellayout.lua` | Panel layouts | Grid/flex layout |
| `Tabset` | `customnodes/panel-tabset.lua` | Tab panels | Tabbed interface |
| `DecoratedCodeBlock` | `customnodes/decoratedcodeblock.lua` | Decorated code | Code with filename, annotations |
| `Theorem` | `customnodes/theorem.lua` | Theorems | Numbered theorem block |
| `Proof` | `customnodes/proof.lua` | Proofs | Proof block |
| `LatexEnv` | `customnodes/latexenv.lua` | LaTeX environments | (LaTeX only) |
| `LatexCmd` | `customnodes/latexcmd.lua` | LaTeX commands | (LaTeX only) |
| `HtmlTag` | `customnodes/htmltag.lua` | Raw HTML tags | Direct HTML |
| `Shortcode` | `customnodes/shortcodes.lua` | Shortcodes | Expanded content |
| `ContentHidden` | `customnodes/content-hidden.lua` | Conditional content | Visible/hidden |

### Analysis Needed
- [ ] Study how each custom node type is created and rendered
- [ ] Determine Rust equivalents (enum variants? struct types?)
- [ ] Understand rendering pipeline for custom nodes

---

## Next Steps

### Immediate Analysis Tasks

1. **Read HTML-critical files**:
   - [ ] `quarto-post/html.lua`
   - [ ] `quarto-post/responsive.lua`
   - [ ] `quarto-post/dashboard.lua`
   - [ ] `quarto-finalize/dependencies.lua`

2. **Verify format-agnostic claims**:
   - [ ] `quarto-pre/code-annotation.lua`
   - [ ] `quarto-pre/panel-*.lua`
   - [ ] `layout/columns.lua`

3. **Understand custom nodes**:
   - [ ] `customnodes/floatreftarget.lua`
   - [ ] `customnodes/callout.lua`
   - [ ] `normalize/astpipeline.lua`

### Create Beads Issues

After analysis, create issues for:
- [ ] Each filter group implementation
- [ ] Custom node system in Rust
- [ ] HTML-specific postprocessors
- [ ] Dependency injection system

---

## Architectural Implications for Rust Quarto

### Filter Group Mapping

| TS Quarto Group | Rust Quarto Equivalent | Notes |
|-----------------|------------------------|-------|
| `quarto_init_filters` | `ParseDocumentStage` setup | Already partially done |
| `quarto_normalize_filters` | New `NormalizationStage`? | Custom node creation |
| `quarto_pre_filters` | `AstTransformsStage` | Expand existing |
| `quarto_crossref_filters` | New `CrossrefStage`? | Or part of transforms |
| `quarto_layout_filters` | New `LayoutStage`? | Complex, may split |
| `quarto_post_filters` | New `HtmlPostProcessingStage` | Format-specific |
| `quarto_finalize_filters` | New `FinalizeStage`? | Or part of template |

### Key Design Decisions

1. **Custom AST nodes**: How to represent in Rust?
   - Option A: Enum variants in Pandoc types
   - Option B: Separate custom node types
   - Option C: Attribute-based markers

2. **Format branching**: Where does format-specific code live?
   - Option A: Conditional in each transform
   - Option B: Separate format-specific stages
   - Option C: Plugin/trait-based dispatch

3. **Filter composition**: How to combine filters?
   - Option A: Sequential stages (current)
   - Option B: Combined traversals (like `combineFilters`)
   - Option C: Visitor pattern

---

## Summary: HTML Parity Implementation Strategy

Based on this analysis, here's the recommended approach for Rust Quarto HTML parity:

### Tier 1: Critical for Basic HTML Parity

These are essential for HTML output to look correct:

| Filter/Feature | TS Quarto Source | Rust Implementation | Effort |
|----------------|------------------|---------------------|--------|
| Table row classes | `quarto-post/html.lua` | HTML postprocessor | Small |
| Figure alignment | `quarto-post/html.lua` | HTML postprocessor | Small |
| Image attributes | `quarto-post/html.lua` | HTML postprocessor | Small |
| Responsive images | `quarto-post/responsive.lua` | HTML postprocessor | Small |
| Responsive tables | `quarto-post/responsive.lua` | HTML postprocessor | Small |
| Code folding | `quarto-post/foldcode.lua` | HTML postprocessor + AST | Medium |
| Shortcodes | `quarto-pre/shortcodes-handlers.lua` | AST transform | Medium |
| Hidden content | `quarto-pre/hidden.lua` | AST transform | Small |

### Tier 2: Important for Feature Parity

These enable commonly-used Quarto features:

| Filter/Feature | TS Quarto Source | Rust Implementation | Effort |
|----------------|------------------|---------------------|--------|
| Code annotations | `quarto-pre/code-annotation.lua` | AST transform + HTML postprocessor | Large |
| Table captions | `quarto-pre/table-captions.lua` | AST transform | Medium |
| Panel layouts | `quarto-pre/panel-*.lua` | AST transform | Medium |
| Column layouts | `layout/columns.lua` | AST transform + HTML postprocessor | Large |
| Lightbox | `layout/lightbox.lua` | HTML postprocessor | Medium |
| Cross-references | `crossref/*.lua` | AST transform | Large |

### Tier 3: Advanced Features

These are less commonly used or can be deferred:

| Filter/Feature | TS Quarto Source | Rust Implementation | Effort |
|----------------|------------------|---------------------|--------|
| Dashboard | `quarto-post/dashboard.lua` | Full subsystem | Very Large |
| Observable JS | `quarto-post/ojs.lua` | Full subsystem | Very Large |
| Book support | `quarto-pre/book-*.lua` | Full subsystem | Large |
| Manuscripts | `layout/manuscript.lua` | AST transform | Medium |
| Theorems | `quarto-pre/theorems.lua` | AST transform | Medium |

### Recommended Phase 1 Scope

For Phase 1 (HTML Postprocessing Infrastructure), focus on:

1. **Infrastructure**:
   - HTML parsing/manipulation (scraper crate)
   - HtmlPostProcessor trait
   - HtmlPostProcessingStage pipeline stage

2. **First Postprocessors** (Tier 1, small effort):
   - Table row classes (`odd`/`even`/`header`)
   - Figure alignment classes
   - Image attribute normalization
   - Responsive image class

These give immediate visible improvements with low implementation cost.

### Format-Agnostic vs Format-Specific Split

The key architectural insight is:

**Format-Agnostic (AST Transforms)**:
- Shortcode expansion
- Code annotation parsing (not rendering)
- Table caption positioning
- Hidden content removal
- Panel/column structure creation
- Cross-reference indexing

**Format-Specific (HTML Postprocessors)**:
- Bootstrap class injection
- `<details>` code folding
- Responsive wrappers
- Figure `<figure>` wrapping
- Code annotation `<span>` decoration
- Cross-reference link styling

This separation allows us to reuse AST transforms across formats while keeping format-specific rendering isolated.

---

## References

- Main filter file: `external-sources/quarto-cli/src/resources/filters/main.lua`
- Filter source directory: `external-sources/quarto-cli/src/resources/filters/`
- Custom nodes: `external-sources/quarto-cli/src/resources/filters/customnodes/`
