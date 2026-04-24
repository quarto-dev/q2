# Phase 4 — Page navigation (prev / next)

**Date:** 2026-04-24
**Beads:** to be filed (parent `bd-0tr6`; blocked-by `bd-fqyg` Phase 3 —
closed; `bd-9svl` Phase 2 — closed).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-24-websites-phase-3.md`
**Status:** Decisions 1–9 confirmed 2026-04-24 after design iteration.
Implementation pending user go-ahead.

## Goal of this phase

Emit the bottom-of-page **prev / next** navigation strip that Q1
produces on website pages, driven by the current page's already-
resolved sidebar. Concretely:

1. Add a `PageNavigation { prev: Option<NavigationItem>, next:
   Option<NavigationItem> }` data type in `quarto-navigation`.
2. Add `PageNavGenerateTransform` (runs right after
   `SidebarGenerateTransform`). Reads the already-picked sidebar from
   `navigation.sidebar`, flattens it per Q1's `flattenItems` +
   `nextAndPrevious` rules, finds the current page, and stores the
   resulting `PageNavigation` at `navigation.page_navigation`. Hrefs
   remain in source-path space (format-agnostic, per Phase 2
   Decision 7).
3. Add `PageNavRenderTransform` (runs right after
   `SidebarRenderTransform`). Rewrites `prev`/`next` hrefs via the
   shared `navigation_href::resolve_href_for_html`, emits Q1-matching
   HTML, stores at `rendered.navigation.page_navigation`.
4. Add template slot `$rendered.navigation.page_navigation$` inside
   `<main class="content">`, after `$body$`, before `</main>`.
5. Honor top-level `page-navigation: false` via the existing
   `is_feature_disabled` helper. Default-on when a sidebar applies and
   at least one neighbor exists; silent no-op otherwise.

**No YAML-surface changes** beyond the top-level `page-navigation: bool`
key that Q1 already uses. Phase 3's "placement follows feature
semantics" principle is preserved: per-document chrome at the top
level, not under `website.`.

This phase does **not** implement:

- **`<link rel="prev">` / `<link rel="next">` meta tags** in `<head>`.
  Q1 emits these; Q2 defers as follow-up — see close-out beads.
- **`usesCustomLayout` suppression**. Q1 hides page-nav when a user
  page opts into a custom layout. Defer until real content hits the
  edge case.
- **Rich-text `aria-label` stripping**. `DocumentProfile.title` is
  `String` today; when titles grow inline markup support, we'll revisit.
- **"Remove chapter number" post-render patch**. Book-specific; out of
  epic scope.
- **`site_libs/` / theme CSS for the nav strip**. Phase 4 emits the
  HTML with the exact Q1 class vocabulary
  (`nav-page-previous`/`nav-page-next`/`pagination-link`) so Phase 5's
  shared-assets work will light up the Q1 CSS without re-markup.

## Reference material

- **Parent epic plan** §"Phase 4 — Page navigation (prev/next)".
- **Phase 2 plan** — sidebar Generate/Render split, sidebar data model.
  `resolve_active_state` / `sidebar_for_page` live in
  `crates/quarto-navigation/src/sidebar.rs`.
- **Phase 3 plan** — the shared navigation helpers
  (`navigation_href`, `navigation_enrich`, `navigation_active`) that
  Phase 4 reuses for href rewriting. `NavigationItem::active` is
  irrelevant here (page-nav targets are *not* the current page by
  construction), but `text` / `href` / `aria_label` all apply.
- **Q2 current code:**
  - `crates/quarto-navigation/src/sidebar.rs` — `Sidebar`,
    `SidebarEntry`, `sidebar_for_page`, `resolve_active_state`,
    `contains_source_path`.
  - `crates/quarto-navigation/src/item.rs` — `NavigationItem`.
  - `crates/quarto-navigation/src/render_html.rs` — place to add
    `page_navigation_to_html`.
  - `crates/quarto-core/src/transforms/navigation_href.rs` —
    `resolve_href_for_html`, `is_external`.
  - `crates/quarto-core/src/transforms/sidebar_generate.rs` — pattern
    Phase 4 mirrors for Generate.
  - `crates/quarto-core/src/transforms/sidebar_render.rs` — pattern
    Phase 4 mirrors for Render.
  - `crates/quarto-core/src/pipeline.rs:623-630` — navigation-phase
    ordering (insert two new entries).
  - `crates/quarto-core/src/template.rs:161-220` — `FULL_HTML_TEMPLATE`,
    place to add the new slot inside `<main>`.
- **Q1 reference:**
  - `external-sources/quarto-cli/src/project/types/website/website-navigation.ts:1188-1227`
    — `nextAndPrevious` (the flatten + dedupe + neighbor algorithm
    Phase 4 mirrors).
  - `external-sources/quarto-cli/src/project/types/website/website-shared.ts:339-354`
    — `flattenItems` (the depth-first walk).
  - `external-sources/quarto-cli/src/resources/projects/website/templates/nav-after-body-postamble.ejs`
    — the target HTML shape.
  - `external-sources/quarto-cli/src/resources/projects/website/navigation/quarto-nav.scss:740+`
    — Q1 `.page-navigation` CSS (informational; Phase 5 ships it).

## Key decisions (confirmed 2026-04-24)

All decisions below were confirmed by the user on 2026-04-24 after the
design sketch. Decision 1 was tempered by a user note that "single-doc
articles probably won't have prev-next navigation" — ergonomics review
left for a follow-up once the feature is in.

### Decision 1 — Config placement: top level `page-navigation: bool`

`page-navigation: true|false` lives at the top level of document
metadata. Not under `website.`. Matches Q1's YAML shape and Phase 3's
principle that per-document chrome stays top-level.

Flows through the existing metadata merge: setting
`page-navigation: false` in `_quarto.yml` disables for all docs in the
project; a per-doc override in frontmatter wins over that.

Reading code uses the shared `is_feature_disabled(&ast.meta,
"page-navigation")` helper — same pattern as navbar / sidebar / footer
/ toc. No new helper needed.

### Decision 2 — Pipeline position: after sidebar Generate / Render

```
TocGenerateTransform
NavbarGenerateTransform
SidebarGenerateTransform
PageNavGenerateTransform   ← NEW — depends on navigation.sidebar
FooterGenerateTransform
TocRenderTransform
NavbarRenderTransform
SidebarRenderTransform
PageNavRenderTransform     ← NEW
FooterRenderTransform
```

`PageNavGenerateTransform` runs after `SidebarGenerateTransform` so it
reads the already-resolved `navigation.sidebar` rather than re-picking
the sidebar itself. That's the single source of truth: whatever sidebar
the user sees, page-nav neighbors come from that same sidebar.

### Decision 3 — Data model: `PageNavigation { prev, next }` in `quarto-navigation`

```rust
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageNavigation {
    pub prev: Option<NavigationItem>,
    pub next: Option<NavigationItem>,
}

