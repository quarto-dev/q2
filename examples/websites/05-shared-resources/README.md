# 05-shared-resources — `_site/site_libs/` deduplication

Demonstrates the **scoped artifact store**: shared assets like the
theme CSS are written once to `_site/site_libs/` and referenced
from every page with correctly relativized URLs.

## What this demonstrates

- **One shared theme CSS file**, regardless of page count.
- **Per-page relativization** of the `<link rel="stylesheet">`
  href: root-level pages link to `site_libs/...`, one-deep pages
  link to `../site_libs/...`, two-deep pages link to
  `../../site_libs/...`. Same file, different relative paths.
- **Content-addressable filenames.** The CSS file's name includes
  a hash derived from its compiled content. If the theme config
  doesn't change, the filename is stable across rebuilds and CDN
  caching is friction-free.
- **Project vs page scope.** Theme CSS is *project-scoped*; engine
  outputs (figures, plots) are *page-scoped*. The same machinery
  routes both — only the destination differs.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/05-shared-resources
```

## What to look for

After rendering, the output tree should look like:

```
_site/
├── index.html
├── docs/
│   ├── api.html
│   └── internals/
│       └── architecture.html
└── site_libs/
    └── quarto/
        └── quarto-theme-<hash>.css         # ONLY one
```

Confirm there's exactly one CSS file:

```bash
find _site -name '*.css'
```

The output should be a single line ending in
`quarto-theme-<hash>.css`.

Confirm the relative paths in each page's `<head>`:

```bash
grep 'rel="stylesheet"' _site/index.html
# href="site_libs/quarto/quarto-theme-<hash>.css"

grep 'rel="stylesheet"' _site/docs/api.html
# href="../site_libs/quarto/quarto-theme-<hash>.css"

grep 'rel="stylesheet"' _site/docs/internals/architecture.html
# href="../../site_libs/quarto/quarto-theme-<hash>.css"
```

## Try it

- Switch the theme by adding to `_quarto.yml`:
  ```yaml
  format:
    html:
      theme: cosmo
  ```
  Re-render. The `<hash>` in the filename should change. If you
  diff the previous and new file lists, the old file is gone and a
  new one with a different hash takes its place.
- Touch one page (`docs/api.qmd`) without changing the theme.
  Re-render. The CSS hash stays the same — the `<head>` `<link>`
  on every page references the unchanged file.

## Notes

- Page-scoped artifact directories (`_site/<stem>_files/`) are
  created next to each rendered page even when empty. Cleanup of
  empty ones is tracked as `bd-78ud`.
- The directory name `site_libs` is currently hard-coded.
  Override via a future `project.lib-dir:` (tracked as `bd-apvo`).
- Extension dependencies (CSS/JS contributed by Quarto extensions
  or Lua filters) flow through the same `Project`-scope path. A
  fixture exercising this end-to-end is tracked as `bd-b9za`.
