# Page-navigation: match Quarto 1 behavior in websites

Beads: **bd-bsut**.

Reference fixture: `examples/websites/06-site-metadata` (3 pages, sidebar
listing all 3).

## Background — what we observed

We rendered the same site under Quarto 1 (`q1-site/`) and Quarto 2
(`_site/`) and compared:

| Behavior | Quarto 1 | Quarto 2 today |
|---|---|---|
| Default visibility on minimal website | **No prev/next strip emitted** | Strip emitted on every page |
| Layout when shown | Single row: prev on the left, next on the right (`display:flex; justify-content:space-between`) | Each link on its own row, left-aligned (no flex layout) |
| Arrow decorations | `bi-arrow-left-short` / `bi-arrow-right-short` glyphs render | `<i class="bi …">` is emitted but the **glyphs are blank** because `bootstrap-icons.css` isn't bundled |

## Quarto 1's logic — the things we sorted out

The user explicitly asked us to figure out how Q1 decides which pages
get the prev/next treatment. Two independent gates control it:

**Gate 1 — config opt-in.** In
`external-sources/quarto-cli/src/project/types/website/website-shared.ts:228`,
`pageNavigation` is read with
`websiteConfigBoolean(kSitePageNavigation, false, project.config)`,
i.e. **default `false`** for websites. Books override this in
`book-config.ts:161` (`book[kSitePageNavigation] !== false`), so books
default to **`true`**.

The Quarto documentation site itself opts in explicitly:
`external-sources/quarto-web/_quarto.yml:28: page-navigation: true`.
That's why the user sees prev/next on quarto.org but not on a vanilla
website.

In `website-navigation.ts:319-325`, the gate is
`formatPageNav !== false && (navigation.pageNavigation || formatPageNav === true) && !usesCustomLayout`.
Document-level frontmatter (`page-navigation: true|false`) wins over
the project default in either direction.

**Gate 2 — must be in a sidebar.** Even when enabled,
`nextAndPrevious(href, sidebar)` returns `{}` if no sidebar contains
this page (`website-navigation.ts:1188`). So a sidebar-less page never
gets prev/next, regardless of config.

Q2 already implements **gate 2** correctly (`page_nav_generate.rs:68`
returns early if `navigation.sidebar` is absent). We're missing
**gate 1**.

## Q2's current state — what we found

- `crates/quarto-core/src/transforms/page_nav_generate.rs:60-70` only
  checks `is_feature_disabled(meta, "page-navigation")` (doc-level
  `false`) and skips if the user already populated the path. There is
  **no project-level config gate** and no website-vs-book default
  distinction. Net effect: any page that lives in a sidebar gets a
  prev/next strip, which is the Q2 default we want to change.
- `crates/quarto-navigation/src/render_html.rs:246` already emits
  the same markup Q1 does, including `bi bi-arrow-left-short` and
  `bi bi-arrow-right-short` `<i>` tags.
- `resources/scss/bootstrap/_bootstrap-rules.scss:319` only sets
  `grid-column`/`grid-row` for `.page-navigation` inside
  `.page-columns`. There are **no rules** for the actual flex layout
  of the strip (`display:flex; justify-content:space-between`) or for
  `.nav-page`/`.nav-page .bi`/`.nav-page a`. Q1's rules live at
  `external-sources/quarto-cli/src/resources/projects/website/navigation/quarto-nav.scss:740-768`
  — straightforward to port.
- Q2's rendered HTML contains **no `bootstrap-icons.css` link**;
  `grep -i "bootstrap-icons" /tmp/q2-guides.html` returns nothing.
  Q1's pages link `site_libs/bootstrap/bootstrap-icons.css`. So the
  `<i class="bi …">` tags Q2 emits are silently invisible.

## Goals

1. **Default visibility matches Q1** — minimal `project: type:
   website` projects do *not* render a prev/next strip unless the
   user opts in.
