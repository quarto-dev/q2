# L5 — Categories sidebar (sub-plan)

**Date:** 2026-05-06
**Beads:** `bd-5vsr` (this phase). Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0 (`bd-n8a4`, closed) — `DocumentProfile.listing_item` /
  `categories_raw`.
- L1 (`bd-izqh`, closed) — `ListingItemInfoStage` auto-fill
  (`profile.listing_item.categories`).
- L2 reference doc:
  `claude-notes/plans/2026-05-06-listings-L2-data-model.md`
  — see §"Per-item template binding" line 546 where
  `category-html` is reserved for this phase.
- L3 / L4 (`bd-ml8z`, `bd-b5jm`, **in-flight on
  `feature/listings`**, awaiting push approval at the time this
  plan is written) — listing types, generate/render transforms,
  built-in templates, vendored `list.min.js` +
  `quarto-listing.js`. The L5 session begins **after** L3 is
  merged onto `feature/listings`. See §"Branch / worktree" below.

**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Land Q1-feature-parity categories markup for listing host pages.
Specifically:

1. **Per-item category badges.** Add a pre-rendered
   `category-html` field to the per-item template binding (L2 line
   546 has been reserving this slot since the data-model plan
   landed). Update `item-default.template` and
   `item-grid.template` to splice it in. Per-item categories
   render as Q1-shape clickable chips that invoke
   `window.quartoListingCategory('<b64>')` on click.
2. **Right-margin categories sidebar.** New AST transform
   `CategoriesSidebarTransform` that runs after
   `ListingRenderTransform`. Reads
   `RenderContext::resolved_listings`, aggregates the unique
   category set across **all** listings on the page where
   `listing.categories != Disabled`, and writes Q1-shape HTML
   (heading + `<div class="quarto-listing-category category-…">`
   container) to the new metadata key
   `rendered.navigation.margin_categories`.
3. **Template plumbing.** Modify `FULL_HTML_TEMPLATE` in
   `crates/quarto-core/src/template.rs` so
   `<div id="quarto-margin-sidebar">` opens whenever **either**
   `rendered.navigation.toc` **or**
   `rendered.navigation.margin_categories` is present, and
   renders both inside (TOC first, categories below).
4. **Three style modes** with Q1-equivalent class semantics:
   - `category-default` — emits a leading "All" pill plus
     `<count>` spans on each pill.
   - `category-unnumbered` — pills only, no counts, no "All".
   - `category-cloud` — pills sized by frequency; class
     `category-cloud-1` … `category-cloud-10` per Q1's
     `Math.ceil((count / total) * 10)` formula.
5. **Click-filter integration.** The vendored
   `quarto-listing.js` (shipped in L3 phase 7) already provides
   `window.quartoListingCategory(category)` and binds click
   handlers on `.quarto-listing-category .category` and
   `.quarto-listing-category-title`. L5 emits the markup those
   handlers expect; no JS changes.
6. **Pass `cargo xtask verify`** (full, including hub-client
   build).

L5 is **markup-only** for visual styling: the category-cloud
font scaling, sidebar padding, hover cursor, and other Q1
visuals all live in `quarto-listing.scss`. That SCSS is
deferred under `bd-57y4` (filed during L3 close-out). L5 ships
a working feature with default browser styling; bd-57y4
restores Q1 visual parity. See §"Visual styling" below.

**Out of scope for L5 (deferred):**
- **Localization.** L5 hardcodes English "Categories" and
  "All". A new follow-up bd issue files the proper plumbing for
  reading `language.listing-page-field-categories` /
  `language.listing-page-category-all` from a localization
  resource. See §"Filing reminder".
- **bd-57y4 SCSS work.** Visual parity tracked separately. L5's
  markup is byte-for-byte compatible with the Q1 SCSS so
  bd-57y4 is purely a CSS-bundling exercise.
- **Click-filter activation animations / page-state hash
  routing.** Already handled by the vendored
  `quarto-listing.js`.
- **Per-item categories on `item-table.template`.** The v1
  table is hardcoded to title/date/author and doesn't yet
  honor `listing.fields`. Categories-as-table-column is folded
  into the broader bd-0wyo `otherFields` work.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L5" + §"Architecture summary".
- L2 data-model:
  `claude-notes/plans/2026-05-06-listings-L2-data-model.md`
  §"Per-item template binding" (line 546 reserves the
  `category-html` field for L5).
- L3 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
  §"Hand-off summary" (the source-of-truth state of the
  branch L5 builds on; specifically, points 1 and 2 in
  §"Things the next session should know" describe the
  `RenderContext::resolved_listings` shape and the host-dir
  `.qmd` link convention L5 inherits).
- Q1 reference (the implementation L5 ports the *shape* of):
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-categories.ts`
    — the `categorySidebar`, `accumCategories`,
    `categoryElement` functions. L5's transform produces the
    same DOM shape with no `deno_dom` dependency (we emit
    HTML strings directly).
  - `external-sources/quarto-cli/src/resources/projects/website/listing/quarto-listing.scss`
    lines 264–299 — the sidebar / cloud styling we *need* the
    bd-57y4 SCSS to provide for visual parity.
  - `external-sources/quarto-cli/src/resources/projects/website/listing/quarto-listing.js`
    — the click-handler attachment logic that consumes our
    markup. Lines 60–87 are the load-bearing block; we don't
    modify the JS.
  - `external-sources/quarto-cli/src/resources/projects/website/listing/item-default.ejs.md`
    lines 64–74 — Q1's per-item category emission. L5 ports
    this into `helpers::category_html`.
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing.ts`
    lines 380–406 — the `listingPostProcess` block that
    inserts the sidebar. L5 replaces this with the AST
    transform + template integration described below.
- Existing Q2 precedent (the pattern L5 follows):
  - `crates/quarto-core/src/transforms/toc_render.rs` — the
    closest analog. Reads structured data from metadata,
    renders HTML, writes back to a `rendered.navigation.*`
    key. L5's `CategoriesSidebarTransform` is shaped almost
    identically but reads from `RenderContext::resolved_listings`
    instead of metadata.
  - `crates/quarto-core/src/template.rs` lines 189–198 —
    the existing `#quarto-margin-sidebar` slot we extend.
- Existing L3 surface L5 reuses:
  - `crates/quarto-core/src/render.rs` line 187 —
    `RenderContext::resolved_listings`.
  - `crates/quarto-core/src/project/listing/binding.rs`
    line 16 — the `category-html` deferral comment that L5
    resolves.
  - `crates/quarto-core/src/project/listing/helpers.rs` —
    sibling helpers (`image_html`, `metadata_attrs`) that
    `category_html` lives next to.
  - `crates/quarto-core/src/transforms/listing_render.rs`
    line 124 — confirms `resolved_listings` is **put back**
    after `ListingRenderTransform` runs (so L5's transform
    can read it).

## Settled inputs

These are decisions, not open questions:

- **Both per-item badges and right-margin sidebar in v1.**
  User-confirmed 2026-05-06. The two pieces together form a
  coherent click-filter UX; shipping just one would feel
  asymmetric.
- **Sidebar lands in `#quarto-margin-sidebar` via a new
  `rendered.navigation.margin_categories` metadata key + a
  small `template.rs` change.** User-confirmed 2026-05-06.
  Mirrors `rendered.navigation.toc` precedent. Categories
  render *after* the TOC inside the same outer container.
- **Markup-only L5; visuals through bd-57y4.**
  User-confirmed 2026-05-06. L5 emits the exact Q1 markup so
  bd-57y4 can ship the SCSS unchanged.
- **Hardcoded English labels + new follow-up bd for
  localization.** User-confirmed 2026-05-06. The follow-up bd
  filing is part of L5 hand-off; see §"Filing reminder".
- **`CategoriesSidebarTransform`, not extension of
  `ListingRenderTransform`.** Decision per the epic plan's L5
  scope ("New transform … or extend …; decide in L5's sub-plan
  based on code shape"). The two transforms have different
  *roles* (per-listing splice vs. cross-listing aggregation)
  and different inputs (`resolved_listings` consumed once vs.
  re-read across all listings on the page). A separate
  transform also makes the disable/enable surface cleaner —
  setting `categories: false` on every listing should produce
  no sidebar even when listings render normally.
- **Pre-rendered HTML, not inline doctemplate iteration, for
  per-item category chips.** Doctemplate's grammar pipe set
  (per L4.1) does not include base64 encoding, and we need
  `b64EncodeUnicode(category_name)` for the click handler to
  match Q1's decoder. Pre-rendering server-side keeps the
  template-binding contract typed.
- **Encoding is `b64(percent-encoded UTF-8)`, not
  `b64(raw UTF-8)`.** Q1's `b64EncodeUnicode` is
  `btoa(encodeURIComponent(s))`
  (`external-sources/quarto-cli/src/core/base64.ts`), and the
  vendored `quarto-listing.js` decoder is
  `decodeURIComponent(atob(b64))`. The Rust encoder L5 ships
  must match: percent-encode the category string with the
  JS-`encodeURIComponent` reserved set first, then base64
  the resulting ASCII bytes. For ASCII-only categories the
  output is identical to a raw-bytes encoding; for non-ASCII
  the JS round-trip only succeeds with the percent-encoded
  form. See §"Filing reminder" for the broader-review
  follow-up bd that revisits whether Q2 should keep this
  scheme or switch to a simpler one (and adjust
  `quarto-listing.js` correspondingly).
- **Per-item chips use inline `onclick`; sidebar pills use
  `data-category` + JS-attached handlers.** Mirrors Q1's
  idiom. Per-item chips are emitted inside the post body
  where the JS click delegate doesn't reach; sidebar pills
  are picked up by `quarto-listing.js`'s
  `querySelectorAll(".quarto-listing-category .category")`
  on page load.
- **Cloud sizing formula:
  `Math.ceil((count / total_items_across_listings) * 10)`,
  clamped to `[1, 10]`.** Q1 uses
  `Math.ceil((count / totalCategories) * 10)` where
  `totalCategories` is actually the total *item* count across
  listings (Q1 named the variable misleadingly; see
  `website-listing-categories.ts` line 38). L5 matches the
  computed value, not the name.

## Architecture

### Where the sidebar transform runs

Pipeline insertion is between `ListingRenderTransform` and
`TocRenderTransform`. The relevant slice of
`build_transform_pipeline` (`pipeline.rs:792–810`) becomes:

```
pipeline.push(Box::new(ListingGenerateTransform::new()));
pipeline.push(Box::new(ListingRenderTransform::new()));
pipeline.push(Box::new(CategoriesSidebarTransform::new()));   // new
pipeline.push(Box::new(TocRenderTransform::new()));
pipeline.push(Box::new(NavbarRenderTransform::new()));
pipeline.push(Box::new(SidebarRenderTransform::new()));
pipeline.push(Box::new(PageNavRenderTransform::new()));
pipeline.push(Box::new(FooterRenderTransform::new()));
```

Order rationale:

- **After `ListingRenderTransform`** so the listing markup is
  already in the AST when the sidebar transform runs. The
  sidebar reads `ctx.resolved_listings` (which
  `ListingRenderTransform` puts back at line 124), not the
  AST, so technically order with `ListingRender` is independent
  — but conceptually all listings work belongs together.
- **Before `TocRenderTransform`** so both transforms write to
  `rendered.navigation.*` before any consumer reads. (The
  consumer is `ApplyTemplate`, much later.)
- **Before the rest of the `*_render` transforms** so the
  pipeline-walk pattern stays "generate first, render second"
  with all renders contiguous. The L3 sub-plan's D2 caveat
  about Lua-injection points (bd-0fd0) applies the same way
  here: the resolved-data → rendered-HTML boundary is one of
  several places that becomes a Lua-filter slot when bd-0fd0
  lands. L5 leaves a `// TODO(bd-0fd0):` marker.

### What `CategoriesSidebarTransform` does

```rust
#[async_trait::async_trait(?Send)]
impl AstTransform for CategoriesSidebarTransform {
    fn name(&self) -> &str { "categories-sidebar" }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext)
        -> Result<()>
    {
        if is_feature_disabled(&ast.meta, "listing") { return Ok(()); }

        // Already-set means a Lua filter or earlier stage produced it.
        if ast.meta.contains_path(&["rendered", "navigation",
                                     "margin_categories"]) {
            return Ok(());
        }

        // Aggregate across all listings on the page that have
        // categories != Disabled.
        let aggregate = aggregate_categories(&ctx.resolved_listings);
        if aggregate.is_none() { return Ok(()); }
        let agg = aggregate.unwrap();

        let html = render_sidebar_html(&agg);
        ast.meta.insert_path(
            &["rendered", "navigation", "margin_categories"],
            ConfigValue::new_string(&html, SourceInfo::default()),
        );

        Ok(())
    }
}
```

Aggregation rule (mirrors Q1's `accumCategories` +
`itemCount`):

1. Walk every `ResolvedListing` on the page.
2. Skip listings where `listing.categories == Disabled`.
3. **Mode resolution.** If listings on the page declare
   *different* category modes (one `Default`, another `Cloud`),
   the page-level mode is the **first** non-`Disabled` mode in
   declaration order. L5 emits a single sidebar; mixing modes
   on one page is unusual and the first-wins rule keeps the
   visible behavior deterministic. (Q1 sidesteps this by
   reading `options[kCategoryStyle]` from the
   `ListingSharedOptions` shared across listings; Q2's typed
   per-listing `categories` field doesn't have that shared
   container yet, hence the rule.) A diagnostic
   `Q-12-11` (new code; see §"Diagnostic codes") fires when
   a page mixes modes.
4. For each non-disabled listing, walk its `items` and
   accumulate counts: `count[category] += 1`.
5. Track `total_item_count` = sum of `items.len()` over
   non-disabled listings (Q1's "totalCategories" variable;
   used by cloud sizing).
6. If the aggregate map is empty:
   - **Some listing did set `categories:` to a non-`Disabled`
     mode**: emit `Q-12-12` with the first such listing's
     `categories:` YAML source span, return
     `AggregatedCategories::Empty { first_listing_id, span }`
     so the transform can record the diagnostic but write no
     `margin_categories` key.
   - **No listing set `categories:`**: return `None` silently.

The aggregation result distinguishes the three outcomes:

```rust
pub enum AggregateOutcome {
    /// No listing on the page enables categories. Silent skip.
    SilentSkip,
    /// At least one listing enables categories but no resolved
    /// item carries any. Emit Q-12-12; write no key.
    EnabledButEmpty {
        first_listing_id: String,
        source_info: SourceInfo,  // span on the `categories:` key
    },
    /// Aggregate is non-empty; render the sidebar.
    Rendered(AggregatedCategories),
}

pub struct AggregatedCategories {
    pub mode: ListingCategoriesMode, // Default | Unnumbered | Cloud
    pub counts: BTreeMap<String, u32>, // sorted-by-key for stable
                                       // case-insensitive sort below
    pub total_items: u32,
}
```

### Sidebar HTML shape

The HTML written to `rendered.navigation.margin_categories` is
the **inner** content (heading + container), not the outer
`#quarto-margin-sidebar` wrapper — the wrapping is the
template's job (mirrors how `rendered.navigation.toc` is just
the inner `<ul>`).

