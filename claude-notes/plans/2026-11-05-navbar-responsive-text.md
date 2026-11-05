# Navbar: breakpoint-responsive item text

**Strand:** bd-navbar-responsive-text-am98rw95
**Status:** implementing (go-ahead 2026-11-05).

## Goal

Let a navbar entry show different text at different viewport widths. Wide
screens get the full label ("Documentation"), and narrow-but-still-expanded
navbars get a short one ("Docs"), so entries don't line-wrap. The author
writes the variants in YAML. The render pipeline sees the variants and emits
DOM whose visibility depends on Bootstrap's breakpoints. No JS is needed.

## How navbars work today (survey)

| Concern | Location | Notes |
|---|---|---|
| Item model | `crates/quarto-navigation/src/item.rs` | `NavigationItem { href, text: Option<ConfigValue>, icon, aria_label, rel, target, menu, active, … }`. Navbar, sidebar, and page-footer all use it. |
| Item parse / serialize | `item.rs` `from_config_value_with` / `to_config_value` | `to_config_value` also carries the Generate→Render handoff through `navigation.navbar`, so whatever we add must round-trip. |
| Navbar model | `crates/quarto-navigation/src/navbar.rs` | `Navbar { left, right, collapse, collapse_below: CollapseBelow, … }`. `CollapseBelow` is already a `sm/md/lg/xl/xxl` enum. |
| Markdown in `text:` | `crates/quarto-core/src/transforms/config_markdown.rs:133-136` | Path patterns `[website,]navbar,{left,right},**,text` parse scalar strings into `PandocInlines`. A map-valued `text:` would not match these patterns. |
| Generate | `transforms/navbar_generate.rs` | Resolves, enriches (fills `text` from the page title when absent, `navigation_enrich.rs:51`), marks active, writes `navigation.navbar`. |
| Render | `transforms/navbar_render.rs` → `render_html::navbar_to_html` | Rewrites hrefs and Link targets inside `text`, then emits an HTML string to `rendered.navigation.navbar`. |
| Item HTML | `render_html.rs:866-946` | `render_navbar_item`, `render_dropdown_item`, `render_item_label` → `<i class="bi bi-…"></i> TEXT`. **Unlike Q1, q2 does not wrap navbar text in `<span class="menu-text">`.** Q1's `navitem.ejs` does. No CSS or JS in either tree selects `.menu-text` on navbar items. |
| Preview | `ts-packages/preview-renderer/src/q2-preview/chromeSlots.tsx:96-105` | `NavbarSlot` injects the same HTML string, so render and preview stay in sync automatically. |
| Other `item.text` readers | `project/search/capture.rs:168` (navbar crumb in search records), `navigation_enrich.rs`, `navigation_href.rs:675` (`rewrite_item_text`) | All treat `text` as one value. |
| Schema | — | q2 has no live YAML-schema validator. The hand-written parsers are the schema, and `crates/pampa/test-fixtures/schemas/definitions.yml` is only a test fixture. |

### Breakpoints

Bootstrap 5 `$grid-breakpoints` (`resources/scss/bootstrap/dist/scss/_variables.scss:492`):

| name | min-width |
|---|---|
| xs | 0 |
| sm | 576px |
| md | 768px |
| lg | 992px (default `collapse-below`) |
| xl | 1200px |
| xxl | 1400px |

Bootstrap's utility API compiles `d-{bp}-{none,inline,…}` for every
breakpoint. The rules are mobile-first (`min-width`) and use `!important`.
They are emitted one breakpoint block at a time in ascending order, so for
the same property a larger breakpoint's rule wins.

There is precedent for emitting these utilities from Rust. `render_html.rs`
already emits `d-none d-lg-block` on title breadcrumbs and `d-sm-block
d-md-none` on repo actions. The navbar itself already requires Bootstrap
(`navbar-expand-*`, collapse JS). Under `theme: none` the navbar is unstyled
anyway, so depending on the utilities costs nothing new.

Because `navbar-expand-*` and `d-*` both compile from the same
`$grid-breakpoints` map, a user who overrides the map in SCSS keeps the two
consistent. (Custom breakpoint *names* are out of scope; see decision 4.)

## Proposed design

### YAML surface

`text:` keeps accepting a scalar. It now also accepts a **map keyed by
breakpoint name**, with mobile-first semantics that match Bootstrap: each
variant applies from its breakpoint up to the next specified one.

```yaml
website:
  navbar:
    collapse-below: lg
    left:
      - href: docs/index.qmd
        text:
          xs: Documentation     # collapsed hamburger menu (< lg): room to spare
          lg: Docs              # expanded but cramped (lg–xl)
          xl: Documentation     # wide
      - href: reference/index.qmd
        text:
          lg: API
          xxl: API Reference    # lowest key (lg) also covers < lg
    right:
      - icon: github
        href: https://github.com/…
        text:
          xs: GitHub
          lg: ""                # icon-only while cramped
          xxl: GitHub
```

