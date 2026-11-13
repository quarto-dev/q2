# 01-basic-layout — Rows, columns and cards

The smallest useful dashboard: two rows, the second split into columns, each
cell of the grid a card.

## What this demonstrates

- **`format: dashboard`** turns headings into a grid: each `##` heading starts
  a row and each `###` heading inside a row starts a column.
- **Sizing.** `{height=60%}` / `{width=40%}` on the headings share the space.
- **Cards.** A `.card` div (with a `title`) holds any markdown: an image, a
  table, text.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/01-basic-layout
```

## What to look for

- The page fills the browser window; resizing it resizes the cards.
- Hovering a card shows an expand button that opens it full screen.