2. **Config knob exists** — `website.page-navigation: true` (or
   top-level `page-navigation: true` per Q1's flexible placement)
   turns the strip on; doc-level frontmatter `page-navigation:
   true|false` overrides per-page in either direction.
3. **When the strip is shown, it lays out like Q1** — single row,
   prev left, next right, with the arrow glyphs visible.

Non-goals for this issue:
- Books — we don't have a `project: type: book` yet. Wire the
  default-true distinction in a hook so it's easy to add later, but
  don't block on it.
- "Hide the strip on lonely pages" beyond what's already implemented
  in `page_nav_generate.rs:88-91`.

## Phase 0 — Pin baselines and write tests first (TDD)

The existing tests in
`crates/quarto-core/tests/page_navigation_pipeline.rs` (Tests 39-44)
already encode the *current* "default-on for any sidebar page"
behavior. Phase 1 will need to update several of those fixtures to
add `page-navigation: true` (since the default flips to off) — those
edits are themselves the regression diff. No literal output snapshot
needed; the pipeline tests are precise enough.

- [x] Write failing pipeline tests in
      `crates/quarto-core/tests/page_navigation_pipeline.rs` for the
      Phase 1 / Phase 2 / Phase 3 contracts:
      - `pipeline_page_nav_default_off_for_websites`: minimal
        website fixture with sidebar, no `page-navigation` setting →
        **no** `<nav class="page-navigation">` in any rendered page.
      - `pipeline_page_nav_top_level_true_enables`: `page-navigation:
        true` at top level → strip present on pages with neighbors.
      - `pipeline_page_nav_website_scope_true_enables`: same but
        under `website:` key.
      - `pipeline_page_nav_doc_overrides_project_default_off`:
        project default off, one page sets `page-navigation: true`
        in frontmatter → that one page gets a strip.
      - `pipeline_page_nav_emits_layout_css`: render a page with
        page-nav enabled; the emitted website CSS contains
        `.page-navigation` and `display:flex` together (regression
        guard against losing the Q1 layout rule). May be in a
        sibling test file if pipeline output doesn't expose CSS.
      - `pipeline_page_nav_links_bootstrap_icons`: rendered page's
        `<head>` contains `<link …bootstrap-icons.css">`.

Status after writing the tests:

- **Red (will go green during Phase 1-3):**
  `pipeline_page_nav_default_off_for_websites`,
  `pipeline_page_nav_doc_overrides_project_default_off`,
  `pipeline_page_nav_emits_layout_css`,
  `pipeline_page_nav_links_bootstrap_icons`.
- **Green now, act as forward-looking regression guards** (they
  pass today because Q2 currently emits the strip unconditionally
  whenever a sidebar exists; they will keep passing only if
  Phase 1's config-reading correctly handles top-level and
  `website.`-scoped placement after the default flips off):
  `pipeline_page_nav_top_level_true_enables`,
  `pipeline_page_nav_website_scope_true_enables`.

Existing tests in this file that will break in Phase 1 and need
`page-navigation: true` added to their fixtures (because they
expect the strip but rely on the current default-on behavior):
`pipeline_page_nav_three_page_website` (Test 39),
`pipeline_page_nav_disabled_at_doc_level` (Test 40, for the index
+ docs assertions),
`pipeline_page_nav_cross_contamination_guard` (Test 43). Their
edits are themselves the regression diff for the default flip.

## Phase 1 — Project config gate (default off)

- [x] **Config placement (decided):** accept both
      `website.page-navigation` and **top-level** `page-navigation`
      in `_quarto.yml`, mirroring Q1's `websiteConfigBoolean`. The
      top-level key plays nicely with Q2's metadata-merging
      behavior, so users can also write `page-navigation: true|false`
      in document frontmatter and have it override the project
      default per page through the normal merge path. Precedence
      (most-specific wins): document frontmatter > `website.…` >
      top-level > built-in default.
- [x] **Implementation (decided to keep the read inline in the
      transform rather than introduce a new pre-resolver stage):**
      added `resolve_website_bool` in `transforms/config.rs`, which
      handles both top-level and `website.`-scoped placements with
      the documented precedence. The transform calls it directly,
      passing the project-type default. No need to surface a new
      `navigation.page_navigation_enabled` boolean since the gate
      computes cheaply from already-merged meta.
- [x] Update `PageNavGenerateTransform` (`page_nav_generate.rs:60`)
      to call `resolve_website_bool(&ast.meta, "page-navigation",
      page_nav_default_for_kind(ctx.project.project_kind()))` and
      early-return on `false`. Document-level frontmatter naturally
      wins because metadata merge places it at the top level.
- [x] **Books hook:** `page_nav_default_for_kind` is a `match` on
      `ProjectKind` with `Book => true` and the others `false`.
      Books aren't reachable in Q2 today; this is a one-line flip
      when they land.
- [x] Confirm `pipeline_page_nav_default_off_for_websites`,
      `pipeline_page_nav_top_level_true_enables`,
      `pipeline_page_nav_website_scope_true_enables`, and
      `pipeline_page_nav_doc_overrides_project_default_off` are
      green; existing pipeline tests 39/40/43 updated to opt in.
- [x] **End-to-end CLI verification:** ran
      `cargo run --manifest-path .../Cargo.toml --bin q2 -- render`
      from `examples/websites/06-site-metadata/`. With no
      `page-navigation` setting, all three rendered pages contain
      zero `page-navigation` markup (matches `q1-site/`). With
      `website.page-navigation: true` added, all three pages render
      the strip with the expected prev/next links.

## Phase 2 — Port Q1's layout SCSS

**How Q1 organizes this** (we sorted this out before deciding where
to put it in Q2):

- Q1 keeps **two distinct buckets** of SCSS:
  - `formats/html/bootstrap/_bootstrap-rules.scss` — applies to
    *every* HTML render that uses the Bootstrap pipeline, websites
    or otherwise. Q1 puts only the **grid placement** of
    `.page-navigation` here (the `.page-columns .page-navigation`
    rule at line 319), because the page-columns grid is a general
    HTML-format feature.
  - `projects/website/navigation/quarto-nav.scss` — applies *only*
    when the website navigation Sass bundle is added to the format
    extras (`websiteNavigationSassBundle()` at
    `website-navigation.ts:1513-1524`). The actual flex layout, the
    `.nav-page .bi` sizing, the link colors — i.e. everything
    "what the page-nav strip looks like" — lives here, at lines
    740-768.
  - One isolated rule lives in `formats/html/_quarto-rules.scss`
    (line 750), inside an `@media print` block, hiding `.nav-page`
    when printing. Print-hide is a format-wide rule, not a
    website-only one, so it's correctly outside the website bundle.

  Net effect: a non-website HTML render that somehow had
  `.page-navigation` markup would still grid-place it correctly via
  `_bootstrap-rules.scss`, but it wouldn't get the flex layout —
  because the flex rules only ship when the website nav bundle
  ships.

- **Q2 mirror (revised after looking at the existing Q2 layout):**
  Q2 currently puts *all* website-scoped CSS — sidebar styles
  (`.sidebar.sidebar-navigation`, `.sidebar.toc-left`,
  `#quarto-margin-sidebar`, etc.) — directly in
  `resources/scss/bootstrap/_bootstrap-rules.scss`. There is no
  separate website Sass bundle yet; the framework + quarto layers
  ship together for every render and rely on selectors being
  scoped enough that they only kick in when the corresponding
  markup is present (e.g. `.sidebar` only exists on website
  pages).
  
  Following that established pattern, the page-nav layout rules go
  into `_bootstrap-rules.scss` next to the existing
  `.page-columns .page-navigation` grid placement at line 319.
  Introducing a website-only Sass bundle is a larger refactor that
  nothing else in Q2 needs yet — defer until there's a second
  website-only feature with stronger scoping needs. Add a TODO
  comment near the rules so the future refactor is easy to find.

- [x] Add Q1's lines 740-768 to
      `resources/scss/bootstrap/_bootstrap-rules.scss` near the
      existing `.page-columns .page-navigation` rule (line 319):

      ```scss
      .page-navigation { display: flex; justify-content: space-between; }
      .nav-page { padding-bottom: 0.75em; }
      .nav-page .bi { font-size: 1.8rem; vertical-align: middle; }
      .nav-page .nav-page-text { padding-left: 0.25em; padding-right: 0.25em; }
      .nav-page a { color: $text-muted; text-decoration: none; display: flex; align-items: center; }
      .nav-page a:hover { color: $link-hover-color; }
      ```
- [x] Add `@media print { .nav-page { display: none; } }` to the
      same SCSS file. (Q1 has it inside a generic `@media print`
      block; we keep ours adjacent to the page-nav rules so the
      group is self-contained.)
- [x] Drop a TODO marker noting that when a second website-only
      feature appears, this group of rules becomes the seed of a
      future website Sass bundle. (Done; see comment in
      `_bootstrap-rules.scss` near the new rules.)
- [x] Confirmed: `pipeline_page_nav_emits_layout_css` is green.
- [x] End-to-end: rendered `06-site-metadata` with
      `website.page-navigation: true`. The compiled
      `_site/site_libs/quarto/quarto-theme-c8344243879f4b5e.css`
      contains the `.page-navigation{display:flex;…}` rule and the
      `.nav-page a:hover{color…}` rule (both grep-confirmed). Visual
      browser inspection still pending until Phase 3 lands the
      Bootstrap Icons font (without it the strip will lay out
      single-row but the arrow glyphs will be blank).

## Phase 3 — Bundle Bootstrap Icons

**Decision:** vendor the full `bootstrap-icons` package, mirroring
Q1.

- [x] Vendored under `resources/bootstrap-icons/` per the
      "External Sources Policy" in CLAUDE.md. Q1 ships only the
      `bootstrap-icons.css` (~99 KB) + `bootstrap-icons.woff` (~180
      KB) pair; we mirror that exactly. README.md alongside records
      provenance and licensing (matches the existing
      `resources/scss/README.md` convention).
- [x] Created `WebsiteBootstrapIconsTransform`
      (`crates/quarto-core/src/transforms/website_bootstrap_icons.rs`)
      and registered it in the pipeline alongside
      `WebsiteFaviconTransform`. The transform stores two
      Project-scope artifacts under `css:bootstrap-icons:…` and
      `font:bootstrap-icons:…` with on-disk paths
      `bootstrap/bootstrap-icons.{css,woff}`. The `<link
      rel="stylesheet">` is emitted **automatically** by
      `apply_template`, which iterates every `css:*` artifact and
      asks the per-page resolver for a URL — that's why the
      transform itself does *not* touch `header-includes` (an
      explicit injection there would create a duplicate `<link>`,
      a regression I caught and fixed during this phase).
- [x] **Existing tests updated:** four `artifact_scoping_pipeline`
      tests assumed the *first* `<link rel="stylesheet">` was the
      theme. With `bootstrap-icons.css` sorting before
      `quarto-theme-...` in `css:*` artifact order, that's no
      longer true. Added `extract_theme_stylesheet_href` which
      filters by `/quarto/quarto-theme-` substring and updated the
      tests. Also re-captured the Phase-5 single-doc baseline hash
      for `doc_files/styles.css` (Phase 2's SCSS additions changed
      the byte content); doc.html hash unchanged (no body markup
      affected).
- [x] Confirmed `pipeline_page_nav_links_bootstrap_icons` is green.
- [x] **End-to-end:** re-rendered `06-site-metadata` with
      `website.page-navigation: true`, served via the existing
      `127.0.0.1:8000` server, opened `_site/guides.html` in a real
      browser. Visual check (full-page screenshot taken, `/tmp/
      q2-page-nav-after.png`): the prev/next strip lays out
      single-row, "Home ←" left-aligned, "→ API Reference"
      right-aligned, both glyphs visible (Bootstrap Icons font
      loaded successfully), links muted-grey per Q1's styling.
      File system: `_site/site_libs/bootstrap/{bootstrap-icons.css,
      bootstrap-icons.woff}` both present. Head links: a single
      `<link rel="stylesheet" href="site_libs/bootstrap/bootstrap-icons.css">`
      on the root page; nested page (`docs/api.html`, tested via
      a temporary `docs/api.qmd` copy) gets
      `../site_libs/bootstrap/bootstrap-icons.css` correctly.

## Phase 4 — Docs

- [x] Documented `website.page-navigation` (and the top-level
      placement) in `docs/navigation.qmd`. The page is now titled
      "Navbars, Page Footers, and Prev/Next Navigation" and gains a
      "Page navigation (prev/next)" section between "Page footer"
      and the existing "Navigation items" reference — covering the
      enable/disable rules, per-document overrides, the
      flatten-and-pick-neighbor algorithm (cross-referencing
      bd-nf50's flatten / dedupe / separator rules), and guidance on
      when to leave the strip off.
- [x] Updated example READMEs and `_quarto.yml` files where they
      relied on the old default-on:
      - `01-minimal/README.md` "Things you may notice" — flipped
        from "the strip appears automatically" to "the strip is off
        by default; opt in with `page-navigation: true`."
      - `02-auto-sidebar/_quarto.yml` — added
        `website.page-navigation: true` so the README's prev/next
        narrative still renders. Re-rendered: 4 of 5 posts get the
        strip; `work-in-progress.html` (a draft excluded from the
        sidebar) correctly does not.
      - `03-nested-sidebar/_quarto.yml` — added
        `website.page-navigation: true`. README now points at
        `docs/navigation.qmd` for the full precedence rules.
        Re-rendered: `installation.html` strip count = 2,
        `tuning.html` count = 0 (doc-level
        `page-navigation: false` correctly overrides the project
        opt-in).
- [x] `06-site-metadata` was the working fixture used during
      development; left as-is so it still illustrates the
      *default-off* behavior (the user can opt in interactively to
      see the strip appear).

## Resolved decisions

1. **Top-level `page-navigation:` in addition to `website.page-navigation`** —
   yes, accept both. Top-level placement also makes per-page
   document-frontmatter overrides ride the normal Q2 metadata-merge
   path.
2. **SCSS placement** — port Q1's lines 740-768 into a new
   website-only partial (`resources/scss/website/quarto-nav.scss`)
   wired into a website Sass bundle. Q2's existing
   `_bootstrap-rules.scss` keeps only the format-wide grid rule. See
   Phase 2 for the rationale (Q1 splits the same way).
3. **Icon bundling** — vendor full `bootstrap-icons` package,
   mirroring Q1.
4. **Books default-true hook** — keep the project-type-keyed
   default in place even though `Book` is currently unreachable in
   Q2; books are next up after non-HTML outputs.

## Follow-up: sidebar-link styling parity (in-session add-on)

After Phase 2 landed and the page-navigation strip looked right,
visual comparison between Q1's `q1-site/` and Q2's `_site/` showed
that Q2's **sidebar links** were still rendering with default
Bootstrap link styling — blue underlined text — while Q1's sidebar
links use muted `$sidebar-fg` color, no underline, and a hover/active
treatment driven by the `$sidebar-hl` family of variables.

The user asked us to extend the SCSS port to cover sidebar links too,
applying the same "all rules in `_bootstrap-rules.scss` for now"
approach. Diff captured by inspecting computed CSS via
`evaluate_script` in DevTools (rule `.sidebar-navigation li a`
strips the underline; `.sidebar-navigation a { color: inherit }`
makes anchors inherit container color; `.sidebar-title > a` covers
the site title).

- [x] Ported the following Q1 rules from `quarto-nav.scss` into
      `resources/scss/bootstrap/_bootstrap-rules.scss`, alongside
      the page-nav block:
      - `.sidebar-logo-link { text-decoration: none }` (line 315 in Q1)
      - `.sidebar-navigation a { color: inherit }` (277)
      - `.sidebar-navigation li a { text-decoration: none }` (319)
      - `.sidebar-title > a { font-size: inherit; text-decoration: none }` (289)
      - `.sidebar-item`, `.sidebar-section`,
        `.sidebar-item .sidebar-item-container` (display:flex
        justify-content:space-between cursor:pointer),
        `.sidebar-item-text { width: 100% }` (348-410)
      - `.sidebar.sidebar-navigation > * { padding-top: 1em }` (352)
      - The `$sidebar-hl` derivation +
        `$sidebar-color`/`$sidebar-hover-color`/`$sidebar-active-color`/
        `$sidebar-disabled-color` defaults (464-472)
      - `div.sidebar-item-container { color/hover/disabled/active }` (525-541)

      The four `$sidebar-*-color` variables flow naturally from
      `$sidebar-fg`/`$sidebar-bg` already declared in
      `_bootstrap-variables.scss`, so theme overrides Just Work.
- [x] Re-captured `doc_files/styles.css` hash in the Phase-5
      single-doc baseline (`tests/fixtures/phase5-single-doc-baseline/
      expected_hashes.txt`); doc.html unchanged.
- [x] **End-to-end:** browser screenshot at
      `/tmp/q2-sidebar-after2.png` matches Q1's `/tmp/q1-sidebar.png`
      visually: site title in dark text without underline, sidebar
      links muted-gray with no underline, active page in slightly
      darker shade.
- [x] `cargo xtask verify --skip-hub-build` passes (8125 tests).

## Definition of done

- [x] All Phase-0 tests are green.
- [x] `cargo xtask verify --skip-hub-build` passes (8125 tests +
      lint + trace-viewer).
- [x] `examples/websites/06-site-metadata` rendered with
      `page-navigation: true` in `_quarto.yml`, viewed in a real
      browser. Visual check (full-page screenshot at
      `/tmp/q2-page-nav-after.png`): prev/next on a single row,
      "Home ←" left-aligned and "→ API Reference" right-aligned,
      arrow glyphs visible, links muted-grey per Q1 styling.
- [x] The same example without the opt-in renders zero
      `page-navigation` markup (`grep -c page-navigation
      _site/*.html` → 0 on every page) — Q1-parity confirmed.
- [x] `bd-bsut` closed; `bd-nf50` closed (the new
      docs/navigation.qmd subsection covers all four flatten/dedupe/
      separator/section-header rules it called for).
