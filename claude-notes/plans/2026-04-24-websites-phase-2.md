# Phase 2 — Sidebar (data model, generate, render, template)

**Date:** 2026-04-24
**Beads:** to be filed (parent `bd-0tr6`; blocked-by `bd-w5os` Phase 1 — closed).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-23-websites-phase-1.md`
**Status:** Decisions confirmed 2026-04-24. Ready for implementation
pending final go-ahead.

## Goal of this phase

Introduce the first **website-specific** feature: a left-column **sidebar**
with page-to-page navigation, driven by `_quarto.yml`.

Concretely:

1. A `Sidebar` data model in `quarto-navigation`, with `SidebarEntry`
   and `SidebarContents` enums that model the Q1 YAML surface closely
   enough that migrating configs is a copy-paste.
2. A `SidebarGenerateTransform` that reads `website.sidebar` from the
   merged metadata, resolves `auto:` entries by consulting the
   `ProjectIndex` populated in Phase 1, picks the right sidebar for
   the page being rendered, and stores the resolved sidebar at
   `navigation.sidebar` (alongside the existing `navigation.navbar` /
   `navigation.footer`).
3. A `SidebarRenderTransform` that emits Bootstrap-5-compatible HTML
   at `rendered.navigation.sidebar` using Q1-matching class names so
   existing Q1 CSS (`resources/scss/`) continues to style us without
   modification.
4. A new template slot `$rendered.navigation.sidebar$` in the full
   HTML template, wrapped in `$if(...)$` so single-doc renders remain
   unchanged.
5. **Active-item highlighting** for the current page — computed in
   Generate by comparing the sidebar item's source path to the
   current page's `DocumentProfile.source_path`; rendered in Render
   as `class="... active"`, with all ancestor sections marked
   `expanded: true`. Active-state computation is format-agnostic
   (no `.html` reference anywhere in Generate).
6. **Sidebar-for-page selection** (Q1 `sidebarForHref` equivalent):
   resolve which of multiple sidebars applies to the current page,
   by explicit id (via `site-sidebar: <id>` document-metadata
   override) or by containment (which sidebar's contents reference
   this page's source path). Single-sidebar projects get it for free.

**No user-visible behavior change for existing single-doc renders.**
The generate transform is a silent no-op without a `website.sidebar`
key; the render transform is a no-op without `navigation.sidebar`; the
template slot is conditional.

This phase does **not** implement:

- **Cross-document link rewriting** — `[link](other.qmd)` → `other.html`
  in *body* content is Phase 6. The sidebar's *own* `.qmd` → `.html`
  href rewriting **is** in scope because it's necessary for the
  sidebar to link to anything useful at all.
- **`site_libs/` / shared artifact store** — Phase 5. The sidebar's
  own CSS/JS (collapse toggle behaviour) in this phase is not emitted
  yet; Phase 5 takes over theme-asset plumbing and the sidebar's
  collapse-JS will ride along with it. Phase 2 produces structural
  HTML that Q1 CSS styles; interactive collapse is deferred.
- **Page navigation (prev/next)** — Phase 4.
- **Navbar project integration / active highlighting** — Phase 3.
  Phase 2 only touches sidebars.
- **Sitemap, favicon, site-url/title** — Phase 7.
- **Search, tools, reader mode, dark toggle, logo/header/footer slots
  on the sidebar** — excluded at the epic level.
- **Book / Manuscript types** — still just-a-default.

## Reference material

- **Parent epic plan** §"Phase 2 — Sidebar", §"Architecture sketch",
  and §"Cross-document index".
- **Phase 1 plan** §"Injecting `ProjectIndex` into per-file rendering"
  — the `StageContext::project_index` / `RenderContext::project_index`
  slots are already wired; Phase 2 is the first consumer.
- **`DocumentProfile` contract** —
  `claude-notes/designs/document-profile-contract.md`. Every profile
  has `source_path`, `output_href`, `title`, `draft`, `outline`. This
  is enough to render sidebars; we do *not* need to extend the profile
  for Phase 2.
- **Q1 reference implementation:**
  - Type definitions:
    `external-sources/quarto-cli/src/project/types.ts:271–313`
    (`Sidebar`, `SidebarItem`).
  - Config read + normalization:
    `external-sources/quarto-cli/src/project/types/website/website-shared.ts:123–280`
    (`websiteNavigationConfig`).
  - Sidebar-for-href resolution:
    `external-sources/quarto-cli/src/project/types/website/website-shared.ts:403–427`
    (`sidebarForHref`) and 470–495 (`containsHref`).
  - Auto expansion:
    `external-sources/quarto-cli/src/project/types/website/website-sidebar-auto.ts`
    (the whole file).
  - HTML emission templates:
    `external-sources/quarto-cli/src/resources/projects/website/templates/sidebar.ejs`,
    `sidebaritem.ejs`.
  - CSS (not touched this phase but structurally relevant):
    `external-sources/quarto-cli/src/resources/formats/html/bootstrap/...`
    for the `.sidebar-*` class vocabulary.
- **Q2 current code:**
  - `crates/quarto-navigation/src/{item,navbar,footer,render_html}.rs`
    — the Generate/Render pattern to mirror.
  - `crates/quarto-core/src/transforms/navbar_{generate,render}.rs`
    — the transform pattern to mirror.
  - `crates/quarto-core/src/template.rs` lines 126–224 — where the
    new template slot lands.
  - `crates/quarto-core/src/pipeline.rs:605–615` — where the new
    `SidebarGenerateTransform` / `SidebarRenderTransform` slot into
    the transform pipeline.
  - `crates/quarto-core/src/project/index.rs` — `ProjectIndex` API
    (lookup_by_source / lookup_by_href / profiles).

## Key decisions (to confirm)

These are proposals; the user will confirm or amend before
implementation begins.

### Decision 1 — Config path: `website.sidebar`, not top-level `sidebar` (confirmed)

Q1 reads sidebars from `website.sidebar`, and bare files sometimes
set a document-level `site-sidebar: <id>` to pick which sidebar
applies. The existing Q2 navbar transform reads from the top-level
`navbar` — which works for single-doc renders but would collide with
Q1's `navbar: object | boolean` convention once `website.navbar`
support lands in Phase 3. For this phase:

- `SidebarGenerateTransform` reads `ast.meta.website.sidebar` only.
- Accepts **either** a single `Sidebar` object **or** an array of
  `Sidebar`s (matches Q1).
- The per-page sidebar id override reads `ast.meta.site-sidebar`
  (document-level), again matching Q1's `kSiteSidebar`.

**Why not support top-level `sidebar:` as a convenience alias?** One
way to do it avoids ambiguity — `website.sidebar` is always the
canonical location — and keeps the config surface honest. If a user
wants the shorthand, their `_quarto.yml` already nests everything
under `website:`.

**Epic-level follow-up (user-directed 2026-04-24):** this leaves Q2
with an inconsistent config surface — `navbar` lives at the top
level, `sidebar` lives under `website.`. That will be hard to teach.
Either everything should move to the top level or everything should
move under a shared nav namespace. **Recorded in the parent epic plan
as a follow-up task** so it doesn't get lost between phases.

### Decision 2 — Trait for "give me all profiles" in generate (confirmed)

`SidebarGenerateTransform` runs inside `AstTransformsStage`, which
bridges `StageContext` to `RenderContext`. `RenderContext` already
has `project_index: Option<Arc<ProjectIndex>>` — Phase 1 shipped it
*unused*. Phase 2 is the first reader. When `project_index` is `None`
(standalone render), the generate transform **emits the sidebar
anyway** if the user wrote one by hand, but `auto:` entries resolve
to empty (with a diagnostic). This preserves the single-doc no-op
when the user didn't configure a sidebar, and matches the principle
from Phase 0 "no `is_project?` branch": the sidebar transform works
the same whether it's a one-file project or a hundred-file project.

### Decision 3 — Crate placement (confirmed)

Data types + HTML rendering → `quarto-navigation`.
Generate/Render transforms → `quarto-core/src/transforms/`.

Mirrors navbar and footer exactly. `quarto-navigation` adds one new
module (`sidebar.rs`) and extends `render_html.rs` with a public
`sidebar_to_html` function. The crate's `README`-ish lib docs are
updated to mention sidebars.

### Decision 4 — Sidebar contents model (confirmed)

Q1's `SidebarItem` is a single struct with optional `contents`,
`section`, `auto`, `href`, `text` fields — effectively a discriminated
union encoded by presence. Q2 uses Rust: model it as an enum so the
shape is type-enforced.

Proposed:

```rust
pub struct Sidebar {
    pub id: Option<String>,
    pub title: Option<ConfigValue>,
    pub subtitle: Option<ConfigValue>,
    pub style: SidebarStyle,          // Docked | Floating; default Floating
    pub collapse_level: u32,          // default 2
    pub background: Option<String>,   // Bootstrap color name or CSS color
    pub contents: Vec<SidebarEntry>,
    pub pinned: bool,                 // default false
}

