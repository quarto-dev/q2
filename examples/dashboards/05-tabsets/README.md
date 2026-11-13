# 05-tabsets — Tabsets and card toolbars

Several cards in one, and extra content in a card's header.

## What this demonstrates

- **Tabsets.** `.tabset` on a row or column heading (here the `Charts`
  column) shows its cards as tabs inside one card; each card's title is its
  tab label.
- **Card toolbars.** A `.card-toolbar` div attaches to the card after it and
  sits in that card's header, after the title.

## How to run

```bash
cargo run --bin q2 -- render examples/dashboards/05-tabsets
```

## What to look for

- "Sales" and "Visits" tabs in the first card switch its content.
- The "Tickets by day" card's header shows its title and the toolbar text.
