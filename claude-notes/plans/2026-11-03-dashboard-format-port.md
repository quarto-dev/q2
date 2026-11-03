---
title: 'Port the Quarto 1 dashboard format (bd-dashboard-format-k21hd4vy)'
date: 2026-11-03
description: 'Plan for `format: dashboard` in Q2: dashboard CustomNodes built by a sugar transform and lowered to bslib markup by a resolve transform, vendored bslib SCSS/JS, a per-format defaults layer, and a React preview mirror.'
status: in-progress  # Phases 0-1 done 2026-11-03; Phase 2 next
braid:
  strand: bd-dashboard-format-k21hd4vy
---

**Strand:** bd-dashboard-format-k21hd4vy (epic). **Related:** bd-cymkcyaf (per-format execute defaults), bd-nn2fou8h (Jupyter visibility options, touches the same cell wrapper), bd-3s50nz0m (Shiny / `q2 serve` research), bd-9fz5fweg (layout-panel CSS), bd-x8hqtvm0 (knitr figure and widget sizing).

**Process:** `claude-notes/instructions/q1-feature-porting.md`. This document is the Phase B output. Phase A ran three studies: the Q1 docs (`external-sources/quarto-web/docs/dashboards/`), the Q1 implementation (`external-sources/quarto-cli/src/format/dashboard/`, the dashboard Lua filters, `src/resources/formats/dashboard/`), and the current state of Q2.

## Overview

A Q1 dashboard is an HTML document. Pandoc writes plain HTML (`pandocTo: "html"`), and three layers turn the document into a bslib page:

1. **Lua filter** (`quarto-post/dashboard.lua` and `modules/dashboard/*.lua`, about 2,600 lines). It runs five passes over the AST and rewrites headings, cells and divs into intermediate divs: `.rows`, `.columns`, `.dashboard-page`, `.card`, `.valuebox`, `.sidebar-panel`, `.toolbar-panel`, `.card-toolbar`, `.card-sidebar`. Options travel as `data-*` attributes.
2. **TypeScript DOM postprocessor** (`dashboardHtmlPostProcessor`, `format-dashboard.ts:318`, plus ten helper modules, about 1,900 lines). It rewrites those divs into bslib markup: CSS grid templates, `html-fill-container` / `html-fill-item`, bslib cards, value boxes, sidebars, card tabsets, and page tabs in the navbar.
3. **Runtime.** A template (`formats/dashboard/template.html`, `title-block.html`, `_nav-container.html`), an SCSS layer (`quarto-dashboard.scss`, 1,059 lines), bslib `components.js` and `web-components.js`, `quarto-dashboard.js` (page and hash navigation, widget fill, sticky table headers), and DataTables (2.4 MB) for itables.

Q2 already holds a copy of the Lua stack in `resources/pandoc-filters/`, but it runs only on the Pandoc-hybrid path, never in the native HTML pipeline. Q2 has no DOM postprocessing stage: the 2025-12 analysis (`2025-12-20-html-postprocessor-analysis.md`) was never built, and since then the convention has been to fold Q1 postprocessing into AST transforms and templates. Revealjs is the precedent. The pipeline has a named seam for this format: `reveal_finalization_transforms` in `crates/quarto-core/src/pipeline.rs` says "a new format that needs late, float-aware reshaping (e.g. `dashboard`, ...) adds its transforms here".

**The shape of the port.** Replace Q1's layers 1 and 2 with one set of Rust AST transforms that build the final bslib structure directly. Vendor the runtime pieces from layer 3. Q1's postprocessors are not reproduced as a DOM pass: every decision they make depends only on the structure the Lua built, which the Rust transforms already hold.

## The Q1 contract, condensed

The rules a dashboard author can rely on. References are to `external-sources/quarto-cli/` unless noted.

### Format and defaults

- `format: dashboard` is HTML-based and registered as `"dashboard"` (`format-dashboard.ts:308`). Base-format checks (`isHtmlDashboardOutput`, Lua `_quarto.format.isDashboardOutput()`) also match `*-dashboard` extension formats (`src/config/format.ts:92`).
- **Defaults** (`format-dashboard.ts:74`):
  - `fig-width: 8`, `fig-height: 5`
  - `echo: false`, `warning: false`
  - `ipynb-shell-interactivity: all`
  - `plotly-connected: false`
  - `page-layout: custom`
- **Document options** (`schema/document-dashboard.yml`):
  - `orientation: rows|columns` (default `rows`)
  - `scrolling: bool` (default false)
  - `expandable: bool` (default true)
  - `logo`, with light and dark variants and `logo-alt`, resolved through brand
  - `nav-buttons`: aliases `linkedin`, `facebook`, `reddit`, `twitter`, `github`, or nav-item objects
  - `theme`, the usual Bootstrap and Bootswatch themes and brand
- **Cell options** (`schema/cell-card.yml`):
  - `title`, `padding`, `expandable`, `width`, `height`, `color`
  - `content: valuebox|card-toolbar|card-sidebar|sidebar|toolbar`. `sidebar` and `toolbar` are in the schema but have no implementation (Quirk Q9).
- **Attributes Q1 accepts that are not in the schema:** `icon`, `bg-color`, `fg-color`, `min-height`, `max-height`, `fill`, `component` (a synonym of `content`), `header-for`, `footer-for`, `target`, `position`.