pub enum SidebarStyle { Docked, Floating }

pub enum SidebarEntry {
    /// A leaf link. From `- about.qmd`, `- {href: …, text: …, icon: …}`,
    /// or `- {section: <path>}` with no `contents:` (rare).
    Link(NavigationItem),

    /// A nested section with children. From
    /// `- section: Title\n  contents: […]` or
    /// `- {section: index.qmd, contents: […]}`.
    Section {
        /// Display text (from `section:` key) or `None` if the section
        /// is keyed on an href.
        text: Option<ConfigValue>,
        /// Optional link for the section's header row.
        href: Option<String>,
        /// Section id (auto-generated from text if absent).
        id: Option<String>,
        /// Child entries.
        contents: Vec<SidebarEntry>,
        /// `expanded: true` forces open regardless of collapse-level;
        /// computed by active-highlighting when an active child is inside.
        expanded: bool,
    },

    /// A visual separator. From `- ---` (three or more dashes).
    Separator,

    /// Plain text heading (no link, no children, no icon). From
    /// `- {text: "Label"}` with nothing else.
    Heading(ConfigValue),

    /// An `auto:` directive. Replaced by concrete entries during
    /// `SidebarGenerateTransform::transform` — this variant does not
    /// survive into the rendered output.
    Auto(AutoSpec),
}

pub enum AutoSpec {
    All,                 // `auto: true`
    Path(String),        // `auto: "docs"` or `auto: "docs/*"`
    Paths(Vec<String>),  // `auto: ["intro.qmd", "advanced/*"]`
}
```

`NavigationItem` is the existing shared shape (already used by navbar
and footer). Reusing it keeps icons, `aria-label`, `rel`, `target`
consistent across all nav surfaces.

**Hrefs stay as author source paths.** A leaf `Link` whose YAML said
`about.qmd` carries exactly `about.qmd` through Generate. The
format-specific rewrite to `about.html` happens in Render (see
Decision 7+8). External URLs (http(s), mailto, etc.) pass through
unchanged at both steps.

**Active and expanded state** is computed by the generate transform
using source-path equality — not output-href equality — so the
Generate result is format-agnostic (see Decision 7+8). The
`Section::expanded` field is output-only; the YAML `expanded: true`
override gets folded in first, then active-state expansion overrides
it to `true` wherever it hits. The leaf `Link` variant gains an
equivalent `active: bool` flag, defaulted `false`.

The `SidebarEntry::Link` form therefore looks like this, slightly
enriched from the bare `NavigationItem`:

```rust
pub enum SidebarEntry {
    Link { item: NavigationItem, active: bool },
    Section { … expanded: bool },
    Separator,
    Heading(ConfigValue),
    Auto(AutoSpec),
}
```

(Or: `item: NavigationItem` stays bare and `active` sits on a parallel
wrapper — the sub-plan's implementation chooses whichever keeps the
code tidy. The contract is that `active` is set by Generate and read
by Render.)

### Decision 5 — Module shape (confirmed)

```
crates/quarto-navigation/src/
    sidebar.rs              # Sidebar, SidebarEntry, AutoSpec, SidebarStyle
    render_html.rs          # add sidebar_to_html(); existing navbar_to_html stays
    lib.rs                  # re-export

