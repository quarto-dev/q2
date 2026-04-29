# 04-navbar-footer — Navbar with dropdown, active highlight, page-footer

Demonstrates the two pieces of per-document chrome: the navbar at
the top of every page, and the page-footer at the bottom.

## What this demonstrates

- **Navbar configuration at top level.** `navbar:` lives at the
  top of `_quarto.yml`, not under `website.`. This makes single-doc
  HTML (e.g. a revealjs deck) able to declare a navbar without
  pretending to be a website.
- **Brand title fallback.** The navbar's brand label is
  `navbar.title ?? website.title ?? document.title`. This example
  sets `navbar.title: "Demo Site"`; remove it and the brand falls
  back to `website.title`.
- **Left and right item lists.** `navbar.left` and `navbar.right`
  each take a list of items.
- **Dropdown menus.** An item with a `menu:` array becomes a
  Bootstrap dropdown.
- **Active item highlighting.** The current page's navbar entry
  carries `class="nav-link active"`. Inside a dropdown, the active
  child gets `class="dropdown-item active"` instead.
- **Icon items.** A right-side GitHub link demonstrates `icon:`
  with an external `href:`.
- **`.qmd` → `.html` rewriting.** Items pointing at `.qmd` source
  paths are rewritten in the rendered HTML.
- **Page-footer regions.** `page-footer.left/center/right` each
  accept either a literal string or a list of navigation items.
- **No active marking on footer items.** Footer entries don't
  carry an active class — they're static cross-site chrome.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/04-navbar-footer
```

## What to look for

After rendering, inspect each page's HTML:

| Page | Expected active item |
|---|---|
| `_site/index.html` | "Home" in navbar |
| `_site/about.html` | "About" in navbar |
| `_site/tools/index.html` | "Overview" inside the Tools dropdown |
| `_site/tools/converter.html` | "Converter" inside the Tools dropdown |

Confirm with:

```bash
grep 'nav-link active' _site/index.html
grep 'dropdown-item active' _site/tools/converter.html
```

The navbar brand should read "Demo Site" everywhere:

```bash
grep 'navbar-brand' _site/index.html
```

The page-footer should appear on every page with three regions:

```bash
grep -A1 'nav-footer' _site/index.html | head -10
```

The center region's footer items should be rendered with `.html`
extensions:

```bash
grep 'href="about.html"' _site/index.html
```

## Try it

- Remove `navbar.title:` from `_quarto.yml` and re-render. The
  brand label should now read "Navbar &amp; Footer" (the
  `website.title`).
- Remove both and re-render. The brand label falls through to the
  current page's `title:` — which means the brand label changes per
  page. Counterintuitive, but documented.
- Set `page-footer: false` in a single page's frontmatter (e.g.
  `tools/converter.qmd`) and re-render. That page should render
  without a footer; the others keep theirs.

## Notes

- Navbar / dropdown / footer hrefs are emitted **page-relative**.
  From `_site/tools/converter.html`, the navbar "Home" link reads
  `../index.html` (walks up to the site root), and the dropdown
  "Overview" entry reads `index.html` (sibling within `tools/`).
  The output is portable across deployment roots.
