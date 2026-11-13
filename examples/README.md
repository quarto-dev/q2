# Quarto 2 example projects

Runnable example projects that exercise Quarto 2 features end to end.
Each example is a self-contained Quarto 2 project with its own
`README.md` explaining what it demonstrates and how to run it.

These examples are **user-driven**: they are not part of the test
suite. Their purpose is twofold:

1. **Manual verification** — confirm shipped features behave as
   expected when run through the real `quarto` CLI.
2. **Documentation seed** — they will eventually be linked from the
   Quarto 2 documentation site and serve as starter templates a user
   can copy.

## Running an example

From the repository root, build the `q2` binary once:

```bash
cargo build --bin q2
```

Then render any example:

```bash
cargo run --bin q2 -- render examples/websites/01-minimal
```

Each example writes its output to `<project>/_site/`. The intermediate
profile cache lives at `<project>/.quarto/cache/`. Both directories
are gitignored.

## Website examples

Examples that exercise the website-projects feature surface
(epic `bd-0tr6`, phases 0–9). Each demonstrates one specific aspect.

| Example | Demonstrates |
|---|---|
| [`websites/01-minimal`](websites/01-minimal/) | Two pages, manual sidebar, cross-document link rewriting |
| [`websites/02-auto-sidebar`](websites/02-auto-sidebar/) | Sidebar populated automatically from a directory walk |
| [`websites/03-nested-sidebar`](websites/03-nested-sidebar/) | Nested sections, multiple sidebars selected by path, prev/next strip |
| [`websites/04-navbar-footer`](websites/04-navbar-footer/) | Navbar with dropdown menu, active-page highlight, page-footer |
| [`websites/05-shared-resources`](websites/05-shared-resources/) | `_site/site_libs/` deduplication of theme CSS across pages at different depths |
| [`websites/06-site-metadata`](websites/06-site-metadata/) | `site-url`, `sitemap.xml`, `robots.txt`, favicon, title prefix, canonical URL |
| [`websites/07-incremental`](websites/07-incremental/) | Full-project vs subset rendering, profile cache, `--clean-cache` |
| [`websites/08-hub-preview`](websites/08-hub-preview/) | Hub-client live preview against a website project (manual browser recipe) |
| [`websites/09-interactive-listing`](websites/09-interactive-listing/) | A `type: metadata` listing, an `{lmd}` cell that writes the page from its items, and a TypeScript React component that filters the posts by tag (bd-react-components-a8flgldm) |
| [`websites/10-federated-search`](websites/10-federated-search/) | One search box over three websites: an umbrella site whose `website.search.federate` list pulls in two product sites, all rendered into one served tree |
| [`websites/11-responsive-navbar`](websites/11-responsive-navbar/) | Navbar item `text:` keyed by breakpoint: full labels on wide screens and in the collapsed menu, short ones where the expanded bar is cramped, icon-only ranges |

## Presentation examples

Minimal `format: revealjs` decks, one per authoring feature, under
[`presentations/`](presentations/). Each is a `type: default` project
with a single `slides.qmd`. They back the reveal.js documentation page
(`docs/presentations/revealjs/index.qmd`).

## Dashboard examples

Minimal `format: dashboard` projects, one per feature, under
[`dashboards/`](dashboards/). Each is a `type: default` project with a single
`dashboard.qmd` (07 is a website). They are engine-free: charts are small SVG
files and tables are markdown, so they render without R or Python. They back
the dashboard documentation page (`docs/dashboards/index.qmd`).

| Example | Demonstrates |
|---|---|
| [`dashboards/01-basic-layout`](dashboards/01-basic-layout/) | Rows and columns from headings, `height` / `width`, `.card` divs |
| [`dashboards/02-pages`](dashboards/02-pages/) | Level-1 headings as pages, a page with its own `orientation` |
| [`dashboards/03-value-boxes`](dashboards/03-value-boxes/) | `.valuebox` divs with icons, theme and custom colors, `showcase-position` |
| [`dashboards/04-sidebars-toolbars`](dashboards/04-sidebars-toolbars/) | A collapsible page sidebar with a `width`, a page toolbar |
| [`dashboards/05-tabsets`](dashboards/05-tabsets/) | A `.tabset` column, a `.card-toolbar` in a card's header |
| [`dashboards/06-navigation`](dashboards/06-navigation/) | Navbar title block, `logo`, `nav-buttons`, light/dark themes with a toggle |
| [`dashboards/07-website`](dashboards/07-website/) | A dashboard page inside a website: site navbar, footer and theme |

## Plot examples

Hephaestus plot documents (`.hep`) rendered at render time
(bd-3qych45b; plan
`claude-notes/plans/2026-09-17-hephaestus-hep-integration.md`).

| Example | Demonstrates |
|---|---|
| `plots/01-hephaestus-basic` | A `.hep` as a captioned figure, at an explicit size, and the fail-soft path for a missing file |

## lmd examples

Minimal `type: default` projects, one per pattern of use of lmd (Lua
cells and markdown fragments; bd-lmd-exploration-jqv2nbl4, PR #823),
under [`lmd/`](lmd/). Each has a single `document.qmd`. They back the lmd
page of the documentation (`docs/guides/authoring/lmd.qmd`), which embeds
their rendered output.

| Example | Demonstrates |
|---|---|
| [`lmd/01-first-cell`](lmd/01-first-cell/) | One cell mapping a front-matter list to a markdown fragment per item |
| [`lmd/02-metadata`](lmd/02-metadata/) | `meta` and `quarto.metadata.get`; counting, joining and string methods on metadata values |
| [`lmd/03-lists`](lmd/03-lists/) | A tight bullet list, items with several blocks, an ordered list, and a section per item |
| [`lmd/04-shared-state`](lmd/04-shared-state/) | A setup cell whose variable and function are used by later inline cells, one in a heading |
| [`lmd/05-fragment-functions`](lmd/05-fragment-functions/) | A `card(title, body)` function with a block cell nested inside its fragment |
| [`lmd/06-conditional`](lmd/06-conditional/) | A callout returned only when a front-matter flag is set; `quarto.doc.is_format` |
| [`lmd/07-walk`](lmd/07-walk/) | `Blocks:walk` on a fragment |
| [`lmd/08-echo`](lmd/08-echo/) | `--| echo: true`, showing the desugared Lua above the result |
| [`lmd/09-modules`](lmd/09-modules/) | `require` of a module written in lmd (`components/cards.lmd`), used by two cells |
| [`lmd/10-project`](lmd/10-project/) | Modules across a project: `lmd: modules:` autoload from `_quarto.yml`, a project-root `require("/…")`, an extension's module by name, and `quarto.include` of two partials |

## Highlighting examples

Syntax highlighting beyond the built-in languages, under
[`highlighting/`](highlighting/). They back the code-highlighting page of
the documentation (`docs/guides/authoring/code-highlighting.qmd`).

| Example | Demonstrates |
|---|---|
| [`highlighting/01-user-grammar`](highlighting/01-user-grammar/) | A tree-sitter grammar (TOML) shipped in `_quarto/grammars/toml/`, highlighting blocks and inline code, and a CSS rule on one `hl-*` capture class |

## Other example categories

Future categories (books, manuscripts) will get their own
subdirectories alongside `websites/` and `presentations/`.
