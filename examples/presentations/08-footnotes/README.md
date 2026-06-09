# 08-footnotes — Per-slide footnotes

Footnotes collected at the bottom of the slide that references them, numbered
per slide.

## What this demonstrates

- **Per-slide footnotes.** A footnote (`^[...]`) is gathered into an aside at
  the bottom of its own slide, rather than into one list at the end of the
  deck.
- **Per-slide numbering.** Each slide numbers its footnotes from one.
- **Coalescing with asides.** When a slide has both an `.aside` and footnotes,
  they share a single block at the bottom.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/08-footnotes
```

## What to look for

- Each slide's footnotes appear in an `<ol class="aside-footnotes">` inside the
  slide's bottom aside; there is no trailing footnotes slide.
- Footnote markers render as plain `<sup>` numbers that restart at `1` on each
  slide.

## Note

This example uses the inline footnote form (`^[footnote text]`). Named
reference footnotes (`[^id]` with a `::: ^id` block definition) do not resolve
yet — that is tracked separately.
