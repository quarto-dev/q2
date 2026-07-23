# Jupyter engine: emit real figures for image outputs

**Braid strand:** bd-5t6wvu7m (pre-existing; carries the in-code TODO).
bd-rwz8kwia (filed from the bd-qbhp2cvv session) is a duplicate — closed
in favor of this one, evidence merged here.
**Status:** plan draft — awaiting review

## Overview

A minimal jupyter doc (python cell, `plt.plot([1,2,3])`) produces no
image in `q2 render` or `q2 preview` — the cell output is the text repr
(`[<matplotlib.lines.Line2D …>]` + `<Figure size 640x480 with 1 Axes>`).
knitr produces real figures; jupyter must reach parity. Once fixed, the
bd-qbhp2cvv capture transport carries jupyter figures into the preview
with no further work.

## Diagnosis (verified empirically, 2026-07-23)

**The kernel already sends the PNG.** Probing a bare `python3` kernel
via `jupyter_client` with the matplotlib fixture yields:

```
[('execute_result', ['text/plain']),
 ('display_data', ['image/png', 'text/plain'])]
```

No `MPLBACKEND` forcing or setup cell is needed for the basic case —
ipykernel's default `matplotlib_inline` integration ships the PNG.
q2 discards it in `format_outputs`
(`crates/quarto-core/src/engine/jupyter/text_execute.rs:427`), which has
two compounding bugs:

1. **MIME priority is inverted.** `text/plain` is checked *first*, so
   the display bundle (`image/png` + `text/plain` fallback) always
   takes the text branch. That's why we see `<Figure size 640x480…>`.
   Q1's `displayDataMimeType`
   (`external-sources/quarto-cli/src/core/jupyter/display-data.ts:45`)
   always puts `text/plain` **last**: for HTML targets the effective
   order is widgets/javascript/`text/html` → `text/markdown` →
   `image/svg+xml` → `image/png` → `image/jpeg` → … → `text/plain`.
2. **The image branch is a placeholder.** Even when reached, it emits
   the literal `[Image output]` (the `TODO(bd-5t6wvu7m)` in the code)
   instead of writing the bytes to a supporting file and emitting a
   figure.

Contrast with Q1's `mdImageOutput`
(`external-sources/quarto-cli/src/core/jupyter/jupyter.ts:1987`): base64-
decode (except literal `<svg`), write to
`<stem>_files/figure-<to>/<filename>.<ext>`, emit an image reference;
supporting files travel on the result.

And with q2's knitr engine, which already does this via the vendored
hooks (`crates/quarto-core/src/engine/knitr/mod.rs:214` reports
`supporting_files`); `JupyterEngine::intermediate_files` already
declares `<stem>_files/` as its output dir, so downstream cleanup
expects exactly this layout.

## Fix plan

All in `crates/quarto-core/src/engine/jupyter/text_execute.rs` (plus the
parity suite):

1. **MIME selection order** in `format_outputs`, per Q1's html-target
   priority, scoped to the types q2 handles today:
   `text/html` → `image/svg+xml` → `image/png` → `image/jpeg` →
   `text/plain` (last). (`text/markdown` and widget payloads remain
   out of scope; note as follow-up.)
2. **Real image emission.** Thread `ctx` (for `source_path`) and a
   per-document figure counter into `render_cell`/`format_outputs`:
   - target `<stem>_files/figure-html/cell-<cell#>-output-<out#>.<ext>`
     next to the document (Q1's naming shape; `figure-html` matches
     both Q1's `figuresDir("html")` and what knitr produces for our
     HTML/preview pipeline);
   - base64-decode PNG/JPEG (strip newlines first — nbformat multiline
     convention); write SVG as text when it starts with `<svg`;
   - emit `![](<stem>_files/figure-html/….png)` inside the
     `.cell-output-display` div;
   - collect the `<stem>_files` dir once into
     `ExecuteResult::with_supporting_files` (mirroring knitr's
     dir-level reporting, which bd-qbhp2cvv's `collect_capture_files`
     walks recursively).
3. **Parity suite**: extend
   `crates/quarto-core/tests/integration/engine_output_parity.rs` with
   a plot case (knitr `plot(1)` vs jupyter matplotlib) asserting both
   engines produce a `.cell-output-display` containing an image whose
   target lives under `<stem>_files/figure-html/`, and that both
   report non-empty `supporting_files`. (Requires R + jupyter — follow
   the suite's existing skip conventions.)

## Test plan (TDD)

- [ ] Unit (no kernel): `format_outputs` on a hand-built
      `KernelExecuteResult` whose display bundle has BOTH `image/png`
      and `text/plain` → writes the PNG file, emits `![](…)`, no
      `[Image output]`, no bare text repr. (Write first; fails today
      by taking the text branch.)
- [ ] Unit: `image/svg+xml` written as text with `.svg` extension.
- [ ] Unit: `text/html` still beats images (DataFrame-style bundles).
- [ ] Unit: multiline base64 (nbformat array-of-lines) decodes.
- [ ] Integration: parity plot case (above).
- [ ] E2E: `q2 render pyfig.qmd` → `<img src="pyfig_files/figure-html/…">`,
      file exists on disk, inspected.
- [ ] E2E: `q2 preview pyfig.qmd` → capture embeds the PNG
      (bd-qbhp2cvv transport), browser shows blob-URL image.

## Work items

- [x] Phase 1: failing unit tests + MIME priority fix (landed with
      Phase 2 — the signature change interlocks them)
- [x] Phase 2: image file emission + supporting_files (FigureWriter;
      commit 69546b4d)
- [x] Phase 3: parity suite plot case (commit 7527df29). Shape decision
      recorded there: jupyter figure divs converge on knitr's
      `cell-output-display`-only class (Q1's engines disagree on this;
      q2 resolves toward knitr).
- [x] Phase 4: E2E (render + preview) — record below

## End-to-end verification record (2026-07-23)

Exact invocations, observed output, output inspected — per the repo's
end-to-end policy. Fixture is the matplotlib doc from the Diagnosis
section, copied to a scratch dir.

**`q2 render` (previously: text repr only, no image):**

```bash
target/debug/q2 render pyfig.qmd
# output tree now contains:
#   pyfig_files/figure-html/cell-1-output-1.png   ← "PNG image data, 556 x 413"
# pyfig.html contains:
#   <img src="pyfig_files/figure-html/cell-1-output-1.png" alt="" />
# .quarto/render-manifest.json records pyfig_files as
#   {kind: Engine, engine: jupyter} — identical shape to knitr.
```

**`q2 preview` (previously: broken/no image):**

```bash
target/debug/q2 preview pyfig.qmd --no-browser
# session capture (gunzip captures/*.bin):
#   files: [{path: "pyfig_files/figure-html/cell-1-output-1.png", bytes(b64): 25512}]
#   result.markdown contains ![](pyfig_files/figure-html/cell-1-output-1.png)
#   supporting_files: [<abs>/pyfig_files]
# In Chrome (DevTools MCP): the rendered iframe contains
#   {"src": "blob:http://127.0.0.1:58943/…", "loaded": true, "w": 556, "h": 413}
# and the screenshot shows the actual matplotlib line plot. This is the
# bd-qbhp2cvv transport carrying a jupyter figure end to end, as
# predicted — no further preview work was needed.
```

Note: the unsuppressed `[<matplotlib.lines.Line2D …>]` execute_result
line above the figure is correct notebook semantics (the cell has no
trailing `;`), matching Q1/Jupyter behavior — not a bug.
