# 10-footer-logo — Footer and logo

Adding deck-level chrome: a footer line and a corner logo.

## What this demonstrates

- **Footer.** A `footer:` entry repeats one line at the bottom of every slide.
  Its content is Markdown, so links and emphasis render.
- **Logo.** A `logo:` entry pins an image to the corner of every slide.
- **Deck-level placement.** Both are set once in the front matter and apply to
  the whole deck.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/10-footer-logo
```

## What to look for

- The footer sits centered at the bottom of each slide, muted, with its link
  rendered as an anchor.
- The logo (`logo.svg`) sits in the bottom-right corner of each slide.
- Both are emitted once, outside the slide container, so reveal.js shows them
  fixed over every slide — Quarto 2 ships no reveal plugin, so there is no
  per-slide footer override yet.
