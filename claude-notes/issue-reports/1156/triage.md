# Issue #1156 — `output: asis` is ignored for Jupyter cells

- **GitHub**: https://github.com/quarto-dev/q2/issues/1156
- **Reporter**: @sarahsdao (Sarah Sdao), 2026-11-10
- **Triage date**: 2026-11-10
- **Branch**: `issue-1156` (topic branch in the main checkout, at the user's request; no worktree), based on `main` @ `a05b4b141`
- **Braid strand**: bd-2evi7xlp
- **Scope**: the entire issue, which makes one report: Jupyter `output: asis`, set on the cell or at document level (`execute: output: asis`). knitr is out of scope because it already works (see below).

## Summary

q2 writes the stdout of a Jupyter `#| output: asis` cell as a fenced code block in
`.cell` / `.cell-output-stdout` divs. Quarto 1 writes it as raw markdown, so it renders as
headings, callouts and tabsets. The bug reproduces on `main`. It is a real bug: the shared
presenter only has knitr's `results = "asis"` mode, and the Jupyter path never maps
`output: asis` to any asis behavior. The fix is moderate rather than a one-liner, because Quarto 1's
Jupyter asis semantics differ from knitr's `results = "asis"` in several output kinds, and the
user wants to match Quarto 1.

## Reproduction

Fixtures are in this directory:

- `repro.qmd`: the reporter's example.
- `exp-variants.qmd` covers more output kinds: stdout plus an `execute_result`, stderr plus an
  IPython `Markdown` display, `echo: true`, a `classes:` option that forces the cell div, and a
  tabset (the reporter's real use case).
- `exp-doc-level.qmd` sets `execute: output: asis` in the document and overrides it with
  `output: true` on one cell.
- `*.q1.md` holds the intermediate markdown from Quarto 1 for each fixture
  (`quarto render X.qmd --to html -M keep-md:true`, Quarto dev build `99.9.9`). These are the
  expected outputs.

q2 (`target/debug/q2 render repro.qmd --to html`, main @ `a05b4b141`):

```html
<div class="cell">
<div class="cell-output cell-output-stdout">
<div class="code-copy-outer-scaffold">
<pre class="code-with-copy"><code>## Generated heading
...
```

Quarto 1 (`repro.q1.md`): the stdout text is spliced in as is: `## Generated heading`, then
`Some **bold** text.`, then the callout. There is no cell div and no output div.

## Localization

- `crates/quarto-core/src/presenter/options.rs:489`: `results: first("results")`. This is
  the only asis switch. `CellOptions::from_config` (the Jupyter path, `options.rs:291`)
  reads `output` only as a visibility bool (`visibility.rs:99`, `resolved_flag`, which ignores a
  string value). `output: "asis"` therefore means "show output" and nothing more.
- `options.rs:806-815`: `results_asis()` and `needs_cell_div()` (hooks.R `needCell`).
- `crates/quarto-core/src/presenter/cell.rs:766-985`: `format_outputs_with` handles the
  per-output asis cases (`results_asis` branches at lines 740, 808 and 848).
- `cell.rs:399` `wrap_cell`: the `.cell` div is dropped when `needs_cell_div()` is false.
- knitr already works. The vendored `crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R`
  (`opts_hooks[["output"]]`) maps `output: asis` to `results = "asis"` on the R side, so knitr
  records arrive with `results: asis` set.
- Every presenter entry point goes through `render_cell_with` / `format_outputs_with`:
  `presenter/document.rs:207`, `presenter/expansion.rs:83,124`,
  `stage/stages/record_splice.rs:355`. One fix there covers `.qmd`, `.ipynb`, expansions and
  record replay.
- Quarto 1 reference: `external-sources/quarto-cli/src/core/jupyter/jupyter.ts:1340-1346`
  (definition of `asis`: the cell option, or a document `execute.output` with no cell
  override), `:1586` and `:1718` (no `.cell-output` div per output), `:1679` (raw stdout text),
  and `:1738` (cell div only when `needCell`).

## Open questions resolved during triage

1. **Can we set `results = "asis"` when `output == "asis"` and reuse the knitr mode?** No,
   not if we want to match Quarto 1. A comparison of `exp-variants.q1.md` with the knitr mode in
   `format_outputs_with` shows these differences:

   | output kind | Q1 Jupyter `output: asis` | q2 knitr `results = "asis"` |
   |---|---|---|
   | stdout stream | raw text, no div | raw text, no div (same) |
   | `execute_result` with only `text/plain` (`'a trailing value'`) | **fenced code block**, no div | raw text, written as markdown |
   | stderr stream | fenced block, no div | fenced block, no div (`fenced_output_div` returns a bare fence under asis) (same) |
   | `text/markdown` display (`IPython.display.Markdown`) | raw markdown, **no div** | inside a `.cell-output-display .cell-output-markdown` div |
   | `text/html` display | `{=html}` raw block, no div (read from `jupyter.ts:1586`/`1718`, not rendered) | inside a `.cell-output-display` div |
   | image display | figure markdown, no output div (read from code, not rendered) | inside a `.cell-output-display` div, unless it is a linked image |
   | error | fenced block, no div | bare fence (same) |
   | `.cell` div | dropped unless `needCell` | dropped unless `needs_cell_div()` (same idea) |

   The fix therefore needs its own Jupyter asis mode. The rule is simple: drop the
   per-output `::: {.cell-output ...}` wrapper for every output, and write stdout text raw.
   Everything else renders as it does without asis.

2. **Does the document-level `execute: output: asis` reach the presenter?** Yes.
   `resolve_cell_options` (`visibility.rs:21`) merges the cell's `#|` map over the document
   `execute` scope, so `from_config` sees `output: asis`. A cell `output: true` overrides it,
   which matches Quarto 1 (`exp-doc-level.q1.md`).

3. **Is knitr affected?** No. See Localization (hooks.R maps it on the R side).

4. **Is there a warning?** No. q2 currently accepts `output: asis` silently because
   `resolved_flag` falls back to `true` for a non-bool. After the fix, the value is meaningful.

## Outcome / recommended next step

Filed bd-2evi7xlp. Fix scope:

1. `CellOptions`: read `output == "asis"` (string value, from the resolved Jupyter options in
   `from_config`) into a new field, for example `output_asis: bool`. Generalize
   `needs_cell_div()` to "neither asis mode, or attributes/caption need the div".
2. `format_outputs_with`, under `output_asis`:
   - stdout `Stream`: push the text raw (Q1 joins the stream text without trimming; keep a
     leading/trailing newline so blocks separate).
   - every other output: render exactly as today, minus the outer
     `::: {.cell-output ...}` / `::: {.cell-output-display}` wrapper. The fence or the markdown
     stays as it is: a text/plain `execute_result` stays a fenced block, `text/markdown` is
     written raw, HTML is a raw `{=html}` block, and an image is bare figure markdown.
     Factoring the wrapper out of `fenced_output_div` and `push_figure` will help here.
3. `needCell` parity: Q1 also keeps the cell div for a label on the cell container
   (`label && labelCellContainer`). Check that q2's `needs_cell_div` keeps the div when
   `cell_div_id()` is non-empty under asis, and add a test case. Derive the expected output
   from `shouldLabelCellContainer` (`jupyter.ts:1364`), or from a Quarto 1 render of a
   labelled asis cell.
4. Tests, written first: unit tests in `presenter/cell.rs` next to the existing
   `results_asis_*` tests (around `cell.rs:2806`), one per row of the table above. Also add an
   integration test that renders `repro.qmd`-equivalent Jupyter outputs end to end (stored
   outputs or a recorded fixture, so CI needs no live kernel) and asserts an `<h2>` and a
   callout in the HTML.
5. Confirm with `exp-variants.qmd` / `exp-doc-level.qmd` that q2's intermediate markdown
   matches the `*.q1.md` files, up to cosmetic whitespace and Q1's `execution_count` / cell ids.

This is more than an `autonomously-fix-small-issue` change: it adds a new presenter mode across
several output kinds, not a one-line mapping.

## Verification commands used

```bash
gh issue view 1156 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build                      # pre-flight on main @ a05b4b141
#   → failed only on engine::knitr::capture_tests::capture_classifies_pieces
#     (rlang_error vs simpleError; local R package skew, tracked as
#     bd-54odm3rz / bd-qwscag8b; unrelated to this issue). Remainder:
cargo nextest run --workspace --no-fail-fast -E 'not test(capture_classifies_pieces)'
#   → 19220 passed, 202 skipped
# Quarto 1 expected outputs (scratch copy of the fixtures):
quarto render repro.qmd --to html -M keep-md:true        # → repro.html.md, saved as repro.q1.md
quarto render exp-variants.qmd --to html -M keep-md:true
quarto render exp-doc-level.qmd --to html -M keep-md:true
# q2 actual:
target/debug/q2 render repro.qmd --to html               # stdout in .cell-output-stdout <pre>
```

## Cross-references

- `crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R`: knitr's `output` opts hook.
- `crates/quarto-core/src/crossref/index.rs:56`: `crossref.ids` manifest for ids that
  `output: asis` blocks produce, which becomes useful once Jupyter asis works.
- `crates/quarto-core/tests/fixtures/dashboard-goldens/contract/c14-hidden-empty-asis-cells.qmd`:
  a dashboard contract that asis cells do not become cards. Re-run the dashboard goldens after
  the fix.

## Implementation follow-up (2026-11-11)

- Step 3 (`needCell`): a `tbl-` label keeps the `.cell` div under Jupyter asis (Q1 writes
  `::: {#tbl-gen .cell tbl-cap=…}` around the generated table, and q2 now does too). A plain
  label (`#| label: my-cell`) does not, because q2 never writes plain labels as the cell div's
  id, asis or not. That is a separate gap, filed as bd-gyzi1s9e.
- Case E of `exp-variants.qmd` originally printed fenced ```` ```bash ```` blocks into
  the tabset. q2 never executed that cell, a separate bug filed as bd-f63cqoke (repro:
  `repro-triple-backtick-in-cell.qmd`). The qmd writer re-serializes the cell with a 4-backtick fence,
  and the Jupyter executor's `runs_in_legacy_executor` only runs 3-backtick cells. Case E now
  uses a tabset without code fences, and `exp-variants.q1.md` was regenerated.
- With the fix, q2 renders every fixture here like the matching `*.q1.md`.
