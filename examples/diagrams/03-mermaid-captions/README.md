# 03-mermaid-captions — Captions, cross-references, and alt text on diagrams

A `format: html` document with two Mermaid diagrams carrying `%%|` cell
options: one captioned, one captioned *and* labelled so it is numbered
and cross-referenceable.

## What this demonstrates

- **`%%|` cell options.** Diagram options go at the top of the block,
  one per line. The marker follows the cell language's own comment
  syntax — `%%` for Mermaid, the way `#|` serves Python and R.
- **`fig-cap` without a label.** The diagram is wrapped in a `<figure>`
  with a `<figcaption>` and no number.
- **`label` + `fig-cap`.** A `fig-`prefixed label makes the diagram a
  numbered float that `@fig-…` can reference.
- **`fig-alt`.** The description is carried into the diagram as
  Mermaid's own `accDescr:` directive, which becomes the drawing's
  accessible description.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/diagrams/03-mermaid-captions
```

The page is written next to the source as `document.html`.

## What to look for

- The first diagram inside `<div class="quarto-figure quarto-figure-center">`
  → `<figure>` → `<pre class="mermaid">` → `<figcaption>`, with no
  "Figure N:" prefix.
- The second inside `<div id="fig-review" class="quarto-float …">`, whose
  `<figcaption id="fig-review-caption">` reads "Figure 1: …", and the
  body text linking to it via `<a href="#fig-review" class="quarto-xref">Figure 1</a>`.
- `accDescr: A draft moves to review; …` as the second line of the
  second `<pre class="mermaid">` — after the `flowchart LR` declaration,
  which is where Mermaid requires it. When the page loads, Mermaid turns
  that line into a `<desc>` element inside the SVG and points the SVG's
  `aria-describedby` at it.
- No `%%|` lines anywhere in the output: the options are consumed, not
  passed through to the drawing.
