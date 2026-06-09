# Reveal.js presentation examples

Minimal, self-contained Quarto 2 presentation projects, one per authoring
feature. Each is a `type: default` project: a `_quarto.yml`, a single
`format: revealjs` `slides.qmd`, and a `README.md`.

These back the reveal.js documentation page
(`docs/presentations/revealjs/index.qmd`): each feature section there links to
the matching example here.

## Running an example

From the repository root, build `q2` once:

```bash
cargo build --bin q2
```

Then render any example (output is written in place as `slides.html`):

```bash
cargo run --bin q2 -- render examples/presentations/03-fragments
```

## Examples

| Example | Demonstrates |
|---|---|
| [`01-creating-slides`](01-creating-slides/) | `format: revealjs`, `##` slides, automatic title slide |
| [`02-sections`](02-sections/) | `#` section stacks and `---` slide breaks |
| [`03-fragments`](03-fragments/) | `.fragment` reveal, variant effects, `fragment-index` |
| [`04-incremental-lists`](04-incremental-lists/) | global `incremental`, `.incremental` / `.nonincremental` |
| [`05-columns`](05-columns/) | `.columns` / `.column width` layout |
| [`06-speaker-notes`](06-speaker-notes/) | `.notes` speaker notes |
| [`07-asides`](07-asides/) | `.aside` peripheral commentary |
| [`08-footnotes`](08-footnotes/) | per-slide footnote coalescing |
