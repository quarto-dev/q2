# 01-creating-slides — A minimal reveal.js deck

The smallest `format: revealjs` deck: a title slide drawn from the document
metadata, followed by two content slides.

## What this demonstrates

- **`format: revealjs`.** A single `.qmd` whose front matter selects the
  reveal.js output format renders to a self-contained slide deck.
- **Slides from headings.** Each level-2 heading (`##`) starts a new slide.
- **An automatic title slide.** The `title:` and `author:` metadata produce
  an opening `<section id="title-slide">` with no extra markup.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/presentations/01-creating-slides
```

The deck is written next to the source as `slides.html`.

## What to look for

- The output is a reveal.js deck: a `<div class="reveal"><div class="slides">`
  wrapper and a `Reveal.initialize` call (not the Bootstrap article layout of
  `format: html`).
- The first `<section>` is the generated `title-slide`; the next two are the
  `Getting up` and `Going to sleep` slides.
