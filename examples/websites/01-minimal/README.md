# 01-minimal — Smallest possible website project

Two pages, a manually configured sidebar, and a cross-document link.
This is the smallest project that exercises the website pipeline
end to end.

## What this demonstrates

- **Multi-page rendering.** A directory containing `_quarto.yml` is
  treated as a project. All `.qmd` files render through the
  two-pass project pipeline.
- **Manual sidebar.** `website.sidebar.contents` is a list of bare
  `.qmd` paths. Each entry's visible text comes from the target
  page's `title:` frontmatter.
- **Active item highlight.** The sidebar entry for the current page
  carries a `.active` class.
- **Cross-document link rewriting.** `[About](about.qmd)` in the
  body is rewritten to `<a href="about.html">` in the rendered
  output.
- **Per-project shared theme CSS.** Both pages share a single
  `_site/site_libs/quarto/quarto-theme-<hash>.css` — there is no
  per-page copy.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/websites/01-minimal
```

Output is written to `examples/websites/01-minimal/_site/`.

## What to look for

After rendering, the output tree looks like:

```
_site/
├── about.html
├── index.html
└── site_libs/
    └── quarto/
        └── quarto-theme-<hash>.css
```

Each rendered page should contain:

1. **A sidebar** with two entries, "Home" and "About". On
   `index.html` the "Home" entry has class `active`; on
   `about.html` the "About" entry has it.

   Confirm with:
   ```bash
   grep 'sidebar-link active' _site/index.html
   grep 'sidebar-link active' _site/about.html
   ```

2. **A rewritten body link.** The source has
   `[About page](about.qmd)`; the rendered HTML has
   `<a href="about.html">About page</a>`.

   Confirm with:
   ```bash
   grep 'href="about.html"' _site/index.html
   ```

3. **A title prefix.** The `<title>` tag reads
   `Home – Minimal Website` (page title — site title) without any
   per-page configuration. The site title comes from
   `website.title`.

4. **A shared theme CSS reference.** The `<head>` of every page
   includes a `<link rel="stylesheet">` pointing at
   `site_libs/quarto/quarto-theme-<hash>.css`.

## Things you may notice

- Empty `_site/<stem>_files/` directories appear next to each
  rendered page. They are reserved for per-page artifacts (engine
  outputs, figures), and for an all-prose document like this one,
  they end up empty. Cleanup is tracked as a follow-up
  (`bd-78ud`) and is harmless for now.
- The pages also include a small **prev / next** strip at the
  bottom, even though this example doesn't configure one. That's
  page-navigation derived automatically from sidebar position.
  Project `03-nested-sidebar` exercises it explicitly.
