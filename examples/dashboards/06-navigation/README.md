# 06-navigation — The navigation bar

Everything a dashboard's navigation bar can show.

## What this demonstrates

- **Title block.** `title`, `subtitle` and `author` appear in the bar.
- **`logo`** (with `logo-alt`) before the title.
- **`nav-buttons`**: the `linkedin` share alias and a custom link with an icon.
- **Dark mode.** A `theme` with `light` and `dark` variants adds a toggle.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/06-navigation
```

## What to look for

- The toggle at the right of the bar switches between the cosmo and darkly
  themes.
- The LinkedIn button shares the page's URL.
