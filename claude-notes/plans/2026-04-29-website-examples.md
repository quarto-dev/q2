# Example Quarto 2 website projects (end-to-end feature exercise)

**Date:** 2026-04-29
**Beads:** `bd-2jwk` (this task); related `bd-0tr6` (website epic, closed),
`bd-tr81` (Quarto 2 docs epic).
**Parent epic:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Status:** Plan approved 2026-04-29 (user agreed location, granularity,
README-plus-prose format, no CI wiring, manual recipe for hub-preview).

## Overview

The website-projects epic (`bd-0tr6`) shipped phases 0–9. Internal
test coverage exists, but the user wants a set of **runnable example
projects** that exercise each feature surface end-to-end. The goals:

1. **Manual end-to-end verification** — confirm each shipped feature
   behaves as documented when run through the real `quarto` binary.
2. **Documentation seed** — these projects (and their READMEs) will
   be referenced from the eventual Quarto 2 docs site (`docs/`,
   `bd-tr81`). The docs site itself will be a Quarto 2 project; the
   examples are reference assets it points at.
3. **Onboarding** — a Quarto 1 user landing on Quarto 2 can copy a
   template that does what they want.

## Location and shape

All example projects live at `examples/websites/<name>/`. Each is a
self-contained Quarto 2 website project with:

- `_quarto.yml` (project + website config)
- `README.md` — what this example demonstrates, exact commands to
  run, what to inspect, what to expect
- One or more `*.qmd` source files. The `index.qmd` (and other
  pages where it makes sense) embeds prose describing the feature
  in user-facing language, so a reader of the rendered site
  understands what they're looking at without needing the README.

Top-level `examples/README.md` lists the projects, indicates which
features each demonstrates, and links to per-project READMEs.

We do **not** wire these into `cargo xtask verify`. The unit-test
fixtures under `crates/quarto-core/tests/fixtures/websites/` already
guard regressions; these examples are for human exercise and
documentation. (Decision confirmed 2026-04-29.)

## Project list

Eight projects, one per major feature area. Granularity confirmed
2026-04-29.

| Name | Demonstrates | Phase(s) |
|---|---|---|
| `01-minimal` | Two pages, manual sidebar, cross-doc link | 1, 2, 6 |
| `02-auto-sidebar` | Sidebar with `auto:` | 2 |
| `03-nested-sidebar` | Nested sections, multiple sidebars, prev/next | 2, 4 |
| `04-navbar-footer` | Navbar dropdown, active highlight, page-footer | 3 |
| `05-shared-resources` | `_site/site_libs/` theme dedup across pages | 5 |
| `06-site-metadata` | site-url, sitemap.xml, robots.txt, favicon, title prefix, canonical URL | 7 |
| `07-incremental` | Mode A vs Mode B render, profile cache, `--clean-cache` | 8 |
| `08-hub-preview` | Hub-client live preview (manual recipe) | 9 |

The numeric prefix orders the directory listing roughly from simple
to advanced. The naming should also work as a teaching sequence
when these become docs.

## Per-project content sketch

### `01-minimal` — two pages, manual sidebar, cross-doc link

**Files:**
- `_quarto.yml` — `project.type: website`, `website.title`,
  `website.sidebar.contents: [index.qmd, about.qmd]`.
- `index.qmd` — landing page; explains in body that this is the
  minimal example; links to about with `[About](about.qmd)`.
- `about.qmd` — about page; explains the feature surface that's
  exercised; links back to home with `[Home](index.qmd)`.
- `README.md` — instructions.

**README points to inspect:**
- After `quarto render`: `_site/index.html`, `_site/about.html`,
  `_site/site_libs/quarto/quarto-theme-<hash>.css` exist.
- The `[About](about.qmd)` source link is rewritten to
  `<a href="about.html">` in the rendered HTML.
- The sidebar appears with both pages, "Home" highlighted on
  `index.html` and "About" highlighted on `about.html`.
- Sidebar entries derive their text from each page's
  `title:` frontmatter when no explicit `text:` is given.

### `02-auto-sidebar` — sidebar with `auto:`

**Files:** four pages plus a `posts/` subdirectory with two more.
`_quarto.yml` uses `website.sidebar.contents: auto: posts/`.

**README points to inspect:**
- Sidebar is populated automatically from filesystem walk.
- File frontmatter `order:` controls position; demonstrate with
  one page out-of-natural-sort order.
- Adding a new `.qmd` makes it appear (recipe).
- A `draft: true` page is excluded.

### `03-nested-sidebar` — nested sections, multiple sidebars, prev/next

**Files:**
- Multi-section: a `guide/` and a `reference/` subdirectory each
  with a few pages.
- `_quarto.yml` declares **two** sidebars keyed by id, each
  rooted in its own subdirectory and selected via path-prefix.
