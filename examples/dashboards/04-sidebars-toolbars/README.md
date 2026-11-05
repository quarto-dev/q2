# 04-sidebars-toolbars — Sidebars and toolbars

Panels beside or above the page content.

## What this demonstrates

- **Sidebar.** A section whose heading has the `.sidebar` class (here with
  `width="260px"`) becomes a collapsible sidebar. Its headings are styled as
  small headings, not as new sections.
- **Toolbar.** A `.toolbar` section is a strip above the rest of the page.
- Both are per page here; a level-1 `.sidebar` heading before the pages would
  be shared by every page.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/04-sidebars-toolbars
```

## What to look for

- On the Sidebar page, the arrow on the sidebar's edge collapses it and the
  card widens.
- On the Toolbar page, the toolbar stays above the chart.
