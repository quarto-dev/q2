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

## Presentation examples

Minimal `format: revealjs` decks, one per authoring feature, under
[`presentations/`](presentations/). Each is a `type: default` project
with a single `slides.qmd`. They back the reveal.js documentation page
(`docs/presentations/revealjs/index.qmd`).

## Other example categories

Future categories (books, manuscripts) will get their own
subdirectories alongside `websites/` and `presentations/`.