impl PageNavigation {
    pub fn from_config_value(cv: &ConfigValue) -> Self { … }
    pub fn to_config_value(&self) -> ConfigValue { … }
    pub fn is_empty(&self) -> bool { self.prev.is_none() && self.next.is_none() }
}
```

Reuses `NavigationItem` (href, text, aria_label, …) so the Phase 3
shared `resolve_href_for_html` helper drops in without a wrapper. The
`active`, `icon`, `menu`, `target`, `rel` fields on `NavigationItem`
are not meaningful for page-nav but are carried along harmlessly by
the roundtrip (omit-default keeps them out of the emitted map).

### Decision 4 — Flatten algorithm (mirror Q1 `nextAndPrevious`)

Given the already-resolved `Sidebar` for the current page:

1. **Depth-first walk** (`flatten_sidebar_entries`) collecting entries
   that qualify for prev/next positioning:
   - `SidebarEntry::Link { item }`: include if `item.href` is
     `Some(_)` and **not** `is_external(href)`.
   - `SidebarEntry::Section { href, contents, … }`: include the
     section header as a positional entry if `href` is `Some(_)` and
     not external. **Always** recurse into `contents` regardless of
     whether the header itself was included.
   - `SidebarEntry::Separator`: include as a `Separator` marker. Q1
     uses these as hard boundaries.
   - `SidebarEntry::Heading(_)`: skip (label only, no position).
   - `SidebarEntry::Auto(_)`: skip defensively (should have been
     expanded by this point; a stray `Auto` at Phase 4 is a bug
     elsewhere — emit nothing rather than panic).

2. **De-duplicate** by `href`, keeping the **first** occurrence. Q1
   does the same. Rationale: if a section-header href matches one of
   its child-link hrefs (common idiom), we want a single entry in
   the prev/next ring. Separators are never deduped (they have no
   href).

3. **Find current-page index**: linear scan for the flat-list entry
   whose `href` equals `page_source` (the current doc's project-
   relative source path, forward-slash form — same helper
   `page_relative_source(ctx)` used by sidebar/navbar Generate).

4. **Pick neighbors**:
   - `prev = index > 0 && !is_separator(list[index-1]) ? list[index-1] : None`
   - `next = index+1 < list.len() && !is_separator(list[index+1]) ? list[index+1] : None`
   - If the current page is not in the flat list (e.g. a page the
     sidebar doesn't list at all, but which still gets picked as its
     sidebar via a wildcard containment match), produce `{prev: None,
     next: None}`.

5. The flattened list is intermediate; only the final `PageNavigation`
   leaves the transform.

### Decision 5 — Default behavior: on when sidebar + neighbor exist

Transform runs unconditionally, subject to `is_feature_disabled`.

- If `page-navigation: false` at any merge level: skip.
- If `navigation.page_navigation` already populated (user override or
  repeat run): skip.
- If `navigation.sidebar` absent: skip.
- If the current page is not found in the flat list: skip (no prev/
  next, no insertion).
- If the current page *is* found and at least one neighbor exists:
  insert `navigation.page_navigation = PageNavigation { prev, next }`.
- If both neighbors are `None` (current page is the only non-separator
  entry): also skip — no point emitting an empty strip.

Silent in every skip case. No diagnostic — absent page-nav is not an
error.

### Decision 6 — Render output (Q1-matching HTML)

`page_navigation_to_html(&PageNavigation) -> String` in
`quarto-navigation::render_html`. Emits exactly Q1's postamble markup:

```html
<nav class="page-navigation">
  <div class="nav-page nav-page-previous">
    <a href="{prev.href}" class="pagination-link" aria-label="{prev.text}">
      <i class="bi bi-arrow-left-short"></i>
      <span class="nav-page-text">{prev.text}</span>
    </a>
  </div>
  <div class="nav-page nav-page-next">
    <a href="{next.href}" class="pagination-link" aria-label="{next.text}">
      <span class="nav-page-text">{next.text}</span>
      <i class="bi bi-arrow-right-short"></i>
    </a>
  </div>