Rules:

1. Valid keys are `xs sm md lg xl xxl`. Any other key is ignored and the
   parser records a warning (see diagnostics below).
2. The **lowest specified key extends down to 0**, so no width is left with
   missing text. Writing `xs` explicitly is allowed but not required.
3. A variant may be `""`. That renders no text at that range, which is the
   icon-only idiom.
4. Each value is markdown, like scalar `text:` today.
5. Map-valued text is honoured on navbar `left` / `right` items and dropdown
   toggles. Inside dropdown `menu:` children it parses and renders the same
   way, because there is no reason to special-case it. Sidebar and footer
   items use the canonical text only (decision 2).

Semantics are **viewport-only**. The collapsed hamburger menu is simply
"below `collapse-below`", and the author picks its text with the `xs` key, as
in the first example. This is predictable and matches what Bootstrap
authors already know. A `collapsed:` pseudo-key is deferred (decision 1).

### Data model (`quarto-navigation`)

- New `Breakpoint` enum (`Xs, Sm, Md, Lg, Xl, Xxl`) with
  `as_str`/`from_str`, in a new `breakpoint.rs`. `CollapseBelow` stays
  as-is for now; unifying the two is optional cleanup.
- `NavigationItem` gains
  `text_variants: Vec<(Breakpoint, ConfigValue)>`, which is empty for scalar
  text and sorted ascending with no duplicates otherwise.
- `text` stays `Option<ConfigValue>` and holds the **canonical** text: the
  highest-breakpoint non-empty variant, which is the fullest description.
  The existing readers (enrich skip check, search-record breadcrumb, any
  future aria/title fallback) keep working without changes. Enrich sees
  `text.is_some()` and does not overwrite it.
- Parse: if `text` is a map, fill `text_variants` and derive `text`. If
  every variant is empty, `text = None`.
- Serialize: if `text_variants` is non-empty, emit `text:` as the breakpoint
  map. Otherwise emit the scalar as today. The authored shape is what
  travels over the wire, and `text` is always re-derived on parse, so the
  two cannot drift.

I rejected two alternatives:

- *A separate sibling key* (`text-short:` / `responsive-text:`). It adds a
  second spelling for one concept, and it doesn't generalize past two
  sizes.
- *Changing `text` to an enum* (`Single | Responsive`). Every consumer of
  `text` would have to change, which is a wide edit for no benefit, since
  the canonical-text field already gives those consumers what they need.

### Pipeline touch points

1. `config_markdown.rs`: add the patterns
   `[website,]navbar,{left,right},**,text,*` so each map variant is parsed
   as markdown. Check that `**` + `text` + `*` cannot match anything
   unintended. A scalar `text` has no children; a map whose keys are not
   breakpoints is harmless because the parser ignores it.
2. `navigation_href.rs::rewrite_item_text`: also rewrite Link/Image targets
   inside each `text_variants` value.
3. `render_html.rs`: when `text_variants` is non-empty,
   `render_item_label` emits one span per variant, using classes computed
   from the variant's interval `[a, b)`:

   | interval | classes |
   |---|---|
   | `[xs, b)` | `menu-text d-{b}-none` |
   | `[a, b)`, a > xs | `menu-text d-none d-{a}-inline d-{b}-none` |
   | `[a, ∞)`, a > xs | `menu-text d-none d-{a}-inline` |

   Empty variants emit no span, because the other spans' classes already
   hide them at that range. The icon `<i>` stays outside the spans and is
   always visible. Example:

   ```html
   <a href="docs/index.html" class="nav-link" aria-label="Documentation">
     <span class="menu-text d-lg-none">Documentation</span><span
       class="menu-text d-none d-lg-inline d-xl-none">Docs</span><span
       class="menu-text d-none d-xl-inline">Documentation</span></a>
   ```

   The dropdown toggle (the `menu` branch of `render_navbar_item`) currently
   inlines `render_text`. It switches to the same helper.
4. **Accessibility.** `display:none` removes hidden spans from the
   accessibility tree, so screen readers read only the visible variant. If
   any variant is empty (an icon-only range) and the author gave no
   `aria-label`, emit `aria-label="<canonical text as plain text>"` so the
   link always has an accessible name. When every variant is non-empty, no
   `aria-label` is added, and the visible text serves as the accessible
   name.
5. Preview: nothing to do, because it consumes the same HTML string.
   Verify once in `q2 preview` anyway.

### Diagnostics

The parsers don't take a diagnostics sink today. Options:

