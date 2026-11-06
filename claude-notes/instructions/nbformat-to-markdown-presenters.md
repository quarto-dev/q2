# Two nbformat-to-markdown presenters

Turning Jupyter-shaped cell outputs (nbformat mime bundles, streams, errors) into
Quarto cell markdown is implemented **twice**. They are not unified, on purpose, and
they can drift.

| | Rust presenter | TypeScript presenter |
| --- | --- | --- |
| Where | `crates/quarto-core/src/presenter/` (`cell.rs`, `figures.rs`, `visibility.rs`, `output.rs`) | `ts-packages/quarto-api/src/jupyter/to-markdown.ts` (with `display-data.ts`) |
| Used by | the live Jupyter engine, the stored-notebook replay (`IpynbReplayEngine`), and later record presentation | the Julia engine, through the Quarto API |
| Origin | q2-native, built around the `::: {.cell}` wrapper (`2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`) | a rewrite of Q1's `jupyterToMarkdown`, with listed simplifications (no ANSI colorization, no retina sizing, no slide delimiters) |
| Compiles for | native and `wasm32` | wherever the TS package runs |

## Rules

- **EXR-8 owns their parity.** Until it lands, do not assume the two produce identical
  markdown for the same notebook, and do not "fix" one to match the other as a drive-by.
- A change to what the Rust presenter emits is a behavior change. The goldens under
  `crates/quarto-core/tests/fixtures/presenter-goldens/` pin it, and must not be
  regenerated to make a refactor pass (`scripts/check-presenter-goldens-unchanged.sh`).
- A new output feature (for example `text/markdown`, `text/latex` or widget payloads, all
  silently dropped by the Rust presenter today) needs a decision about both presenters, not
  just the one you are editing.

See `claude-notes/plans/2026-10-28-exr-01-presenter-extraction.md` and the epic's EXR-8.