crates/quarto-core/src/transforms/
    sidebar_generate.rs     # SidebarGenerateTransform
    sidebar_render.rs       # SidebarRenderTransform
    sidebar_auto.rs         # expand_auto(project_index, spec) helper,
                            # kept separate because it's ~200 LOC and
                            # testable in isolation
```

### Decision 6 — Sidebar-for-page selection (confirmed, with caveat)

Mirrors `sidebarForHref` in `website-shared.ts:403`, but operates on
source paths rather than output hrefs so it stays format-agnostic:

1. If the page's metadata sets `site-sidebar: <id>` (or
   `website.sidebar-id: <id>` — see alternative below), prefer the
   sidebar with that `id`.
2. Otherwise, if exactly one sidebar is configured *and* it has no
   `id`, that sidebar applies (Q1 wildcard).
3. Otherwise, find the first sidebar whose contents (recursively)
   reference the current page's source path. The comparison is on
   source paths: an entry `href: about.qmd` matches the page whose
   `DocumentProfile.source_path` is `about.qmd`.
4. Otherwise, no sidebar for this page — `navigation.sidebar` is not
   populated.

For the per-page `site-sidebar` override: Q1 uses the key
`site-sidebar`, which is awkward (dashes) but established. Accept
both `site-sidebar: <id>` (Q1 compat) and `website.sidebar-id: <id>`
(clearer). Rule 1 checks both; document the canonical name as
`site-sidebar` for migration continuity.

**Epic-level follow-up (user-directed 2026-04-24):** same note as
Decision 1 — the `site-sidebar` key lives at the document's top level
while the sidebar config lives under `website.`. When we unify nav
config placement, revisit this key's name too. Recorded in the epic.

### Decision 7 — Active highlighting is format-agnostic; set in Generate (revised 2026-04-24)

User feedback (2026-04-24): the existing
`navbar_generate` / `navbar_render` split draws a sharp line —
Generate is format-agnostic, Render is format-specific. The original
draft put `.qmd → .html` rewriting inside Generate, which silently
violated that invariant. Revised rule:

- **Generate** marks each entry's `active: bool` / section's
  `expanded: bool` by comparing against the current page's
  *source path*, looked up via `ProjectIndex::lookup_by_source`.
  This uses `ctx.document.input`, stays in Rust path space, and never
  references an `.html` extension. A hypothetical future non-HTML
  format (PDF-per-page, reveal.js-per-page, whatever) inherits the
  same `active` data without re-running Generate.
- **Render** reads the resolved `Sidebar` from `navigation.sidebar`
  and emits an HTML `active` class on `active: true` items.

Algorithm, run after auto-expansion, over the chosen sidebar:

1. Resolve the current page's source path from
   `ProjectIndex::lookup_by_source(&ctx.document.input)`. If no index
   (standalone render) or no hit, skip active-marking.
2. Walk the tree, looking for the first `SidebarEntry::Link` whose
   href, interpreted as a project-relative source path, equals the
   current page's source path. Mark it `active: true`.
3. Mark every ancestor `SidebarEntry::Section` `expanded: true`.
4. If no leaf matched but a `Section::href` matches, mark the section
   itself active and its ancestors expanded.

External URLs never match; comparing a source path to `https://…`
yields `false` trivially.

### Decision 8 — `.qmd → .html` href rewriting lives in Render (revised 2026-04-24)

`.qmd → .html` is a format-specific transformation and belongs in
the HTML-aware Render step, not Generate. Revised plumbing:

- Generate stores a `Sidebar` at `navigation.sidebar` whose hrefs are
  the author's source paths (e.g. `about.qmd`) and/or external URLs.
- `SidebarRenderTransform` resolves each href at emit time:
  - If `ProjectIndex::lookup_by_source(href)` hits, emit the
    profile's `output_href` (Phase 1 `DocumentProfile` already
    computes this as the HTML output-relative path).
  - Otherwise (external URL, `#` fragment, unknown local path),
    emit the string unchanged.
  - On a miss-that-looks-like-a-source-path (`ends_with(".qmd")` and
    no index hit), emit a `DiagnosticMessage::warning` naming the
    sidebar and the missing target. The href is left unchanged (Q1
    dangles silently; we do better with a diagnostic).

This keeps the invariants clean:

- `navigation.sidebar` (post-Generate) is format-agnostic; a future
  PDF-per-page render could reuse it and apply its own link rewrite.
- `rendered.navigation.sidebar` (post-Render) is HTML.
- Body-content `[link](x.qmd)` rewriting is still Phase 6.

**Why not do the rewrite in a separate "normalize hrefs" transform
that runs before Render?** Because that would re-introduce the same
format-coupling the split is meant to avoid: anyone implementing a
non-HTML output format would still have to run or skip the rewrite.
Having Render own format-specific concerns is the cleanest contract.

### Decision 9 — Collapse-level default (confirmed)

Q1 default is `2`. Q2 matches. YAML-configurable via `collapse-level`.

### Decision 10 — Defer: search, tools, logo, sidebar-header/footer (confirmed)

Epic excludes search; tools are mostly search + reader/dark toggles.
Deferring these simplifies this phase substantially. Logo/subtitle
*display* on the sidebar is deferred too — v1 supports `title` only.
If a user writes `logo:` or `tools:` in their sidebar config, the
YAML is parsed and stored but the renderer ignores it. Follow-ups
will slot them in with the same Q1 class names.

### Decision 11 — Style: Docked vs Floating (confirmed)

