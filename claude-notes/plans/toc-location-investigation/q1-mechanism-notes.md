# Q1 `toc-location` mechanism notes (bd-e2kpwy7n investigation)

Exploration of `external-sources/quarto-cli` (1.10.15), 2026-08-14. All
paths below are relative to `external-sources/quarto-cli/` unless noted.

## 1. Declaration / read sites

- Constant: `src/config/constants.ts:603` — `kTocLocation = "toc-location"`.
- Schema: `src/resources/schema/document-toc.yml:36-52` —
  `enum: ["body", "left", "right", "left-body", "right-body"]`, default
  `right`, `formats: [$html-doc]`.
- Readers (all six in the tree):
  - `src/format/html/format-html-bootstrap.ts:127-129` — standalone/article path
  - `src/format/html/format-html-bootstrap.ts:349` — double-TOC (`*-body`) detection
  - `src/format/html/format-html-title.ts:171` — banner header class
  - `src/project/types/website/website-navigation.ts:252-256, 302` — website path
  - `src/project/types/manuscript/manuscript.ts:454-455` — manuscripts default to `left`
  - `src/project/types/website/about/website-about.ts:106` — about pages force `right` (and `toc: false`)
- No runtime normalization beyond the YAML enum; the raw string is passed to
  EJS and compared with `===`.

## 2. Placement mechanism

Two template paths emit a `div#quarto-toc-target` placeholder; one shared
DOM postprocessor moves `nav[role="doc-toc"]` into it.

### 2a. Standalone / non-website article

`src/format/html/format-html-bootstrap.ts:126-142` renders
`src/resources/formats/html/templates/before-body-article.ejs`:

```ejs
<%
const navbarTocLeft = tocLocation === "left" || tocLocation === "left-body";
const navbarTocRight = tocLocation === "right" || tocLocation === "right-body";
%>
<div id="quarto-content" class="page-columns page-rows-contents page-layout-<%- pageLayout %><%- (navbarTocLeft) ? " toc-left" : ""%>">
<% if (navbarTocLeft) { %>
<div id="quarto-sidebar-toc-left" class="sidebar toc-left">
  <div id="quarto-toc-target"></div>
</div>
<% } %>
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
  <% if (navbarTocRight) { %>
  <div id="quarto-toc-target"></div>
  <% } %>
</div>
<main class="content" id="quarto-document-content">
```

`body` produces **no** target at all. `#quarto-margin-sidebar` is always
emitted (it may hold margin content even without a TOC).

### 2b. Website projects

`src/project/types/website/website-navigation.ts:252-256, 302, 379` →
`src/resources/projects/website/templates/nav-before-body.ejs:3-4, 111-125`:

```ejs
const navbarTocLeft = nav['toc-location'] === "left" || nav['toc-location'] === "left-body";
...
<!-- sidebar -->
<% if (nav.sidebar || navbarTocLeft) { %>
  <% partial('sidebar.ejs', { sidebar: nav.sidebar, sidebarStyle: nav.sidebarStyle, navbar: !!nav.navbar, toc: navbarTocLeft, ... }) %>
<% } %>
<!-- margin-sidebar -->
<% if (nav.layout === "article" || nav.layout === "full") { %>
    <div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
      <% if (nav.hasToc && navbarTocRight) { %>
        <div id="quarto-toc-target"></div>
      <% } %>
    </div>
<% } %>
```

`src/resources/projects/website/templates/sidebar.ejs:1, 95-97`:

```ejs
<nav id="quarto-sidebar" class="sidebar collapse collapse-horizontal quarto-sidebar-collapse-item sidebar-navigation <%- sidebarStyle || (toc ? "floating" : "") %> overflow-auto">
...
  <% if (toc) { %>
    <div id="quarto-toc-target"></div>
  <% } %>
</nav>
<div id="quarto-sidebar-glass" class="quarto-sidebar-collapse-item" ...></div>
```

Asymmetries vs. the standalone path: no `toc-left` class on
`#quarto-content`; the left target lives inside `nav#quarto-sidebar`, not a
separate `#quarto-sidebar-toc-left`.

- **Nav sidebar + left TOC** → one `nav#quarto-sidebar`, nav items first,
  TOC appended after — merged, no second container.
- **No nav sidebar + left TOC (in a website)** → `sidebar.ejs` renders with
  `sidebar === undefined`; the wrapper holds only the TOC target and gets
  class `floating` (ternary fallback). `website-navigation.ts:538-543` then
  latches `floating` onto `<body>`, so the `page-columns-float-*` grids
  apply — **not** the `toc-left` grids, which are gated on
  `body:not(.floating):not(.docked)`.

### 2c. The mover (shared DOM postprocessor)

`src/format/html/format-html-bootstrap.ts:342-412`
(`bootstrapHtmlPostprocessor`):

