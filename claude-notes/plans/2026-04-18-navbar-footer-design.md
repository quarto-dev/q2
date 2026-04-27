# Top-Level Navbars and Page Footers for HTML Documents

Beads: `bd-imiw`

## Overview

Quarto 1 exposes navbars and page footers only as project-level (website/book) features, configured under a `website:` key in `_quarto.yml` and rendered through EJS partials + DOM post-processing. Quarto 2 has already generalized the Table of Contents: TOCs are controlled by YAML metadata (`toc`, `toc-depth`, `toc-title`), represented as structured data on `ast.meta.navigation.toc`, rendered to HTML by a dedicated transform, and injected into the template via `$rendered.navigation.toc$`.

This plan extends that same mechanism to **top-level navbars** and **page footers** so a single HTML document (not just a project) can declare them via YAML, in a way that is familiar to existing Quarto 1 users but fits cleanly into Quarto 2's render pipeline.

**Scope: single documents only.** Books and websites will reuse the same generate stages via a future dedicated session; see §"Scope decisions and future-session concerns" below. This plan does *not* touch project-type logic.

## Reference: how TOC works today in Quarto 2

| Stage | File | Role |
|-------|------|------|
| Generate | `crates/quarto-core/src/transforms/toc_generate.rs` | Reads `toc` / `toc-depth` / `toc-title` from `ast.meta`; calls `pampa::toc::generate_toc`; stores at `navigation.toc`. Skips if `navigation.toc` already exists. |
| Data types | `crates/pampa/src/toc.rs` | `TocConfig`, `TocEntry`, `NavigationToc` + `to_config_value` / `from_config_value`. |
| Render (HTML) | `crates/quarto-core/src/transforms/toc_render.rs` | Reads `navigation.toc`, emits HTML, stores at `rendered.navigation.toc`. Skips if pre-rendered. |
| Template wiring | `crates/quarto-core/src/template.rs:163-173` | `$if(rendered.navigation.toc)$ … $rendered.navigation.toc$ … $endif$`. |
| Pipeline | `crates/quarto-core/src/pipeline.rs` stage 7 (`AstTransformsStage`) | After user pre-filters, before HTML body rendering. |

Two conventions worth preserving:

1. **Two-stage split.** Generate builds structured data; Render turns it into HTML. Users can override at either level.
2. **Structured storage under `navigation.*`.** Navbar and footer sit alongside: `navigation.navbar`, `navigation.footer`.

## Reference: how Quarto 1 does navbars and footers

See `external-sources/quarto-cli/src/resources/schema/definitions.yml`:

- `navbar` (L992-1068): object with `title`, `logo`, `background`, `foreground`, `search`, `pinned`, `collapse`, `collapse-below`, `left`, `right`, `tools`, `toggle-position`, `tools-collapse`. Or boolean.
- `page-footer` (L658-681): object with `left`, `center`, `right`, `border`, `background`, `foreground`. Or string.
- `navigation-item` (L107-165): path or object with `href`, `text`, `icon`, `aria-label`, `rel`, `target`, `menu`.
- `page-footer-region` (L523-527): string or array of `navigation-item`.

Quarto 1 flow: YAML → `src/project/types/website/website-shared.ts` → EJS templates → DOM post-processing. Project-only.

## Key Quarto 2 infrastructure this design leans on

These were investigated during design and are load-bearing for the decisions below:

### 1. Markdown-in-metadata is a built-in feature

`crates/quarto-pandoc-types/src/config_value.rs:124-134` defines `InterpretationContext`:
- `DocumentMetadata` (default): strings are **parsed as markdown** → `ConfigValueKind::PandocInlines(..)` / `PandocBlocks(..)`.
- `ProjectConfig` (for `_quarto.yml`): strings are **kept literal**.

Per-value overrides via YAML tags (`crates/quarto-config/src/tag.rs:177-181`):
- `!md` — force markdown interpretation
- `!str` — force literal string

Consequence for us: **footer text written in a document's frontmatter gets markdown parsing for free** (bold, links, emphasis). No work required at our layer. In `_quarto.yml`, users write `left: !md "Copyright 2026, **Acme** Co."` to get the same effect. Our navbar/footer types should therefore accept `ConfigValue` (which can be a `Scalar`, `PandocInlines`, or `PandocBlocks`) for text fields, and the Render stage must emit inline HTML by walking whatever shape comes in.

