# 11-responsive-navbar — Navbar text that changes with the window width

Demonstrates breakpoint-keyed navbar `text:`
(bd-navbar-responsive-text-am98rw95). Each item can show different text at
different window widths, so a long navbar keeps full labels on wide screens
and short ones where the expanded bar is cramped, without wrapping.

## What this demonstrates

- **A `text:` map keyed by breakpoint.** `xs`, `sm`, `md`, `lg`, `xl`,
  `xxl`. Each variant applies from its breakpoint up to the next key
  (mobile-first, like Bootstrap).
- **Full text in the collapsed menu.** Below `collapse-below: lg` the
  navbar collapses into a toggle menu with room for full labels, so most
  items give `xs` the full text, `lg` a short one, and `xl` the full text
  again.
- **The lowest key covers narrower widths.** The "API" item has only
  `lg` and `xxl`; `lg` also applies in the collapsed menu.
- **Dropdowns.** The "Community" toggle and the "About" entry inside its
  menu both use variants.
- **Icon-only ranges.** The GitHub item has `lg: ""`, so it shows only
  its icon between 992px and 1400px. Because some range has no text,
  the link gets `aria-label="GitHub"` automatically.

## How to run

```bash
cargo run --bin q2 -- render examples/websites/11-responsive-navbar
```

Open `_site/index.html` in a browser and resize the window across
992px, 1200px and 1400px.

## What to look for

| window width | navbar |
|---|---|
| below 992px | collapsed menu with full labels; "API" and "Community Resources" |
| 992px to 1200px | Start, Config, Tutorials, Releases, API, Community, GitHub icon |
| 1200px to 1400px | full labels except "API"; GitHub icon only |
| 1400px and wider | full labels, "API Reference", and "GitHub" with its icon |

At 1000px the full labels would wrap onto two lines; the short ones
fit on one.

Each variant is a `<span class="menu-text">` with Bootstrap display
classes:

```bash
grep 'menu-text' _site/index.html
```

The GitHub link carries the synthesized label:

```bash
grep 'aria-label="GitHub"' _site/index.html
```

## Try it

- Change `lg: Start` to `lgg: Start` and re-render. The render reports a
  `Q-13-18` warning that points at `lgg`, and "Getting Started" shows at
  every width.
- Remove the `xs:` keys and re-render. The collapsed menu now shows the
  short labels (the lowest remaining key covers it).
- Give the GitHub item its own `aria-label:`. It replaces the
  synthesized one.

## Notes

- Variants follow the window width, not the navbar's collapsed state;
  the `xs` key is how the collapsed menu gets its own text.
- Responsive text is a navbar feature. A map-valued `text:` on a
  sidebar or page-footer item shows only its widest variant.
- See `docs/guides/projects/navbar-responsive-text.qmd` for the full
  reference.
