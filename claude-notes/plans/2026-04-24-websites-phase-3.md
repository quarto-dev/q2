# Phase 3 — Navbar / page-footer project integration

**Date:** 2026-04-24
**Beads:** to be filed (parent `bd-0tr6`; blocked-by `bd-9svl` Phase 2 —
closed).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-24-websites-phase-2.md`
**Status:** Decisions 1–8 confirmed 2026-04-24 after design iteration.
Implementation in progress.

## Goal of this phase

Bring the navbar and the page-footer into the project model Phase 1
built and Phase 2 began consuming. Concretely:

1. Keep navbar / page-footer config exactly where it is today — at the
   **top level** of document metadata (`navbar:`, `page-footer:`).
   These are feature-scoped, not website-scoped; they work in
   single-doc contexts (including non-HTML formats like revealjs) and
   in multi-page projects alike. The project-level `_quarto.yml` still
   reaches each document through the existing metadata merge. **No YAML
   surface change.**
2. Resolve each navbar / footer item's href through the `ProjectIndex`
   — `.qmd` source paths become `.html` output hrefs in the HTML render
   step. Items pointing at unknown `.qmd`s emit a warning. When there
   is no index (standalone render) hrefs pass through unchanged.
3. Enrich bare-path items (`- about.qmd`) with the referenced
   document's title from the `ProjectIndex` when no `text:` was
   supplied. Mirrors the Phase 2 sidebar enrichment. No-op when the
   index is absent.
4. Mark the **active** navbar item for the current page, format-agnostic
   (source-path keyed, set in Generate), and render it with Q1-matching
   `active` class in Render. Recurses into dropdown `menu` items.
5. Add a **navbar brand fallback** via `website.title`: the brand
   anchor's label is `navbar.title ?? website.title ?? document.title`
   (in that order). `website.title` is read from the already-merged
   metadata — reading *site-wide* values from under `website.` is
   fine even though the navbar config itself is top-level; the
   feature is "use this title as the brand label," which is genuinely
   a site-scoped setting.
6. **Page-footer** gets the same `.qmd` → `.html` rewrite and text
   enrichment treatment. Footer items do **not** get active marking
   (matches Q1; page-footer is static cross-site chrome).

**No YAML-surface changes.** Existing single-doc renders with
`navbar:` or `page-footer:` at the top level continue to work
unchanged. The changes are all downstream of the YAML read: enrichment,
active-marking, and href-rewriting all silently become no-ops when no
`ProjectIndex` is attached (standalone single-doc renders).

This phase does **not** implement:

- **Cross-document link rewriting in body content** — `[link](other.qmd)`
  in a paragraph is still Phase 6. Phase 3 only rewrites hrefs in the
  structured `navigation.{navbar,footer}` subtree.
- **`site_libs/`, shared CSS/JS** — Phase 5. The navbar emits Bootstrap
  classes as before; theme plumbing is separate.
- **Sitemap, favicon, `<title>` prefix** — Phase 7. The navbar brand
  reads `website.title` but `<title>` tag prefixing is Phase 7.
- **Navbar tools, reader-mode, dark-toggle, search** — excluded from
  the epic MVP.
- **Sub-navbar (book chapter row), breadcrumbs, repo-actions** —
  excluded from the epic MVP.

## Reference material

- **Parent epic plan** §"Phase 3 — Navbar / footer project integration".
- **Phase 2 plan** — the Generate/Render split (§Decision 7/8) and the
  format-agnostic active-state algorithm (§"Generate transform flow")
  are the templates this phase copies. `resolve_href_for_html` in
  `sidebar_render.rs` is the helper this phase extracts and reuses.
- **Phase 2 close-out follow-ups**:
  - `bd-n9dr` (nav-config placement unification) — Phase 3 **refines
    the framing** of this follow-up rather than closing it. The
    original direction was "unify everything under one namespace." The
    new direction: *placement follows feature semantics*. Features that
    only make sense for a website (sidebar, site-url, favicon, title
    prefix) live under `website.`; features that work in single-doc
    and multi-doc contexts alike (navbar, page-footer) live at the top
    level. The dangling inconsistency becomes narrower: `site-sidebar`
    (the per-doc override selecting *which* sidebar applies) lives at
    the doc top level while the sidebar configs themselves live under
    `website.`. Update `bd-n9dr`'s description to match this revised
    framing when the phase lands.
  - `bd-2quy` (`StageContext` ↔ `RenderContext` bridge completeness) —
    Phase 3 is the second consumer of `ctx.project_index` in the
    Generate step, so will surface any remaining gaps if they exist.
- **Q2 current code:**
  - `crates/quarto-navigation/src/{item.rs,navbar.rs,footer.rs,
    render_html.rs}` — data shapes, YAML parsing, HTML emission.
  - `crates/quarto-core/src/transforms/{navbar_generate.rs,
    navbar_render.rs,footer_generate.rs,footer_render.rs}` — current
    transforms that Phase 3 rewires.
  - `crates/quarto-core/src/transforms/sidebar_render.rs:135-181`
    (`resolve_href_for_html`, `is_external`) — the helper to extract.
  - `crates/quarto-core/src/transforms/sidebar_generate.rs:125-171`
    (`enrich_text_from_index`) — the enrichment helper to extract.
  - `crates/quarto-core/src/pipeline.rs:607-619` — navigation phase
    ordering (no change needed — the existing slot works).
  - `crates/quarto-core/src/project/index.rs` — `ProjectIndex` API.
- **Q1 reference:**
  - `external-sources/quarto-cli/src/project/types/website/
    website-shared.ts:123-150` — `websiteNavigationConfig()`: navbar
    read from `website.navbar` via `websiteConfig(kSiteNavbar, ...)`.
  - `external-sources/quarto-cli/src/project/types/website/
    website-shared.ts:489-492` — `itemHasNavTarget()`: exact href
    equality, plus a `/index.html` → `/` forgiveness.
  - `external-sources/quarto-cli/src/project/types/website/
    website-navigation.ts:1161-1170` — per-page active-marking pass.
  - `external-sources/quarto-cli/src/resources/projects/website/
    templates/navitem.ejs` — class vocabulary (`nav-link`, with `active`
    appended when `item.active`).
  - `external-sources/quarto-cli/src/resources/types/schema-types.ts:
    336-344` — `PageFooter` schema (left/center/right each
    `string | NavigationItem[]`).

## Key decisions (confirmed 2026-04-24)

All decisions below were confirmed by the user on 2026-04-24 after
iteration on Decision 1. Decisions 2–8 approved as drafted; Decision 1
was rewritten in response to user feedback (see revision note below).

### Decision 1 — Keep navbar / page-footer config at the top level

User feedback (2026-04-24): the initial draft proposed moving both
under `website.` to mirror Q1 and Phase 2's sidebar. Rejected. The
argument: navbar and page-footer are **feature-scoped, not
website-scoped**. A revealjs deck with a `page-footer:` is a perfectly
reasonable single-doc use case; forcing the user to write

```yaml
website:
  page-footer: ...