</nav>
```

When `prev` is `None`, the `<div class="nav-page nav-page-previous">`
wrapper is still emitted (empty). Same for `next`. Matches Q1
(templates always render both divs; the CSS layout relies on the
two-column symmetry). When *both* are None we skip the entire block —
the Generate transform already guards this, so Render only sees
populated structures.

`text` defaults to the item's `href` when `text` is missing (defensive;
enrichment in Generate should have filled this). `aria-label` uses the
same `text` value (MVP — the Q1 `plainText` stripping is out of scope
per Phase 4 non-goals).

HTML escaping matches `navbar_to_html` — `escape_html` for node text,
`escape_attr` for attribute values.

### Decision 7 — No `<link rel="prev/next">` in `<head>` (deferred)

Q1 emits `<link rel="prev" href="…">` / `<link rel="next" href="…">`
inside `<head>` alongside the visible strip (for SEO hints + browser
preload). Phase 4 **defers**: wiring into `<head>` touches the
template's `<link>` plumbing and the HTML render config, which is
tangential to the page-nav feature itself.

Follow-up: file `bd-<new>` at close-out — "Emit `<link rel="prev">` /
`<link rel="next">` when page-navigation is active".

### Decision 8 — Template slot inside `<main>`, after `$body$`

Add a new slot inside the existing `FULL_HTML_TEMPLATE`:

```
<main class="content" id="quarto-document-content">
$if(title)$
<header id="title-block-header" …>…</header>
$endif$

$body$

$if(rendered.navigation.page_navigation)$       ← NEW
$rendered.navigation.page_navigation$
$endif$
</main>
```

Placement note: Q1 emits the strip inside the content region just
before the footer, via the `nav-after-body-postamble.ejs` partial.
Placing it inside `<main>` after `$body$` gives semantically correct
nesting (the nav belongs to the article) and keeps all changes inside
the template. We can revisit placement once we see the whole feature
holistically (epic note).

Template comment in the doc block lists the new slot alongside the
others (`$rendered.navigation.page_navigation$`).

### Decision 9 — Documentation note carry-forward

User flagged that the flatten + dedupe + separator-boundary rules are
"fairly complicated behavior." The Q2 docs site (`bd-tr81`) is the
reason we're doing websites, and this feature needs proper user-
facing documentation once the epic lands. Add to the epic plan's
close-out checklist: **"Phase 4 prev/next rules need a dedicated
docs section"**, linked to `bd-tr81`.

## Architecture sketch

### Generate-transform flow

```
PageNavGenerateTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "page-navigation"): return.
    if navigation.page_navigation already populated: return.

    let Some(sidebar_cv) = ast.meta.get_path(["navigation", "sidebar"]) else { return };
    let sidebar = Sidebar::from_config_value(sidebar_cv);

    let page_source = page_relative_source(ctx);
    let flat = flatten_for_page_nav(&sidebar.contents);   // Vec<FlatEntry>
    let Some(idx) = flat.iter().position(|e| e.is_link_with_href(&page_source)) else {
        return;  // current page not in sidebar flat list
    };

    let prev = neighbor(&flat, idx, Direction::Prev);
    let next = neighbor(&flat, idx, Direction::Next);

    if prev.is_none() && next.is_none(): return;  // lonely page

    let page_nav = PageNavigation { prev, next };
    ast.meta.insert_path(&["navigation", "page_navigation"],
                         page_nav.to_config_value());
```

`flatten_for_page_nav` lives in `crates/quarto-navigation/src/sidebar.rs`
next to `flatten_items_for_containment` (if we have one — otherwise a
new pub helper). It returns a `Vec<FlatEntry>` where:

```rust
enum FlatEntry {
    Item(NavigationItem),
    Separator,
}
```

`Separator` is private to the algorithm; only `NavigationItem`s leave
the transform wrapped in `Option<_>`.

`neighbor(flat, idx, direction)` walks one step in the given direction
and returns `Some(NavigationItem)` if the step lands on an `Item`,
`None` if it lands on a `Separator` or runs off the end.

### Render-transform flow

```
PageNavRenderTransform::transform(ast, ctx):
    if is_feature_disabled(&ast.meta, "page-navigation"): return.
    if rendered.navigation.page_navigation already populated: return.

    let Some(cv) = ast.meta.get_path(["navigation", "page_navigation"]) else { return };
    let mut page_nav = PageNavigation::from_config_value(cv);

    let mut local_diags = std::mem::take(&mut ctx.diagnostics);
    if let Some(ref mut item) = page_nav.prev {
        if let Some(href) = item.href.as_mut() {
            *href = resolve_href_for_html(href, ctx.project_index.as_deref(),
                                          Some("Page navigation"), &mut local_diags);
        }
    }
    if let Some(ref mut item) = page_nav.next {
        // same as above
    }
    ctx.diagnostics = local_diags;

    let html = page_navigation_to_html(&page_nav);
    ast.meta.insert_path(&["rendered", "navigation", "page_navigation"],
                         ConfigValue::new_string(&html, SourceInfo::default()));