Q1 `docked` = sidebar always visible, takes layout column. `floating`
= sidebar overlays on narrow viewports and sits beside on wide.
Phase 2 emits both with the same Q1 CSS classes (`sidebar-docked` /
`sidebar-floating`). No JS is emitted yet; the `collapse`/`expand`
chevrons are inert until Phase 5 lands the collapse-toggle JS (or a
trivial inline `<details>`-based fallback, to be decided when JS
plumbing lands).

## Architecture sketch

### Pipeline position

`SidebarGenerateTransform` and `SidebarRenderTransform` join the
navigation phase, alongside navbar/footer/toc:

```
…
TocGenerateTransform
NavbarGenerateTransform
SidebarGenerateTransform        ← NEW
FooterGenerateTransform
TocRenderTransform
NavbarRenderTransform
SidebarRenderTransform          ← NEW
FooterRenderTransform
…
```

The order inside each mini-phase doesn't matter (each transform reads
its own slice of metadata), but keeping the Generate-then-Render
split visible preserves the existing invariant: "all structured data
at `navigation.*` is populated before anything reads it for HTML".

### Generate transform flow (format-agnostic)

```
SidebarGenerateTransform::transform(ast, ctx):
    if feature-disabled (`sidebar: false`): return.
    if `navigation.sidebar` already populated: return.  // user override

    sidebars_yaml = ast.meta.get_path(["website", "sidebar"])
    if sidebars_yaml is None: return.

    sidebars = Sidebar::parse_list_from_config(sidebars_yaml)
                // accepts single object or array

    // Sidebar-for-page selection uses source paths, not output hrefs.
    picked = sidebar_for_page(&sidebars, ctx.project_index,
                              &ctx.document.input, &ast.meta)
    if picked is None: return.

    let mut sb = picked.clone();
    if let Some(idx) = ctx.project_index.as_deref() {
        expand_auto(&mut sb, idx, &mut diagnostics);
        // Compare against source path, not output href, so this step
        // doesn't hard-code HTML.
        let self_source = idx.lookup_by_source(&ctx.document.input)
                             .map(|p| p.source_path.clone());
        if let Some(src) = self_source {
            resolve_active_state(&mut sb, &src);
        }
    } else {
        // No index → auto expands to empty (with a warning).
        strip_auto(&mut sb, &mut diagnostics);
    }

    ast.meta.insert_path(&["navigation", "sidebar"], sb.to_config_value());
```

The stored `navigation.sidebar` still carries `.qmd` paths; the
Render transform rewrites them. `active: true` / `expanded: true`
flags are already set on the entries they apply to.

`sidebar_for_page` implements Decision 6 — source-path comparison,
no HTML assumptions.

### Render transform (HTML-specific)

Reads `navigation.sidebar`, resolves any project-relative source
paths through `ProjectIndex::lookup_by_source` to the profile's
`output_href`, then calls `sidebar_to_html`. Emits the result at
`rendered.navigation.sidebar`. Skip conditions mirror the navbar
render:

- `sidebar: false` at document or project level.
- `rendered.navigation.sidebar` already populated (user filter
  pre-rendered).
- `navigation.sidebar` absent.

The href-resolution helper is a tight function:

```rust
fn resolve_href_for_html(
    raw: &str,
    index: Option<&ProjectIndex>,
    diagnostics: &mut Vec<DiagnosticMessage>,
    sidebar_id: Option<&str>,
) -> String {
    // External URLs, anchors, mailto: → unchanged.
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Project-relative source lookup.
    if let Some(idx) = index {
        if let Some(profile) = idx.lookup_by_source(Path::new(raw)) {
            return profile.output_href.clone();
        }
    }
    // Looks like a source path but didn't resolve → warn.
    if raw.ends_with(".qmd") {
        diagnostics.push(DiagnosticMessage::warning(format!(
            "Sidebar{} references unknown document {}",
            sidebar_id.map(|i| format!(" '{}'", i)).unwrap_or_default(),
            raw
        )));
    }
    raw.to_string()
}
```

Called once per `href`-bearing sidebar entry immediately before
emission.

### `sidebar_to_html` (rendering)

Class vocabulary to match Q1 (so `resources/scss/` Just Works):

- `<nav id="quarto-sidebar" class="sidebar sidebar-docked|sidebar-floating">`
- `<div class="sidebar-menu-container">`
  `<ul class="list-unstyled mt-1">`
- Leaf: `<li class="sidebar-item">
         <div class="sidebar-item-container">
           <a class="sidebar-item-text sidebar-link [active]" href="…">…</a>
         </div>
       </li>`
- Section:
  `<li class="sidebar-item sidebar-item-section">
     <div class="sidebar-item-container">
       <a class="sidebar-item-text sidebar-link" href="…">…</a>
       <a class="sidebar-item-toggle [collapsed]"
          data-bs-toggle="collapse" data-bs-target="#<section-id>"
          aria-expanded="true|false">
         <i class="bi bi-chevron-right ms-2"></i>
       </a>
     </div>
     <ul id="<section-id>" class="collapse list-unstyled sidebar-section depth<N> show|">
       [children…]
     </ul>
   </li>`
- Separator: `<li class="px-0"><hr class="sidebar-divider"></li>`
- Heading (text, no link): `<li class="sidebar-item">
     <span class="menu-text">…</span>
   </li>`

Icons use the same `<i class="bi bi-…">` shape as the navbar renderer.
Section IDs are stable hashes of the section's path for predictable
`#anchors`.

### Auto expansion

`expand_auto(sidebar, index)` walks the contents tree, replacing each
`SidebarEntry::Auto(spec)` with a flattened list derived from the
`ProjectIndex`. Algorithm:

1. Collect candidate profiles: for `AutoSpec::All`, every profile in
   the index. For `Path("docs")` or `Path("docs/*")`, every profile
   whose `source_path` is under `docs/`. For `Paths([…])`, the union
   of each pattern's matches.
2. Exclude `index.qmd`-style top-level index files from the auto
   expansion's *sibling* level (Q1 behaviour: the directory's index
   page becomes the section's `href`, not an item inside it).