For `category-default` mode:

```html
<h5 class="quarto-listing-category-title">Categories</h5>
<div class="quarto-listing-category category-default">
<div class="category" data-category="<b64-of-empty-or-all>">All <span class="quarto-category-count">(15)</span></div>
<div class="category" data-category="<b64-of-design>">design <span class="quarto-category-count">(8)</span></div>
<div class="category" data-category="<b64-of-rust>">rust <span class="quarto-category-count">(7)</span></div>
</div>
```

For `category-unnumbered`: same structure but no
`<span class="quarto-category-count">` and no leading "All".

For `category-cloud`: each `<div class="category">` wraps its
contents in `<span class="quarto-category-count category-cloud-N">…</span>`,
where `N = max(1, min(10, ceil(count/total_items * 10)))`.

Sort: case-insensitive alphabetical (Q1 uses
`a.toLocaleLowerCase().localeCompare(b.toLocaleLowerCase())`).

`b64-of-X` = base64-encoded UTF-8 bytes of category name X.
For "All", Q1 uses the *localized* string base64-encoded —
matched here by base64-encoding the same English "All" we
emit as text. (When the localization follow-up bd lands, it
threads through both.)

### Per-item category HTML (added to the binding)

Q1's per-item chips:

```html
<div class="listing-categories">
<div class="listing-category" onclick="window.quartoListingCategory('<b64>'); return false;">design</div>
<div class="listing-category" onclick="window.quartoListingCategory('<b64>'); return false;">rust</div>
</div>
```

L5 adds `helpers::category_html(item)` and exposes it on the
binding as `category-html`:

```rust
fn category_html(item: &ListingItem) -> String {
    if item.categories.is_empty() { return String::new(); }
    let mut s = String::from(r#"<div class="listing-categories">"#);
    for cat in &item.categories {
        let b64 = b64_encode_unicode(cat);
        s.push_str(&format!(
            r#"<div class="listing-category" onclick="window.quartoListingCategory('{}'); return false;">{}</div>"#,
            html_escape(&b64),
            html_escape(cat),
        ));
    }
    s.push_str("</div>");
    s
}

/// Mirror Q1's `b64EncodeUnicode` from `core/base64.ts`:
/// `btoa(encodeURIComponent(s))`. The vendored
/// `quarto-listing.js` decodes with `decodeURIComponent(atob(b64))`,
/// so the Rust side must percent-encode UTF-8 before base64-encoding.
/// Sub-plan §"Filing reminder" tracks a future review of whether
/// Q2 should keep this scheme or switch to a simpler one (and
/// adjust both encoder and decoder together).
fn b64_encode_unicode(s: &str) -> String {
    let percent = encode_uri_component(s);
    base64::engine::general_purpose::STANDARD.encode(percent.as_bytes())
}

/// JavaScript-compatible `encodeURIComponent`. Encodes every UTF-8
/// byte as `%XX` except the unreserved set:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let b = *byte;
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
```

(No new workspace deps — `percent-encoding` is a transitive
dep but not directly used anywhere in our crates, and the
follow-up bd may rip out this whole encode/decode pair, so
the inline version stays self-contained in `helpers.rs`.
The unreserved set is the JS-`encodeURIComponent` set
verbatim — validate against the test fixtures below.)

The binding (`binding.rs`) inserts `category-html` next to the
existing `image-html`, `metadata-attrs`,
`description-placeholder` keys. The two item templates use
it:

`item-default.template` after `$if(subtitle)$ … $endif$` and
before `$if(description)$`:

```
$if(show.categories)$
$if(category-html)$
```{=html}
$category-html$
```
$endif$
$endif$
```

`item-grid.template` similarly, slotted between subtitle and
description.

(The fenced `{=html}` block lets the raw HTML string survive
the markdown re-parse step in `ListingRenderTransform` step 6
without being interpreted as text. This is the same idiom Q1
uses inside its EJS templates.)

`item-table.template` is unchanged — Q1 doesn't put per-item
category chips on table rows either; the categories there are
a column when configured. Folded into bd-0wyo.

### Template change in `template.rs`

Replace lines 189–198 of `FULL_HTML_TEMPLATE` with:

```
$if(rendered.navigation.toc)$
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
<nav id="TOC" role="doc-toc" class="toc-active">
$if(navigation.toc.title)$
<h2 id="toc-title">$navigation.toc.title$</h2>
$endif$
$rendered.navigation.toc$
</nav>
$if(rendered.navigation.margin_categories)$
$rendered.navigation.margin_categories$
$endif$
</div>
$else$
$if(rendered.navigation.margin_categories)$
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
$rendered.navigation.margin_categories$
</div>
$endif$
$endif$
```

This is verbose but each branch is independently auditable.
The structure:

- TOC present → sidebar opens; TOC nav inside; categories
  appended after the nav if present.
- TOC absent, categories present → sidebar opens with just
  categories.
- Neither → no sidebar (preserves current behavior on
  non-listing, no-toc pages).

`MINIMAL_HTML_TEMPLATE` (line 80) does **not** carry the
margin sidebar — it's intentionally minimal — and L5 leaves
it untouched.

### Module layout