```

### Module shape

```
crates/quarto-navigation/src/
    sidebar.rs              # add `flatten_for_page_nav` (pub fn) +
                            # pub enum FlatEntry (or keep FlatEntry
                            # crate-private and expose a narrower API)
    page_nav.rs             # NEW — PageNavigation struct,
                            # from_config_value / to_config_value
    render_html.rs          # add page_navigation_to_html
    lib.rs                  # re-export PageNavigation

crates/quarto-core/src/transforms/
    page_nav_generate.rs    # NEW — PageNavGenerateTransform
    page_nav_render.rs      # NEW — PageNavRenderTransform
    mod.rs                  # wire up the two new modules + re-exports

crates/quarto-core/src/
    pipeline.rs             # insert 2 lines (after sidebar Generate,
                            # after sidebar Render)
    template.rs             # add $rendered.navigation.page_navigation$
                            # slot + comment in the doc block
```

### Data flow summary

```
_quarto.yml / frontmatter  →  MetadataMergeStage  →  ast.meta.website.sidebar
                                                     ast.meta["page-navigation"] (bool)
                                                ↓
                          SidebarGenerateTransform
                                                ↓
                          ast.meta.navigation.sidebar   (resolved, format-agnostic)
                                                ↓
                          PageNavGenerateTransform
                              (flatten + find current + neighbors)
                                                ↓
                          ast.meta.navigation.page_navigation   (format-agnostic)
                                                ↓
                          PageNavRenderTransform
                              (href rewrite via ProjectIndex + emit HTML)
                                                ↓
                          ast.meta.rendered.navigation.page_navigation (HTML)
                                                ↓
                          ApplyTemplateStage → slot in <main>
```

## DocumentProfile change

**None.** Phase 4 reads `source_path` and `title` (through sidebar
enrichment done in Phase 2/3) — all present since Phase 1. No
profile-version bump.

## Tests (TDD: write and fail first)

Every test authored before the code that makes it pass. Failing baseline
captured before implementation.

### Unit tests — `quarto-navigation::page_nav`

1. **`page_navigation_default_is_empty`** — `PageNavigation::default()`
   has `prev: None` and `next: None`; `is_empty()` returns true.
2. **`page_navigation_roundtrip_preserves_prev_next`** — populate with
   items bearing `href` + `text`; roundtrip through
   `to_config_value` / `from_config_value` preserves both sides.
3. **`page_navigation_roundtrip_empty_side_omits_key`** — only `next`
   set; the emitted map has no `prev` key; `from_config_value` on
   that map yields `prev: None`.

### Unit tests — `quarto-navigation::sidebar::flatten_for_page_nav`

4. **`flatten_includes_internal_links_only`** — sidebar with one
   internal `Link` and one external `Link` (`https://…`); the flat
   list contains only the internal one.
5. **`flatten_includes_section_header_with_href`** — section carrying
   `href: "index.qmd"` appears as a flat entry; its children also
   appear, in depth-first order.
6. **`flatten_skips_section_header_without_href`** — section with
   text + contents but no header href: header omitted, children
   walked.
7. **`flatten_includes_separators_as_markers`** — `Separator` entries
   appear in the flat list (used downstream to break adjacency).
8. **`flatten_skips_headings`** — `Heading(_)` omitted.
9. **`flatten_skips_stray_auto`** — `Auto(_)` omitted (defensive;
   should never reach this point).
10. **`flatten_dedupes_by_href_keeping_first`** — section `href:
    "docs.qmd"` followed by a child `Link { href: "docs.qmd" }`: only
    the section appears in the flat list.
11. **`flatten_dedupe_does_not_collapse_separators`** — two separators
    surrounding a link stay as three flat entries.
12. **`flatten_depth_first_order_matches_q1`** — regression guard: a
    handcrafted sidebar with two levels of nesting produces exactly
    the Q1-expected order. (Fixture included verbatim in the test for
    reviewer eyeballing.)

### Unit tests — `quarto-navigation::render_html::page_navigation_to_html`

13. **`page_nav_html_emits_prev_and_next_divs`** — both sides filled:
    output contains both `nav-page-previous` and `nav-page-next`
    wrappers with `pagination-link` anchors.
14. **`page_nav_html_empty_prev_wrapper_when_missing`** —
    `prev: None, next: Some(_)`: previous `<div>` is emitted but has
    no `<a>` inside.
15. **`page_nav_html_uses_text_in_aria_label`** — item with
    `text: "About"` produces `aria-label="About"` on the anchor.
16. **`page_nav_html_escapes_text_and_attributes`** — text with
    `<` / `&` / `"` is HTML-escaped in the `<span>` and attribute-
    escaped in `aria-label` / `href`.
17. **`page_nav_html_falls_back_to_href_when_text_missing`** — item
    with `href: "a.qmd"` and `text: None` renders `a.qmd` in the
    visible span.
18. **`page_nav_html_emits_q1_bootstrap_icons`** — output contains
    `<i class="bi bi-arrow-left-short"></i>` on prev and
    `<i class="bi bi-arrow-right-short"></i>` on next.