3. Exclude profiles with `draft: true` (TODO: respect a
   `draft-mode: include|exclude` project option — for Phase 2,
   **always exclude drafts**).
4. Group by directory. A subdirectory with its own `index.qmd`
   becomes a `Section` whose `href` is the index's **source path**
   (`docs/index.qmd`, not `.html` — the Render step rewrites) and
   whose `text` is the index page's title; its `contents` are the
   siblings. A subdirectory without an index becomes a `Section`
   with no href, text = capitalized directory name.
5. Sort within each directory by:
   - explicit `order:` frontmatter (asc),
   - then by title (case-insensitive, alphabetical).
   Deterministic; matches Q1.

Requires one new `DocumentProfile` field — **`order: Option<i32>`**
extracted from frontmatter. This is an additive change to the
profile: its default is `None`, so the `profile_version` does **not**
need to bump (see the contract doc). But an entry in the contract's
change log is required.

### Template slot

In the full HTML template (`crates/quarto-core/src/template.rs`
lines 160–215), add a conditional sidebar block alongside the
existing TOC block. Proposed placement: inside
`<div id="quarto-content">`, just before the TOC column:

```html
$if(rendered.navigation.sidebar)$
<div id="quarto-sidebar-container" class="sidebar-column">
$rendered.navigation.sidebar$
</div>
$endif$
```

Bootstrap-grid math is the same as Q1's layout; when Phase 5 lands
the theme CSS, the existing Q1 grid rules (`.sidebar` + main content)
apply. We do not edit `crates/pampa/resources/templates/html/main.html`
(the minimal template) — sidebars only appear in the full template.

### Data flow summary

```
_quarto.yml  →  MetadataMergeStage  →  ast.meta.website.sidebar
                                         (raw YAML ConfigValue)
                                     ↓
                            SidebarGenerateTransform
                            (reads raw YAML + project_index)
                                     ↓
                            ast.meta.navigation.sidebar
                            (resolved Sidebar as ConfigValue)
                                     ↓
                            SidebarRenderTransform
                                     ↓
                     ast.meta.rendered.navigation.sidebar
                            (HTML string)
                                     ↓
                            ApplyTemplateStage
                                     ↓
                   <nav id="quarto-sidebar">…</nav> in output
```

## DocumentProfile change

One additive field:

```rust
pub struct DocumentProfile {
    …
    /// `order:` frontmatter value, used by auto-sidebar to sort
    /// entries. `None` when the author didn't specify.
    pub order: Option<i32>,
}
```

Extracted in `DocumentProfileStage` by calling `meta.get("order")`
and coercing to `i32` (bool/string rejected). Treated as additive,
so no version bump; contract doc's change log adds a line.

## Tests (TDD: write and fail first)

Per CLAUDE.md §"TEST-DRIVEN DEVELOPMENT": every test is authored
before the code that makes it pass.

### Unit tests — `quarto-navigation::sidebar`

1. **`parse_sidebar_single_object`** — `website.sidebar: {contents: [a.qmd, b.qmd]}`
   parses into one `Sidebar` with two `SidebarEntry::Link`s.
2. **`parse_sidebar_array_form`** — `website.sidebar: [{id: main, contents: [...]}, {id: other, contents: [...]}]`
   parses into two `Sidebar`s.
3. **`parse_sidebar_nested_section`** — `{section: "Docs", contents: [x.qmd]}`
   becomes `SidebarEntry::Section { text: "Docs", contents: [Link(x.qmd)] }`.
4. **`parse_sidebar_auto_variants`** — `auto: true`, `auto: "docs"`,
   `auto: ["a", "b"]` each produce the right `AutoSpec`.
5. **`parse_sidebar_separator`** — string of three dashes produces
   `SidebarEntry::Separator`.
6. **`parse_sidebar_defaults`** — missing `style` → `Floating`,
   missing `collapse-level` → `2`.
7. **`roundtrip_sidebar_to_config_value`** — full sidebar survives
   `to_config_value`+`from_config_value` with fields intact.
8. **`sidebar_render_minimal_manual`** — snapshot-style assertion
   over the HTML for a two-entry manual sidebar; class names match
   the Q1 vocabulary listed in "sidebar_to_html".
9. **`sidebar_render_nested_section_collapsed`** — a section with
   `expanded: false` renders with `aria-expanded="false"` and the
   `.collapse` class without `.show`.
10. **`sidebar_render_nested_section_expanded`** — same with
    `expanded: true`; `.show` present, `aria-expanded="true"`.
11. **`sidebar_render_active_leaf`** — a leaf with `active: true`
    gets `class="…sidebar-link active"`.
12. **`sidebar_render_separator`** — renders `<hr class="sidebar-divider">`.
13. **`sidebar_render_heading_plain_text`** — a `Heading` entry with
    markdown inlines renders escaped inline HTML (no `<a>`).

### Unit tests — `sidebar_for_page` resolution

14. **`resolve_single_sidebar_without_id_matches_every_page`**.
15. **`resolve_explicit_id_override_wins`** — two sidebars with ids
    `main` and `reference`, page sets `site-sidebar: reference` →
    `reference` returned.
16. **`resolve_containment_fallback`** — no explicit id, two sidebars
    each with distinct contents; current page's source path
    `docs/api.qmd` is referenced in the second sidebar → second
    sidebar returned. (Comparison is source-path-keyed, per
    Decision 6.)
17. **`resolve_no_match_returns_none`** — no explicit id, page not
    referenced in any sidebar → `None`.
18. **`resolve_containment_checks_nested_sections`** — page source
    path only appears inside a `Section` → still matches.

### Unit tests — auto expansion

19. **`auto_true_lists_all_renderable_profiles`** — 3 profiles,
    `auto: true`, result is 3 `Link` entries in deterministic order.
20. **`auto_excludes_index_as_sibling`** — 3 profiles with one
    `index.qmd`, `auto: true` returns 2 `Link`s (index is not a
    sibling).