### Structure (Lua pass 2, `dashboard.lua:347`)

- **Level-1 headings are pages.** The heading text becomes the navbar tab. Per-page `orientation=` and `scrolling=` attributes are allowed. Q1 `page.lua` emits `.dashboard-page`.
- **Level-2 and deeper headings are rows or columns.** Orientation alternates at each level change, starting from the document `orientation`. The heading text is discarded.
  - Sizing: `{height=70%}` / `{width=...}`, `{.fill}` / `{.flow}`.
  - A `{.tabset}` heading makes a tabset card: each child card is a tab, titled by its card title or by its first heading.
- **`{.sidebar}` on a heading at any level:**
  - On a level-1 heading it is a global sidebar.
  - Otherwise it is placed inline in its row or column.
  - It goes on the left, or on the right if it comes after content, with `width=`.
  - It forces the parent container to column orientation.
- **`{.toolbar}`:** the same rules, horizontal, at the top or bottom. It forces row orientation.
- **Leading content** before the first heading is emitted as-is, without card chrome.

### Cards (Lua pass 1, `card.lua`; TS `format-dashboard-card.ts`)

- **What becomes a card:**
  - every code cell that produces output;
  - every `::: {.card}` div;
  - loose markdown blocks in a container, merged into one flow card;
  - a layout panel (`layout-ncol`).
- **Not a card:**
  - cells with no output, and `output: false` cells;
  - hidden cells;
  - `output: asis` cells, passed through unchanged;
  - literal `.card-header` + `.card-body` divs, left as written.