- Pages in `guide/` get the guide sidebar, pages in `reference/`
  get the reference sidebar, root pages get the default.

**README points to inspect:**
- Sidebar selection swaps as you navigate between subtrees.
- Prev/next strip at bottom of each page reflects sidebar order.
- A `page-navigation: false` override on one page suppresses the
  strip.
- Nested sections collapse/expand reflect the active page (HTML
  classes; JS for collapse comes from a future phase).

### `04-navbar-footer` — navbar dropdown, active highlight, page-footer

**Files:** five pages including a `tools/index.qmd` and
`tools/converter.qmd` reachable from a navbar dropdown.

**README points to inspect:**
- Navbar renders at top of every page; active page gets `active`
  class.
- Dropdown menu under "Tools" lists nested pages with proper
  hrefs.
- Brand label falls through `navbar.title → website.title →
  document.title`.
- Page-footer renders left/center/right items at the bottom.
- Footer items pointing at `.qmd` are rewritten to `.html`.

### `05-shared-resources` — `_site/site_libs/` theme dedup

**Files:** three pages at different directory depths (root,
`docs/api.qmd`, `docs/internals/architecture.qmd`).

**README points to inspect:**
- After render: exactly one `_site/site_libs/quarto/quarto-theme-<hash>.css`
  file, not three copies.
- Each page's HTML `<head>` includes a `<link>` whose `href` is
  the *correct relative path* to that single file
  (`site_libs/...` for root, `../site_libs/...` for one-deep,
  `../../site_libs/...` for two-deep).
- A diff inspection recipe: `find _site -name '*.css'` shows one
  copy.
- An `extensions/`-style fixture with a CSS dependency to confirm
  extension resources also flow through `Project` scope.

### `06-site-metadata` — sitemap, robots.txt, favicon, title prefix, canonical URL

**Files:** three pages, a `favicon.ico` (or `.svg`), a
`_quarto.yml` with `website.site-url`, `website.title`,
`website.favicon`.

**README points to inspect:**
- `_site/sitemap.xml` exists and lists every rendered page with
  `<loc>` URLs prefixed by `site-url`.
- `_site/robots.txt` references the sitemap.
- `_site/favicon.ico` is copied.
- Each page's `<head>` includes `<link rel="icon">`,
  `<link rel="canonical">`, and `<title>Page — Site</title>`.
- A page with an explicit `title-prefix: false` (or however it
  ends up) suppresses the prefix (if supported in MVP — confirm
  during implementation).

### `07-incremental` — Mode A vs Mode B render

**Files:** five pages including a `posts/` subdirectory.

**README recipe:**
1. `quarto render examples/websites/07-incremental` — full project,
   cold cache. Note timing.
2. Re-run same command — full project, warm profile cache. Note
   timing improvement (Pass-1 hits cache).
3. `quarto render examples/websites/07-incremental/posts/first.qmd`
   — Mode B, only `first.qmd` re-renders. Verify other `_site/*.html`
   mtimes are unchanged.
4. Edit `posts/first.qmd` body, repeat step 3. Confirm only that
   one page rebuilt.
5. `quarto render examples/websites/07-incremental --clean-cache`
   — wipes `.quarto/cache/`. Re-run is now slow again.
6. Edit `_quarto.yml`'s sidebar — observe how Mode A re-renders
   everything; Mode B on a single page would not see sibling
   sidebars rebuild (call this out explicitly as a known
   limitation per Phase 8 §`bd-par3` follow-up).

**README points to inspect:**
- `.quarto/cache/profiles/` contains one JSON file per
  `DocumentProfile`.
- `_site/sitemap.xml` is merged on Mode B (entries for
  non-targets preserved, target entry updated).

### `08-hub-preview` — hub-client live preview

**Files:** three pages with a sidebar, similar to
`crates/quarto-core/tests/fixtures/websites/hub-smoke/` but
*outside* the test tree so users can copy-paste it.

**README recipe (manual):**
1. Build hub: `cd hub-client && npm run build:all`.
2. Start hub server: `cargo run --bin quarto -- hub serve` (or
   the equivalent invocation; document exactly).
3. Open the printed URL in Chrome.
4. Connect to a new project; upload the
   `examples/websites/08-hub-preview/` directory.
5. Open `index.qmd` in the editor pane; the preview iframe shows
   the rendered page **with the sidebar** (this is the Phase 9
   payoff — pre-9 the preview would have been bare).
6. Edit a sibling page's title in the editor; switch back to
   `index.qmd`; the sidebar entry's text updates within one
   debounce cycle.
7. Click a sidebar entry in the preview iframe; the editor
   navigates to that file (`MorphIframe.onNavigateToDocument`).