21. **`auto_path_scopes_to_subdir`** — profiles `a.qmd`, `docs/b.qmd`,
    `docs/c.qmd`; `auto: docs` → 2 entries (`b`, `c`).
22. **`auto_groups_into_section_with_index`** — profiles
    `docs/index.qmd`, `docs/b.qmd`, `docs/c.qmd`; `auto: true`
    produces a `Section` with `href = docs/index.qmd` (Generate
    stays format-agnostic — see Decision 7/8), title from index
    profile, and two children. The Render test suite (28/28a)
    covers the follow-up `.qmd → .html` rewrite.
23. **`auto_sorts_by_order_then_title`** — profiles with
    `order: 1` / `order: 2` / no order are sorted 1, 2, then alpha by
    title.
24. **`auto_drops_drafts`** — draft profile excluded.
25. **`auto_without_index_is_noop`** — standalone render (no
    `project_index`) with `auto: true` logs a diagnostic and emits
    no items.

### Unit tests — active-state resolution (Generate, format-agnostic)

26. **`active_state_marks_leaf_and_expands_ancestors`** — two-level
    section, active page is inside the inner section → inner and outer
    sections become `expanded: true`; the matching leaf is `active`.
    Assertion is on the `active: bool` / `expanded: bool` fields of
    the resolved `Sidebar` — no HTML involved.
27. **`active_state_no_self_source_no_changes`** — if the current
    page's source path isn't referenced in the sidebar, nothing is
    marked active.
27a. **`active_state_is_source_path_keyed`** — a sidebar link to
    `about.qmd` matches the current-page profile whose `source_path`
    is `about.qmd`, regardless of what `output_href` would be.
    Proves the Generate-step active logic is format-agnostic. The
    test builds a mock `ProjectIndex` whose profile has a synthetic
    `output_href: "about.foo"` and still marks active correctly.

### Unit tests — href rewriting (Render, HTML-specific)

28. **`render_rewrites_qmd_hrefs_to_output_href`** — a `Link`
    entry with `href: "about.qmd"` + a profile whose
    `output_href: "about.html"` produces `<a href="about.html">` in
    the rendered HTML.
28a. **`render_rewrites_nested_qmd_hrefs`** — `docs/api.qmd`
    becomes `docs/api.html` (tests subdirectory preservation).
28b. **`render_passes_external_urls_through_unchanged`** —
    `href: "https://example.com"` is emitted verbatim.
28c. **`render_passes_fragment_anchors_unchanged`** —
    `href: "#section"` is emitted verbatim.
29. **`render_qmd_href_lookup_miss_emits_diagnostic`** — a link to
    `missing.qmd` is left unchanged and a warning diagnostic is
    pushed. The warning names the sidebar id when the sidebar has one.
29a. **`render_works_without_project_index`** — the render transform
    is still callable when `ProjectIndex` is `None` (standalone
    render with a hand-written sidebar that only uses external
    URLs). Raw hrefs pass through; no diagnostics.

### Integration tests — transforms

30. **`sidebar_generate_skips_when_feature_disabled`** —
    `sidebar: false` at document level, no `navigation.sidebar`.
31. **`sidebar_generate_skips_when_absent`** — no `website.sidebar`,
    no `navigation.sidebar`.
32. **`sidebar_generate_honors_user_override`** — if
    `navigation.sidebar` is pre-populated by a user filter, the
    transform leaves it alone.
33. **`sidebar_generate_produces_resolved_tree`** — end-to-end on a
    small fixture: yaml in, resolved `navigation.sidebar` tree out
    (auto expanded, hrefs rewritten, active marked).
34. **`sidebar_render_skips_when_missing`** — no `navigation.sidebar`,
    no `rendered.navigation.sidebar`.
35. **`sidebar_render_produces_html`** — round-trip
    generate→render→read back the HTML from
    `rendered.navigation.sidebar`, assert structure.

### Pipeline / integration tests — `crates/quarto-core/tests/`

Add new test file `sidebar_pipeline.rs`:

36. **`pipeline_renders_sidebar_for_two_page_website`** — fixture with
    `_quarto.yml: { project: { type: website }, website: { sidebar:
    { contents: [index.qmd, about.qmd] } } }`; render both pages;
    assert each output HTML contains `<nav id="quarto-sidebar"`, with
    the current page's link carrying `active`, and the other not.
37. **`pipeline_auto_sidebar_lists_all_pages`** — same shape but
    `website.sidebar: { contents: [{auto: true}] }`; assert both
    pages are in each rendered output.
38. **`pipeline_multiple_sidebars_select_by_containment`** — 4 pages,
    two sidebars (contents `[a, b]` and `[c, d]`); pages a and b
    render with the first sidebar; c and d with the second.
39. **`pipeline_cross_page_links_are_written_as_html`** — sidebar
    entry `about.qmd` produces `href="about.html"` in the rendered
    HTML of both pages (validates that the Render-step rewrite runs
    inside the full pipeline, not just in isolated unit tests).
39a. **`pipeline_navigation_sidebar_preserves_qmd_paths`** — inspect
    `ast.meta.navigation.sidebar` *between* Generate and Render
    (via a test-only transform that snapshots it) and assert the
    paths are still `.qmd`. Proves the format-agnostic invariant
    survives end-to-end.

### CLI end-to-end — `crates/quarto/tests/`

40. **`cli_renders_website_with_sidebar`** — extend an existing
    Phase-1 website fixture to add `website.sidebar`; run
    `cargo run --bin quarto -- render <dir>`; assert
    `_site/index.html` contains the expected `<nav id="quarto-sidebar">`
    block and class names.

### Snapshot tests

A few targeted snapshots under `crates/quarto-navigation/snapshots/`
for the three sidebar-HTML shapes (manual, auto-with-sections,
active-marked). Per CLAUDE.md §"Snapshot Test Changes", flag any
unexpected diffs to the user explicitly.

### End-to-end CLI verification

Per CLAUDE.md §"End-to-end verification before declaring success":

