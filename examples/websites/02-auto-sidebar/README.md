# 02-auto-sidebar — Sidebar populated from a directory walk

Demonstrates the `auto:` directive in `website.sidebar.contents`,
which expands to one entry per renderable `.qmd` file in a
directory.

## What this demonstrates

- **`auto:` expansion.** `auto: posts/` walks the `posts/` directory
  at render time and emits a sidebar entry for each `.qmd` it
  finds.
- **`order:` frontmatter.** A numeric `order:` in a page's
  frontmatter pins its position in the sidebar.
- **Title fallback.** Pages without `order:` sort after ordered
  pages by title (case-insensitive).
- **Draft exclusion.** `draft: true` in a page's frontmatter
  removes it from the sidebar (the page itself still renders).
- **Sections wrapping `auto:`.** A `section:` entry can hold an
  `auto:` directive in its `contents`, producing a labeled
  collapsible group.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/02-auto-sidebar
```

## What to look for

After rendering, sidebar entries on every page (e.g. `index.html`)
should appear in this exact order:

1. Getting Started   (order: 1)
2. Advanced Topics   (order: 2)
3. Aardvark          (no order; sorts alphabetically)
4. Zebra             (no order; sorts alphabetically)

The "Work in Progress" page is **not** listed in the sidebar, even
though `_site/posts/work-in-progress.html` does exist.

Confirm with:

```bash
grep menu-text _site/index.html
grep -c "Work in Progress" _site/index.html      # → 0
ls _site/posts/work-in-progress.html             # exists
```

## Try it

- Add a new file `posts/new-post.qmd` with a title and re-render.
  It should appear in the sidebar without you touching
  `_quarto.yml`.
- Toggle `draft: true` on `posts/getting-started.qmd` and re-render.
  Watch the sidebar shrink to three entries; all subsequent pages
  in the sidebar's prev/next strip shift forward.
- Remove the `order:` keys from both `getting-started.qmd` and
  `advanced-topics.qmd` and re-render. The sidebar order becomes
  fully alphabetical: Aardvark, Advanced Topics, Getting Started,
  Zebra.

## Notes

- `auto:` accepts several shapes: `auto: true` (whole project,
  excluding the top-level `index.qmd`), `auto: "posts"` (a single
  directory), `auto: ["posts", "tutorials"]` (a list of directories
  / globs). This example uses the directory form.
- A bare `contents: auto` (i.e. the literal string) is also
  accepted as Q1-idiomatic shorthand.