```
crates/quarto-core/src/transforms/
  categories_sidebar.rs   ← new

crates/quarto-core/src/project/listing/
  config.rs               ← add `categories_source: SourceInfo`
                            on Listing so Q-12-12 has a span
  helpers.rs              ← add `category_html(item) -> String`
  binding.rs              ← bind `category-html` per item
  templates/
    item-default.template ← splice in per-item category block
    item-grid.template    ← splice in per-item category block

crates/quarto-core/src/
  template.rs             ← extend FULL_HTML_TEMPLATE margin slot
  pipeline.rs             ← register CategoriesSidebarTransform

crates/quarto-error-reporting/
  error_catalog.json      ← add Q-12-11 (mixed category modes)
                            and Q-12-12 (categories enabled but
                            no item has any)
```

The transform module follows the L3 sibling pattern
(`listing_render.rs`, `listing_generate.rs`). Module size
will be ~150–200 lines including aggregation helpers and
HTML emission.

## Diagnostic codes

L5 adds two new entries under the existing `Q-12` ("listing")
subsystem:

- **`Q-12-11`** — *page declares more than one category style*.
  Emitted on the host-page diagnostic stream when at least two
  listings on the page have non-`Disabled` `categories` modes
  that disagree (e.g. one `Default`, another `Cloud`). Severity
  warning. The first non-`Disabled` mode in declaration order
  wins for the rendered sidebar.
- **`Q-12-12`** — *categories enabled but no item has any
  categories*. Emitted once per host page when at least one
  listing on the page sets `categories:` to a non-`Disabled`
  mode but the page-level aggregate is empty (no items have
  any categories). Severity warning. Source span points at the
  first such listing's `categories:` YAML key. Message body
  per §"Open questions" item 5. Suppressed when the user
  explicitly sets `categories: false` (which produces no
  sidebar with no diagnostic).

Catalog entries land in
`crates/quarto-error-reporting/error_catalog.json` as part of
L5; the L3 plan's catalog batch already established the
`Q-12-N` series and the entry-shape convention.

## Visual styling (relationship to bd-57y4)

L5's emitted markup is byte-for-byte equivalent to Q1's. The
visual classes L5's HTML uses, all of which require the
deferred `quarto-listing.scss`:

| Class                                | Source SCSS lines (Q1) | Used by                   |
|--------------------------------------|------------------------|---------------------------|
| `.quarto-listing-category-title`     | 270–274                | sidebar heading           |
| `.quarto-listing-category`           | 277–284                | sidebar container         |
| `.quarto-listing-category .category` | 278–283                | sidebar pills             |
| `.quarto-category-count`             | (referenced inline)    | sidebar count + cloud span|
| `.category-cloud-1` … `-10`          | 286–299                | cloud-mode font scaling   |
| `.listing-categories`                | (per-item block)       | per-item chip container   |
| `.listing-category`                  | (per-item block)       | per-item chip             |

Without bd-57y4, browsers render:

- Sidebar pills as a vertical stack of plain divs.
- Per-item chips as a horizontal-ish stack of plain divs (both
  `display: block` by default; the SCSS adds the inline
  treatment).
- Cloud mode shows pills at uniform size (the `font-size`
  scaling lives in `.category-cloud-N`).
- No hover cursor (the SCSS sets `cursor: pointer`).

The functionality is intact in all three browsers — clicks
still fire `quartoListingCategory(...)` and the listing
filters via `list.js` correctly. The session that lands
bd-57y4 should run the L5 fixture again and confirm visual
parity.

The user-facing reference doc for listings (lands in L8 or
L11) should carry a callout: "Visual styling for category
sidebars is shipped via the listing SCSS bundle. If you've
disabled bundled CSS or are using a heavily customized theme,
you may need to provide your own category styles."

## Open questions

These are non-blocking but the L5 session author should
resolve inline rather than punt:

1. **Page with no listings, but `meta.listings` already set
   (Lua-filter or test override).** Do we still aggregate?
   *Recommend yes:* read from
   `RenderContext::resolved_listings` only — there's no
   Lua-filter slot today (bd-0fd0) and the typed field is the
   single source of truth. If a future Lua slot needs to inject
   resolved listings, it routes through the typed field at the
   same boundary.
2. **Categories rendered for pages that aren't website
   members.** Q1 ties category sidebar to `WebsiteProjectType`
   post-processing; Q2's transform runs on every render. If
   someone declares `listing: default; categories: true` on a
   standalone `.qmd` (no website context), they get a sidebar.
   *Recommend ship as-is:* the sidebar is harmless on non-
   website pages, and the user opted in by configuring
   `categories:`. Q1's restriction is incidental, not
   load-bearing.
3. **"All" pill mode-default text.** Q1 uses `localizedString(format,
   kListingPageCategoryAll) = "All"`. *Recommend hardcode
   "All"*; localization follow-up bd handles overrides.
4. **`categories: true` on `type: table`.** Should the
   categories sidebar still emit even though the table itself
   doesn't render per-item chips? *Recommend yes* — the
   sidebar lets table-listing users filter by category via
   list.js, which uses the existing `data-categories` attrs
   that `helpers::metadata_attrs` already emits for every
   item regardless of listing type.
5. **Empty-category-list edge case.** A listing has
   `categories: default` (or `unnumbered` / `cloud`) but every
   item has zero categories. *Recommend no sidebar emitted +
   `Q-12-12` warning* on the host page, with a source span on
   the listing's `categories:` YAML key. Message: *"Listing
   `<id>` configures `categories: <mode>`, but no resolved
   listing item has any categories defined; sidebar is
   suppressed. Add `categories: [...]` to the listing's
   content posts (in their frontmatter or `listing-item:`
   block), or set `categories: false` to silence this
   warning."* The diagnostic fires once per host page (not
   once per listing) when the page-aggregate is empty but at
   least one listing has a non-`Disabled` mode set.

## Decisions log

- **D1 (separate transform, not extension):** confirmed
  2026-05-06.
- **D2 (sidebar HTML lives at
  `rendered.navigation.margin_categories`):** confirmed
  2026-05-06. Mirrors `rendered.navigation.toc`.
- **D3 (template tweak in `FULL_HTML_TEMPLATE` only):**
  confirmed 2026-05-06. `MINIMAL_HTML_TEMPLATE` stays
  intentionally minimal.
- **D4 (markup-only L5 + bd-57y4 unchanged):** confirmed
  2026-05-06.
- **D5 (hardcoded English; new follow-up bd for
  localization):** confirmed 2026-05-06. The follow-up bd is
  filed at L5 hand-off — see §"Filing reminder".
- **D6 (per-item chips via pre-rendered `category-html`
  binding):** chosen because doctemplate's grammar pipe set
  has no base64 encoder and the click handler needs the
  encoding. Matches L2 line 546.
- **D7 (mixed-mode page → first-wins + `Q-12-11` warning):**
  recommended in §"Settled inputs". Sub-plan author confirms
  during impl after surveying real-world Q1 pages — if mixed
  modes are exceedingly rare, "first-wins silently" is also
  acceptable.
- **D8 (worktree on `feature/listings`, branched after L3
  merge):** see §"Branch / worktree" below.

## Branch / worktree

L5 starts **after** L3 (`bd-ml8z` + `bd-b5jm`) is merged onto
`feature/listings`. The L5 worktree lives at:

```
.worktrees/bd-5vsr-listings-categories-sidebar/
```

Branch: `beads/bd-5vsr-listings-categories-sidebar`, branched
off `feature/listings`. Same pattern as L1 / L3.

Per `.claude/rules/worktrees.md`:

```bash
cd .worktrees/bd-5vsr-listings-categories-sidebar
echo "../../../.beads" > .beads/redirect
npm install
cargo xtask verify --skip-hub-build  # baseline before changes
```

Before starting, the L5 session must verify:

- `feature/listings` is current (post-L3-merge HEAD includes
  the listing transforms, the vendored `quarto-listing.js`,
  and the `RenderContext::resolved_listings` field).
- `cargo nextest run --workspace` passes on the new branch
  before any changes.
- Baseline test count to record in this plan after the
  worktree is bootstrapped (the L5 hand-off's "+N" delta is
  measured against this baseline, not L0/L1/L3's).

If for some reason L3 has **not** yet merged when L5 begins,
the L5 session must stop and ask the user before proceeding —
the L5 sub-plan assumes the L3 surface is on `feature/listings`.

## Tests (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch pass.

### Unit tests — helpers

In `crates/quarto-core/src/project/listing/helpers.rs`:

1. **`category_html_empty_when_item_has_no_categories`** —
   item with `categories: vec![]` → empty string. (No wrapping
   `<div class="listing-categories">` either.)
2. **`category_html_emits_one_div_per_category`** — item with
   `categories: ["rust", "design"]` → string contains
   `<div class="listing-category"` exactly twice.
3. **`category_html_b64_encodes_handler_arg`** — item with
   `categories: ["café"]` → onclick attribute contains
   `Y2FmJUMzJUE5` (base64 of `caf%C3%A9`, matching Q1's
   `btoa(encodeURIComponent("café"))`). An ASCII-only
   companion case `categories: ["rust"]` → `cnVzdA==`
   (identical under either scheme; locks the ASCII path).
4. **`category_html_html_escapes_display_text`** — item with
   `categories: ["<script>"]` → display text is
   `&lt;script&gt;` (no raw `<`).

### Unit tests — sidebar aggregation

In `crates/quarto-core/src/transforms/categories_sidebar.rs`
(or a separate `aggregation.rs` sub-module):

5. **`aggregate_returns_none_when_no_listings`** — zero
   listings → `None`.
6. **`aggregate_skips_disabled_listings`** — one listing with
   `categories: Disabled` → `None`.
7. **`aggregate_counts_categories`** — listing with items
   `[a,a,b]` → `{a: 2, b: 1}`, total 3, mode `Default`.
8. **`aggregate_unions_across_listings`** — two listings on
   one page, listing A items `[a,b]`, listing B items
   `[b,c]` → `{a:1, b:2, c:1}`, total 4.
9. **`aggregate_inherits_first_non_disabled_mode`** — two
   listings, A `Cloud` + B `Default` (in declaration order) →
   mode `Cloud`. Diagnostic `Q-12-11` recorded.
10. **`aggregate_drops_items_with_no_categories`** — listing
    with 5 items, only 2 of which have categories → counts
    reflect the 2 items; total_items uses listing's full item
    count (matches Q1).
11. **`aggregate_case_insensitive_sort_alphabetic`** — counts
    map `{B:1, a:1}` produces sort order `[a, B]` after
    aggregation. (BTreeMap is case-sensitive; the sort happens
    at HTML-emit time.)

### Unit tests — sidebar HTML emission

12. **`emits_default_mode_with_all_pill`** — `Default` mode +
    counts `{rust:2, design:1}`, total 3 → output contains
    a heading, an "All" pill with count "3", and pills for
    rust and design with their counts.
13. **`emits_unnumbered_mode_no_counts_no_all`** —
    `Unnumbered` mode → no `<span class="quarto-category-count">`
    elements; no "All" pill.
14. **`emits_cloud_mode_with_size_classes`** — `Cloud` mode +
    counts `{a:5, b:1}`, total 6 → pill for `a` has class
    `category-cloud-9` (ceil(5/6 * 10) = 9), pill for `b`
    has `category-cloud-2` (ceil(1/6 * 10) = 2).
15. **`cloud_mode_clamps_to_one_minimum`** — count 0 (won't
    happen by aggregation but defensive); cloud mode 1.
16. **`heading_is_categories`** — the sidebar always emits
    `<h5 class="quarto-listing-category-title">Categories</h5>`.
17. **`pills_b64_encode_data_category`** — sidebar pill for
    "café" has `data-category="Y2FmJUMzJUE5"` (base64 of
    `caf%C3%A9`, Q1-compatible). The "All" pill has
    `data-category=""` (b64 of the empty string is the empty
    string under either scheme).
18. **`pills_html_escape_display_text`** — sidebar pill for
    `"<bold>"` displays `&lt;bold&gt;`.
19. **`pills_sorted_case_insensitive`** — counts
    `{Zebra:1, apple:1}` → output order is `apple` then
    `Zebra`.

### Transform tests

20. **`transform_no_op_when_listing_disabled_in_meta`** —
    `meta.listing: false` → no `margin_categories` key
    written.
21. **`transform_no_op_when_already_set_in_meta`** — existing
    `rendered.navigation.margin_categories` (e.g. set by Lua
    filter or earlier stage) is not overwritten.
22. **`transform_no_op_with_empty_resolved_listings`** —
    `ctx.resolved_listings.is_empty()` → no key written.
23. **`transform_no_op_when_all_listings_have_categories_disabled`**
    — even with multiple resolved listings, none with
    `categories != Disabled` → no key written, no diagnostic.
23b. **`transform_emits_q_12_12_when_enabled_but_no_item_has_categories`**
    — listing with `categories: default` and items present
    but none have any categories → no `margin_categories`
    key written; one `Q-12-12` warning recorded with the
    listing's `categories:` source span. Subsequent listings
    on the same page in the same state do not stack
    additional diagnostics (one per page).
23c. **`transform_no_q_12_12_when_categories_explicitly_false`**
    — listing with `categories: false` (Disabled) → no key,
    no diagnostic, even when items happen to carry
    categories.
24. **`transform_writes_html_to_meta_path`** — happy-path
    listing → `meta.rendered.navigation.margin_categories`
    is a `ConfigValue::String` containing the heading +
    container.
25. **`transform_does_not_consume_resolved_listings`** —
    after the transform runs, `ctx.resolved_listings` is
    still populated (downstream transforms / tests rely on
    it being available).

### Per-item template tests

26. **`item_default_renders_category_chips_when_categories_field_enabled`**
    — listing with `fields: [..., categories]` and an item
    with `["rust", "design"]` → rendered HTML contains two
    `<div class="listing-category"` blocks with the right
    text.
27. **`item_default_omits_category_chips_when_field_disabled`**
    — listing with `fields:` excluding `categories` →
    rendered HTML contains zero `<div class="listing-category"`
    blocks.
28. **`item_grid_renders_category_chips`** — same as #26
    against the grid template.
29. **`item_table_unchanged`** — table template emits no
    category chips (matches v1 limitation).

### Snapshot tests

30. **`builtin_default_with_categories_default_mode`** —
    fixture with three posts, `categories: default` →
    rendered HTML snapshot. Locks the per-item chip layout
    plus the sidebar shape.
31. **`builtin_default_with_categories_cloud_mode`** —
    fixture with three posts, `categories: cloud` → snapshot.
32. **`builtin_default_with_categories_unnumbered_mode`** —
    snapshot.
33. **`page_with_two_listings_aggregates_sidebar`** — fixture
    with two listings on one page, both `categories: default`
    → single sidebar with the union.

### Template-level tests (in `template.rs` test module)

34. **`full_template_emits_margin_sidebar_with_only_toc`** —
    `rendered.navigation.toc` set, `margin_categories` not →
    the sidebar div opens with just the TOC nav.
35. **`full_template_emits_margin_sidebar_with_only_categories`**
    — `margin_categories` set, `toc` not → sidebar opens
    with just the categories block.
36. **`full_template_emits_margin_sidebar_with_both`** — both
    set → sidebar opens with TOC nav first, categories
    second.
37. **`full_template_omits_margin_sidebar_when_neither_set`**
    — preserves the existing no-TOC, no-listing behavior
    (no `<div id="quarto-margin-sidebar">` in output).

### Integration test

38. **`pipeline_renders_listing_with_categories_end_to_end`**
    — fixture project: host page declares
    `listing: { type: default, categories: true }`, three
    posts in `posts/` with overlapping category lists,
    `cargo run --bin q2 -- render` produces an
    `_site/index.html` with:
    - per-item chips inside each rendered post block;
    - the sidebar inside `<div id="quarto-margin-sidebar">`
      with all unique categories grouped + counts;
    - the vendored `<script src="…/quarto-listing.js">`
      reference still present (verifying L5 didn't perturb
      L3's artifact-store wiring).

    **End-to-end CLI verification per CLAUDE.md.**

### End-to-end CLI verification record

**Recorded 2026-05-07** per CLAUDE.md §"End-to-end verification
before declaring success".

**Fixture:** `/tmp/q2-l5-e2e/` — a website project with one host
(`posts/index.qmd`) declaring `listing: { type: default,
categories: true }` and three sibling posts:

- `posts/a.qmd` — categories `[rust, design]`
- `posts/b.qmd` — categories `[rust]`
- `posts/c.qmd` — categories `[elm]`

**Exact invocation:**

```
cargo run --bin q2 -- render /tmp/q2-l5-e2e
```

**Output files produced:**
`_site/posts/{index,a,b,c}.html` (4 files), plus
`_site/site_libs/listing/{list.min.js,quarto-listing.js}`.

**Observed counts** (via `grep -o`) on
`_site/posts/index.html`:

- 4 per-item `<div class="listing-category"` chips
  (a=2, b=1, c=1).
- 4 sidebar `<div class="category"` pills (1 "All" + 3 unique
  categories: design, elm, rust).
- 1 `<div id="quarto-margin-sidebar"` wrapper.
- 1 `quarto-listing.js` script reference.

**Snippet of observed output** (per-item chip block on post `a`):

```html
<div class="listing-categories">
<div class="listing-category" onclick="window.quartoListingCategory('cnVzdA=='); return false;">rust</div>
<div class="listing-category" onclick="window.quartoListingCategory('ZGVzaWdu'); return false;">design</div>
</div>
```

**Snippet of observed sidebar block:**

```html
<h5 class="quarto-listing-category-title">Categories</h5>
<div class="quarto-listing-category category-default">
<div class="category" data-category="">All <span class="quarto-category-count">(3)</span></div>
<div class="category" data-category="ZGVzaWdu">design <span class="quarto-category-count">(1)</span></div>
<div class="category" data-category="ZWxt">elm <span class="quarto-category-count">(1)</span></div>
<div class="category" data-category="cnVzdA==">rust <span class="quarto-category-count">(2)</span></div>
</div>
```

**Sidebar wrapper context** (TOC + categories share the same
`#quarto-margin-sidebar`, TOC first as the plan specifies):

```html
<div id="quarto-content" class="quarto-container ...">
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
<nav id="TOC" role="doc-toc" class="toc-active">
...TOC nav...
</nav>
<h5 class="quarto-listing-category-title">Categories</h5>
...categories pills...
</div>
```

**Output was inspected directly** (not just inferred from passing
tests).

### Hub-client smoke (deferred)

**Deferred 2026-05-07 to `bd-ra5j`.** Same call as L3: a real
browser session against a running hub. The functional surface
to confirm: chips render, sidebar renders, clicks fire
`window.quartoListingCategory(...)`, `list.js` filters items.
Until `bd-57y4` (SCSS bundle) lands, the visuals will look
bare; the smoke is about markup + JS gluing together, not
visual parity.

Original session intent (kept here for the bd-ra5j worker):

After Rust changes are in, before declaring done:

```bash
cd hub-client
npm run build:all
npm run dev
```

Open the dev server in a browser, load a multi-listing
fixture project (or hand-author one), confirm:

- Each post block shows clickable category chips.
- The right margin shows the categories sidebar with the
  expected pills + counts.
- Clicking a chip *or* a sidebar pill filters the listing
  via list.js. (This exercises the L3-vendored
  `quarto-listing.js` against L5's markup.)

If the hub-client smoke is deferred to a later session (as
happened with L3 — see L3 hand-off summary), say so
explicitly and file a follow-up bd in §"Filing reminder".

## Pipeline-builder wiring

Two places to update (matching L3's pattern):

- `crates/quarto-core/src/pipeline.rs` —
  `build_transform_pipeline`. Insert
  `CategoriesSidebarTransform` after `ListingRenderTransform`,
  before `TocRenderTransform`. Native CLI path.
- WASM path: the same `build_transform_pipeline` is reused by
  `build_wasm_html_pipeline` per L3's wiring (the L3 session
  wired both paths via the same pipeline; verify in impl).

The WASM build is sensitive: `base64` is already a workspace
dep (`Cargo.toml:61`) and `quarto-core/Cargo.toml:64` already
imports it. No new transitive deps for L5. **Run
`cargo xtask verify` (full) before declaring done.**

## Risks and mitigations

- **Risk: doctemplate `$else$` syntax doesn't behave as
  expected in the nested `$if$` structure of `template.rs`.**
  *Mitigation:* tests #34–37 cover all four enable/disable
  combinations of (toc × margin_categories). If `$else$` is
  flaky, fall back to two independent `$if$` blocks emitting
  two separate `#quarto-margin-sidebar` containers — Q1's CSS
  is permissive about a single id appearing twice in a
  document and the fallback still renders correctly. (Note:
  HTML technically allows only one element per id; we'd want
  to file a follow-up to combine cleanly when bd-0fd0's
  injection slot lands.)
- **Risk: WASM build pulls `base64` differently than native.**
  *Mitigation:* `base64` already runs in WASM via its
  workspace usage in other crates; verified by current
  `cargo xtask verify` runs. L5 adds no new conditional cfg.
- **Risk: aggregation across listings double-counts an item
  that appears in two listings on the same page.** *Mitigation:*
  Q1 deliberately double-counts (each listing reports its
  view of items); L5 matches this. Test #8 locks the
  behavior.
- **Risk: list.js click handlers don't bind correctly because
  the markup is emitted post-`CodeHighlightStage` but the JS
  expects a fully-rendered DOM.** *Mitigation:* none needed —
  list.js is a normal `<script>` that runs on
  `DOMContentLoaded`; the markup we emit is in the rendered
  HTML by then. Verified via the hub-client smoke and the
  CLI integration test.
- **Risk: Categories appearing twice on a page (once
  per-item, once in sidebar) confuse users about
  click-filter scope.** *Mitigation:* Q1 has the same
  shape; the convention is well-understood by Quarto users.
  The user-facing reference doc (L8 or L11) should briefly
  describe the relationship.
- **Risk: bd-57y4 SCSS lands later and the mixing of TOC and
  categories inside one `#quarto-margin-sidebar` produces an
  unexpected visual layout.** *Mitigation:* Q1's SCSS already
  handles this case; the pattern of "TOC nav above category
  list inside the sidebar" is part of Q1's documented
  behavior. The bd-57y4 session should verify this layout
  visually as part of close-out.
- **Risk: `is_feature_disabled(meta, "listing")` in the
  sidebar transform is too coarse — a user disables
  `listing:` at the project level but a single page declares
  `categories: true` outside a listing context.**
  *Mitigation:* this combination is meaningless (categories
  without items to categorize). The transform's read-from-
  `resolved_listings` design naturally falls through to "no
  resolved listings → no sidebar"; no diagnostic needed.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [x] Re-read
      `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [x] Re-read `.claude/rules/wasm.md` (`?Send`,
      WASM-cfg gating).
- [x] Re-read L3 hand-off summary
      (`claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
      §"Hand-off summary") for the state-of-the-branch
      L5 builds on.
- [x] Confirm L3 has merged onto `feature/listings`. If
      not, **stop and ask the user**. (Confirmed
      2026-05-07: merge commit `b4f2238c`.)
- [x] Create the worktree at
      `.worktrees/bd-5vsr-listings-categories-sidebar/` per
      §"Branch / worktree". Branch
      `beads/bd-5vsr-listings-categories-sidebar`, branched
      off `feature/listings`.
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules so `br`
      uses the main repo's `.beads/`.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests` and record the test count here.
      **Baseline 2026-05-07: 8570 passing / 195 skipped** at
      `feature/listings` HEAD `43256c1a` (the L5-plan
      commit). `cargo xtask verify --skip-hub-build
      --skip-hub-tests` clean.

### Follow-up bd issues — file at start

Two issues to file before impl begins, with
`--deps discovered-from:bd-5vsr`:

- [x] **Localization for category labels.** Title:
      *"Localize listing category sidebar labels (Categories,
      All)"*. Type: task, p3. Description: today's L5 hardcodes
      English; we need a localization plumbing pattern (likely
      similar to whatever crossref settles on — see the comment
      in `crossref_render.rs`). Link this plan.
      **Filed 2026-05-07 as `bd-99ru`.**

- [x] **Encoding-review follow-up.** Title: *"Review category
      click-handler encoding scheme (b64+percent-encoding)"*.
      Type: task, p3. Open questions on whether to drop the
      encode/decode pair entirely or replace with a simpler
      Q2-native idiom (re-encoding the JS side correspondingly).
      **Filed 2026-05-07 as `bd-754f`.**

- [x] **bd-57y4 cross-reference.** No new issue — just confirm
      the existing `bd-57y4` (vendor-and-integrate
      `quarto-listing.scss`) is current and add a comment to
      its description: "L5 (bd-5vsr) lands the markup that
      consumes this SCSS; merging bd-57y4 restores Q1 visual
      parity for category sidebars and per-item category
      chips." This step is `br update`, not `br create`.
      **Done 2026-05-07.**

### TDD phase 1 — `helpers::category_html`

- [x] Write tests #1–4 in `helpers.rs`. Fail.
- [x] Implement `category_html`. Tests pass.
- [x] Workspace-level `cargo nextest run --workspace`
      passes. (8570 → 8578; +8 from helpers.)

### TDD phase 2 — Per-item binding

- [x] Write a binding test verifying `category-html` lands
      on the per-item map (extend the existing
      `binding.rs` test module).
- [x] Add the `category-html` insertion to
      `build_item_map`.
- [x] Tests pass. (8578 → 8580; +2 binding tests.)

### TDD phase 3 — Item templates

- [x] Write template-render tests #26–29.
- [x] Update `item-default.template` and
      `item-grid.template` to splice in the category block
      conditional on `show.categories`.
- [x] Tests pass. (8580 → 8584; +4 template tests.)

### TDD phase 4 — Sidebar aggregation + HTML emission

- [x] Write aggregation tests #5–11 and HTML-emission tests
      #12–19 in `categories_sidebar.rs` (or a sub-module).
      Fail.
- [x] Implement `aggregate_categories` and
      `render_sidebar_html`. Tests pass.
      (8584 → 8600; +16 sidebar tests. Also added
      `Listing.categories_source: SourceInfo` ahead of phase 8
      so the aggregation can carry the right span when phase 8
      wires `Q-12-12`.)

### TDD phase 5 — `CategoriesSidebarTransform`

- [x] Write transform tests #20–25. Fail.
- [x] Implement the transform. Tests pass.
      (Plus #23b empty-but-enabled silent-skip placeholder; the
      `Q-12-12` diagnostic itself wires up in phase 8.)

### TDD phase 6 — Pipeline wiring

- [x] Insert `CategoriesSidebarTransform` into
      `build_transform_pipeline` between `ListingRender`
      and `TocRender`. (`pipeline.rs` line ~810; the WASM
      path reuses the same builder via
      `AstTransformsStage::run`, so both native and WASM
      pick it up.)
- [x] Add `// TODO(bd-0fd0):` marker noting the future
      Lua-injection slot. (8600 → 8607.)

### TDD phase 7 — Template change

- [x] Write template-level tests #34–37. Fail.
- [x] Update `FULL_HTML_TEMPLATE` per §"Template change
      in `template.rs`". Tests pass.
      (8607 → 8611. Note: tests use a new `render_full`
      helper because `render_with_template` selects the
      MINIMAL template by default — `FULL_HTML_TEMPLATE`
      is the right place for the new sidebar logic but
      the convenience entry point doesn't reach it.)

### TDD phase 8 — Diagnostic catalog + SourceInfo plumbing

- [x] Add a `categories_source: SourceInfo` field to
      `Listing` (defaults to `SourceInfo::default()`); update
      `parse_categories_mode` (config.rs:714) to capture the
      `categories:` entry's source-info while parsing.
      (Done in phase 4 prep; the parser captures
      `entry.key_source` so the diagnostic underlines the
      `categories:` key.)
- [x] Add `Q-12-11` and `Q-12-12` entries to
      `crates/quarto-error-reporting/error_catalog.json`.
- [x] Wire the `Q-12-11` mixed-mode diagnostic. (Detected in
      `aggregate_categories`; emitted from the transform when
      `agg.mixed_modes` is true.)
- [x] Wire the `Q-12-12` enabled-but-empty diagnostic in the
      transform's outcome handler. Span on the
      `categories:` key from the first such listing.
- [x] Verify both diagnostics surface in the existing
      diagnostic-catalog tests. (8611 → 8614; 4 new
      diagnostic tests, 1 of which renamed an existing test.)

### TDD phase 9 — Snapshot + integration

- [x] Write snapshot tests #30–33. Added `insta.workspace = true`
      to `quarto-core`'s dev-deps (matches the convention used in
      `pampa`, `quarto-highlight`, etc.) and wrote four
      end-to-end snapshot tests in `tests/listing_pipeline.rs`,
      each driving the full `ProjectPipeline` and snapshotting
      a focused L5-owned slice of the rendered HTML (chip blocks
      + sidebar block, or sidebar block alone for the
      two-listing case). Snapshots live under
      `tests/snapshots/`. The focused-slice approach (rather
      than full HTML) keeps the snapshots small, readable, and
      resilient to unrelated changes.
- [x] Write integration test #38. (`listing_pipeline.rs` —
      `listing_with_categories_renders_chips_and_sidebar_e2e`.
      Drives the full `ProjectPipeline` through a real
      4-file fixture: 1 host with `listing: { categories: true }`
      + 3 posts; asserts chips × 4, sidebar pills × 4 (incl.
      "All"), counts, the `#quarto-margin-sidebar` wrapper, and
      that `quarto-listing.js` is still emitted.)
