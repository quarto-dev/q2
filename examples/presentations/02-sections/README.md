# 02-sections — Sections and slide breaks

How deck structure follows from heading levels, plus the horizontal-rule
slide break.

## What this demonstrates

- **Section slides.** A level-1 heading (`#`) starts a *section*: a
  horizontal stack whose level-2 slides become vertical children beneath it.
- **Slide breaks.** A horizontal rule (`---`) starts a new, untitled slide
  within the current section — useful for a slide with no heading.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/02-sections
```

## What to look for

- Two top-level section stacks (`Part One`, `Part Two`), each wrapping its
  level-2 slides as nested `<section>` elements.
- A trailing untitled slide produced by the `---` rule.