### Unit tests — `page_nav_generate`

19. **`page_nav_generate_skips_when_feature_disabled`** —
    `page-navigation: false` at doc level: no
    `navigation.page_navigation` written.
20. **`page_nav_generate_skips_when_sidebar_absent`** — no
    `navigation.sidebar` on the meta: skip.
21. **`page_nav_generate_skips_when_already_populated`** — a
    pre-set `navigation.page_navigation` survives verbatim.
22. **`page_nav_generate_skips_when_page_not_in_sidebar`** — current
    page has a sidebar assigned but its source path doesn't appear in
    the flat list: no insertion.
23. **`page_nav_generate_skips_when_lonely_page`** — current page is
    the only non-separator entry in a single-item sidebar: no
    insertion (both neighbors would be None).
24. **`page_nav_generate_middle_page_has_both_neighbors`** — three-
    page linear sidebar, rendering page 2: prev = page 1, next =
    page 3.
25. **`page_nav_generate_first_page_only_has_next`** — three-page
    sidebar, rendering page 1: prev = None, next = page 2.
26. **`page_nav_generate_last_page_only_has_prev`** — three-page
    sidebar, rendering page 3: prev = page 2, next = None.
27. **`page_nav_generate_separator_breaks_adjacency`** — sidebar:
    `[a.qmd, ---, b.qmd]`. Rendering `a.qmd`: next = None (separator
    is next). Rendering `b.qmd`: prev = None.
28. **`page_nav_generate_keeps_qmd_hrefs`** — format-agnostic
    invariant: stored prev/next hrefs end in `.qmd`.
29. **`page_nav_generate_carries_enriched_text_from_sidebar`** — a
    bare-path sidebar entry `- about.qmd` was enriched by
    `SidebarGenerateTransform` (Phase 2). Page-nav picks that entry
    as a neighbor; the `text` field comes along on the
    `NavigationItem`.
30. **`page_nav_generate_respects_section_header_as_neighbor`** —
    sidebar contains a section with an href; that section can be a
    prev or next neighbor of a leaf sibling.

### Unit tests — `page_nav_render`

31. **`page_nav_render_skips_when_absent`** — no
    `navigation.page_navigation` → no `rendered.navigation.page_navigation`.
32. **`page_nav_render_skips_when_feature_disabled`**.
33. **`page_nav_render_skips_when_already_prerendered`**.
34. **`page_nav_render_rewrites_qmd_hrefs_to_html`** — prev.href
    `about.qmd` becomes `about.html` in the emitted HTML (via
    ProjectIndex).
35. **`page_nav_render_passes_external_urls_through`** (defensive —
    Generate filters externals, but Render must be robust if a user
    filter inserts one).
36. **`page_nav_render_emits_diagnostic_for_unknown_qmd`** —
    `source_label = "Page navigation"` in the diagnostic.
37. **`page_nav_render_no_index_passes_hrefs_through`** — standalone
    render, no project index: hrefs stored verbatim, no diagnostic.
38. **`page_nav_render_populates_rendered_slot`** — happy path: HTML
    string lands at `rendered.navigation.page_navigation`.

### Integration tests — `crates/quarto-core/tests/`

New file `page_navigation_pipeline.rs` modeled on
`sidebar_pipeline.rs` / `navbar_footer_pipeline.rs`:

39. **`pipeline_page_nav_three_page_website`** — fixture with
    `_quarto.yml` declaring a sidebar `[index.qmd, about.qmd,
    docs.qmd]`. Assertions:
    - `index.html`: contains `nav-page-next` pointing at
      `about.html`; `nav-page-previous` div is empty.
    - `about.html`: both prev (`index.html`) and next (`docs.html`)
      populated.
    - `docs.html`: `nav-page-previous` pointing at `about.html`; next
      div empty.
40. **`pipeline_page_nav_disabled_at_doc_level`** — same fixture,
    `about.qmd` frontmatter sets `page-navigation: false`. Assertions:
    - `about.html`: no `<nav class="page-navigation">`.
    - `index.html` / `docs.html`: page-nav present (doc-level disable
      does not spill across documents).
41. **`pipeline_page_nav_disabled_at_project_level`** — fixture
    declares `page-navigation: false` at the top of `_quarto.yml`. No
    page emits `page-navigation`.
42. **`pipeline_page_nav_honors_separator_boundary`** — sidebar
    `[a.qmd, ---, b.qmd]`. `a.html`: next empty. `b.html`: prev empty.
43. **`pipeline_page_nav_cross_contamination_guard`** — rendering
    `index.qmd` does not mark or leak neighbors into the other two
    pages' output (regression against stateful-transform bugs, same
    shape as the navbar cross-contamination test from Phase 3).
44. **`pipeline_single_doc_no_page_nav`** — a bare `doc.qmd` with no
    `_quarto.yml`, top-level `page-navigation: true`: no
    `<nav class="page-navigation">` in the output (no sidebar → no
    page-nav, matches default-on semantics).

### CLI end-to-end (per CLAUDE.md §End-to-end verification)