- [x] Run all together; iterate until green.

**Bug surfaced and fixed during phase 9:** snapshot test #33
(two listings on one page with subdir-relative `contents:`
globs) initially produced empty listings. Root cause: Quarto's
YAML parser tags strings like `posts/*.qmd` as `PandocInlines`
(a `Span` carrying class `yaml-markdown-syntax-error`), but
`parse_contents` (`crates/quarto-core/src/project/listing/config.rs:572`)
matched only `Scalar(Yaml::String)` / `Glob` / `Array`,
silently dropping the explicit contents and letting
`apply_type_defaults` overwrite with the sibling-only `*.qmd`
default. The fix routes `parse_contents` through
`as_plain_text` first (matching `parse_listings`'s
shorthand-string handling), with two new unit tests covering
the `PandocInlines` paths. The broader audit of sibling parser
branches that may share the same vulnerability is filed as
**`bd-nwyp`**.

Test count: 8614 → 8621 (+7 from this phase: 4 snapshot tests +
2 new parser tests + 1 e2e integration test).

### Verification and close-out

- [x] `cargo build --workspace` clean. (Implicit in
      `cargo xtask verify`.)
- [x] `cargo nextest run --workspace` — all pass; record
      test-count delta against the baseline.
      **Baseline 8570 → final 8621 (+51 over the L5 work
      across phases 1, 2, 3, 4, 5, 7, 8, 9, plus the
      bd-nwyp parser-fix unit tests.)**
