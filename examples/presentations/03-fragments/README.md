# 03-fragments — Incremental reveal of elements

Revealing pieces of a slide one click at a time with the `.fragment` class.

## What this demonstrates

- **Fragments.** A `.fragment` div appears on the next click instead of with
  the rest of the slide.
- **Fragment variants.** Effect classes such as `.fade-up` and
  `.highlight-red` change how a fragment animates in.
- **Fragment order.** `fragment-index` sets the reveal order explicitly,
  independent of source position.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/03-fragments
```

## What to look for

- Each fragment renders as `<div class="fragment ...">` — reveal.js shows
  these on successive clicks.
- The `fragment-index` values become `data-fragment-index` attributes, so the
  second-written fragment reveals first.