```

for a document that has nothing to do with a website is bad UX. The
same argument applies to navbar (single-doc HTML with a navbar is
unusual but coherent).

The design principle this locks in: **placement follows feature
semantics**. Features that only make sense in a multi-page website
(sidebar, sitemap/site-url, favicon, title prefix) live under
`website.`. Features that are per-document chrome — whether the doc
lives alone or in a project — live at the top level. Project-level
`_quarto.yml` still configures them via the existing metadata merge:
setting `navbar: {...}` in `_quarto.yml` flows to each document's
`ast.meta.navbar` as it does today.

Concretely for Phase 3:

- `NavbarGenerateTransform` continues to read `ast.meta.navbar` (no
  change).
- `FooterGenerateTransform` continues to read `ast.meta.page-footer`
  (no change).
- `SidebarGenerateTransform` continues to read `ast.meta.website.sidebar`
  (Phase 2, unchanged).
- **Site-scoped values that navbar/footer care about** —
  `website.title` for brand fallback, potentially `website.site-url`
  later — are read from `ast.meta.website.<key>` at the specific
  render point where they matter. A renderer reading values from two
  namespaces is fine when the two namespaces describe genuinely
  different concepts.

**Compat note.** No existing Q2 tests break; no migration required.
Phase 3 is YAML-surface-compatible with Phase 2. Q1-compatibility
(reading `website.navbar` as an alias) is **not** attempted in this
phase — if real-world Q1 migration shows users stuck on that spelling
we can add a deprecation-warning alias in a follow-up.

### Decision 2 — Active marking: add `active: bool` to `NavigationItem`

Currently, sidebar's `SidebarEntry::Link { item: NavigationItem,
active: bool }` keeps `active` outside `NavigationItem`. Adding the
same flag separately for navbar would be a second parallel-wrapper
pattern. Cleaner to move `active` onto `NavigationItem` once:

```rust
pub struct NavigationItem {
    pub href: Option<String>,
    pub text: Option<ConfigValue>,
    pub icon: Option<String>,
    pub aria_label: Option<String>,
    pub rel: Option<String>,
    pub target: Option<String>,
    pub menu: Vec<NavigationItem>,
    pub active: bool,          // NEW — defaults to false
}
```

Default is `false`; every existing call site uses `..Default::default()`
or field-by-field init. `SidebarEntry::Link` loses its `active` field,
reads `item.active` instead. Sidebar tests assert against `item.active`;
the existing surface churn is small (one struct field, one match arm
simplification).

**Why add the field rather than compute in Render?** Phase 2 Decision 7
locked in "Generate is format-agnostic, Render is format-specific."
Active-marking is source-path-keyed comparison; that's format-agnostic
data. Computing it in Render duplicates the comparison logic per format
and breaks the contract for hypothetical non-HTML outputs.

**Alternative considered:** keep `active` out of `NavigationItem` and
introduce a `NavbarItem` wrapper mirroring `SidebarEntry::Link`. I'm
recommending against it: navbar has `menu` (recursive) and left/right
(flat) — wrapping doubles the type count with no new semantics.

### Decision 3 — Extract `resolve_href_for_html` and `is_external` into a shared module

Phase 2 put these two helpers inside `sidebar_render.rs`. Phase 3
needs the same logic for navbar and footer render. Extract to a new
module:

```
crates/quarto-core/src/transforms/navigation_href.rs
    pub fn resolve_href_for_html(
        raw: &str,
        index: Option<&ProjectIndex>,
        source_label: Option<&str>,   // e.g. "Sidebar 'docs'" or "Navbar" or "Page footer"
        diagnostics: &mut Vec<DiagnosticMessage>,
    ) -> String;
    pub(crate) fn is_external(href: &str) -> bool;
```

`source_label` replaces the Phase 2 `sidebar_id` parameter with a more
general "who's asking" string so the warning diagnostics can name the
context uniformly. The module has no other dependencies.

Phase 2 callers in `sidebar_render.rs` migrate to the new location in
the same phase-3 commit. Their test behavior is unchanged (the
diagnostic wording shifts slightly — explicit in the migration note).

### Decision 4 — Extract `enrich_text_from_index` for navbar/footer reuse

Phase 2 put sidebar's text enrichment inside `sidebar_generate.rs`.
Navbar and footer need the same "bare `- about.qmd` pulls its label
from the index" behavior.

Two ways to share:

- **A.** Extract a generic `enrich_navigation_items(&mut [NavigationItem],
  &ProjectIndex)` that walks navbar `left`/`right` (and recursively
  into `menu`) and footer `Items` regions. Because sidebar items are
  also `NavigationItem`-under-an-enum-wrapper, sidebar's enrichment
  could delegate to this helper for the `Link` case.
- **B.** Keep each transform's enrichment inlined.

A is cleaner and removes the duplication; B is less disruption to
Phase 2 code. Propose **A**, in the shared module:

```
crates/quarto-core/src/transforms/navigation_enrich.rs
    pub fn enrich_navigation_items(
        items: &mut [NavigationItem],
        index: &ProjectIndex,
    );
```

Sidebar's `enrich_text_from_index` becomes a thin wrapper that calls
this for each `SidebarEntry::Link` and recurses into `SidebarEntry::Section`.

### Decision 5 — Active marking algorithm (navbar)

Mirrors sidebar Decision 7. In `NavbarGenerateTransform::transform`:

1. Compute `page_source`: the current doc's project-relative source
   path (forward-slash form). Same helper as sidebar — lift into a
   shared utility (`project_relative_source(ctx)` in a new
   `transforms::navigation_context` module, or just as a free fn).
2. For each navbar item (left, right, recurse into `menu`):
   - If `item.href` is an external URL or a `#`-anchor, leave `active`
     as `false`.
   - Else if `item.href` (interpreted as source path) equals
     `page_source`, mark `item.active = true`.
3. No "expand ancestors" step — navbar has no expansion semantics.
   Dropdown menus just have one active leaf at most.

Unknown-href items (`foo.qmd` with no matching profile) are **not**
marked active; they'll emit the unknown-doc warning at Render time.

Index-forgiveness. Q1's `itemHasNavTarget` also treats
`about/index.html` as matching `about/`. In Q2's source-path space,
that means: an item whose href is `about/index.qmd` matches the page
whose source path is `about/index.qmd`. No ambiguity — we compare
source-to-source, and Q1's slash-forgiveness was about output-href
normalization that the Render step handles.