- Build a fresh fixture at `/tmp/q2-phase2-smoke/`:
  ```
  _quarto.yml:
    project: { type: website }
    website:
      sidebar:
        title: "Docs"
        contents: [index.qmd, about.qmd, {section: "Guides",
                   contents: [guides/intro.qmd]}]
  ```
  plus the three `.qmd` files.
- Run `cargo run --bin quarto -- render /tmp/q2-phase2-smoke/`.
- Inspect `_site/index.html` and `_site/guides/intro.qmd.html`
  manually:
  - Both contain `<nav id="quarto-sidebar"`.
  - The current page's link carries `class="…active"`.
  - The "Guides" section's expanded state depends on whether the
    current page is inside it.
  - `<a href="about.html">` not `<a href="about.qmd">`.
- Record the raw HTML fragment in the plan's §"Close-out" or in the
  commit message so the human reviewer can verify without re-running.

## Work items (checklist)

### Preparation
- [ ] Re-read `claude-notes/instructions/testing.md`, `coding.md`,
      `review.md`.
- [ ] Confirm user agreement with Decisions 1–11 before starting.
- [ ] Create `bd` issue `Phase 2 — Sidebar`, parent `bd-0tr6`,
      blocked-by `bd-w5os` (closed).
- [ ] Commit directly on `feature/websites` (Phase 1 precedent).

### Profile extension
- [ ] Add `order: Option<i32>` to `DocumentProfile`; extract in
      `DocumentProfileStage`. Update
      `claude-notes/designs/document-profile-contract.md` change log.
- [ ] Unit test: profile carries `order` when frontmatter provides it.

### Data model — `quarto-navigation`
- [ ] Create `sidebar.rs` with `Sidebar`, `SidebarEntry`,
      `SidebarStyle`, `AutoSpec` types.
- [ ] Implement `Sidebar::from_config_value` / `parse_list_from_config`.
- [ ] Implement `Sidebar::to_config_value`.
- [ ] Write unit tests 1–7; run; confirm fail. Implement to pass.

### HTML rendering — `quarto-navigation::render_html`
- [ ] Add `sidebar_to_html(&Sidebar) -> String`.
- [ ] Write unit tests 8–13; run; confirm fail. Implement to pass.
- [ ] Add snapshot tests for three canonical shapes.

### Sidebar-for-page + active state — `quarto-navigation`

Both are format-agnostic and operate on source paths.

- [ ] Add `sidebar_for_page(&[Sidebar], Option<&ProjectIndex>,
      page_source: &Path, meta: &ConfigValue) -> Option<&Sidebar>`.
- [ ] Add `resolve_active_state(&mut Sidebar, self_source: &Path)`
      (source-path keyed; no HTML assumption).
- [ ] Tests 14–18, 26–27, 27a.

### Auto expansion — `quarto-core/src/transforms/sidebar_auto.rs`
- [ ] Implement `expand_auto(&mut Sidebar, &ProjectIndex,
      &mut Diagnostics)`.
- [ ] Tests 19–25.

### Generate transform — `quarto-core/src/transforms/sidebar_generate.rs`

Format-agnostic. Hrefs stay as source paths at this step.

- [ ] Implement `SidebarGenerateTransform` following the flow in
      §"Generate transform flow".
- [ ] Wire into `build_transform_pipeline` between
      `NavbarGenerateTransform` and `FooterGenerateTransform`.
- [ ] Tests 30–33 (`skips_when_feature_disabled`,
      `skips_when_absent`, `honors_user_override`,
      `produces_resolved_tree`).

### Render transform — `quarto-core/src/transforms/sidebar_render.rs`

HTML-specific. Rewrites `.qmd` source paths to `output_href` at emit
time.

- [ ] Implement `SidebarRenderTransform` including
      `resolve_href_for_html` helper per §"Render transform".
- [ ] Wire into `build_transform_pipeline` between
      `NavbarRenderTransform` and `FooterRenderTransform`.
- [ ] Tests 28–29a (href rewriting), 34–35 (skip conditions + basic
      render).

### Template slot — `crates/quarto-core/src/template.rs`
- [ ] Add conditional `$if(rendered.navigation.sidebar)$…$endif$`
      block inside `FULL_HTML_TEMPLATE`.
- [ ] Do NOT touch `crates/pampa/resources/templates/html/main.html`.
- [ ] Verify existing rendered-HTML snapshot tests still pass (no
      diff expected when `navigation.sidebar` absent).

### Integration tests — `crates/quarto-core/tests/sidebar_pipeline.rs`
- [ ] Tests 36–39.

### CLI end-to-end — `crates/quarto/tests/`
- [ ] Test 40.
- [ ] Manual smoke per §"End-to-end CLI verification"; record output
      in commit/plan close-out.

