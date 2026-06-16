# 09-themes — Built-in themes

Restyling a deck with the `theme:` option.

## What this demonstrates

- **Theme selection.** Setting `theme: dark` in the `revealjs` front matter
  swaps the whole deck onto the built-in dark theme.
- **Built-in themes and aliases.** Quarto ships the reveal.js theme set;
  `white` aliases to the `default` theme and `black` aliases to `dark`.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/09-themes
```

## What to look for

- The deck renders with a dark background and light text rather than the white
  default — the theme drives the colors, fonts, and link styling.
- Omitting `theme:` would render the `default` theme instead.
