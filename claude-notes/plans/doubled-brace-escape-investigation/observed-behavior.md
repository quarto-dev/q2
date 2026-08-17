# Observed behavior at HEAD (60cc579e, 2026-08-17)

All four fixtures in this directory were run through
`cargo run --bin pampa -- -t native <file>` at main @ 60cc579e.

| Fixture | Input | Result |
| --- | --- | --- |
| `prose.qmd` | `X {{python}} Y` | **Bare parse error** — "unexpected character or token here" at the second `{` (1:4), no error code, no hint |
| `prose-single.qmd` | `X {python} Y` | **[Q-2-41]** "Curly braces are reserved for attribute syntax" with actionable hint (`\{...\}` escape, attribute syntax) |
| `fence.qmd` | ` ```{{python}} ` opener inside a displayed ` ````markdown ` fence | Parses fine; content kept **verbatim**: `CodeBlock ... "```{{python}}\n1 + 1\n```"` — the doubled braces are shown to the reader (Quarto 1 collapses to `{python}`) |
| `fence-single.qmd` | ` ```{python} ` opener inside a displayed ` ````markdown ` fence | Parses fine, verbatim: `CodeBlock ... "```{python}\n1 + 1\n```"` — already the desired display form |

## Mechanism notes

- Fence bodies are assembled verbatim from source bytes in
  `crates/pampa/src/pandoc/treesitter_utils/code_fence_content.rs`
  (`process_code_fence_content`) — it only splices around
  `block_continuation` markers and does no unescaping. This is why the
  doubled pair survives to output, and also why the single-brace form
  needs no escape at all.
- Prose parse errors are mapped to `Q-*` codes via the merr-style
  (state, sym) table: `crates/pampa/resources/error-corpus/_autogen-table.json`,
  looked up in `crates/pampa/src/readers/qmd_error_message_table.rs`.
  `Q-2-41.json` currently has two cases, failing at LR states 2613
  (bare paragraph) and 2589 (link text). The doubled-brace form errors
  at the *second* `{` — a different (state, sym) pair not in the table —
  so it falls through to the uncoded fallback in
  `crates/quarto-parse-errors/src/error_generation.rs:247`
  ("unexpected character or token here").
- Adding cases to `Q-2-41.json` (or a new sibling `Q-*.json`) and running
  `crates/pampa/scripts/build_error_table.ts` regenerates the table; the
  script runs the parser on each case and records the error state
  automatically (see `crates/pampa/CLAUDE.md` § Error messages).

## External repro

The strand's original repro lives outside this repo at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/escaped-executable-fence/`;
its `index.qmd` fence case is reproduced here as `fence.qmd`.
