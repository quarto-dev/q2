# 11-auto-stretch — Auto-stretch single-image slides

Sizing a lone slide image to fill the available space.

## What this demonstrates

- **Auto-stretch.** A slide whose only content is one image gets reveal's
  `.r-stretch` class automatically, so the image fills the slide instead of
  overflowing. This is on by default.
- **Opting out.** `auto-stretch: false` (deck-wide), an explicit
  `width`/`height` on the image, or a `.nostretch` class each leave the image
  at its natural size.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/11-auto-stretch
```

## What to look for

- On the first slide, the lone image carries `class="r-stretch"` and reveal
  sizes it to fill the slide.
- On the second slide, the `.nostretch` image keeps its natural size.
- A slide that mixes an image with other content, or holds more than one image,
  is left untouched.
