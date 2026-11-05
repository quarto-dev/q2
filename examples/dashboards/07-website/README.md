# 07-website — A dashboard in a website

A website with an ordinary page and a dashboard page.

## What this demonstrates

- **Site chrome.** The dashboard keeps the site's navigation bar and footer;
  its own bar (title and page tabs) sits under the site's.
- **Theme.** The dashboard uses the site's `format: html` theme (`cosmo`)
  because it sets none of its own.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/07-website
```

Output is written to `examples/dashboards/07-website/_site/`.

## What to look for

- The "Dashboard" link in the site navbar opens the dashboard; its "Regions"
  and "Details" tabs switch pages.
- The site footer stays visible below the dashboard.