- [x] `cargo xtask lint` clean. (`693 files checked`.)
- [x] `cargo xtask verify` (full, including hub-client +
      WASM build) — all green. (After moving `base64`
      out of the native-only block in
      `crates/quarto-core/Cargo.toml` — see commit
      message for context.)
- [x] End-to-end CLI verification fixture rendered;
      output inspected; recorded inline above the
      §"End-to-end CLI verification record" stub.
- [x] Hub-client browser smoke deferred to **bd-ra5j**
      (per the L5 plan's "if deferred, file a follow-up"
      clause and the L3 precedent). Note: until bd-57y4
      lands, the visual smoke is partial anyway — the
      functional smoke is the load-bearing part.
- [ ] Stop and request user permission before any push
      (per CLAUDE.md §"GIT PUSH POLICY").
- [ ] After user approval: `br update bd-5vsr --status
      closed`.
- [ ] `br sync --flush-only && git add .beads/ && git
      commit` from the **main repo** (per
      `.claude/rules/worktrees.md` §"Committing beads
      changes").
- [ ] Update the listings epic table
      (`claude-notes/plans/2026-05-05-listings-epic.md`)
      to mark L5 closed with the merge commit hash.

## Filing reminder

This sub-plan corresponds to **one** bd issue:

- `bd-5vsr` — L5, the categories sidebar.

After impl, close with a reason that references the landed
commit. Update the issue description with a one-line link to
this file.

### Follow-up bd issues filed at hand-off

To be filed at start of impl with
`--deps discovered-from:bd-5vsr`:

1. **Localize listing category sidebar labels** — task, p3.
   Today's hardcoded English "Categories" / "All" should
   route through whatever localization pattern Q2 settles on
   (cf. `crossref_render.rs`'s localization comment). Not
   blocking; Q1's defaults are also English.

2. **Review category click-handler encoding scheme** —
   task, p3. L5 ships `b64(percent-encoded UTF-8)` (Q1's
   `b64EncodeUnicode`) on the Rust encoder side and
   inherits the matching `decodeURIComponent(atob(...))`
   decoder on the JS side via the vendored
   `quarto-listing.js`. The scheme works but inherits Q1's
   choice without revisiting it. Open questions for the
   review: (a) is `data-category` better off carrying the
   raw category string instead, with the JS reading it
   directly (no encode/decode pair, simpler in both halves)?
   (b) if we keep an encoded form, should we move to a
   single Rust crate's idiom (e.g. base64-only on UTF-8 +
   matching Rust-style decoder in JS) and drop the
   percent-encoding round-trip? (c) does any reserved
   character in category names interact poorly with HTML
   attribute values, JS string literals, or URL hash
   routing — and would a different scheme avoid those
   edge cases? Out of scope for L5 (we want Q1 parity
   first); file when encoding lands so the discussion
   isn't lost.

### Conditional follow-ups (file during impl if they trigger)

2. **Hub-client smoke deferred** — task, p3 (only if the
   session can't run a real browser smoke). Follows the L3
   precedent.
3. **`$else$` doctemplate syntax** *(conditional)* — only if
   the nested `$if$` / `$else$` template structure proves
   flaky and the fallback (two independent `$if$` blocks)
   produces visible duplicate-id markup that needs cleanup
   when bd-0fd0 lands.
4. **Category-mode mixing edge case** *(conditional)* — only
   if real-world Q1 fixtures show that mixed modes happen
   often enough that "first-wins + warning" is the wrong
   default and we'd rather diagnose harder.