45. **Manual smoke** at `/tmp/q2-phase4-smoke/`:
    ```
    _quarto.yml:
      project: { type: website }
      website:
        title: "Q2 Phase 4 Smoke"
        sidebar:
          contents: [index.qmd, about.qmd, docs.qmd]
    index.qmd, about.qmd, docs.qmd:  (three minimal pages)
    ```
    - `cargo run --bin q2 -- render /tmp/q2-phase4-smoke/`.
    - Inspect each rendered HTML:
      - `index.html`: `<nav class="page-navigation">` present; left
        div empty; right div `<a href="about.html" …>About</a>` with
        the right-arrow icon.
      - `about.html`: left points at `index.html`, right at
        `docs.html`.
      - `docs.html`: left at `about.html`; right div empty.
    - Record the observed HTML snippet in the plan close-out.
46. **Separator variant** at `/tmp/q2-phase4-separator-smoke/`:
    sidebar `[a.qmd, "---", b.qmd, c.qmd]`. Confirm `a.html` has no
    next, `b.html` has no prev, `b.html`→`c.html` and back.
47. **Regression:** Phase 2 / Phase 3 smokes (`/tmp/q2-phase2-smoke/`,
    `/tmp/q2-phase3-smoke/`) unchanged — now also carry a
    `page-navigation` strip when appropriate, but sidebar/navbar
    output is otherwise pixel-identical.

### Snapshot tests

None in Phase 4 — inline asserts cover the vocabulary (same choice
Phase 2 and Phase 3 made).

## Work items (checklist)

### Preparation
- [ ] Re-read `claude-notes/instructions/testing.md`, `coding.md`,
      `review.md`.
- [ ] Confirm user agreement with Decisions 1–9. **DONE 2026-04-24.**
- [ ] Create `bd` issue `Phase 4 — Page navigation (prev/next)`
      (new id), parent `bd-0tr6`, parent-child dependency linked.
- [ ] Commit directly on `feature/websites` (Phase 1/2/3 precedent).

### `PageNavigation` data model (`quarto-navigation/src/page_nav.rs`)
- [x] New module `page_nav.rs` with struct + `from_config_value` /
      `to_config_value` / `is_empty`.
- [x] `lib.rs` re-exports `PageNavigation`.
- [x] Tests 1–3 (all 3 passing).

### Sidebar flattening (`quarto-navigation/src/sidebar.rs`)
- [x] Added `pub enum FlatEntry { Item(NavigationItem), Separator }`
      with `is_link_with_href` helper.
- [x] `pub fn flatten_for_page_nav(&[SidebarEntry]) -> Vec<FlatEntry>`
      depth-first + dedupe-by-href + separator preservation per
      Decision 4. Local `is_external_href` keeps `quarto-navigation`
      free of `quarto-core` dep; semantics match the shared helper.
- [x] Tests 4–12 (+ 1 bonus `is_link_with_href` sanity test). All 100
      quarto-navigation tests pass.

### HTML renderer (`quarto-navigation/src/render_html.rs`)
- [x] `pub fn page_navigation_to_html(&PageNavigation) -> String`
      emitting Q1-matching markup per Decision 6. Empty-side wrappers
      always emitted; aria-label falls back to href when text is
      empty.
- [x] Tests 13–18 (all 6 passing). 106/106 quarto-navigation tests
      pass.

### `PageNavGenerateTransform` (`quarto-core/src/transforms/page_nav_generate.rs`)
- [x] New module. Skip conditions per Decision 5.
- [x] Reads `navigation.sidebar` via `Sidebar::from_config_value`,
      applies `flatten_for_page_nav`, finds current page, picks
      neighbors via `neighbor_before` / `neighbor_after` (Separator
      → None).
- [x] `mod.rs` re-export. Pipeline registration deferred to Task 7.
- [x] Tests 19–30 (12 from plan + 1 bonus separator variant covering
      "neighbor remains on the unblocked side"). 13 passing.

### `PageNavRenderTransform` (`quarto-core/src/transforms/page_nav_render.rs`)
- [x] New module. Skip conditions symmetric to Generate.
- [x] Rewrites hrefs via shared `resolve_href_for_html` with
      `source_label = "Page navigation"`.
- [x] Stores HTML at `rendered.navigation.page_navigation`.
- [x] `mod.rs` re-export.
- [x] Tests 31–38 (8 from plan + 1 bonus visible-span text check).
      All 9 passing.

### Pipeline wiring (`quarto-core/src/pipeline.rs`)
- [x] Inserted `PageNavGenerateTransform::new()` immediately after
      `SidebarGenerateTransform` (with comment explaining the ordering
      dependency).
- [x] Inserted `PageNavRenderTransform::new()` immediately after
      `SidebarRenderTransform`.

### Template slot (`quarto-core/src/template.rs`)
- [x] Added `$if(rendered.navigation.page_navigation)$ …$endif$` block
      inside `<main>`, after `$body$`, before `</main>`.
- [x] Updated template doc-block listing.
- [x] `cargo build --workspace` clean; all 1156 quarto-core tests
      pass after wiring (no regressions).

### Integration tests (`quarto-core/tests/page_navigation_pipeline.rs`)
- [x] Tests 39–44 written and passing on first run. Use the same
      `ProjectContext::discover` / `ProjectPipeline::run` helper
      shape as `sidebar_pipeline.rs`. 6/6 tests pass.