- `right`/`left` → `toc.remove(); tocTarget.replaceWith(toc)`; adds
  `.toc-active`, `nav-link` classes, `data-scroll-target`, `.collapse` on
  nested `ul`s, `data-toc-expanded`.
- `body` → `tocTarget` is null → TOC left in `main`, **no** decorations
  (plain static list; smoke test
  `tests/docs/smoke-all/issues/3473-toc-side-body/body.qmd`).
- `left-body`/`right-body` → clone with `id="TOC-body"` (`.toc-actions`
  stripped) inserted before the original, original moved to the sidebar.

## 3. Layout / CSS

SCSS (already ported into q2's `resources/scss/bootstrap/`):

| What | Q1 file:line |
|---|---|
| `.page-columns.toc-left` wide grid | `_bootstrap-rules.scss:62-70` |
| `.page-columns.toc-left` mid grid | `_bootstrap-rules.scss:145-152` |
| narrow collapse + `nav[role="doc-toc"] { display:none }` | `_bootstrap-rules.scss:219-232` |
| `.sidebar.toc-left` grid placement (`page-start / body-start`) | `_bootstrap-rules.scss:303-306` |
| toc-left margin-element reflow into body column | `_bootstrap-rules.scss:545-557` |
| `#quarto-margin-sidebar` / `#quarto-sidebar-toc-left` hidden below md | `_bootstrap-rules.scss:574-581` |
| sticky sidebar rules | `_bootstrap-rules.scss:1075-1087` |
| sidebar-TOC typography/active/border | `_bootstrap-rules.scss:1201-1330` |
| `page-columns-tocleft-wide/-mid` mixins | `_bootstrap-mixins.scss:1302-1334` |
| grid track vars | `_bootstrap-mixins.scss:846-895` |

`.sidebar.quarto-banner-title-block-sidebar` (`_bootstrap-rules.scss:1090-1094`)
appears dead — nothing assigns the class.

## 4. Banner interaction

`src/format/html/format-html-title.ts:169-175`: when banner (or manuscript)
and `toc-location === "left"` **exactly** (`left-body` misses it — latent Q1
inconsistency), set `templateParams["banner-header-class"] = "toc-left"`.
Consumed by `templates/banner/title-block.html:2` and
`templates/manuscript/title-block.html:1` — same `$if$` hook q2 already
carries at `crates/quarto-core/src/template.rs:337`. Why needed: Q1 moves
the banner header out of `#quarto-content`
(`format-html-title.ts:262-268`), so it loses the inherited `toc-left` grid
and must carry the class itself for its `column-body` to align.

## 5. Standalone `left` output shape

```html
<div id="quarto-content" class="page-columns page-rows-contents page-layout-article toc-left">
  <div id="quarto-sidebar-toc-left" class="sidebar toc-left">
    <nav id="TOC" role="doc-toc" class="toc-active" data-toc-expanded="…"> … </nav>
  </div>
  <div id="quarto-margin-sidebar" class="sidebar margin-sidebar"></div>
  <main class="content" id="quarto-document-content"> … </main>
</div>
```

No `collapse`/`sidebar-navigation`/`floating`/`#quarto-sidebar-glass` —
those are website-only. The empty margin sidebar survives
(`bootstrapHtmlFinalizer` only removes it when there is no TOC and no
margin content, `format-html-bootstrap.ts:1030-1032`) and gets
`zindex-bottom` when empty (`:1084-1091`).

Other consumers of these ids:

- `format-html-bootstrap.ts:857-865` — "Other Formats/Links" target chain:
  `nav[role=doc-toc]` → `#quarto-sidebar-toc-left` → `#quarto-margin-sidebar`.
- `src/resources/formats/html/quarto.js:53-58, 556-572` — runtime toggles:
  `#quarto-margin-sidebar` → `quarto-toc-toggle`, `#quarto-sidebar` →
  `quarto-sidebarnav-toggle`, `#quarto-sidebar-toc-left` →
  `quarto-lefttoc-toggle`. (So in the website left case the TOC gets the
  *sidebarnav* toggle, another behavioral difference between the paths.)

## 6. Gotchas for a q2 implementation

1. `toc-left` as a grid class only exists on the standalone path; websites
   ride `body.floating`/`body.docked` grids. Two layout regimes, one option.
2. `body` = absence of a target = plain undecorated list (no scroll-spy).
3. `*-body` = clone `id="TOC-body"` with `.toc-actions` removed.
4. `banner-header-class: toc-left` gated on `=== "left"` exactly.
5. The article path emits `#quarto-margin-sidebar` unconditionally even for
   `left`; downstream code (`getLinkTarget`, `zindex-bottom`,
   `fullcontent`/`slimcontent` heuristics at
   `format-html-bootstrap.ts:1036-1073`) depends on its presence.
