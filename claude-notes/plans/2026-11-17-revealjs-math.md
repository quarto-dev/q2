# RevealJS math: load MathJax / KaTeX in decks

## Overview

A q2 RevealJS deck loaded no math engine, so `\(…\)` and `\[…\]` stayed raw.
`MathJsStage` runs for decks and fills `meta.math` with the engine's
configuration and loader markup, and the HTML templates print it through
`$math$`. The reveal scaffold (`render_revealjs_document` in
`crates/quarto-core/src/revealjs/assemble.rs`) writes its own `<head>` and
never printed that slot. The fix prints `meta.math` in the deck `<head>`, after
the stylesheets and before the header includes, which is where Quarto 1's
reveal template prints `$math$`.

`html-math-method: mathml` already worked: `MathMlStage` converts the math in
the AST and needs no loader.

## Checklist

### Phase 0: inputs

- [x] `meta.math` is set for a deck with math, including math that is only in
  a callout, a table cell or a speaker note (integration tests below pass only
  if it is).
- [x] `plain`, `webtex` and `gladtex` leave `meta.math` unset, as in HTML
  (`unsupported_math_methods_load_no_engine`).

### Phase 1: the fix (tests first)

- [x] Unit tests in `assemble.rs`: the slot is printed once, after the
  stylesheets, inside `<head>`, before `reveal.js`; it precedes the header
  includes; without `meta.math` nothing is added. Three of them failed before
  the fix as expected (the "nothing added" one passes on both sides).
- [x] Integration tests in `tests/integration/revealjs_features.rs`: MathJax,
  KaTeX, no math, unsupported methods, MathML, and math only in a callout /
  table / note. The four that need the loader failed before the fix.
- [x] Smoke-all fixture `crates/quarto/tests/smoke-all/revealjs/math.qmd`.
- [x] Print `meta.math` in the scaffold.
- [x] `Reveal.layout()` after typesetting: measured, changes nothing, not
  added (see Details).

### Verification

- [x] Browser check against Quarto 1.9.38 at 1400 by 900 (numbers in Details).
- [x] `cargo xtask verify`.

## Details

### Decisions

- **MathJax 3 at page level**, the same markup as the HTML format, not Quarto
  1's RevealMath plugin with MathJax 2. One code path for all HTML formats.
  MathJax 3 typesets hidden slides and fragments on load, so no per-slide
  re-typeset is needed. Cost: MathJax 3 output is a little larger than MathJax
  2 output (numbers below), the same difference q2 HTML pages already have.
- **No `Reveal.layout()` script.** Reveal's stretch and fit-text sizes already
  reflect the typeset math.

### End-to-end check

`cargo run --bin q2 -- render deck.qmd` (the six-slide fixture with inline,
labelled display, columns, fragment and far-slide math). Inspected `<head>`:

```html
<link rel="stylesheet" href="deck_files/revealjs/quarto-reveal.css">
<script>
window.MathJax = {
  tex: {
    inlineMath: [['\\(', '\\)']],
    ...
</script>
<script defer src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml-full.js" type="text/javascript"></script>
</head>
```

Headless Chromium (Playwright) at 1400 by 900, served with
`python3 -m http.server` on 127.0.0.1, waiting for `MathJax.startup.promise`
(or the KaTeX / MathJax 2 equivalent) before measuring. Sizes are width by
height in screen pixels of the typeset element.

| Expression | q2 MathJax 3 | Q1 MathJax 2 | q2 KaTeX | Q1 KaTeX | q2 MathML | Q1 MathML |
|---|---|---|---|---|---|---|
| inline `e^{iπ}+1=0` | 261 x 70 | 228 x 60 | 266 x 66 | 265 x 66 | 204 x 44 | 204 x 44 |
| display, labelled (1) | 1218 x 136 | 1215 x 133 | 1218 x 139 | 1215 x 139 | 1120 x 79 | 1215 x 79 |
| inline in column | 288 x 70 | 256 x 60 | 289 x 66 | 288 x 66 | 224 x 44 | 244 x 44 |
| inline in fragment | 74 x 70 | 64 x 60 | 75 x 66 | 75 x 66 | 49 x 46 | 49 x 46 |
| matrix, far slide | 1218 x 126 | 1215 x 124 | 1218 x 135 | 1215 x 134 | 1218 x 105 | 1215 x 105 |

All expressions typeset in every engine, including the fragment that is not
yet shown and the far slide; the labelled equation shows "(1)"; no console
errors. A forced `MathJax.startup.document.rerender()` gives the same sizes, so
the MathJax 3 sizes are not a font-loading race. The MathML differences come
from the MathML converter, not from this change.

`Reveal.layout()` probe (deck opened directly on each slide by URL hash, so
reveal's own layout on navigation does not hide a stale first layout):

| Slide | q2 MathJax | +layout | Q1 MathJax 2 | q2 KaTeX | +layout | Q1 KaTeX |
|---|---|---|---|---|---|---|
| `.r-stretch` image after a 3-row matrix (height) | 345 | 345 | 363 | 368 | 368 | 367 |
| `{.center}` slide with two displays (section top) | 53px | 53px | 65.5px | 64px | 64px | 64px |
| `.r-fit-text` with inline math (font size) | 160.2px | 160.2px | 170.7px | 181.6px | 181.6px | 197.0px |

### Other checks

- **Math in a callout, a table, a speaker note and code-cell output**
  (Python `output: asis`): all typeset in MathJax and KaTeX. Callout math is
  larger than in Quarto 1 because q2's callout body text is 40px against Quarto
  1's 28px; that is a callout styling gap, not math.
- **`?print-pdf`**: all 5 expressions typeset in both engines; KaTeX sizes are
  identical to Quarto 1. Chromium's PDF export embeds the MathJax fonts.
- **`embed-resources: true`**: q2 does not implement it yet for HTML or
  RevealJS (assets stay linked). Quarto 1 also loads MathJax from the CDN under
  `embed-resources`, for both formats. The deck does the same as an HTML page.
- **Speaker view**: q2 ships no reveal notes plugin, so there is no speaker
  view. Math in `.notes` is typeset in the (hidden) aside.
- **Preview**: `q2 preview` / QuartoHub draw slides with `RevealDeck`, which
  uses the shared `previewRegistry`, whose `Math` component renders KaTeX
  directly (`math-js` is excluded there). No change needed.
  Checked in a browser with `q2 preview` built from this branch: all
  expressions typeset with KaTeX, whatever `html-math-method` says.
- **Preview gap found, not fixed here:** on the slide preview, a labelled
  equation shows no "(1)" and `@eq-one` renders as empty text. An HTML page in
  the same preview and `q2 render` of the deck both show "(1)" and
  "Equation 1".

### Verification results

- `cargo nextest run --workspace --no-fail-fast`: 20255 of 20257 pass. The two
  failures are local missing packages, unrelated: `smoke_all` (9 typst
  fixtures need Python `great_tables` and R `gt`/`flextable`) and
  `dashboard_skeletons_match_goldens` (R `bsicons`).
- `cargo xtask verify --skip-rust-tests`: all steps pass.