### 2. Merge control via `!prefer` and `!concat`

`crates/quarto-pandoc-types/src/config_value.rs:62-85` defines `MergeOp`:
- `Prefer` (from `!prefer`): override / reset previous value. For arrays: clears previous items. For maps: replaces whole map.
- `Concat` (from `!concat`, default for arrays/maps): append to / deep-merge previous value.

These tags are composable with interpretation tags: `!prefer_md`, `!concat_path`, etc. (`tag.rs:52`).

Consequence for us (answering question 9): we do not need to invent per-field merge policy for navbars/footers. The general metadata-merge infrastructure already lets users control exactly what they want:

```yaml
# _quarto.yml:
navbar:
  left: [index.qmd, about.qmd]

# doc.qmd frontmatter — replace left entirely:
navbar:
  left: !prefer [special.qmd]

# doc.qmd frontmatter — append to left:
navbar:
  left: !concat [extra.qmd]
```

We should add **integration tests** that explicitly exercise these tags against navbar/footer config to document the behavior and prevent regressions.

### 3. `false` as an affirmative disable

Currently (`crates/quarto-core/src/transforms/toc_generate.rs:80-88`) the TOC generator only checks for `toc: true` / `toc: "auto"`:

```rust
let should_generate = match ast.meta.get("toc") {
    Some(v) if v.as_bool() == Some(true) => true,
    Some(v) if v.as_str() == Some("auto") => true,
    _ => false,
};
```

**This is a gap**: `toc: false` in a document's frontmatter is indistinguishable from "no `toc:` key at all". If `_quarto.yml` sets `toc: true`, a document cannot opt out with `toc: false`. To suppress inherited TOC today, the user has to use `toc: !prefer false`, which is confusing.

We want a **uniform rule**: for `toc`, `navbar`, and `page-footer`, `false` (after metadata merge) always means "do not emit this element, regardless of what else is in the merged metadata." This is the natural user expectation and worth adopting across all three.

**Follow-up issue filed as part of this plan** (see Phase 0): fix `toc: false` semantics so the new navbar/footer transforms and TOC share identical logic. The fix for TOC is one line in the `match`. We should land it in this session so all three are uniform.

### 4. No search client exists in Quarto 2 today

Quarto 2 has no search subsystem yet. (For reference, Quarto 1 supports search via Fuse.js for local indexes and Algolia for hosted indexes — not Lunr, despite a stray claim in an earlier draft of this plan.) The navbar plan emits a placeholder `<div class="quarto-search">` and nothing else; actual search wiring is a separate future issue.

## Proposed YAML schema

### Design principles

- **Familiarity first.** Keep the Quarto 1 keys (`navbar.left/right`, `page-footer.left/center/right`, `navigation-item` shape) so migration feels natural.
- **Top-level keys, not under `website:`.** Decision from q1: `website:` implies this thing is a website, which is wrong for a standalone document. Config at `_quarto.yml`, directory `_metadata.yml`, or document frontmatter all use the same `navbar:` / `page-footer:` keys; metadata merge is the unifier. Future projects/websites can accept the same keys under whichever nesting makes sense, but this session's implementation only looks at the top-level keys.
- **Two names, one mental model.** User-facing input: `navbar` / `page-footer`. Structured storage: `navigation.navbar` / `navigation.footer`. Rendered HTML: `rendered.navigation.navbar` / `rendered.navigation.footer`.
- **`false` means off, uniformly.** `toc: false`, `navbar: false`, `page-footer: false` all suppress the element. This is the only special shortcut we ship in v1.

### Navbar example

```yaml
navbar:
  title: "My Site"                 # string | false (false = suppress title)
  # logo: images/logo.png          # path | false
  # logo-alt: "Logo"
  # logo-href: /
  background: primary              # bootstrap-named or hex
  # foreground: light
  search: true                     # default false; emits placeholder div (no backend yet)
  pinned: false
  collapse: true
  collapse-below: lg               # sm|md|lg|xl|xxl
  toggle-position: left            # left|right
  left:
    - text: Home
      href: index.qmd
    - text: Docs
      menu:
        - text: Getting Started
          href: start.qmd
        - text: Reference
          href: reference.qmd
    - about.qmd                    # shorthand: path → { href: about.qmd }
  right:
    - icon: github
      href: https://github.com/…
      aria-label: GitHub
```

