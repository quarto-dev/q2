# 02-pages — Multi-page dashboards

Level-1 headings split a dashboard into pages.

## What this demonstrates

- **Pages.** Each `#` heading is a page, listed as a tab in the navigation bar.
- **Per-page options.** `# Support {orientation="columns"}` lays that page out
  in columns while the first page uses the default rows.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/02-pages
```

## What to look for

- "Sales" and "Support" tabs in the navigation bar switch pages.
- Opening `dashboard.html#support` shows the Support page first.