- (a) Have `NavbarGenerateTransform` validate the raw
  `navigation.navbar` items before resolving them, and push a new
  `Q-13-x` warning (in the navigation error-catalog range) for unknown
  breakpoint keys and for a `text:` map with no valid keys. The warning
  points at the key's `SourceInfo`.
- (b) Skip diagnostics in v1.

I recommend (a). A typo'd key like `md2` would otherwise drop text
silently, which is hard to debug.

### Tests

- `item.rs` unit tests: map parse (sorted/dedup), canonical derivation
  (highest non-empty), all-empty → `None`, round-trip preserves variants,
  unknown keys ignored.
- `render_html.rs` unit tests: class computation for each interval shape,
  empty variant omitted, `aria-label` added only when an empty range exists
  and none was authored, dropdown toggle path, scalar path byte-identical
  to today.
- `config_markdown.rs`: a map variant becomes inlines, including inside a
  nested `menu:`.
- Integration (`crates/quarto-core/tests/integration/navbar_footer_pipeline.rs`):
  a website whose navbar has a responsive item renders the expected spans,
  and a `.qmd` link inside a variant is rewritten.
- Smoke-all fixture under `crates/quarto/tests/smoke-all/navigation/` for the
  end-to-end HTML.
- Manual: render a site and resize across sm/md/lg/xl/xxl in a browser,
  in both render and `q2 preview`.

### Docs

Add a section to the navbar user docs (`old-docs/navigation.qmd`, or
wherever navbar docs live now) that includes the "collapsed menu gets full
text" idiom from the first YAML example.

## Decisions (review round 1, 2026-11-05)

1. **No `collapsed:` pseudo-key in v1.** Ship viewport-only semantics,
   see how people actually use it, and add `collapsed:` later only if the
   `xs`/`lg`/`xl` idiom proves clunky.
2. **Navbar only.** This is an experiment. Sidebar and footer items parse
   map-valued text but render only the canonical text. The docs must say
   this.
3. **`menu-text` span parity with Q1 is a separate strand:**
   bd-navbar-menu-text-span-lil1bdes. In v1, only responsive items get
   `menu-text` spans. Scalar items keep emitting bare text, so their output
   is byte-identical to today.
4. **Custom breakpoint names are out of scope.** Authors who need fully
   custom responsive behaviour should replace the whole navbar with a
   custom component (e.g. the React-component route), not extend this
   feature.
5. **Keys are bare breakpoint names** (`xs sm md lg xl xxl`) directly under
   `text:`.
6. **Diagnostics: option (a)**, a warning for unknown breakpoint keys and
   for a map with no valid keys. Confirmed in review round 2.

## Implementation phases (after go-ahead)

1. [x] `Breakpoint` enum (`breakpoint.rs`, with `range_classes`),
       `text_variants` field, parse/serialize/canonical derivation,
       `refresh_canonical_text`, plus unit tests.
2. [x] Markdown parsing for map variants, and `rewrite_item_text` for
       variants, plus tests.
3. [x] Renderer span/class emission, accessibility fallback, dropdown
       toggle, plus unit tests. The footer keeps `render_item_label`
       (canonical text only).
4. [x] `Q-13-18` warning from `NavbarGenerateTransform`, catalog entry,
       `docs/errors/navigation/Q-13-18.qmd`.
5. [x] Integration test
       `navbar_footer_pipeline::pipeline_navbar_responsive_text_renders_ranges_per_page`.
       Manual browser check of the rendered site at 500, 800, 1000, 1250, and 1450px: every range shows the intended variant (Chrome). Not checked in `q2 preview`, which injects the same HTML string.
6. [x] Docs: `docs/guides/projects/navbar-responsive-text.qmd`.
7. [ ] `cargo xtask verify`.

## Implementation notes (deviations from the design above)

- **Markdown paths.** A `*` segment in `config_markdown.rs` matches only
  array elements, not map entries. Rather than widening `*` for every
  existing pattern, a separate `RESPONSIVE_TEXT_PATHS` list (navbar
  only) is expanded with each breakpoint name as a literal final
  segment. A side effect is that non-breakpoint keys are never parsed
  as markdown.
- **Link rewriting.** When variants exist, `rewrite_item_text` rewrites
  only the variants and then re-derives `text` from them. This way each
  target is resolved, and any `Q-13-2` reported, exactly once, including
  targets that appear only in a narrower variant.
- **Run merging.** Adjacent variants that render to identical HTML
  merge into one range. If a single range remains, the item renders
  byte-identically to scalar text, with no span.
- **Blank detection** is by value kind (null, whitespace string, empty
  inlines or blocks), not by plain text, so an image-only variant is
  not blank.
- **Smoke-all fixture skipped.** The integration test covers the same
  end-to-end HTML.