### Decision 6 — Navbar brand title fallback chain

Current behavior: `Navbar.title == NavbarTitle::Default` falls back to
`ast.meta.title` (the document's title). For a project, each page has
its own title, so each page's navbar shows a different brand label —
wrong.

New chain (in priority order), computed in `NavbarRenderTransform`:

1. `navbar.title == NavbarTitle::Text(cv)` → use `cv`.
2. `navbar.title == NavbarTitle::Default`:
   - If `ast.meta.website.title` exists, use that.
   - Else fall back to `ast.meta.title` (document title).
3. `navbar.title == NavbarTitle::Hidden` → no title.

Reads `website.title` from the already-merged metadata — no new
profile field needed. Single-doc renders without `website.title`
continue to fall through to the document title (no regression).

**Why touch this in Phase 3 rather than Phase 7?** Because it's a
navbar-rendering concern, not a sitemap concern. Phase 7 owns the
HTML `<title>` tag ("page — site" prefix), which is a distinct
concern. Calling `website.title` at render time doesn't prejudge how
Phase 7 emits it.

### Decision 7 — Navbar Generate takes ProjectIndex; Footer Generate does too

Today, `NavbarGenerateTransform::transform` takes `_ctx:
&mut RenderContext` (ignored). Phase 3 actually uses `ctx` for:

- `ctx.project_index` to enrich text and mark active.
- `ctx.project.dir` + `ctx.document.input` to compute `page_source`.
- `ctx.diagnostics` to push unknown-index warnings (rare — Generate
  mostly defers warnings to Render).

Footer Generate takes the same dependency, minus active-marking:

- `ctx.project_index` to enrich text in `FooterRegion::Items`.

The `RenderContext::project_index` plumbing is already in place
(Phase 2 fix `bd-9svl`). Nothing new to wire.

### Decision 8 — Footer items get href rewrite but NOT active marking

Footer items (icons, social links, "copyright" rows) aren't page-
scoped nav. Q1 doesn't mark them active. Phase 3 matches: `FooterRender`
rewrites `.qmd` → `.html` in `FooterRegion::Items`, but doesn't call
the active-state algorithm.

Footer `Text` regions (string values that may include markdown) are
**not** scanned for `.qmd` links in Phase 3 — that's body-content
link rewriting, which is Phase 6's territory.

## Architecture sketch

### Pipeline position — unchanged

The transforms already sit in the right order
(`pipeline.rs:612-619`):

```
TocGenerateTransform
NavbarGenerateTransform   ← Phase 3 rewires internals
SidebarGenerateTransform
FooterGenerateTransform   ← Phase 3 rewires internals
TocRenderTransform
NavbarRenderTransform     ← Phase 3 rewires internals
SidebarRenderTransform
FooterRenderTransform     ← Phase 3 rewires internals
```

No changes to `pipeline.rs`. Phase 3 is internal-only.

### NavbarGenerateTransform (new flow)

```
NavbarGenerateTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "navbar"): return.
    if navigation.navbar already populated: return.

    let Some(mut navbar) = resolve_navbar(&ast.meta) else { return };
    //  ^^^ unchanged — reads top-level ast.meta.navbar as today.

    if let Some(index) = ctx.project_index.as_deref() {
        enrich_navigation_items(&mut navbar.left, index);
        enrich_navigation_items(&mut navbar.right, index);
        let page_source = page_relative_source(ctx);
        mark_active(&mut navbar.left, &page_source);
        mark_active(&mut navbar.right, &page_source);
    }
    // Standalone render (no index): no enrichment, no active marking.
    // The navbar YAML is still honored as-is; hrefs remain as authored.

    ast.meta.insert_path(&["navigation", "navbar"],
                         navbar.to_config_value());
```

`resolve_navbar` stays as-is in `quarto-navigation::navbar` — no
rename, no signature change. Phase 3's work is purely the
ProjectIndex-aware post-processing between resolve and store.

Hrefs in `navigation.navbar` are still source paths (format-agnostic,
per Phase 2 Decision 7). Render rewrites.

### NavbarRenderTransform (new flow)

```
NavbarRenderTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "navbar"): return.
    if rendered.navigation.navbar already populated: return.

    let navbar_cv = ast.meta.get_path(["navigation", "navbar"])?;
    let mut navbar = Navbar::from_config_value(navbar_cv);

    // Rewrite hrefs on all items (left, right, and recursively menu).
    let mut local_diags = std::mem::take(&mut ctx.diagnostics);
    rewrite_navigation_item_hrefs(&mut navbar.left,
                                  ctx.project_index.as_deref(),
                                  "Navbar",
                                  &mut local_diags);
    rewrite_navigation_item_hrefs(&mut navbar.right,
                                  ctx.project_index.as_deref(),
                                  "Navbar",
                                  &mut local_diags);
    ctx.diagnostics = local_diags;

    // Brand fallback chain: navbar.title → website.title → document.title.
    let fallback = brand_title_fallback(&ast.meta);
    let html = navbar_to_html(&navbar, fallback.as_deref());

    ast.meta.insert_path(&["rendered", "navigation", "navbar"],
                         ConfigValue::new_string(&html, SourceInfo::default()));
```

`rewrite_navigation_item_hrefs` walks left/right and recurses into
`menu`. Each item's `href` is passed through `resolve_href_for_html`.

### navbar_to_html changes

- Emit `active` class on `<a class="nav-link ...">` when
  `item.active == true`.
- Dropdown leaves: emit `active` on dropdown-item anchor too (Q1 does
  this for sidebar; we extend to navbar dropdowns for consistency).
- Brand fallback: renderer already accepts `document_title_fallback`;
  the Render transform now computes this via `brand_title_fallback`
  which consults `website.title` first.

### FooterGenerateTransform (new flow)

```
FooterGenerateTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "page-footer"): return.
    if navigation.footer already populated: return.

    let Some(mut footer) = resolve_page_footer(&ast.meta) else { return };
    //  ^^^ unchanged — reads top-level ast.meta["page-footer"] as today.

    if let Some(index) = ctx.project_index.as_deref() {
        enrich_footer_region(&mut footer.left, index);
        enrich_footer_region(&mut footer.center, index);
        enrich_footer_region(&mut footer.right, index);
    }

    ast.meta.insert_path(&["navigation", "footer"], footer.to_config_value());
```

`enrich_footer_region` inspects the region: if `FooterRegion::Items`,
calls `enrich_navigation_items`; else no-op.

### FooterRenderTransform (new flow)

```
FooterRenderTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "page-footer"): return.
    if rendered.navigation.footer already populated: return.

    let footer_cv = ast.meta.get_path(["navigation", "footer"])?;
    let mut footer = PageFooter::from_config_value(footer_cv);

    let mut local_diags = std::mem::take(&mut ctx.diagnostics);
    for region in [&mut footer.left, &mut footer.center, &mut footer.right] {
        if let FooterRegion::Items(items) = region {
            rewrite_navigation_item_hrefs(items,
                                          ctx.project_index.as_deref(),
                                          "Page footer",
                                          &mut local_diags);
        }
    }
    ctx.diagnostics = local_diags;

    let html = page_footer_to_html(&footer);
    ast.meta.insert_path(&["rendered", "navigation", "footer"],
                         ConfigValue::new_string(&html, SourceInfo::default()));
```

### Module shape

```
crates/quarto-navigation/src/
    item.rs                 # add `active: bool` to NavigationItem
    navbar.rs               # unchanged — `resolve_navbar` still reads
                            # top-level `navbar:` as today
    footer.rs               # unchanged — `resolve_page_footer` still
                            # reads top-level `page-footer:`
    render_html.rs          # `navbar_to_html` emits `active` class;
                            # dropdown active handling
    sidebar.rs              # SidebarEntry::Link loses `active` field;
                            # reads `item.active` via NavigationItem
                            # (Phase 2 tests migrate)

crates/quarto-core/src/transforms/
    navigation_href.rs      # NEW — resolve_href_for_html + is_external
                            # (moved from sidebar_render.rs)
    navigation_enrich.rs    # NEW — enrich_navigation_items + text-enrich
                            # helper (consumed by navbar/footer/sidebar)
    navigation_active.rs    # NEW — mark_active(&mut [NavigationItem],
                            # &page_source) + recurse-into-menu logic
    navbar_generate.rs      # rewrite — uses ProjectIndex, marks active
    navbar_render.rs        # rewrite — rewrites hrefs, brand fallback
    footer_generate.rs      # rewrite — uses ProjectIndex
    footer_render.rs        # rewrite — rewrites hrefs in Items regions
    sidebar_render.rs       # migrate to navigation_href::resolve_…
    sidebar_generate.rs     # migrate to navigation_enrich
```

### Data flow summary

```
_quarto.yml       →  MetadataMergeStage  →  ast.meta.navbar
or doc frontmatter                          ast.meta.page-footer
                                            (raw YAML ConfigValue,
                                             top-level as today)
                                          ↓
                     NavbarGenerateTransform / FooterGenerateTransform
                     (read raw YAML + ProjectIndex for post-processing)
                                          ↓
                         ast.meta.navigation.{navbar,footer}
                         (resolved structure as ConfigValue;
                          hrefs still in source-path space;
                          active: bool already set on items)
                                          ↓
                     NavbarRenderTransform / FooterRenderTransform
                     (rewrite hrefs via ProjectIndex;
                      read ast.meta.website.title for brand fallback)
                                          ↓
                   ast.meta.rendered.navigation.{navbar,footer}
                          (HTML strings)
                                          ↓
                            ApplyTemplateStage
                                          ↓
                   navbar/footer injected into output HTML
```

### Template slots — unchanged

`template.rs:162-164` (navbar) and `225-227` (footer) stay as-is.
Phase 3 doesn't restructure the template.

## DocumentProfile change

**None.** Phase 3 reads `source_path`, `title`, and `output_href` —
all present since Phase 1. No profile-version bump.

## Tests (TDD: write and fail first)

Every test authored before the code that makes it pass. Failing baseline
captured before implementation.

### Unit tests — `quarto-navigation::item`

1. **`navigation_item_default_has_inactive`** — the `active` field
   defaults to `false` for every factory path (bare string, object
   form, menu).
2. **`navigation_item_roundtrip_preserves_active`** — `to_config_value`
   emits `active: true` when set; `from_config_value` reads it back.
   (Active state *needs* to roundtrip so the Generate → Render handoff
   via `navigation.navbar` ConfigValue preserves it.)

### Unit tests — `quarto-navigation::navbar` / `footer`

No new tests here. The existing `resolve_navbar` /
`resolve_page_footer` tests already cover the top-level-YAML
parsing contract, and Phase 3 doesn't change that contract. (A
regression guard — "top-level `navbar:` still resolves" — is covered
implicitly by the un-modified Phase 2 tests that exercise these
resolvers.)