### CLI end-to-end + regression
- [x] Smoke fixture `/tmp/q2-phase4-smoke/` — three-page sidebar.
      Observed HTML on each page (per CLAUDE.md §End-to-end
      verification):
      * **index.html** (first page): empty `nav-page-previous` div,
        next `<a href="about.html" … aria-label="About">About</a>`
        with right-arrow icon.
      * **about.html** (middle): prev →
        `<a href="index.html" … aria-label="Home">Home</a>` with
        left-arrow; next → `<a href="docs.html" … aria-label="Documentation">Documentation</a>`
        with right-arrow.
      * **docs.html** (last): prev →
        `<a href="about.html" … aria-label="About">About</a>` with
        left-arrow, empty `nav-page-next` div.
      * Bare-path entries enriched with profile titles ("Home",
        "About", "Documentation") via Phase 2 / shared
        `enrich_navigation_items` machinery.
- [x] Smoke fixture `/tmp/q2-phase4-separator-smoke/` — separator
      boundary `[a, ---, b, c]`. Observed:
      * **a.html**: separator-as-next, no prev → strip skipped
        entirely (lonely page).
      * **b.html**: prev empty (separator), next → `c.html`.
      * **c.html**: prev → `b.html`, next empty.
      Confirms Decision 4 separator semantics end-to-end.
- [x] Re-ran `/tmp/q2-phase2-smoke/` + `/tmp/q2-phase3-smoke/` after
      Phase 4 wiring. Sidebar (5 navbar elements + 1 footer) and
      Phase 3 active-class behavior unchanged. Page-nav now appears
      on Phase-2-smoke pages because they have a sidebar — matches
      the default-on contract.

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — 7797 tests pass, 195 skipped.
      New tests added in Phase 4: 3 (PageNavigation roundtrip) + 11
      (sidebar flatten + FlatEntry sanity) + 6 (page-nav HTML render)
      + 13 (page-nav generate) + 9 (page-nav render) + 6 (integration)
      = **48 new tests**, all passing.
- [x] `cargo xtask lint` passes (628 files checked).
- [x] `cargo xtask verify --skip-hub-tests` end-to-end green —
      Rust build + tests + fmt + clippy + lint + hub-client build
      (incl. WASM) + trace-viewer build + trace-viewer tests, all 9
      steps clean.
