# Jupyter inline-expression fixtures (EXR-3 Phase 4)

Each `<case>.qmd` was rendered with Q1 (quarto-cli, `format: html`, a Positron venv with
ipykernel as `QUARTO_PYTHON`) on 2026-10-31. `<case>.q1.html` holds the lines of Q1's output
that carry the span (`<p>Before|Outside ...`, plus the code line for `r-fence`). CI renders
the same document with q2 against a real kernel (`jupyter_inline_expressions.rs`,
`q1_equivalence_fixtures`) and compares q2 only; it never runs Q1. The expectation is
`<case>.q2.html` when that file exists, otherwise `<case>.q1.html`.

| Case | Q1 vs q2 |
| --- | --- |
| `none`, `markdown`, `html`, `plain-value`, `string-value` | identical |
| `eval-false` | identical: a span ignores `execute: eval: false` in both |
| `empty-string` | Both show the span source. Only the text differs: q2's writer escapes the quotes inside `<code>` as `&quot;`, Q1 leaves them raw. Same rendered page. |
| `r-fence` | **Intentional difference.** Q1 evaluates the span inside the `{r}` fence (`# a 2 span`). q2 leaves it as literal text, since spans live in prose. The span outside the fence matches. |
| `statement` | Both fail. Q1: `SyntaxError: invalid syntax` (`statement.q1.txt`). q2: error `Q-18-10` naming the span, with the same `SyntaxError`. |

Not here: the "span with no usable result" case. A real ipykernel never returns an
image-only value (IPython always adds `text/plain`), so Q1 cannot be made to produce it.
q2 leaves the span source and warns; `no_usable_result_warns_and_leaves_the_span_source`
(fake kernel, `engine/jupyter/text_execute.rs`) covers it.