### Unit tests — `quarto-navigation::render_html` (navbar)

9. **`navbar_render_emits_active_class_on_leaf`** — navbar with a
   leaf item whose `active: true` emits `class="nav-link active"`.
10. **`navbar_render_no_active_class_when_inactive`** — no `active`
    substring in the emitted `class` attribute.
11. **`navbar_render_active_propagates_into_dropdown_leaves`** —
    a dropdown menu containing an `active` leaf emits the leaf's
    anchor with `class="dropdown-item active"`.
12. **`navbar_render_brand_uses_fallback_string`** — existing test
    `falls_back_to_document_title` continues to pass after the
    fallback helper is extracted.

### Unit tests — `quarto-core::transforms::navigation_href`

13. **Migrated from `sidebar_render.rs`:** the six existing tests
    (`render_passes_external_urls_through_unchanged`, etc.) move to
    `navigation_href.rs` and are renamed to test the extracted helper
    directly. Behavior unchanged.
14. **`resolve_href_source_label_appears_in_diagnostic`** — warning
    for a miss carries the `source_label` string verbatim (e.g.
    `"Navbar"`, `"Page footer"`).

### Unit tests — `quarto-core::transforms::navigation_enrich`

15. **`enrich_fills_missing_text_from_profile_title`** — `[{href:
    "about.qmd"}]` + profile with `title: "About"` → `text: "About"`.
16. **`enrich_does_not_clobber_explicit_text`** — existing `text`
    survives.
17. **`enrich_recurses_into_menu`** — nested dropdown items enriched.
18. **`enrich_skips_external_urls`** — `href: "https://…"` never gets
    `text` filled from index.

### Unit tests — `quarto-core::transforms::navigation_active`

19. **`mark_active_matches_by_source_path`** — an item whose href is
    `about.qmd` gets `active: true` when `page_source == "about.qmd"`.
20. **`mark_active_does_not_match_other_pages`** —
    `page_source == "index.qmd"` leaves `about.qmd` item inactive.
21. **`mark_active_recurses_into_menu`** — an item inside a dropdown
    whose href matches `page_source` becomes active.
22. **`mark_active_skips_external_urls`** — external href never
    matches.

### Unit tests — `navbar_generate` (extended; existing tests preserved)

The existing skip tests (`skips_when_absent`, `skips_when_false`,
`skips_when_bare_true`, `skips_when_navigation_navbar_already_set`,
`populates_navigation_navbar_from_full_config`) remain and continue to
pass unchanged — Phase 3 is additive. New tests:

23. **`navbar_generate_marks_active_item_for_current_page`** —
    two-item navbar (`index.qmd`, `about.qmd`); rendering `about.qmd`
    marks the second item active.
24. **`navbar_generate_does_not_mark_active_without_index`** —
    standalone render (no `ProjectIndex`): active stays `false`
    everywhere.
25. **`navbar_generate_enriches_item_text_from_index`** — bare-path
    item `- about.qmd` gets `text: "About"` from the profile.
26. **`navbar_generate_keeps_qmd_paths`** — the resolved
    `navigation.navbar` still carries `.qmd` hrefs (format-agnostic
    invariant check).
27. **`navbar_generate_no_index_passes_through_unchanged`** — no
    enrichment, no active marking, no diagnostics; navbar structure
    is stored verbatim (regression guard for single-doc, non-website
    formats like revealjs).

### Unit tests — `navbar_render` (extended)

Existing tests (`skips_when_navigation_navbar_missing`,
`skips_when_navbar_false`, `skips_when_prerendered`,
`renders_navbar_html`, `falls_back_to_document_title`) stay. New:

28. **`navbar_render_rewrites_qmd_hrefs_to_output_href`** — leaf items
    `about.qmd` → `href="about.html"` in the emitted HTML.
29. **`navbar_render_rewrites_dropdown_hrefs`** — same in dropdown menu.
30. **`navbar_render_passes_external_urls_through`**.
31. **`navbar_render_emits_diagnostic_for_unknown_qmd`** — `source_label`
    is `"Navbar"`.
