# 02-mermaid-revealjs — Mermaid diagrams on reveal.js slides

A `format: revealjs` deck where two of the three content slides carry a
Mermaid diagram.

## What this demonstrates

- **The same ` ```mermaid ` syntax works on slides.** A fenced diagram
  block is ordinary slide content; nothing changes between `format:
  html` and `format: revealjs`.
- **Diagrams on any slide.** Diagrams draw correctly even on slides
  that are not visible when the deck loads — advance to the last slide
  to see the state machine.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/diagrams/02-mermaid-revealjs
```

The deck is written next to the source as `slides.html`.

## What to look for

- A `<pre class="mermaid">` element inside each diagram slide's
  `<section>`.
- The mermaid loader script sits after the `Reveal.initialize` call,
  delivered through the deck's after-body include slot.
- In the browser, the flowchart is drawn on slide 2 and the state
  machine on slide 4, both at their natural size.
