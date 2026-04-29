# 03-nested-sidebar — Multiple sidebars, nested sections, prev/next

Demonstrates two distinct sidebars that swap as the reader moves
between subtrees, plus the prev/next strip at the bottom of each
page.

## What this demonstrates

- **Multiple sidebars.** `_quarto.yml` declares two sidebar
  objects, each with its own `id` and `title`. A page renders with
  whichever sidebar contains its source path.
- **Containment-based selection.** No per-page configuration —
  `guide/*.qmd` automatically gets the User Guide sidebar;
  `reference/*.qmd` automatically gets the Reference sidebar; the
  top-level `index.qmd` gets neither (it's not in either sidebar's
  contents).
- **Nested sections.** The User Guide sidebar groups pages under
  *Getting Started* and *Advanced* via `section:` entries. The
  active page's ancestor sections are marked `expanded: true`.
- **Prev / next strip.** Every page in a sidebar gets a
  `pagination-link` strip at the bottom, derived from the
  flattened sidebar order.
- **Per-page page-navigation override.** `guide/tuning.qmd` sets
  `page-navigation: false` in its frontmatter, suppressing the
  strip on that page only.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/03-nested-sidebar
```

## What to look for

After rendering, open these pages in a browser (or grep them) and
confirm:

| Page | Sidebar applied |
|---|---|
| `_site/index.html` | None (page not in any sidebar) |
| `_site/guide/index.html` | User Guide |
| `_site/guide/installation.html` | User Guide; "Installation" active |
| `_site/reference/api.html` | Reference; "API" active |
| `_site/reference/cli.html` | Reference; "CLI" active |

The flattened User Guide order is:

1. User Guide (the section landing page)
2. Installation
3. First Steps
4. Tuning

So `installation.html` should have prev = "User Guide", next =
"First Steps". `first-steps.html` should have prev =
"Installation", next = "Tuning" (crossing the section boundary).
`tuning.html` should have **no** pagination strip at all because
of its `page-navigation: false` frontmatter.

Confirm with:

```bash
grep -c "pagination-link" _site/guide/installation.html   # → 2
grep -c "pagination-link" _site/guide/tuning.html         # → 0
grep "sidebar-link active" _site/reference/api.html
```

## Notes

- Sidebar / navbar / footer hrefs are emitted **page-relative**:
  inside `_site/guide/installation.html`, a sibling sidebar link
  reads `installation.html` (not `guide/installation.html`); a
  cross-subtree link to `reference/api.qmd` reads `../reference/api.html`.
  This makes the rendered output portable across deployment roots
  (any subdirectory or top-level).