Only shortcut: `navbar: false` suppresses the navbar. `navbar: true` and `navbar: "string"` are **not** supported in v1 (per q2/q3).

### Page footer example

```yaml
page-footer:
  left: "Copyright 2026, **Acme** Co."   # markdown (in document frontmatter); string (in project config, use !md to opt in)
  center:
    - text: Privacy
      href: /privacy.html
    - text: Terms
      href: /terms.html
  right:
    - icon: github
      href: https://github.com/…
  border: true                           # true | false | "#888"
  background: light
  foreground: dark
```

Short form (matches Quarto 1): `page-footer: "Copyright 2026, Acme Co."` — centered single-string form, markdown-parsed in document context.

`page-footer: false` — suppress footer.

### navigation-item shape (shared)

```
href | file               # required (one; `file` is alias)
text                      # optional; defaults to target's title if resolvable
icon                      # Bootstrap icon name
aria-label
rel
target
menu: [ navigation-item ] # nested; only valid for items with no href
```

Bare-string sugar: `about.qmd` → `{ href: about.qmd }`.

### Metadata storage (parallel to `navigation.toc`)

```yaml
# After Generate stage (structured):
navigation:
  navbar:
    title: "My Site"
    background: primary
    search: true
    left: [ NavItem, … ]
    right: [ NavItem, … ]
  footer:
    left:   [ FooterRegionItem, … ]      # text (as Inlines) OR nav items
    center: [ FooterRegionItem, … ]
    right:  [ FooterRegionItem, … ]
    border: true
    background: light
    foreground: dark

# After Render stage (HTML strings):
rendered:
  navigation:
    navbar: "<nav class=\"navbar …\">…</nav>"
    footer: "<footer class=\"page-footer …\">…</footer>"
```

User override points: pre-populate `navigation.navbar` / `navigation.footer` to skip Generate; pre-populate `rendered.navigation.navbar` / `rendered.navigation.footer` to skip Render.

## Proposed implementation

### New crate: `quarto-navigation`

Per q4: keep pampa's remit on "Rust port of Pandoc" and put document-model navigation types in a new crate `crates/quarto-navigation/`. (`pampa::toc` can stay where it is for now; migrating TOC into `quarto-navigation` is a tracked follow-up — see §"Deferred work" below.)

Module layout:

```
crates/quarto-navigation/
  src/
    lib.rs
    item.rs        # NavigationItem + parsing from ConfigValue
    navbar.rs      # Navbar struct + resolve_navbar()
    footer.rs      # PageFooter + FooterRegion + resolve_page_footer()
    render_html.rs # HTML emission for all three (keeps emission logic in one place)
```

Dependencies: `quarto-pandoc-types` (for `ConfigValue`, `PandocInlines`, `PandocBlocks`), `quarto-source-map`, `quarto-error-reporting`.

### New transforms in `quarto-core`

Mirror the TOC pattern:

1. `crates/quarto-core/src/transforms/navbar_generate.rs` — reads raw `navbar` from `ast.meta`, calls `quarto_navigation::resolve_navbar`, stores at `navigation.navbar`. Skips if `navbar == false` or result already present.
2. `crates/quarto-core/src/transforms/footer_generate.rs` — analogous for footer.
3. `crates/quarto-core/src/transforms/navbar_render.rs` — reads `navigation.navbar`, calls `quarto_navigation::render_html::navbar_to_html`, stores at `rendered.navigation.navbar`.
4. `crates/quarto-core/src/transforms/footer_render.rs` — analogous for footer.
5. Register in `crates/quarto-core/src/transforms/mod.rs`.

### Pipeline wiring

Per q5: **all Generate stages run first, then all Render stages**. This lets user filters between phases inspect/modify the full structured navigation state, and lets future non-HTML formats (slideshows, dashboards) reuse the Generate stages and provide their own Render stages.

Concretely in `pipeline.rs`, stage 7 ordering becomes:

```
Existing transforms … (Callout, Theorem, Proof, FloatRefTarget, CrossrefIndex, …)
TocGenerateTransform
NavbarGenerateTransform
FooterGenerateTransform
TocRenderTransform           # HTML-specific
NavbarRenderTransform        # HTML-specific
FooterRenderTransform        # HTML-specific
```

(Render transforms are HTML-specific; when we add slideshow/dashboard pipelines, we'll skip or substitute them.)

### Template changes

`crates/quarto-core/src/template.rs` `FULL_HTML_TEMPLATE`:

- Insert after `<body …>` and before `$include-before$`:
  ```
  $if(rendered.navigation.navbar)$
  $rendered.navigation.navbar$
  $endif$
  ```
- Insert after `$include-after$`, just before `</body>`:
  ```
  $if(rendered.navigation.footer)$
  $rendered.navigation.footer$
  $endif$
  ```

Each rendered string is a complete `<nav>…</nav>` / `<footer>…</footer>`, so the template's only job is conditional inclusion.

### HTML output shape

Match Quarto 1 class names so existing themes/CSS keep working.

Navbar:

```html
<nav class="navbar navbar-expand-lg navbar-dark bg-primary" data-bs-theme="dark">
  <div class="container-fluid">
    <a class="navbar-brand" href="/">My Site</a>
    <button class="navbar-toggler" …>…</button>
    <div class="collapse navbar-collapse" id="navbarCollapse">
      <ul class="navbar-nav me-auto">…</ul>       <!-- left -->
      <!-- optional: <div class="quarto-search"></div> placeholder -->
      <ul class="navbar-nav ms-auto">…</ul>       <!-- right -->
    </div>
  </div>
</nav>
```

Footer:

```html
<footer class="footer" style="background-color: …">
  <div class="nav-footer">
    <div class="nav-footer-left">…</div>
    <div class="nav-footer-center">…</div>
    <div class="nav-footer-right">…</div>
  </div>
</footer>
```

Dropdown menus: standard Bootstrap 5 (`nav-item dropdown`, `dropdown-menu`, `dropdown-item`).

Search (per q6): emit `<div class="quarto-search"></div>` placeholder only. Backend is a separate future issue.

### Rendering footer text: markdown-aware

Since document-frontmatter strings arrive as `PandocInlines`, footer rendering must walk inlines to HTML. Strategy:

- For a `FooterRegion` that is `PandocInlines`, walk the inlines and emit HTML (Emph → `<em>`, Strong → `<strong>`, Link → `<a href>`, Str → escape and emit, …). This is a small subset of inlines; reuse (or mirror) whatever inline→HTML helper already exists in `quarto-core` / `pampa` for title rendering. *During implementation, confirm whether there's already a `inlines_to_html` helper to reuse; if not, add a small one in `quarto-navigation::render_html`.*
- For a region that is a list of `NavigationItem`, emit `<ul>` / `<li>` / `<a>` with attributes.

This gives markdown in footer strings "for free" from document frontmatter, and via `!md` from project config. No separate v1/v2 split needed — dropping the earlier plan's Option A/B compromise.

## YAML schema (documented, not yet validated)

Per q8: Quarto 2 does not yet have a runtime YAML validation pipeline wired up. The schemas under `crates/quarto-yaml-validation/test-fixtures/schemas/` are fixtures for the validator crate's unit tests, not the authoritative runtime schema. We will:

- **Record the desired schema in this document** (below) so a future validation session can adopt it wholesale.
- **Not wire up runtime validation** as part of this session.

### Proposed schemas (for future validation session)

```yaml
# navbar (top-level key)
navbar:
  anyOf:
    - boolean                # `false` = disable; `true` reserved for future use
    - object:
        closed: true
        properties:
          title:         anyOf: [string, boolean]
          logo:          string               # (light/dark specifier: future)
          logo-alt:      string
          logo-href:     string
          background:    string
          foreground:    string
          search:        boolean
          pinned:        boolean
          collapse:      boolean
          collapse-below: enum: [sm, md, lg, xl, xxl]
          toggle-position: enum: [left, right]
          tools-collapse: boolean
          left:          arrayOf: ref(navigation-item)
          right:         arrayOf: ref(navigation-item)

# page-footer (top-level key)
page-footer:
  anyOf:
    - boolean                # `false` = disable
    - string                 # centered short form
    - object:
        closed: true
        properties:
          left:       ref(page-footer-region)
          center:     ref(page-footer-region)
          right:      ref(page-footer-region)
          border:     anyOf: [boolean, string]
          background: string
          foreground: string

# page-footer-region
page-footer-region:
  anyOf:
    - string                                  # markdown text (single line)
    - arrayOf: ref(navigation-item)

# navigation-item (shared)
navigation-item:
  anyOf:
    - path                                    # shorthand: a file path
    - object:                                 # full form
        closed: true
        properties:
          href:       string
          file:       string                  # alias for href (hidden)
          text:       string
          icon:       string
          aria-label: string
          rel:        string
          target:     string
          menu:       arrayOf: ref(navigation-item)
```

## Testing strategy (TDD)

### Phase A: parsing (`quarto-navigation`)

- `NavigationItem` from short path form (`"about.qmd"`).
- `NavigationItem` from object form with `text`, `icon`, `href`, `menu`.
- `resolve_navbar` on `navbar: false` → `None`.
- `resolve_navbar` on full-object YAML → fully-populated `Navbar`.
- `resolve_page_footer` on `page-footer: false` → `None`.
- `resolve_page_footer` on string → `{ center: Text(inlines), …defaults }`.
- `resolve_page_footer` on object with markdown-inlines `left`.
- `to_config_value` / `from_config_value` round-trip for each type.

### Phase B: Generate transforms

- Populates `ast.meta.navigation.navbar` / `.footer`.
- Skips when key absent.
- Skips when key is `false`.
- Skips when result already present (user override).

### Phase C: Render transforms

- Snapshot test: canonical navbar YAML → expected HTML.
- Snapshot test: canonical footer YAML → expected HTML.
- Snapshot test: footer region containing markdown inlines → emits `<em>` / `<strong>` / `<a>`.
- HTML-escape safety: `text: "A & B"` renders as `A &amp; B`.
- Skips if pre-rendered override exists.

### Phase D: Integration — end-to-end

- Render `.qmd` with `navbar:` in frontmatter; final HTML contains the navbar.
- Render `.qmd` with `page-footer:` in frontmatter; final HTML contains the footer.
- Render with both unset: no navbar, no footer.
- Render with project `navbar:` + document `navbar: false`: suppressed.

### Phase E: Merge semantics integration tests

Exercise `!prefer` / `!concat` explicitly so users have a documented path:

- Project `navbar.left: [a, b]`, document `navbar.left: !prefer [c]` → merged left is `[c]`.
- Project `navbar.left: [a, b]`, document `navbar.left: !concat [c]` → merged left is `[a, b, c]`.
- Project `navbar.left: [a, b]`, document `navbar.left: [c]` → default behavior (document these results; current default is concat for arrays per `quarto-pandoc-types`).
- Project `page-footer: "A"`, document `page-footer: !prefer "B"` → footer shows "B".

### Phase F: `toc: false` parity fix (tracked in follow-up issue; may do in same session)

- Test: project `toc: true`, document `toc: false` → no TOC rendered.
- Fix `toc_generate.rs` to treat `false` as affirmative disable.
- Apply identical logic to navbar/footer generate transforms.

### Phase G: Full-workspace verification

- `cargo nextest run --workspace`
- `cargo xtask verify`

## Work plan

Phases are ordered for TDD: each starts with tests, then implementation.

### Phase 0: Alignment and parity fix

- [x] File follow-up beads issue for `toc: false` semantics (`bd-ltmn`).
- [x] Add `is_feature_disabled(meta, key)` helper in `transforms/config.rs` (reusable by navbar/footer).
- [x] Add test for Render: `toc: false` + pre-populated `navigation.toc` → no rendered HTML. Verified it fails under current code.
- [x] Add test for Generate: `toc: false` → no generation.
- [x] Apply `is_feature_disabled` guard at the top of both `toc_render.rs::transform` and `toc_generate.rs::transform`.
- [x] All 7450 workspace tests pass.

### Phase 1: Data model (`quarto-navigation` crate)

- [x] Create crate skeleton; add to workspace `Cargo.toml`.
- [x] Write tests for `NavigationItem` parsing (short form + object form + menu).
- [x] Write tests for `Navbar` parsing (including `false`, `true`, full object, `title: false`/`true`).
- [x] Write tests for `PageFooter` parsing (including string shorthand, `false`, border variants).
- [x] Write tests for `to_config_value` / `from_config_value` round-trips (item, navbar, footer).
- [x] Implement types + parsers. All 27 tests pass.

### Phase 2: Generate transforms (`quarto-core`)

- [x] Write tests for `NavbarGenerateTransform` (populate, skip-if-false, skip-if-bare-true, skip-if-exists, skip-if-absent).
- [x] Write tests for `FooterGenerateTransform` (same, plus string-shortcut populating center, object form populating regions).
- [x] Implement transforms.
- [x] Register in `transforms/mod.rs` and wire into pipeline *after* `TocGenerateTransform` (and before `TocRenderTransform`). Renamed phase to "Navigation Phase" in pipeline docs.
- [x] 10 new transform tests pass; full workspace 7487 tests green.

### Phase 3: Render transforms (HTML)

- [x] Implement `render_html` in `quarto-navigation`: `navbar_to_html`, `page_footer_to_html`, + shared `inlines_to_html` walker covering Str/Space/Emph/Strong/Code/Link/Strikeout/Underline/Sup/Sub/SmallCaps/Quoted/Span/RawInline(html).
- [x] 18 unit tests in `render_html` covering: title rendering (explicit/fallback/hidden), dropdown menus, search placeholder, icons, aria-label, collapse-below variations, toggle position, HTML escaping, markdown title, footer regions (empty/text/items/markdown), background/foreground/border variants.
- [x] Implement `NavbarRenderTransform` and `FooterRenderTransform` in `quarto-core`; 9 unit tests covering: skip-missing, skip-false-override, skip-prerendered, renders-html, document-title-fallback.
- [x] Register in pipeline: `TocGenerate → NavbarGenerate → FooterGenerate → TocRender → NavbarRender → FooterRender` (all generates before any renders).
- [x] 44 `quarto-navigation` tests + 19 navbar/footer transform tests pass; full workspace 7513 tests green.

### Phase 4: Template wiring

- [x] Update `FULL_HTML_TEMPLATE` in `template.rs`: navbar slot immediately after `<body>` (before `$include-before$`); footer slot after `$include-after$` (before `</body>`). Both guarded by `$if(rendered.navigation.{navbar,footer})$`.
- [x] 3 unit tests on the template slots (navbar populated, footer populated, both absent).
- [x] End-to-end integration suite in `crates/quarto-core/tests/navigation_e2e.rs` (5 tests): full YAML → generate → render → final HTML; positioning asserts; `navbar: false` suppresses while footer still renders; user pre-rendered HTML preserved through the pipeline.
- [x] Full workspace: 7521 tests pass.

### Phase 5: Merge-tag integration tests

- [x] Integration suite in `crates/quarto-core/tests/navigation_merge.rs` (6 tests) exercises the merge machinery against navbar/footer YAML layers.
- [x] **Observed semantics** (pinned down for future docs session):
  - Arrays merge via `!concat` by default: project `left: [a, b]` + doc `left: [c]` → `[a, b, c]`.
  - `!prefer` on an array replaces it entirely.
  - Scalars default to last-wins; `!prefer` makes the intent explicit but does not change the outcome.
  - `!prefer` on an entire map replaces the whole map — sibling keys in earlier layers are discarded.
  - Untagged map-vs-map merge deep-merges: sibling keys from the lower layer are preserved when the higher layer doesn't mention them.
  - `navbar: false` at document layer beats a full project `navbar:` map — affirmative disable wins without needing `!prefer`.

### Phase 6: Verification

- [x] `cargo build --workspace` — clean.
- [x] `cargo nextest run --workspace` — 7527 tests pass (47 new since Phase 0 baseline: 27 in `quarto-navigation`, 10 transform unit, 5 e2e, 6 merge-tag, 3 template-slot, plus 4 for `is_feature_disabled` and `toc: false` parity).
- [x] `cargo xtask verify` — Rust + hub-client WASM build + hub-client tests + trace-viewer build + trace-viewer tests all pass.
- [x] Manual render: `/tmp/q2-navbar-demo/demo.qmd` renders to a 64-line HTML doc with correct navbar (primary background, brand, dropdown menu, search placeholder, icon-only GitHub link) and footer (markdown-bold "Acme" in left region, GitHub icon in right). Markdown-in-metadata works "for free" as designed.

### Phase 7: User documentation stub

- [x] Added `docs/navigation.qmd` (181 lines) covering: navbar options, footer options, navigation-item shape, markdown-in-metadata semantics, `!prefer`/`!concat` behavior, and the `false` affirmative-disable shortcut.
- [x] Wired into the docs website navbar in `docs/_quarto.yml`.

## Post-landing follow-ups (this file)

### Footer and navbar body wrapped in `.container-fluid` (landed)

- [x] Footer HTML now wraps `.nav-footer` in a `.container-fluid`, mirroring the navbar's existing structure, so plain HTML pages (no website container) inherit Bootstrap's gutter padding and the three-region flex layout doesn't sit flush against viewport edges. Q1's website footer got this spacing implicitly from the surrounding site layout; Q2's general case needs to provide it explicitly.
- [x] Two new tests (`footer_wraps_body_in_container_fluid`, `navbar_wraps_body_in_container_fluid`) pin the wrapper and nesting order (`<footer> > .container-fluid > .nav-footer`).

### Bootstrap Icons shipping (deferred to `bd-djpt`)

- [ ] Icons like `<i class="bi bi-github"></i>` render as empty boxes because the Bootstrap Icons font + CSS package isn't shipped. `bd-djpt` (related to `bd-ulgr`) will design resource shipping for Bootstrap Icons alongside the JS-deps work.

### Footer layout SCSS ported from Q1 (landed)

- [x] Ported Q1's footer layout rules (three-region flex, responsive stacking, font sizes, border, backgrounds) from `src/resources/projects/website/navigation/quarto-nav.scss:806-926` into Q2's `resources/scss/bootstrap/_bootstrap-rules.scss` (~115 lines appended).
- [x] All variables (`$footer-bg`, `$footer-fg`, `$footer-border`, `$footer-*-font-size`) already exist in Q2's `_bootstrap-variables.scss` and the `theme-contrast` function is present in `_bootstrap-functions.scss` — no variable/function shims needed.
- [x] Updated the footer HTML renderer (`quarto-navigation::render_html`) to emit `<ul class="nav footer-items">` on item regions so Q1's selectors match Q2's DOM without further template surgery.
- [x] Assertions added to `test_compile_default_css` that the compiled default CSS ships `.nav-footer`, `.nav-footer-left`, `.footer-items`.
- [x] Manual render of a `page-footer:` with left/center/right regions produced the expected flex-based three-column layout (e.g. `.nav-footer{display:flex;...justify-content:space-between}`) in the shipped CSS.
- [x] Q1's footer styling is website-project-only; in Q2 we include it unconditionally as part of the Quarto layer that rides on top of Bootstrap, so any HTML doc with a `page-footer:` gets proper layout.

### Default theme behavior matches Q1 (landed)

- [x] `CompileThemeCssStage` now compiles the full Bootstrap + Quarto layer when `theme:` is absent. `theme: none` is the explicit opt-out that ships the lightweight static `DEFAULT_CSS`. This was the root cause of the "ugly navbar" symptom (DOM correct, Bootstrap CSS missing).
- [x] New field `ThemeConfig.suppress_bootstrap: bool`, set only when `theme:` literal string `none` is seen (case-insensitive). `theme: null` and missing `theme:` both hit the compile-default path.
- [x] `compile_default` helper in the stage wraps `quarto_sass::compile_default_css` for native (sync) and WASM (async).
- [x] 5 new tests (3 in stage, 2 in pipeline); 3 new tests in `quarto-sass::config`; existing `test_null_theme_uses_default_css` rewritten to assert the new semantics.
- [x] Sample render now ships ~302 KB of Bootstrap 5.3.1 including `.navbar`, `.navbar-brand`, `.dropdown`, `.btn`, etc. Full workspace 7532 tests + `cargo xtask verify` green.
- [ ] **Remaining behaviors blocked on `bd-ulgr`**: navbar dropdown and hamburger collapse still do nothing at runtime because Bootstrap JS is not shipped. Tracked in `bd-ulgr`; outline at `claude-notes/plans/2026-04-18-html-js-deps-design.md`.
- [ ] **`theme: pandoc` sentinel**: Q1 also supports `theme: pandoc` (skip Quarto CSS entirely). Deferred; not requested in this session. File follow-up if users need it.

## Deferred work (separate issues)

Filed or to file as follow-ups:

1. **`toc: false` parity** — if we don't fix it in Phase 0, track separately. Plan includes fixing it.
2. **Search backend** — actual search index + runtime. Ship placeholder div for now.
3. **Sidebar / book / website navigation** — book chapter sidebar, prev/next pagination, site-wide nav. Uses the same `quarto-navigation` + Generate transform pattern; will extend the crate.
4. **`logo` light-dark specifier** — Quarto 1's `logo-light-dark-specifier` schema. v1 takes a plain path.
5. **Migrating `pampa::toc` into `quarto-navigation`** — low priority; clean-up once navigation crate is well-established.
6. **Accessibility pass** — sensible `aria-label` defaults for navbar/footer.
7. **Theme coupling (`navbar-dark` vs `navbar-light`)** — auto-infer from `foreground`/theme.
8. **Markdown inline rendering shared helper** — if we end up writing our own `inlines_to_html`, see whether it should be unified with existing helpers in `quarto-core`.

## Scope decisions and future-session concerns

Points to preserve so we don't paint ourselves into a corner (answering q10):

- **Schema nesting for books/websites.** This session uses top-level `navbar:` / `page-footer:`. A future project-type session may want to accept the same keys under `website:` / `book:` for compatibility. The design of the Generate transforms reads only from the merged `ast.meta`, so nothing in this implementation prevents a future metadata pre-step that promotes `website.navbar` → `navbar`. Do not hard-code "navbar only lives at top level" anywhere.
- **Sidebar.** Not in v1. But the `navigation.*` namespace reserves space for `navigation.sidebar`. Keep `quarto-navigation` open for adding a `Sidebar` type without restructuring.
- **Per-page "prev/next" pagination.** A separate navigation concept; belongs alongside sidebar.
- **Cross-document title resolution.** Today `navigation-item` supports `text` falling back to target document's title. In a single-document context we can't resolve other documents' titles. Deferred: treat missing `text` as "use href as display text" with a TODO; real resolution happens when project context is threaded through.
- **Merge semantics documentation.** Quarto 2's `!prefer` / `!concat` are the right primitives, but user-facing docs do not yet exist. Phase 7 adds a stub; a future docs session should write a full "metadata merge" explainer that covers these tags across all config.
- **HTML pipeline variants.** Slideshows (reveal.js) and dashboards will have different body templates and likely different navbar/footer behavior. The Generate/Render split means they can reuse `quarto-navigation` types and Generate transforms while providing their own Render or skipping it.
- **WASM / hub-client.** The hub-client consumes the HTML pipeline via `wasm-quarto-hub-client`. Verify Phase 6 includes `cargo xtask verify` so WASM builds stay green. No hub-client UI changes expected for v1 (no preview surface for navbar/footer yet).

## Questions resolved

| # | Q | Resolution |
|---|---|------------|
| 1 | Top-level vs `website.*` | Top-level. |
| 2 | `navbar: true` shorthand | Drop. Only `false` is meaningful. Pursue uniform `false` = disable rule across toc/navbar/footer; fix `toc: false` in Phase 0. |
| 3 | `navbar: "string"` shorthand | Drop. |
| 4 | Home crate | New `quarto-navigation` crate. |
| 5 | Transform ordering | All generates first, then all renders. |
| 6 | Search | Placeholder div only in v1. Q2 has no search client yet; Q1 uses Fuse.js / Algolia. |
| 7 | Markdown in footer strings | Free from `InterpretationContext::DocumentMetadata`; `!md` tag in project config. Render walks `PandocInlines`. |
| 8 | Schema location | No real validation pipeline exists yet. Schema recorded in this doc for a future validation session. |
| 9 | Per-document override | Use `!prefer` / `!concat`. Add integration tests (Phase 5). No bespoke merge policy. |
| 10 | Books/websites | Out of scope. Design preserves headroom (see "Scope decisions"). |