32. **`navbar_render_preserves_active_class_on_rewritten_href`** —
    after href rewrite, `class="nav-link active"` is still there (the
    rewrite doesn't clobber `active`).
33. **`navbar_render_brand_uses_website_title_fallback`** —
    `{website: {title: "My Site"}}` with a default navbar title shows
    "My Site" in the brand.
34. **`navbar_render_brand_prefers_navbar_title_over_website_title`**
    — explicit `navbar.title` wins.
35. **`navbar_render_brand_falls_back_to_document_title_when_no_website_title`**
    — regression guard for single-doc renders (existing
    `falls_back_to_document_title` covers part of this; make the
    ordering explicit).
36. **`navbar_render_no_index_passes_hrefs_through_unchanged`** —
    standalone render, no `ProjectIndex`; a navbar entry `about.qmd`
    is emitted verbatim (no rewrite, no diagnostic).

### Unit tests — `footer_generate` (extended)

Existing tests (`skips_when_absent`, `skips_when_false`,
`string_shortcut_populates_center`, `object_form_populates_regions`,
`skips_when_navigation_footer_already_set`) stay. New:

37. **`footer_generate_enriches_items_in_regions`** — bare
    `- about.qmd` in a footer `left` region gets its text enriched
    from index.
38. **`footer_generate_does_not_enrich_text_regions`** — a string
    region survives untouched.
39. **`footer_generate_keeps_qmd_paths`** — format-agnostic invariant.
40. **`footer_generate_no_index_passes_through_unchanged`** —
    standalone render: no enrichment.

### Unit tests — `footer_render` (new)

`footer_render` has no existing Rust tests (the transform is thin and
delegates to `page_footer_to_html`). Phase 3 adds:

41. **`footer_render_rewrites_qmd_hrefs_in_items_region`** — leaf
    `about.qmd` → `about.html`.
42. **`footer_render_leaves_text_regions_unchanged`** — markdown-like
    text in `center` survives.
43. **`footer_render_emits_diagnostic_for_unknown_qmd`** —
    `source_label` is `"Page footer"`.
44. **`footer_render_no_index_passes_hrefs_through`** — standalone
    render: `about.qmd` stays as-is, no diagnostic.

### Integration tests — `crates/quarto-core/tests/`

New file `navbar_footer_pipeline.rs` mirroring `sidebar_pipeline.rs`:

45. **`pipeline_renders_navbar_for_two_page_website`** — fixture
    `_quarto.yml`:
    ```
    project: { type: website }
    navbar: { title: "Site", left: [index.qmd, about.qmd] }
    ```
    Render both pages; each output HTML contains `<nav class="navbar...`,
    with the current page's `nav-link` carrying `active`.
46. **`pipeline_navbar_dropdown_href_rewriting`** — navbar with a menu
    containing `about.qmd` becomes `about.html` in both pages' HTML.
47. **`pipeline_renders_page_footer_for_two_page_website`** — fixture
    with top-level `page-footer: { left: "© 2026", right: [about.qmd] }`
    in `_quarto.yml`; assert both pages' HTML contains
    `<footer class="footer"` and the `about.qmd` href rewritten to
    `.html`.
48. **`pipeline_navbar_active_never_cross_contaminates`** — rendering
    `index.qmd` does not mark the `about.qmd` item active in
    `index.html` output (regression against stateful transform bugs).
49. **`pipeline_navigation_subtree_is_format_agnostic`** — inspect
    `ast.meta.navigation.navbar` between Generate and Render via a
    test-only snapshot transform; assert `.qmd` paths intact, `active`
    booleans present.
50. **`pipeline_single_doc_navbar_unchanged`** — regression: render a
    standalone `doc.qmd` (no `_quarto.yml`) with a top-level `navbar:`
    in its frontmatter; assert the resulting navbar HTML is
    byte-identical to the pre-Phase-3 output (or close to it, modulo
    the new `active` class being absent). Protects the single-doc UX
    story from silent breakage.

### CLI end-to-end

51. **Manual smoke** (per CLAUDE.md §"End-to-end verification"): extend
    `/tmp/q2-phase3-smoke/` fixture:
    - `_quarto.yml`:
      ```
      project: { type: website }
      website:
        title: "Q2 Phase 3 Smoke"
      navbar:
        left:
          - index.qmd
          - text: About
            href: about.qmd
          - text: Docs
            menu:
              - guides/intro.qmd
      page-footer:
        left: "© 2026 Quarto"
        right:
          - { icon: github, href: "https://github.com/quarto-dev/quarto" }
      ```
      (`website.title` stays under `website.` because it *is* a
      site-scoped concept; `navbar` and `page-footer` are at the top
      level.)
    - Three `.qmd` files.
    - `cargo run --bin q2 -- render /tmp/q2-phase3-smoke/`.
    - Inspect each rendered HTML:
      - `<nav class="navbar ...">` with `nav-link` entries pointing at
        `.html` files.
      - Current page's `nav-link` has `active`.
      - Dropdown menu renders with `dropdown-item` entries, rewriting
        `guides/intro.qmd` → `guides/intro.html`.
      - `<footer class="footer">` present; `© 2026 Quarto` in left
        region; github icon link in right region.
      - No warnings for known `.qmd` references; missing ones produce
        diagnostics (test by introducing a `- missing.qmd` temporarily).
    - Record the observed HTML snippet in the commit message or plan
      close-out so reviewers don't need to re-run.
52. **Regression:** run Phase 2 fixtures (`/tmp/q2-phase2-smoke/`)
    unchanged; assert sidebar output is pixel-identical (the sidebar
    Generate/Render migration to shared helpers shouldn't alter output).
53. **Single-doc revealjs smoke:** render a standalone `deck.qmd`
    with `format: revealjs` and a top-level `page-footer: "© 2026"`.
    Confirm the footer renders without any `website.` namespacing
    being required. (Belt-and-suspenders on the UX story the user
    raised when rejecting the original Decision 1.)

### Snapshot tests

None in Phase 3 — the inline asserts listed above cover the vocabulary,
and sidebar Phase 2 chose explicit-asserts over snapshots for the same
reasons (see Phase 2 Decision 6).

## Work items (checklist)

### Preparation
- [x] Re-read `claude-notes/instructions/testing.md`, `coding.md`,
      `review.md`.
- [x] Confirm user agreement with Decisions 1–8 before starting.
- [x] Create `bd` issue `Phase 3 — Navbar / footer project integration`
      (`bd-fqyg`), parent `bd-0tr6`, parent-child dependency linked.
- [x] Commit directly on `feature/websites` (Phase 1 + 2 precedent).

### `NavigationItem` adds `active: bool`
- [x] Add `active: bool` to `NavigationItem` struct (defaults to
      `false`; documented as a Generate→Render handoff field).
- [x] Update `from_config_value` / `to_config_value` roundtrip.
      `active: true` roundtrips; `active: false` is omitted from the
      emitted map (omit-default convention). `active: true` alone no
      longer triggers the all-fields-empty rejection.
- [x] Update `roundtrip_preserves_basic_fields` test to use
      `..NavigationItem::default()`; all other callsites already used
      the default-spread pattern.
- [x] Tests 1 and 2 (+ an additional
      `navigation_item_inactive_omits_active_key` guard on the
      omit-default convention). 84 quarto-navigation tests pass;
      workspace build clean.

### `SidebarEntry::Link` sheds its `active` field
- [x] Remove `active: bool` from `SidebarEntry::Link`; variant is now
      `Link { item: NavigationItem }`.
- [x] `SidebarEntry::from_config_value` no longer extracts `active`
      separately; `NavigationItem::from_config_value` handles it.
- [x] `SidebarEntry::to_config_value` for Link delegates directly to
      `item.to_config_value()` (was a custom re-packaging path).
- [x] Migrate all pattern matches — sidebar.rs (9 sites incl. tests),
      render_html.rs (1 renderer + 2 test helpers), sidebar_auto.rs
      (2 sites), sidebar_render.rs (1 rewriter + 2 tests),
      sidebar_generate.rs (1 enrichment site).
- [x] All 1149 quarto-navigation + quarto-core tests pass. Behavior
      unchanged — the `active` bit lives on `item` now, the path
      through `ConfigValue` roundtrip is identical.

### Extract shared `navigation_href` module
- [x] Created `crates/quarto-core/src/transforms/navigation_href.rs`
      with `resolve_href_for_html(raw, index, source_label, diags)`
      and `is_external(href)`. `source_label` generalizes Phase 2's
      `sidebar_id` parameter.
- [x] Migrated `sidebar_render.rs` to call the shared helper; dropped
      the local copies. Sidebar builds `source_label` as
      `"Sidebar '<id>'"` when it has an id, else `"Sidebar"`.
- [x] Tests 13–14 (plus edge-case tests:
      `no_index_passes_raw_href_through`, `non_qmd_miss_does_not_emit_diagnostic`,
      `is_external_classification`, query/fragment preservation).
- [x] **Contract shift from Phase 2**: when `index` is `None`, the
      resolver no longer emits a warning for `.qmd`-shaped misses.
      Rationale: standalone single-doc renders (including revealjs)
      don't have project context, so the renderer can't tell if the
      user's `.qmd` href is broken or intentionally literal. This
      keeps the non-website use case quiet. All 1074 quarto-core
      tests pass (Phase 2 test `render_works_without_project_index`
      used external-only hrefs, so the shift is behaviorally safe).

### Extract shared `navigation_enrich` module
- [x] Created `crates/quarto-core/src/transforms/navigation_enrich.rs`
      with `enrich_navigation_items(&mut [NavigationItem], &ProjectIndex)`
      and a crate-internal `enrich_one(item, index)` delegator.
      Recurses into `menu` for dropdown items.
- [x] Migrated `sidebar_generate.rs::enrich_text_from_index`:
      `SidebarEntry::Link` now delegates to `enrich_one`; `Section`
      retains its section-specific enrichment path (title from
      section href profile) and recurses.
- [x] Tests 15–18 (+ 2 edge cases:
      `enrich_noop_for_item_without_href`, `enrich_noop_when_index_miss`).
      1080 quarto-core tests pass.

### Create `navigation_active` shared module
- [x] Created `crates/quarto-core/src/transforms/navigation_active.rs`
      with `mark_active(&mut [NavigationItem], page_source)`.
      Source-path equality only; no HTML assumptions; no expand-
      ancestors semantic (navbar/footer convention per Decision 5).
      Recurses into `menu`.
- [x] Tests 19–22 (+ 2 edge cases: hrefless-item descent, duplicate-
      href multi-match). 6 new tests, all passing.
- [x] Dead-code warnings on `mark_active` + `enrich_navigation_items`
      are expected — they resolve when Tasks 7 (navbar_generate) and
      9 (footer_generate) consume the helpers.

### `SidebarEntry::Link` sheds its `active` field
- [ ] Remove the field; read `item.active` instead.
- [ ] Update sidebar unit tests and integration tests to read
      `item.active` (~7 tests in Phase 2 touched this).
- [ ] Re-run Phase 2 sidebar tests; confirm no behavior change.

### Extract shared helpers
- [ ] Create `crates/quarto-core/src/transforms/navigation_href.rs`.
      Move `resolve_href_for_html` + `is_external` from
      `sidebar_render.rs`; rename `sidebar_id` parameter to
      `source_label: Option<&str>`.
- [ ] Update all existing sidebar tests (13 in `sidebar_render.rs`)
      to point at the new module or to use the new API; assert
      behavior unchanged.
- [ ] Create `crates/quarto-core/src/transforms/navigation_enrich.rs`
      with `enrich_navigation_items`. Sidebar's `enrich_text_from_index`
      becomes a thin wrapper that recurses and delegates.
- [ ] Create `crates/quarto-core/src/transforms/navigation_active.rs`
      with `mark_active(&mut [NavigationItem], &page_source)`.
- [ ] Tests 13–22.

### Navbar YAML source — no change
- [ ] No work. `resolve_navbar` continues to read top-level
      `navbar:` as today. Confirm Phase 2 tests for `resolve_navbar`
      still pass after the `NavigationItem` shape change.

### Footer YAML source — no change
- [ ] No work. `resolve_page_footer` continues to read top-level
      `page-footer:`. Same confirmation.

### NavbarGenerateTransform extension
- [x] Signature changed from `_ctx` to `ctx`; consume
      `ctx.project_index` when present.
- [x] Added `enrich_navigation_items` + `mark_active` passes over
      `left` and `right`.
- [x] `page_relative_source(ctx)` lifted from `sidebar_generate.rs`
      into `navigation_active` so both transforms share it.
      sidebar_generate delegates through the new location.
- [x] Tests 23–27. Existing Phase 2 skip-tests preserved (5). 10
      navbar_generate tests total, all passing. 1091 quarto-core
      tests pass workspace-wide.

### NavbarRenderTransform extension
- [x] `navbar_to_html` / `render_navbar_item` / `render_dropdown_item`
      updated to emit `active` class on `nav-link` and `dropdown-item`
      anchors when `item.active == true`.
- [x] Tests 9–11 (navbar active class rendering). 87 quarto-navigation
      tests pass.
- [x] `rewrite_navigation_item_hrefs` walks left/right (recursing
      into `menu`) and rewrites hrefs via the shared
      `resolve_href_for_html`. `source_label = "Navbar"` for
      diagnostics.
- [x] `brand_title_fallback(meta)` extracted; returns
      `website.title ?? meta.title` (the `navbar.title` level is
      already consumed by `navbar_to_html`'s own fallback handling
      since it passes the fallback only when the navbar title is
      `Default`).
- [x] Tests 28–36 in navbar_render.rs. 14 navbar_render tests,
      all passing. 1100 quarto-core tests pass workspace-wide.

### FooterGenerateTransform extension
- [x] `_ctx` → `ctx`; consume `ctx.project_index`.
- [x] `enrich_footer_region(region, index)` helper walks each of
      left/center/right and delegates to `enrich_navigation_items`
      when the region is `FooterRegion::Items`. `Text` and `Empty`
      regions are skipped (Phase 6's territory).
- [x] Tests 37–40. 9 footer_generate tests total (5 preserved Phase 2
      + 4 new), all passing.

### FooterRenderTransform extension
- [x] `_ctx` → `ctx`; `rewrite_region_hrefs` applies the shared
      `resolve_href_for_html` across items inside `FooterRegion::Items`
      in each of left/center/right. `source_label = "Page footer"`.
- [x] `rewrite_items_hrefs` is symmetric with navbar — recurses into
      `menu` for safety (footers rarely nest menus, but the type
      allows it).
- [x] Tests 41–44 plus the 4 preserved Phase 2 tests. 8 footer_render
      tests total, all passing. 1108 quarto-core tests pass
      workspace-wide.

### Integration tests
- [x] `navbar_footer_pipeline.rs` created with tests 45–50.
      6 tests, all passing on first run. Drives the real
      `ProjectPipeline` end-to-end via the same helper shape as
      `sidebar_pipeline.rs` (temp dir, `ProjectContext::discover`,
      `ProjectPipeline::run`). Covers:
      * navbar rendering + active-item highlighting per page
      * dropdown menu href rewriting + dropdown active class
      * page-footer rendering + footer-item href rewriting
      * active-class cross-contamination guard
      * format-agnostic invariant spot check (`.qmd` paths survive
        Generate, Render rewrites them, active class survives)
      * single-doc-in-project regression (doc-level frontmatter
        navbar still works; doesn't spill into siblings)

### CLI end-to-end + regression
- [x] Smoke fixture at `/tmp/q2-phase3-smoke/` with three pages,
      top-level `navbar:` (with dropdown) and `page-footer:` (with
      icon + copyright), and `website.title` set. Rendered clean.
      Observed HTML per page (quoted here for close-out review,
      per CLAUDE.md §End-to-end verification):
      * **index.html**:
        - Brand uses site title: `<a class="navbar-brand" href="/">Q2 Phase 3 Smoke</a>`
        - `<a href="index.html" class="nav-link active">Home</a>` (active)
        - `<a href="about.html" class="nav-link">About</a>` (rewritten, not active)
        - Dropdown `Docs` with `<a href="guides/intro.html" class="dropdown-item">Guide Intro</a>` (enriched text from profile)
        - `<footer class="footer">` with left `© 2026 Quarto`, right github icon link (`<i class="bi bi-github">`)
      * **about.html**: same navbar structure, active class flipped
        to `About` only (`href="about.html" class="nav-link active"`).
      * **guides/intro.html**: dropdown leaf active
        (`<a href="guides/intro.html" class="dropdown-item active">`);
        dropdown ancestor stays inactive (Decision 5 — matches Q1).
- [x] Revealjs/standalone single-doc smoke at
      `/tmp/q2-phase3-revealjs-smoke/deck.qmd` with top-level
      `page-footer: "© 2026 Standalone"`. Rendered clean without any
      `website:` namespacing; the UX story Decision 1 hinged on is
      intact. Footer HTML contains the literal copyright text in
      `nav-footer-center`.
- [x] Phase 2 sidebar smoke re-run at `/tmp/q2-phase2-smoke/`. Output
      includes `<nav id="quarto-sidebar">`, active class on the
      current page's `sidebar-link`, nested-section expansion. No
      regression from the Phase 2/Phase 3 refactor.

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — 7801 tests pass, 195 skipped
      (gained 6 integration + 30 unit tests over the 7795 Phase 2
      baseline).
- [x] `cargo xtask lint` passes (622 files checked).
- [x] `cargo xtask verify --skip-hub-tests` end-to-end green — Rust
      build + tests, hub-client build (including WASM), trace-viewer
      tests.
- [x] Filed follow-ups: `bd-jfyl`, `bd-jbml`, `bd-bwwv`, `bd-9m8p`,
      `bd-15dw`. Description of epic-wide `bd-n9dr` refreshed.
- [x] `br close bd-fqyg`.
- [x] `br sync --flush-only && git add .beads/ claude-notes/ crates/ && git commit`.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** adding `active: bool` to `NavigationItem` ripples through
  navbar and sidebar tests en masse. *Mitigation:* the default is
  `false`, so most construction sites that use `..Default::default()`
  compile unchanged. Sidebar's `Link { item, active }` loses its
  local `active` field in one atomic commit.

- **Risk:** single-doc users in non-HTML formats (revealjs,
  beamer) unexpectedly see their `page-footer:` stop rendering because
  the transform takes a different branch without `ProjectIndex`.
  *Mitigation:* Phase 3's non-index branch is explicitly a pass-through
  — the resolver runs, the structure lands at `navigation.footer`, the
  render emits it verbatim. Integration test 50 + smoke test 53 lock
  this in.

- **Risk:** extracting the href / enrich / active helpers into three
  new modules fragments a small codebase. *Mitigation:* three small
  modules is cheaper than three duplicated in-transform
  implementations; each module owns one concept and one pub fn.

- **Risk:** active-marking performance is O(N_items × 1) per-file, so
  O(N_files × N_items) for a project sweep. For a 100-file site with
  a 10-item navbar, 1,000 comparisons — negligible.

- **Risk:** dropdown-active propagation introduces a surprising "nav
  looks active when current page is deep inside a submenu" behavior.
  *Mitigation:* only the matching leaf is marked; the dropdown
  ancestor carries no `active` class (matches Q1's behavior; also a
  simpler contract). Test 11 locks this in.

- **Risk:** the brand-title fallback via `website.title` conflicts
  with Phase 7's ownership of the site-title concept. *Mitigation:*
  Phase 3 only *reads* `website.title`; Phase 7 adds the `<title>`-tag
  prefixing as a separate concern. No overlap in state.

- **Risk:** `enrich_navigation_items` + `mark_active` duplicate tree
  walks across the navbar items. *Mitigation:* if the profile-
  dashboard shows this up, merge into a single walk — but v1 keeps
  them separate for readability.

- **Risk:** footer `Text` regions may contain `.qmd` links inline —
  e.g. `"See [our docs](docs.qmd)"`. Phase 3 doesn't rewrite those.
  *Mitigation:* Phase 6 is the body-link-rewrite phase; call out
  in the Phase 3 docs that footer `Text` regions are not project-
  link-aware, and surface `bd-<new>` for "footer-text link
  rewriting" as a follow-up if users hit this.

## Explicit non-goals for this phase

- No `site_libs/`, no shared CSS/JS (Phase 5).
- No sitemap, favicon, `<title>` prefix (Phase 7).
- No breadcrumbs, no repo-actions, no search, no reader/dark toggles,
  no announcements (epic-excluded).
- No body-content `[link](x.qmd)` rewriting (Phase 6).
- No changes to the pipeline ordering or the template slots.
- No changes to sidebar behavior beyond the `active: bool` location
  refactor and migration to the shared helpers.
- No book or manuscript types.
- No navbar "search" button wired up — stays a stub.

## Follow-up beads (filed at close-out)

- `bd-jfyl` — Footer `Text` region project-link rewriting (depends on
  body-link Phase 6's contract).
- `bd-jbml` — `itemHasNavTarget`-style index-forgiveness (Q1 treats
  `about/` and `about/index.html` as equivalent). Phase 3 uses strict
  source-path equality; revisit if a real site hits the edge case.
- `bd-bwwv` — Navbar sub-row (book-style sub-navbar), epic-excluded
  for MVP.
- `bd-9m8p` — `navbar.pinned` behavior (sticky-on-scroll JS); rides
  with Phase 5 (`site_libs/` ships the JS).
- `bd-15dw` — Text-enrichment tie-breaker: if an item supplies `icon`
  but no `text`, should we still enrich `text` from the profile title?
  Phase 3 says no (`icon`-only items are intentional); confirmed.

Epic-wide follow-up (description refreshed in Phase 3):

- `bd-n9dr` reframed: placement follows feature semantics, not
  uniformity. Remaining tension: `site-sidebar` at doc-level for a
  website-scoped feature. See the bead for migration options.

## Decisions log (confirmed 2026-04-24)

1. **Navbar / page-footer config placement.** Stays at the top level
   (no YAML surface change). Originally proposed to move under
   `website.` to match Q1; rejected after user feedback pointing out
   that non-HTML formats like revealjs can reasonably set
   `page-footer:` without any website context. Principle established:
   *placement follows feature semantics*. See Decision 1 for detail.
2. **`active: bool` on `NavigationItem`.** Added as a field with
   default `false`. Sidebar's `SidebarEntry::Link { item, active }`
   loses its local `active` and reads `item.active`. Unifies the nav
   model across navbar / sidebar / footer.
3. **Shared `navigation_href` module.** `resolve_href_for_html` and
   `is_external` move from `sidebar_render.rs` into a new
   `crates/quarto-core/src/transforms/navigation_href.rs`. Phase 2
   callers migrate in the same commit. The `sidebar_id` parameter
   generalizes to `source_label: Option<&str>`.
4. **Shared `navigation_enrich` module.** Title-enrichment logic
   extracted into `enrich_navigation_items(&mut [NavigationItem],
   &ProjectIndex)`; sidebar's wrapper delegates. Navbar + footer are
   new consumers.
5. **Active-marking algorithm.** Format-agnostic, source-path keyed,
   set in Generate. Recurses into dropdown `menu` items. No "expand
   ancestors" semantics for navbar (unlike sidebar). Dropdown
   ancestors stay inactive even when a leaf is active.
6. **Navbar brand fallback chain.** `navbar.title → website.title →
   document.title`. Phase 3 reads `website.title` from merged
   metadata; Phase 7 still owns the `<title>` tag prefixing.
7. **Navbar and footer Generate take `ProjectIndex`.** Both transforms
   switch their `_ctx` signature to `ctx` and consume
   `ctx.project_index` for enrichment and active-marking. No-index
   branch silently skips post-processing.
8. **Footer items get href rewrite, no active marking.** Matches Q1.
   Footer `Text` regions are not scanned for `.qmd` links in Phase 3
   — that's Phase 6's body-link-rewrite territory.

## Epic-level impact

Phase 3 completes the project-aware navigation surface for websites:
navbar + sidebar + page-footer all participate in project rendering,
all link across documents, all highlight the current page. Together
with Phases 5+7, this is the minimum-shape deliverable for `bd-tr81`
(the Q2 docs site bootstrap).

After Phase 3:

- A website with a navbar, a sidebar, and a footer renders correctly
  on a cold project.
- Every internal `.qmd` link in navigation surfaces becomes an `.html`
  link in the output HTML.
- The "you are here" cue is present in both sidebar (already shipped)
  and navbar (this phase).
- Single-doc renders — including non-HTML formats like revealjs — keep
  working with the same top-level `navbar:` / `page-footer:` YAML
  surface they had before.
- The epic's "where does nav config live?" question has a principled
  answer: **placement follows feature semantics**. Site-scoped
  features (sidebar, sitemap, favicon, site title) live under
  `website.`; document-level chrome (navbar, page-footer) lives at
  the top level.
