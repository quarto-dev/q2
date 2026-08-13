# Investigation artifacts — bd-v9zs83zj

Repro for the missing `Footnotes` appendix heading. See
`../2026-08-12-footnotes-appendix-heading.md` for the analysis.

## Files

- `repro.qmd` — minimal document with two footnote definitions.
- `repro-observed-at-de2375f0.html` — the output at `main` @ `de2375f0`, kept as
  the pre-fix baseline.

## Reproduce

```bash
cargo run --bin q2 -- render repro.qmd --to html
```

## What to look for

Inside `<div id="quarto-appendix" class="default">`:

```html
<section id="footnotes" class="footnotes section" role="doc-endnotes">
<hr />
<ol type="1">
```

Quarto 1 emits, in place of that `<hr />`:

```html
<h2 class="anchored quarto-appendix-heading">Footnotes</h2>
```

Two differences, not one — the heading is absent **and** the `<hr>` is present.
Q1's `prependHeading` (`format-html-shared.ts:388-410`) removes the rule when it
inserts the heading. The strand description asserts the `<hr />` is correct; it is
not. See Finding 3 in the plan.