**README points to inspect:** all of the above as a checklist;
plus the rendered HTML in the iframe should include
`<link rel="stylesheet" href=".../quarto-theme-...css">` resolving
to a VFS path under `/.quarto/project-artifacts/` (Phase 9
flushed Project artifacts to VFS).

## Top-level `examples/README.md`

Brief — describes the directory structure, names the projects in
order, and explains:

- These are runnable demos / reference projects, not unit tests.
- They will eventually be linked from the Quarto 2 docs site.
- Each project's own `README.md` contains the recipe.

## Risks and mitigations

- **Risk:** features behave differently than I think they do once
  exercised end-to-end. *Mitigation:* this is exactly the value of
  this exercise. Each project's README must be filled in *after*
  observing the actual output, not from memory of plan documents.
  If a feature is broken or behaves unexpectedly, file a `bd` issue
  and call it out in the project README.
- **Risk:** the projects drift as the codebase evolves. *Mitigation:*
  defer to a follow-up. We may eventually wire them into
  `cargo xtask verify` once features stabilise.
- **Risk:** the `08-hub-preview` recipe is fragile (depends on hub
  CLI invocation that may change). *Mitigation:* keep the recipe in
  the README rather than baking it into a script; users can adapt
  the steps as the hub UX evolves.

## Work items

### Setup
- [x] Create `examples/` and `examples/websites/` directories.
- [x] Write top-level `examples/README.md`.
- [x] Add `examples/websites/.gitignore` excluding `_site/`,
      `.quarto/`, `*_files/`.

### Per-project (in this order — simplest first)
- [x] `01-minimal`: rendered, verified sidebar + active highlight +
      cross-doc link rewrite + title prefix; README written.
- [x] `02-auto-sidebar`: rendered, verified `order:` sort,
      title-fallback sort, `draft:` exclusion; README written.
- [x] `03-nested-sidebar`: rendered, verified containment-based
      sidebar selection, nested sections, prev/next strip,
      `page-navigation: false` override; README written.
      **Surfaced `bd-swpy`** (sidebar/navbar/footer hrefs not
      relativized to current page in nested directories).
- [x] `04-navbar-footer`: rendered, verified navbar dropdown,
      active highlighting, brand label fallback, footer regions;
      README written. Same `bd-swpy` issue applies to navbar +
      footer hrefs from subdirectories.
- [x] `05-shared-resources`: rendered, verified single shared
      `quarto-theme-<hash>.css` and per-depth-correct relative
      paths in `<link rel="stylesheet">` tags; README written.
- [x] `06-site-metadata`: rendered, verified
      `_site/sitemap.xml`, `_site/robots.txt`, copied favicon,
      `<title>` prefix, `<link rel="canonical">`,
      `<link rel="icon">`; README written.
- [x] `07-incremental`: walked through full recipe — Mode A cold,
      Mode A warm, Mode B subset (only `posts/first.html` mtime
      moved), sitemap merge (only target's `<lastmod>` updated),
      `--clean-cache`; README written with confirmed observations.
- [x] `08-hub-preview`: built fixture files; documented manual
      recipe for hub-client browser verification. Not exercised
      in browser this session.

### Wrap-up
- [x] Top-level `examples/README.md` populated with project list
      and links.
- [x] `bd-swpy` filed for sidebar/navbar/footer href
      relativization gap surfaced by `03-nested-sidebar` and
      `04-navbar-footer`.
- [ ] `br close bd-2jwk --reason ...`.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.

## Bugs / gaps surfaced

- **`bd-swpy`** — Sidebar / navbar / page-footer hrefs are emitted
  in project-root-relative form (e.g. `guide/installation.html`).
  When the current page lives in a subdirectory, the browser
  resolves these against the current URL and 404s
  (`_site/guide/guide/installation.html`). Body links go through
  a separate code path that *does* relativize. Fix: thread
  `ResourceResolverContext` through `resolve_href_for_html` in
  `crates/quarto-core/src/transforms/navigation_href.rs` and
  apply `page_url_for` on hits, mirroring the body-link path.
  Surfaced by `03-nested-sidebar`, also visible in
  `04-navbar-footer`. Discovered-from `bd-2jwk`, parent-child
  `bd-0tr6`.

## Decisions log (user confirmed 2026-04-29)

- **Location:** `examples/websites/` at repo root. Future
  `examples/books/` etc. fit alongside.
- **Granularity:** eight projects as listed above.
- **Per-project doc format:** README.md with run instructions,
  plus user-facing prose embedded in the qmd files themselves
  (so the rendered site is self-explanatory).
- **CI:** strictly user-driven; not wired into
  `cargo xtask verify`.
- **Hub-preview:** manual recipe, no Playwright.
- **Future:** these will become supporting assets for the Quarto 2
  docs site (`bd-tr81`), referenced from `docs/` rather than
  living inside it.
