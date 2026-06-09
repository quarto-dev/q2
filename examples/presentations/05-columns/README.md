# 05-columns — Side-by-side columns

Laying slide content out in columns with the `.columns` / `.column` classes.

## What this demonstrates

- **Column layout.** A `.columns` container arranges its `.column` children in
  a row.
- **Column widths.** Each column's `width` attribute sets its share of the
  row.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/05-columns
```

## What to look for

- The `.columns` div is a flex row; each `.column`'s `width="40%"` /
  `width="60%"` becomes an inline `flex-basis` so the two sit side by side.