- [ ] **BLOCKED — `br` tool rejects all commands due to a stale
      `k-02o9` issue at line 124 of `.beads/issues.jsonl` (prefix
      mismatch with the project's `bd` prefix).** This blocks bead
      creation and `br close` / `br sync`. Surfaced to user;
      follow-up bead filing deferred until the JSONL data is
      reconciled or the user decides on a path forward.
- [ ] **Follow-ups to file once `br` is unblocked** (each tied back
      to this sub-plan):
      * `Emit <link rel="prev/next"> meta tags for page-nav` —
        Decision 7 deferred Q1's SEO/preload links to a follow-up;
        wiring goes through the HTML render config + template
        `<head>` slot, tangential to the page-nav feature itself.
      * `Suppress page-nav for custom-layout pages` — Q1 hides the
        strip when a page sets `page-layout: custom`; defer until a
        real page hits the edge case.
      * `Plain-text aria-label projection for rich titles` — once
        `DocumentProfile.title` supports inline markup, strip
        formatting for ARIA labels (current Phase 4 uses the title
        verbatim; titles are plain `String` today so this is a
        no-op).
      * `Index-forgiveness for page-source matching` — strict equality
        today; mirror Phase 3's `bd-jbml` framing if real content
        hits `about/` vs `about/index.qmd` drift.
      * *(epic-wide)* **`bd-tr81` docs site needs a dedicated
        section on page-navigation rules** — flatten + dedupe +
        separator-as-boundary + section-header-as-neighbor are
        non-obvious and the user explicitly flagged them
        (Decision 9). Tie to `bd-tr81`.
- [x] Updated the epic plan's "Work items" checklist — Phase 4 marked
      done, sub-plan linked.
- [x] Added the documentation reminder to the epic plan's
      "Epic-wide follow-ups surfaced by sub-plans" section.
- [ ] `br close <phase-4-bd>` — blocked (no bd issue ever created).
- [ ] `br sync --flush-only && git add .beads/ claude-notes/ crates/ && git commit`
      — beads sync skipped; commit will cover `claude-notes/` +
      `crates/` only.
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** re-picking the sidebar in `PageNavGenerateTransform`
  would duplicate logic and risk drift from the sidebar the user
  sees. *Mitigation:* read `navigation.sidebar` (resolved, post-pick)
  only. Zero re-picking.

- **Risk:** flatten-order mismatch with Q1 would produce "wrong next
  page" for sections with multi-level nesting. *Mitigation:* Test 12
  pins depth-first order against a handcrafted fixture reviewers can
  eyeball; `flatten_items` in Q1 is also depth-first so the match is
  natural.

- **Risk:** dedupe-by-href drops a legitimately distinct entry when
  two sidebar entries share an href with different `text`.
  *Mitigation:* Q1 accepts the same risk; keeping first-occurrence
  matches user expectations ("the top of the sidebar is the canonical
  entry"). Revisit if a real site complains.

- **Risk:** separators used as visual-only dividers (not intended as
  hard boundaries) will surprise users. *Mitigation:* Q1 users have
  lived with this semantics for years; matching is safer than
  inventing. Document the boundary behavior in the docs site (epic
  follow-up tied to Decision 9).

- **Risk:** a stray `Auto` entry reaching Phase 4 after Phase 2/3
  expansion would be silent (per Decision 4). *Mitigation:* it
  shouldn't happen (Auto expansion lives in
  `SidebarGenerateTransform`, and `strip_auto` removes anything that
  couldn't expand). Test 9 documents the defensive skip. An
  assertion-failure alternative was considered and rejected —
  Phase 4 shouldn't panic on upstream bugs, just not-render.

- **Risk:** `page-navigation: false` at the project level not
  flowing to doc-level via metadata merge. *Mitigation:* uses the
  same `is_feature_disabled` path as sidebar/navbar/footer/toc —
  already tested in Phase 2/3; Test 41 locks in the project-level
  disable path.

- **Risk:** single-doc users with no sidebar setting
  `page-navigation: true` expecting *something* to appear.
  *Mitigation:* Test 44 pins the no-sidebar-no-navigation contract;
  surface in docs that page-navigation requires a sidebar.

- **Risk:** HTML markup drift from Q1 breaks Q1 CSS reuse in Phase 5.
  *Mitigation:* Test 18 pins the icon class names; Test 13/14 pin
  the wrapper div / link classes; integration Test 39 inspects the
  rendered strings for the exact Q1 class vocabulary.

## Explicit non-goals for this phase

- No `<link rel="prev">` / `<link rel="next">` in `<head>` (follow-up).
- No `usesCustomLayout` suppression (follow-up if a real page needs it).
- No `plainText` aria-label stripping (requires rich-title support).
- No changes to sidebar/navbar/footer transforms. Their outputs are
  inputs to page-nav, not the other way around.
- No changes to `ProjectIndex`, `DocumentProfile`, or `ProjectType`.
- No book/manuscript-specific page-nav semantics (chapter numbering,
  appendix handling).
- No CSS / JS (Phase 5).
- No sitemap / favicon / title prefix (Phase 7).
- No `quarto preview`-side behavior (separate epic).

## Follow-up beads (to file at close-out)

- **Head `<link rel>` for page-nav** — emit `<link rel="prev">` /
  `<link rel="next">` in `<head>` when page-nav is active. Q1 does
  this for SEO + browser preload.
- **`usesCustomLayout` suppression** — mirror Q1's check that hides
  page-nav when a user page opts into `page-layout: custom`.
- **`aria-label` plain-text stripping** — once `DocumentProfile.title`
  supports inline markup, add a plain-text projection for ARIA
  labels.
- **Index-forgiveness for page source matching** — today page-nav
  looks for `page_source == entry.href` (exact). If real content
  hits `about/` vs `about/index.qmd` path-normalization drift, add
  the same forgiveness Phase 3 filed as `bd-jbml` for navbar.
- *(epic-wide, cross-phase)* **Prev/next rules need dedicated user-
  facing docs in the Q2 docs site** — dedupe, separator, section-
  header-as-neighbor are all non-obvious. Tie to `bd-tr81`.

## Decisions log (confirmed 2026-04-24)

1. **Config placement** stays top-level (`page-navigation: bool`). Not
   under `website.`. Matches Q1 and Phase 3 "per-doc chrome at top".
2. **Pipeline position**: Generate after `SidebarGenerateTransform`;
   Render after `SidebarRenderTransform`. No other ordering changes.
3. **Data model**: `PageNavigation { prev, next: Option<NavigationItem> }`
   in `quarto-navigation`. Reuses existing item type + Phase 3
   helpers.
4. **Flatten algorithm**: depth-first; include internal Links /
   Sections-with-href / Separators; skip Heading / Auto / externals;
   dedupe by href; separators break adjacency. Matches Q1.
5. **Default behavior**: on whenever a sidebar applies and at least
   one neighbor exists. Silent no-op otherwise. Top-level
   `page-navigation: false` disables.
6. **Render output**: Q1-matching HTML (`nav-page-previous`,
   `nav-page-next`, `pagination-link`, Bootstrap icons). Empty-side
   wrappers still emitted.
7. **`<link rel>` meta tags**: deferred to follow-up.
8. **Template slot**: `$rendered.navigation.page_navigation$` inside
   `<main>`, after `$body$`.
9. **Documentation note**: the flatten+dedupe+separator rules need a
   dedicated docs section in `bd-tr81` work; carry as an epic-wide
   follow-up reminder.

## Epic-level impact

Phase 4 completes the **user-visible** navigation surface for websites:

- navbar (top) — Phase 3
- sidebar (column) — Phase 2
- prev/next strip (page bottom) — Phase 4
- page-footer (site bottom) — Phase 3

After Phase 4, the **information architecture** for a website is
complete. What's left in the epic is **production plumbing**: shared
CSS/JS via `site_libs/` (Phase 5), cross-document body-link rewriting
(Phase 6), post-render artifacts like sitemap and favicon (Phase 7),
incremental rebuilds (Phase 8), and hub-client live preview (Phase 9).

Phase 4 is the last "structural feature" phase; everything afterwards
is about making the structure ship-quality.