- **Card options:** `title` (a cell option or a div attribute; markdown in the title is kept inline, Q1 #14646), `padding`, `height`, `min-height`, `max-height`, `expandable` (default true; false for value boxes and flow cards), `fill` (`.flow` / `.fill` classes too).
- **Dynamic title:** if the first output is stdout beginning `title=`, that output becomes the card title.
- **Bodies and footer:**
  - Each `.cell` or explicit `.card-body` inside a card is its own body.
  - A nested `.card` flattens into the parent, and its title becomes the tab or body title.
  - A blockquote or `.card-footer` becomes the footer.
  - Headings inside a card are demoted to `Div.h4` or below.
- **Engine-emitted bslib HTML** (a raw html output containing `bslib-`) is lifted out of the card, and the rest of the cell still becomes a card. This is Q1 #15023, which also fixed false positives on the word "bslib".
- **Images in a cell output** lose their `width`/`height` attributes and get `quarto-dashboard-img-contain` (`object-fit: contain`).

### Value boxes (`valuebox.lua`, `format-dashboard-valuebox.ts`)

- **Markdown form:** `::: {.valuebox icon=... color=...}`. The title comes from the `title` attribute, else the first block. The value is the next block; any remaining blocks are extra content.
- **Cell form:** `content: valuebox`. Options come from YAML, or from a printed Python `dict(...)` or R `list(...)`. The Python form is parsed by evaluating its repr as Lua with `load()`; the R form by recognising `$value\n[1] ...`. Keys: `value`, `title`, `icon`, `color`, `bg-color`, `fg-color`.
- **Color:** a `#hex` value becomes an inline style; a name becomes `bg-<name>` or `text-<name>`. The default is `bg-secondary`. The aliases `primary` … `dark` are themable through `$valuebox-bg-*`.
- **Icon:** any Bootstrap Icon name, placed in `.value-box-showcase`. The layout is always `showcase-left-center` (Quirks Q11 and Q13).

### Card inputs

- **Card toolbar:** `content: card-toolbar` or `.card-toolbar`. Default: the header of the next card. `previous`, `footer`, `header-for=<id>` and `footer-for=<id>` change the target.
- **Card sidebar:** `content: card-sidebar` or `.card-sidebar`. It goes on the left of the next card, or on the right when the target is `previous`, with `width=`.
- An unplaced toolbar or sidebar, or a target id that does not resolve, is a fatal error in Q1 (pass 4).

### Chrome and runtime

- **Header:** `header#quarto-dashboard-header > nav.navbar.slim[data-bs-theme=dark]` holds the logo, title, subtitle, author (multiple authors: Q1 #10334), page tabs (`ul.navbar-nav[role=tablist]`) and nav buttons.
- **Dark mode:** a color-scheme toggle in the navbar for multi-page dashboards when dark mode is configured.
- **Page navigation:**
  - The URL hash selects a page.
  - A hash that names no page keeps the current page (Q1 #14818).
  - Back and forward history works (#9411).
  - Focus is reset correctly when a page is opened from its hash (#14819).
  - Each page has its own `data-scrolling`.
- **Expand:** a bslib full-screen button on expandable cards.
- **Sidebar:** collapse toggle; collapsed at widths of 576px and below.
- **Layout:** `body.dashboard-fill` fills the viewport unless scrolling. Below the `sm` breakpoint every grid row becomes `max-content`.
- **Accessibility:** a skip link (#14684). The axe report UI (#14125) is out of scope here.

## What exists in Q2 today

| Area | Q2 status | Notes |
|---|---|---|
| `format: dashboard` recognised | ❌ | `FormatIdentifier` is closed (`format.rs:23`); `q2 render` rejects it at `commands/render.rs:1971`. `KNOWN_BASE_FORMATS` (`extension/discover.rs:262`) lacks `dashboard`. |
| Per-format writer defaults (echo false, 8×5 figures) | ❌ | No layer exists (bd-cymkcyaf). The TS engine host uses fixed defaults (`ts-packages/quarto-engine-host-node/src/metadata-as-format.ts:217`); knitr special-cases only typst (`engine/knitr/format.rs:288`). |
| Cell options on the cell Div | ⚠️ | knitr forwards unknown chunk options as attributes (vendored `hooks.R:330`). **The native Jupyter engine does not:** it writes a bare `::: {.cell}` (`engine/jupyter/text_execute.rs:582`), so `#| title:`, `padding`, `content` are lost. This blocks cards with Jupyter. |
| `ipynb-shell-interactivity: all` | ❓ | Not checked yet. Needed for "every top-level expression shows" (Q1 #11150). |
| Dashboard Lua | ⚠️ | Vendored (`resources/pandoc-filters/filters/quarto-post/dashboard.lua`) but runs only on the Pandoc-hybrid path. Useful as a reference, not as the runtime. |
| DOM postprocessing | ❌ by design | No post-HTML DOM stage. `scraper` and `lol_html` are native-only, so they would not run in WASM preview. |
| bslib SCSS/JS, htmltools fill CSS | ❌ | Not vendored anywhere (`resources/vendored.toml` has no entry). Q1 compiles bslib component SCSS (about 1,100 lines) into every Bootstrap page (`format-html-scss.ts:120`). |
| Bootstrap 5.3, Bootswatch, brand layers, Bootstrap Icons | ✅ | `resources/scss/bootstrap/`, `quarto-sass`, `transforms/website_bootstrap_icons.rs`. |
| Template seam for a non-default HTML scaffold | ⚠️ | `ApplyTemplateStage` already branches for revealjs (`apply_template.rs:302`). Dashboards need a built-in template or partial set. |
| Panel tabsets (Bootstrap nav-tabs) | ✅ | `transforms/panel_tabset*.rs`. The markup differs from bslib card tabsets, which use `card-header-tabs` inside a card, but the building blocks exist. |
| Layout panels (`layout-ncol`) | ❌ | Not native (bd-9fz5fweg family). The dashboard "card layout" feature depends on it. |
| Websites with mixed formats | ✅ | Revealjs pages keep their format inside a site (`orchestrator.rs:1063`). Dashboards in a site need the navbar merge Q1 does in `format-dashboard-website.ts`. |
| Shiny, OJS | ❌ | Neither exists in Q2 (bd-3s50nz0m for Shiny). |
| WASM preview | ⚠️ | `format_class` (`pandoc_request/formats.rs:108`) classes only Html/Revealjs as Preview. A Preview format without a pseudo-format already falls through to the full-DOM iframe (`hub-client/src/components/render/getQ2Format.ts`, `mode: 'dom'`). |

## Design decisions

Each divergence from Q1 is tagged `[decided]` or `[needs user sign-off]`. Divergences from Q1 must be justified by lever 1 (strictness with an actionable diagnostic) or lever 2 (explicit declaration over inference).

### D1. Dashboard CustomNodes: one sugar transform and one resolve transform; no DOM postprocessor `[decided 2026-11-03]`

Q1 splits the work: the Lua builds structure and a TS pass decorates it as HTML. That split exists because Q1's Lua could not see the HTML writer's output and its TS could not see the AST. Q2 has neither limit.

**How the typed model flows through the pipeline.** Transforms pass only the AST to each other. A standalone Rust struct built in one transform has nowhere to live until the next one, short of stashing it in metadata, which would hide it from Lua filters and from the preview.

So the AST carries the typed structure as **CustomNodes**, the same way `Callout`, `PanelTabset` and `FloatRefTarget` already do:

- `slots` hold the AST content (card bodies, titles, sidebar contents);
- `plain_data` holds the typed options as JSON;
- `crates/quarto-pandoc-types/resources/custom-node-schema.json` is the contract shared by Rust and the TS preview (`schemaConformance.test.ts`).

The "typed model" on the Rust side is a set of structs with `from_custom_node` / `to_custom_node`, used *inside* a transform. It is a typed view of the nodes, not a second carrier:

```text
DashboardPage      plain_data { orientation, scrolling, title, id }
                   slots      { content }
DashboardContainer plain_data { direction: rows|columns, layout: fill|flow,
                                grid_template, tabset?: { id, titles } }
                   slots      { children }
DashboardCard      plain_data { title?, padding?, height?, min/max-height?,
                                expandable, fill, classes }
                   slots      { title, bodies[], footer?, toolbar?, sidebar? }
DashboardValueBox  plain_data { color|bg-color|fg-color, icon?, showcase-position }
                   slots      { title, value, extra }
DashboardSidebar / DashboardToolbar
                   plain_data { position, width|height, scope: page|global|inline|card }
                   slots      { content }
```

The node lifecycle, mirroring callouts:

1. **`DashboardTransform`** (sugar). It runs in the Normalization phase after engine output is in the AST, and it replaces `Sectionize` and `TitleBlock`, as `RevealSlides` does. It reads headings, cells and divs and builds the nodes. It also *computes the layout*:
   - grid templates, which ports Q1's `computeRowLayouts` / `toGridSize`;
   - fill and flow decisions;
   - expandable defaults;
   - tabset ids.

   The results are stored in `plain_data`. This way the sizing rules exist once, in Rust, and the React preview reads `grid_template` instead of reimplementing them.
2. **User Lua filters** run between the two transforms and see the nodes, for example a filter that walks every card.
3. **`DashboardResolveTransform`** (Finalization phase, through the existing `reveal_finalization_transforms` seam, after crossref has resolved floats inside cards). It lowers the nodes to Divs and RawBlocks that carry the final bslib markup:
   - fill classes (`recursiveApplyFillClasses`);
   - card header and title assembly;
   - tab nav markup;
   - sidebar and toolbar wrappers;
   - image contain classes.
4. **Preview.** `DashboardResolveTransform` goes on the preview deny-list (`Q2_PREVIEW_TRANSFORM_EXCLUDED`), as `CalloutResolve` does, so React receives the CustomNodes (D8).

**Why not intermediate classes, as Q1 does?** The Q1 bugs below (Q1–Q7) are mostly cases of stringly-typed state lost between passes: attributes never renamed, swapped return values, a global orientation toggle. Typed views at both ends remove that class of bug.

**Card bodies: indexed slots `[decided 2026-11-03]`.** `Slot` has no list-of-block-lists variant (`quarto-pandoc-types/src/custom.rs:20`). The existing `Tabset` node already handles ordered sections with indexed slots, `title-{i}` / `content-{i}` (`transforms/panel_tabset.rs:297`). Cards follow that precedent:

- slots `body-{i}`;
- a `plain_data.bodies` array carrying per-body options (title, height, min-height, max-height, fill).

Dashboard tabset cards use the same shape, with `title-{i}` taken from each body's title.

Container children are `Block::Custom` nodes inside a `children: Blocks` slot. `custom-node-schema.json` also records a Pandoc-hybrid `route` per type. The dashboard types never reach a Pandoc writer because `DashboardResolveTransform` lowers them for HTML. Check the schema comment's ExampleEmbed precedent (resolved upstream, absent from `types`) against the conformance test's "schema types == registered TS types" rule, and pick whichever fits.

### D2. A new `FormatIdentifier::Dashboard` variant `[decided]`

- Add `Dashboard` to `is_native()`, `is_html_based()` and `is_multi_file()`.
- Add a `PipelineProfile` member, or reuse `HtmlRender` with an `is_dashboard` flag. The implementation will choose whichever keeps the phase-ordering test simplest; the test's format loop gains `dashboard`.
- Add `dashboard` to `KNOWN_BASE_FORMATS` so `foo-dashboard` extension formats resolve.
- Lua filters see `FORMAT == "html"`, and `quarto.format.is_format("dashboard")` returns true. The vendored knitr `execute.R` keys on `base-format == "dashboard"` for the DT defaults, so the knitr handoff must send that value.

### D3. A per-format defaults layer, built here, fixing bd-cymkcyaf `[decided 2026-11-03: general layer]`

Dashboards need `echo: false`, `warning: false`, `fig-width: 8`, `fig-height: 5` and `ipynb-shell-interactivity: all` as defaults that sit below user metadata. Q2 has no such layer. The proposal:

- Add a built-in "format defaults" metadata layer, lowest precedence, keyed by format name.
- Fill it for `dashboard`, and also for `revealjs`, `pptx` and `beamer`. That is exactly what bd-cymkcyaf asks for, so this plan absorbs that strand.

The alternative is a dashboard-only special case, which would leave bd-cymkcyaf open.

**Correction found in Phase 1 (2026-11-03):** the layer already existed. `format_execute_defaults` in `stage/stages/engine_execution.rs` (pandoc-hybrid P7 Task 5, after bd-cymkcyaf was filed) is a per-format `execute:` defaults table, merged under the document's own `execute:` by `merge_execute_scope`. It already covered pptx and the JS slide formats. It lacked revealjs and dashboard, and it reached knitr and Jupyter (through `ExecutionContext.execute_scope`) but **not TS engines**, which read `execute:` from the metadata map. Phase 1 therefore added the revealjs and dashboard rows and folded the merged scope into the TS metadata map (`ts_document_metadata`). beamer has no Q2 format yet (the LaTeX epic), so it stays out.

### D4. Vendor bslib components and htmltools fill CSS from the quarto-cli copy `[decided 2026-11-03: vendor, dashboard pages only]`

- Vendor from the quarto-cli copy, pinned to the quarto-cli tag already in `vendored.toml`:
  - `src/resources/formats/html/bslib/components/scss/*` and `bslib-scss/*`
  - `components/dist/components.js` and `web-components.js`
  - `htmltools/fill.css`

  These go in new `vendored.toml` entries, so the `upgrade-vendored-deps` survey covers them. Taking them from quarto-cli rather than upstream bslib guarantees we run the same version as Q1.
- **Divergence:** compile the bslib component SCSS into dashboard pages only, not into every Bootstrap page as Q1 does. Every other Q2 HTML page stays the same size and keeps its current CSS (no visual regressions risked across existing fixtures).

  Cost: an HTML document that embeds bslib components itself (an R `bslib::card()` output in a plain html page) would be unstyled in Q2. That already happens today, so it is not a regression.
- **Alternative:** hand-write the small subset of bslib JS we need (full-screen expand, sidebar collapse) instead of vendoring `components.js` and `web-components.js` (about 52 KB minified).

  Recommendation: vendor. Shiny-for-R output and `bslib::value_box()` cells expect the real runtime, and parity is cheaper to keep than to reimplement.

### D5. Fix the Q1 bugs instead of porting them `[decided 2026-11-03]`

The goal is "very familiar, straightforward to port", not byte compatibility. Fix them all. Each fix gets a contract fixture and a line in the user-facing migration note. The study found these (details in the research notes):

| # | Q1 bug | Proposal |
|---|---|---|
| Q1 | Toolbar `height` / `position` options are never read (undefined `kSidebarHeightAttr`, nil argument at `dashboard.lua:694`) | Implement `height=` on toolbars as documented. |
| Q2 | Container `.fill` / `.flow` on headings is ignored for grid sizing (only cards' `data-fill` is read) | Honour it. The docs promise it ("specify filling or flowing behavior using the `.fill` and `.flow` classes applied to a row"). |
| Q3 | Orientation is a global toggle flipped on every heading-level change (`dashboard.lua:481`) | Orientation by nesting depth in the section tree. **Measured in Phase 0 (2026-11-03):** besides the two contract fixtures written for it (c37, c38), 2 of the 55 Q1 and regression fixtures change: `q1/3-valuebox-knitr.qmd` and `q1/loose-content.qmd`. In both, a deeper heading (`### TODOS`) comes before shallower `##` sections, and Q1\'s toggle then lays the `##` sections out the opposite way from the documented rule (`##` under `orientation: rows` is a row). The depth rule matches the docs, so keep it; the two become `divergence` fixtures in Phase 3. |
| Q4 | Headings in sidebars and toolbars are not demoted (undefined `contents`, `dashboard.lua:40`) | Demote, as cards already do. |
| Q9 | `content: sidebar` / `content: toolbar` are in the schema but do nothing | Implement them, since they are cheap once the model exists. The alternative is a Q-code warning ("use a `{.sidebar}` heading"). |
| Q11/13 | `showcase=` / `showcase-position` are effectively ignored | Implement `showcase-position` (`left-center`, `top-right`, `bottom`), all of which bslib supports. |
| Q14 | Markdown value-box title nests paragraphs: `<p class="value-box-title"><p>Title</p></p>` (`valuebox.lua:171` takes the title as a `Para`; the TS pass turns its wrapper into a `p`). Browsers close the outer `p` early, so the documented form (`::: {.valuebox}` with the title as the first paragraph) shows an empty title slot and the title as an unstyled paragraph. Found by the Phase 0 goldens (`contract/c20`, `q1/3-valuebox-static`). | Emit the title's inlines inside `p.value-box-title`. |
| Q15 | `header-for` / `footer-for` only resolve an id *inside* the target card; the card\'s or cell\'s own id is a fatal error (`dashboard.lua:732`). Found while writing `contract/c31`. | Accept the target\'s own id too (`contract/c43`, no Q1 golden). An id that resolves nowhere is a located `Q-*` error. |
| Q5–Q8, Q10 | Internal no-ops | Nothing to port. |

### D6. Parse value-box output with a safe parser, and diagnose failures `[decided]` (lever 1)

Q1 turns Python dict output into Lua code and runs it with `load()`, and recognises R list output by its printed shape. A parse failure is either silent or an opaque Lua error. Q2 will use a small, total parser for the two printed shapes:

- a Python dict repr with string, number, bool and None values;
- R `$key\n[1] value` blocks.

On failure it emits a new `Q-*` warning, located at the cell, that names the shape it expected. Accepted inputs stay the same as Q1's.

### D7. Jupyter cell options: depend on the execution-records work; build nothing here `[decided 2026-11-03]`

Without forwarding, Jupyter cards have no titles, padding or value-box content. Q1 forwards every non-visibility cell option as a `key='value'` attribute on the cell div (`src/core/jupyter/jupyter.ts:1427`). Q2's knitr path already does the same through the vendored `hooks.R:330`.

**What PR #1007 changes.** PR #1007 (`feature/execution-records-epic`, read 2026-11-03):

- It moves Jupyter cell presentation out of `engine/jupyter/text_execute.rs` into a shared, WASM-clean Rust presenter (`crates/quarto-core/src/presenter/`). `presenter/cell.rs`\'s `render_cell` still emits a bare `::: {.cell}`.
- Its follow-on, the knitr execution-records epic (`claude-notes/plans/2026-10-30-knitr-execution-records-epic.md`, decision 12, and `2026-10-30-knr-02-knitr-presenter-layer.md` Phase 2C, both on that branch), already plans the **Quarto cell-option layer in the shared presenter, applied to Jupyter as well**. That layer includes "unknown options are forwarded as attributes by rule, not by enumeration", alongside labels, captions, `layout*`, `panel` and `code-fold`.

Forwarding written now in `text_execute.rs` would be deleted when #1007 merges. Forwarding written in the presenter would collide with KNR-2 Phase 2C, which owns that code and its goldens.

**Decision:**

- This epic builds no forwarding.
- Dashboard *Jupyter* cards depend on KNR-2 Phase 2C landing. That epic deliberately has no braid strand (its decision 18), so track the dependency here and in a braid comment.
- Until then, the corpus exercises cards through knitr (forwarding works today), markdown `.card` / `.valuebox` divs, and captured outputs. Jupyter card fixtures are skipped with a reason naming KNR-2.

**Smaller items, coordinated with that workstream:**

- **Option names must survive forwarding.** `title`, `padding`, `expandable`, `content`, `icon`, `color`, `width`, `height` and `fill` must stay "unknown" in the presenter's sense. They must not be swallowed by a known-options table, the way a new internal option leaks or gets eaten in `hooks.R` (`other_opts`). Raise this with the KNR-2 owner; a test on our side should pin it.
- **`ipynb-shell-interactivity: all`** (the dashboard default, Q1 #11150) is not implemented by the native Jupyter engine: the key only appears in config tables. It is execution-relevant (it changes outputs), so it belongs on the kernel side and in the record's execution-input hash, not in the presenter. Filed as bd-uqma6gec, sequenced after #1007 merges.
- **Record format neutrality.** Parent decision 27 keeps records free of presenter inputs. Dashboard defaults (D3) reach execution through `execute:` metadata, which records already hash, so dashboards add no format dependence to records.

### D8. Preview: a React mirror in this epic `[decided 2026-11-03: include the React mirror]`

The user chose to build the React-native preview in this epic, as revealjs did (B1). That is why D1 uses CustomNodes. The analysis below is kept for context; the work is in Phase 8.

- **Execution results.** `q2 preview` already carries captured engine outputs into the WASM render, so the React preview shows real cards there.
- **Hub multiplayer.** It has no execution, so code-cell cards are empty. It needs no hub-specific work in this epic; its value grows once the execution-results story lands.

There are three separate targets here:

1. **`q2 preview` (local).** It runs the HTML pipeline in WASM in the browser, using engine outputs captured on the server (`engine/preview_record.rs`, `capture_splice.rs`). So "defer the WASM renderer" would also mean "dashboards have no `q2 preview`", which is a big gap for the main authoring loop.
2. **Hub-client multiplayer.** There is no execution, so code cells have no outputs and a dashboard reduces to its markdown cards and value boxes. As the brief says, that has limited value until execution results can flow into the session.
3. **A React-native preview** (the revealjs B1 approach). It patches in place instead of re-rendering the iframe.

The original recommendation, superseded by the decision above to build (3) as well:

- **Do (1) in this epic.** It is cheap if D1 holds: the transforms are pure AST code that already compiles for wasm32. Classing `dashboard` as `FormatClass::Preview` with no pseudo-format routes it to the existing `mode: 'dom'` iframe, which runs the whole pipeline.

  The work: make sure the bslib and dashboard JS load in the iframe; the native-only `BootstrapJsStage` pattern needs a dom-mode equivalent. Add a render/preview parity check for one fixture.
- **(2) comes almost for free** from the same routing, since the hub uses the same WASM. It simply shows whatever outputs exist (none, today). We do not advertise it and do not build anything hub-specific.
- **Defer (3)** to a follow-up strand, filed `discovered-from` this epic and gated on the execution-results story.

### D9. Out of scope for this epic; each gets its own strand `[decided 2026-11-03]`

Filed 2026-11-03, each `discovered-from` the epic: DataTables/itables bd-6f0kslxu; Shiny bd-c9gy4cez; OJS bd-oq1i764m; card `layout-ncol` bd-mlxvysee; knitr widget hoisting bd-1n0eb3l3; axe report UI bd-gtbcfcsu; engine-side presentation defaults bd-0jutey8o.

- **DataTables / itables integration.** 2.4 MB of vendored JS (pdfmake and fonts), connected-mode module rewriting, and the itables-version-specific rewrite. Static markdown tables work without it.
- **Shiny** (`server: shiny`) and **OJS.** Q2 has neither runtime.
- **Card layout via `layout-ncol`.** Blocked on native layout panels.
- **The knitr `html_preserve` / htmlwidget hoisting** (`dashboard.lua:107`). It depends on how Q2's knitr widget path emits dependencies (bd-5oyk1xce, bd-x8hqtvm0); check this during Phase 0.
- **The axe accessibility report UI** (Q1 #14125).
- **Engine-side niceties:** the Altair container theme patch, plotly margins, and the itables option defaults from `notebook.py` / `setup.py`.

## Phased checklist

Strict TDD per `AGENTS.md`. Every phase ends with `cargo xtask verify` green, a real `q2 render` of a fixture with the output inspected, and the checkboxes updated here.

### Phase 0: Compatibility corpus (done 2026-11-03)

Built as a golden harness rather than smoke-all copies: Q1 output is reduced to a *dashboard skeleton* (grid containers and templates, the `html-fill-*` chain, bslib card / value-box / sidebar markup, page tabs, navbar; no theme CSS, scripts or card content) and Q2 output is reduced with the same function. Corpus: `crates/quarto-core/tests/fixtures/dashboard-goldens/` (README there).

- [x] Skeleton extractor, shared by capture and test: `crates/quarto-output-extract/src/dashboard.rs`; `quarto-output-extract extract <file>.html` prints one. Manifest schema and golden naming: `dashboard_corpus.rs`.
- [x] Copy the Q1 fixtures (`q1/`, the 40 top-level fixtures plus `_content/` as `content/`, `bugs/8552`, `notebooks/gapminder.ipynb`) and 11 Q1 regression fixtures (`regressions/`: #9056, #9796, #10334, #10340, #11338, #11549, #11150, #14646, #14684, the dark-mode toggle, #15023), from quarto-cli `d3263862a`. The brand logo variants are left for Phase 7, where logo resolution is implemented.
- [x] Write 43 engine-free contract fixtures (`contract/`), one per contract item, cells written as pre-executed `::: {.cell}` divs so they run in CI.
- [x] Add real-world acceptance dashboards from quarto-web (`acceptance/`: gapminder, navigation, valuebox).
- [x] `cargo xtask capture-dashboard-goldens` renders with Quarto 1 and writes `goldens/*.q1.skeleton` + provenance. 97 goldens captured (101 after the gap fill below) with a Q1 dev build of `d3263862a` (no release binary was at hand; recorded in `goldens/q1-provenance.txt`).
- [x] Assertion harness `crates/quarto-core/tests/integration/dashboard_goldens.rs`: manifest and tree agree, goldens exist exactly where promised, notes explain each status, and every `match` / `divergence` fixture renders to its golden. Negative-checked: flipping `c01` to `match` fails with `Unknown format: dashboard`, Phase 1\'s first target.
- [x] Measure Q3 across the corpus (result recorded in D5).
- [x] Sort the corpus (`manifest.toml`): 99 `pending` (each note names its phase and, for the D5 items, the expected divergence) and 7 `deferred` (OJS, Shiny, DataTables, each naming its strand).

**Findings recorded elsewhere:** Q14 and Q15 added to D5.

**Capture gaps:** none in scope. The four gaps of the first capture (`q1/6-sidebar-global`, `q1/9-bslib`, `regressions/11150`, `acceptance/gapminder`) were filled on 2026-11-03 after installing R bsicons and shiny and Python plotly and seaborn; 101 Q1 goldens in all. Still without a golden: `contract/c43` (Q1 is fatal, Q15) and four deferred fixtures (Shiny, itables, DT).

### Phase 1: Prerequisites

- [x] `FormatIdentifier::Dashboard` (native, HTML-based, `.html` output), `PipelineProfile::DashboardRender`, `dashboard` in `KNOWN_BASE_FORMATS` (so `acme-dashboard` resolves). For now a dashboard runs the plain HTML transform list; the dashboard transforms arrive in Phases 2–6.
- [x] Lua: `FORMAT == "html"` (`lua_format_for`, `Format::lua_format`), and a new `Format::base_format()` (`"dashboard"`) carried to Lua through `HostContext::base_format`, so `quarto.doc.is_format("dashboard")` is true. The same matcher (`pampa::lua::quarto_doc::is_format_match_with_base`) serves `when-format="dashboard"` in conditional content.
- [x] knitr hand-off: `pandoc.to: html` (so `knitr::is_html_output()` holds) and `identifier.base-format: dashboard` (so the vendored `execute.R`\'s `is_dashboard_output` fires). Every format now reports its base format to knitr; only that dashboard check reads it.
- [x] Per-format defaults (D3, see the correction there): revealjs (fig 10×5, echo/warning off) and dashboard (fig 8×5, echo/warning off, `ipynb-shell-interactivity: all`, `plotly-connected: false`) rows; TS engines receive the merged scope. Unit tests plus end-to-end probe-engine tests (`pandoc_execute_defaults.rs`).
- [x] `crates/quarto-core/tests/integration/dashboard_format.rs`: a dashboard renders to `.html`; a user filter sees `FORMAT=html`, `is_format("dashboard")`; `format: dashboard:` options flatten into metadata.
- [x] End to end with the CLI (2026-11-03): `q2 render` of `q1/8-pages.qmd` (R cells, `format: dashboard:` in front matter) renders through knitr with no echoed source and 8×5 figures. Found on the way, not a dashboard issue: q2 knitr figures are 1× where Q1\'s are 2× retina for every HTML format (`fig-format: png` default vs Q1\'s `retina`); noted on bd-x8hqtvm0.
- [ ] Jupyter cell-option forwarding: **not built here** (D7). Track KNR-2 Phase 2C. Add a test pinning that the dashboard option names reach the cell Div once it lands.
- [ ] `ipynb-shell-interactivity: all` (bd-uqma6gec), after PR #1007 merges. The default now reaches Jupyter in the `execute:` scope; the engine does not act on it yet.
- [ ] Deferred to Phase 8: `format_class("dashboard")` (hub/preview routing) and `q2 preview --static` (`check_servable_format`) still exclude dashboards.

### Phase 2: Page shell and runtime

- [ ] Vendor bslib components, htmltools `fill.css` and the dashboard SCSS layer (D4); add `vendored.toml` entries.
- [ ] Add a `quarto-dashboard` SCSS layer to the hard-coded layer list in `quarto-sass/src/compile.rs:87`, gated on the format; check that brand and Bootswatch theming still compose.
- [ ] Add a dashboard template and partials: header, navbar, title, subtitle, multiple authors, logo light and dark, `page-layout-custom` body, skip link.
- [ ] Ship the JS artifacts: bslib components and web-components, `quarto-dashboard.js` (vendored from quarto-cli, page and hash navigation), and `stickythead.js`.

### Phase 3: Layout core

- [ ] Build the structure model: pages, rows and columns with level-keyed orientation, leading content, and loose content merged into flow cards.
- [ ] Grid sizing: explicit height and width, fill and flow with fr sharing, the 3em minimum, container `.fill` / `.flow` (Q2), `scrolling` per document and per page, and fill classes.
- [ ] Multi-page navbar tabs, the page `tab-pane` structure, and the `data-scrolling` handoff to the JS.

### Phase 4: Cards and tabsets

- [ ] Code cells and `.card` divs become cards: title (markdown inline), padding, heights, expandable defaults, fill and flow, footer, demoted headings, image contain.
- [ ] Hidden, no-output, `output: false` and `asis` cells; literal card markup passed through; engine bslib HTML lifted out (#15023 semantics).
- [ ] Dynamic `title=` from the first stdout output.
- [ ] Tabset cards: tab titles from cards or headings, ids, active state, `.dashboard-card-no-title` (#11549), and a footer that stays outside the tab content.

### Phase 5: Value boxes

- [ ] The markdown `.valuebox` form, with color aliases, `#hex`, `bg-color` / `fg-color`, icon, and `showcase-position` (D5).
- [ ] The `content: valuebox` cell form, using the safe Python and R output parser and a `Q-*` diagnostic on failure (D6). Test it with knitr and Jupyter captures.
- [ ] `$valuebox-bg-*` theme variables.

### Phase 6: Sidebars and toolbars

- [ ] Page, global (level 1) and inline sidebars: position, width, the border flags at page level, collapse.
- [ ] The same for toolbars, including `height` (Q1) and the bottom position.
- [ ] Card toolbars and card sidebars, with next, previous, id, header and footer targeting. An unresolved target gets a located `Q-*` error, not Q1's fatal crash without a location.
- [ ] `content: sidebar` / `content: toolbar` (Q9).

### Phase 7: Navigation chrome

- [ ] `nav-buttons` (aliases, the GitHub context, nav-item objects, the `|url|` share placeholder).
- [ ] The dark-mode toggle for multi-page dashboards; logo resolution through brand, relative to the document (#14946).

### Phase 8: React preview (D8)

Following the revealjs rule ("every phase from P2 on ships render and preview together"), each of Phases 3–7 should land its React component alongside the render path. This phase is the scaffolding, done first, and the closing parity sweep.

- [ ] Add the dashboard node types to `custom-node-schema.json`, with `*_PLAIN_DATA_KEYS` and conformance tests.
- [ ] Add a pseudo-format (for example `q2-dashboard`, by analogy with `q2-slides`) in `builtin_pseudo_format` / `preview_pseudo_format`. `format_class("dashboard") == Preview`, routed by `classifyPreviewMode` to `react`.
- [ ] `DashboardResolveTransform` goes on `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
- [ ] React components in `ts-packages/preview-renderer/src/q2-preview/custom/`: `DashboardPage` (navbar tabs, hash state), `DashboardContainer` (reads `grid_template`), `DashboardCard` (expand), `DashboardValueBox`, and `DashboardSidebar` / `DashboardToolbar`.
- [ ] bslib CSS, plus the subset of bslib behavior React needs (full screen, sidebar collapse) without loading `components.js` into React. Decide this here; it depends on how revealjs handled `reveal.js` in `RevealDeck.tsx`.
- [ ] Render/preview parity tests per node type (`/preview-render-parity`), plus `parity.integration.test.tsx` coverage for one multi-page fixture.
- [ ] Playwright on rendered output: hash navigation (#14818, #9411), expand, sidebar collapse, skip link.

### Phase 9: Websites

- [ ] A dashboard page in a website: the theme comes from the site, the dashboard header merges under `#quarto-header`, and the body offset is right. Q1 also compiles the dashboard SCSS into every site page; decide whether to follow, which ties into D4.

### Wrap-up

- [ ] Write user docs under `docs/` and add a migration note for any accepted deviations.
- [ ] File follow-on strands for everything in D9 and the React preview (D8), each `discovered-from` the epic.

## Open questions for the user

None. All design decisions are settled (see the log below); the plan awaits the go-ahead to execute.

## Decision log

- **2026-11-03, D1:** the user asked how a typed model would flow through the transform pipeline. Answer: as CustomNodes, with Rust typed views at the sugar and resolve ends. Accepted along with D8.
- **2026-11-03, D3:** build the general per-format defaults layer; absorb bd-cymkcyaf.
- **2026-11-03, D4:** vendor bslib from the quarto-cli copy; compile it into dashboard pages only.
- **2026-11-03, D8:** include the React preview in this epic.
- **2026-11-03, D5:** fix the Q1 bugs. Familiarity and easy porting matter more than exact compatibility.
- **2026-11-03, D7:** fine to forward via cell attributes, but don't step on PR #1007. After reading it: build nothing here, and depend on KNR-2 Phase 2C\'s shared cell-option layer.
- **2026-11-03, D9:** all scope cuts accepted; follow-up strands filed.
- **2026-11-03, D1 sub-question:** card bodies use indexed slots, following the `Tabset` precedent (settled from the code; no user input needed).
