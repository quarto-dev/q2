# 03-value-boxes — Value boxes

Headline numbers above a chart.

## What this demonstrates

- **`.valuebox` divs.** The first paragraph is the title, the second the value.
- **`icon`** names a [Bootstrap Icons](https://icons.getbootstrap.com) icon.
- **`color`** takes a theme color (`primary`, `success`, `danger`, …) or any
  CSS color (`#6f42c1`).
- **`showcase-position`** moves the icon (`top-right` on the last box).

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/03-value-boxes
```

## What to look for

- A row of four colored boxes, each with an icon, sized to a quarter of the
  page height.

A value box can also come from a code cell with `content: valuebox`, printing a
list (R) or dict (Python) with `value`, `icon` and `color`; see the dashboard
docs (`docs/dashboards/index.qmd`).
