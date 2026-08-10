# Investigation artifacts — bd-mermaid-cell-options-9wo3crl0

Probes used for the findings in
`claude-notes/plans/2026-08-10-mermaid-cell-options.md`. Render each with
`cargo run --bin q2 -- render claude-notes/plans/mermaid-cell-options-investigation/<file>.qmd`
from the repo root, then read the generated HTML. (The `.html`/`_files`
outputs are not committed.)

- `repro.qmd` — copy of the strand's three-section probe (from the
  local-only `q2-connect-docs` repo). A: GFM fence works; B: `%%|`
  options survive verbatim; C: `{mermaid}` brace form unrecognized.
- `probe.qmd` — marker-prefix probe. B1 `%%|` without label, B2 `%%|`
  with label, **B3 `#|` with label (works fully today)**, B4 non-leading
  `%%|` (correctly left as a comment).
- `probe2.qmd` — caption fidelity on the working `#|` path: C1 unquoted
  caption (good), C2 quoted caption (quotes leak into the figcaption) +
  `fig-alt` (silently dropped), C3 markdown caption (not parsed), C4
  `fig-cap` with no label (nothing happens).
- `probe3.qmd` — the same caption defects on a plain ```python``` cell,
  establishing that they are general `#|`-shorthand bugs and not
  mermaid-specific.