### Verification and close-out
- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace` — all green. Flag any
      snapshot diffs per CLAUDE.md.
- [ ] `cargo xtask lint` passes.
- [ ] `cargo xtask verify` (full) — because this phase touches
      `quarto-core`, `quarto-navigation`, and `DocumentProfile`.
- [ ] File follow-up bd issues for: (a) sidebar search integration,
      (b) sidebar tools (reader/dark/etc.), (c) sidebar logo /
      subtitle / header / footer slots, (d) collapse-toggle JS
      plumbing (rides with Phase 5 site_libs), (e) draft-mode
      visible/include option, (f) `expanded: true` explicit YAML
      override (already parsed but active-state currently overrides
      unconditionally).
- [ ] `br close <phase-2-id> --reason …`.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** single-doc renders regress because
  `SidebarGenerateTransform` is in the standard pipeline.
  *Mitigation:* the transform is a no-op without `website.sidebar` in
  `ast.meta`. The full pipeline tests from Phase 1 must pass
  unchanged. Add a specific `cli_single_file_unchanged_by_phase_2`
  assertion over a Phase-1 fixture's MD5 if needed.
- **Risk:** Q1 class-name drift (someone renames one in
  `quarto-cli/src/resources/formats/html/bootstrap/` and our HTML
  still uses the old name). *Mitigation:* this phase emits *structural*
  HTML; CSS plumbing is Phase 5. We document the expected vocabulary
  in `sidebar.rs` and an explicit comment in `sidebar_to_html` points
  at the Q1 EJS templates as the source of truth.
- **Risk:** ambiguous YAML shapes (single object vs array, `auto`
  key at the contents level vs item level) produce surprising parses.
  *Mitigation:* tests 1–6 exercise every canonical shape; `from_config_value`
  never silently succeeds on malformed input — it returns a diagnostic.
- **Risk:** `sidebar_for_page` picks the wrong sidebar in
  corner-case configurations. *Mitigation:* follow Q1's exact
  algorithm (tests 14–18 codify all four rules); add a focused
  integration test for the "page appears in two sidebars" ambiguity
  (first-match wins, document this loudly).
- **Risk:** auto-expansion produces non-deterministic output due to
  `ProjectIndex`'s internal `HashMap`. *Mitigation:* auto expansion
  iterates `ProjectIndex::profiles()`, which preserves insertion
  order; sorting is explicit and deterministic (order then title).
- **Risk:** active-state highlighting misses because hrefs differ in
  trailing slash / encoding / fragment. *Mitigation:* compare on the
  normalized `output_href` computed by `DocumentProfile`; write a
  test covering a subpath page (`docs/api.html`).
- **Risk:** the transform trait method chain is invoked repeatedly
  and per-file auto expansion is O(N²) across a large project.
  *Mitigation:* Phase 2 accepts this; a follow-up bd issue can memoize
  per-file-resolved sidebars in `ProjectIndex` if it shows up on the
  perf dashboard. In the common case (one sidebar, small project)
  the work is negligible.

## Explicit non-goals for this phase

- No search, tools, reader/dark toggles, logo, subtitle display,
  header/footer slots, or breadcrumbs.
- No `site_libs/` / shared CSS / shared JS — the collapse-toggle
  chevrons exist in the HTML but are inert until Phase 5.
- No page-navigation prev/next (Phase 4).
- No navbar active highlighting (Phase 3).
- No sitemap / favicon (Phase 7).
- No incremental rebuilds (Phase 8).
- No hub-client wiring (Phase 9).
- No book or manuscript types.
- No changes to existing single-doc render behavior.
- No changes to `crates/pampa` templates (full-template-only slot).

## Decisions log (confirmed 2026-04-24)

1. **Config path.** `website.sidebar` only — no top-level `sidebar:`
   shorthand. Follow-up recorded in the epic to unify nav config
   placement across navbar / sidebar / site-sidebar-id.
2. **Sidebar-id override key.** Accept both `site-sidebar` (Q1
   compat) and `website.sidebar-id`; document `site-sidebar` as
   canonical for now. Will be revisited when nav config placement
   unifies (see #1).
3. **Draft handling in auto.** Always exclude drafts in Phase 2.
   Follow-up bd for `draft-mode: include|exclude|visible`.
4. **Template placement.** Sidebar column rendered *before* the TOC
   column, both on the right, for this phase. The Q1 layout puts the
   sidebar on the left and TOC on the right; moving to that layout is
   a separate template-restructuring task, **recorded as a follow-up
   bd issue** so Phase 2 doesn't get blocked on it.
5. **Interactive collapse.** Inert HTML now, live JS with Phase 5
   (`site_libs/`). Collapse JS is delicate and deserves its own
   attention. `<details>` fallback is not worth the markup churn.
6. **Snapshot tests.** Add snapshots for three canonical sidebar
   shapes, but rely primarily on inline string assertions. Website
   templates will churn as Q1 features port over; snapshots would
   make every unrelated template adjustment noisy.

## Epic-level follow-ups recorded this phase

Because this phase surfaces two structural issues that are epic-wide,
not phase-local, they must be recorded in the parent epic plan so
later phases don't forget them:

1. **Nav-config placement inconsistency.** `navbar` lives at the
   top level of document metadata; `sidebar` lives under `website.`;
   `site-sidebar` (the per-page id override) lives at the top level.
   Pick one convention and migrate. The parent epic plan's Work-items
   section gains a new entry tracking this.
2. **Sidebar template placement.** Phase 2 puts the sidebar column
   beside the TOC on the right, which is the minimum-churn slot in
   the existing full HTML template. Q1's convention is sidebar-left,
   TOC-right. Moving to the Q1 layout means restructuring
   `FULL_HTML_TEMPLATE` (and touching any layout-sensitive tests);
   separate task.

## Follow-up beads (to be filed at close-out)

Epic-wide (see §"Epic-level follow-ups"):

- **Unify nav config placement** (`navbar` vs `website.sidebar` vs
  `site-sidebar` override-key). Single epic-level bd; touches navbar,
  sidebar, future footer-project-config, and probably `site-url` /
  `title-prefix` plumbing in Phase 7.
- **Move sidebar to Q1 template position** (sidebar-left,
  TOC-right). Template-restructuring task; separate from the sidebar
  data-model work.

Phase-local:

- Sidebar search integration (depends on search epic).
- Sidebar tools: reader-mode, dark-toggle, etc.
- Sidebar logo / subtitle / header / footer rendering.
- Collapse-toggle JS (ride with Phase 5 site_libs).
- Draft-mode include/exclude option.
- Explicit `expanded: true` in YAML respected through active-state
  resolution (Phase 2 currently lets active-state always override).
- Memoize sidebar-per-page resolution in `ProjectIndex` if auto
  expansion shows up as a perf hotspot on larger projects.
- Sidebar-for-page diagnostic when a page appears in multiple
  sidebars without an explicit `site-sidebar` override.

## Epic-level impact

Phase 2 unlocks the minimum-shape deliverable called out in the epic
under "bd-tr81": a rendered website with navbar + sidebar + shared
resources. Combined with Phases 3 + 5–7, this is the bootstrap surface
for Quarto-2's own documentation site.
