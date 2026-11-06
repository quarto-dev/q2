# Jupyter record shadow fixtures (EXR-5)

Each `<case>.qmd` is rendered against a real Python kernel by
`tests/integration/jupyter_record_shadow.rs` (skipped with a message when no
`jupyter` runtime or Python kernelspec is on `PATH`). The image cases display a
one-pixel PNG through `IPython.display.Image`, so ipykernel is the only
requirement. The Jupyter engine builds an execution
record beside its live markdown; the test presents the record with the EXR-1
presenter and asserts the result equals the live markdown byte for byte,
including written figure file names and the `supporting_files` directory.

| Case | What it covers |
| --- | --- |
| `stdout` | a stream output |
| `plain-value` | an `execute_result` with `text/plain` |
| `html` | an `execute_result` with `text/html` |
| `png-plus-text` | a `display_data` with `image/png` and `text/plain`; figure file names |
| `error-allowed` | an error output with `error: true` |
| `echo-false`, `output-false`, `include-false` | visibility resolved at present time |
| `warning-false-stderr` | a stderr stream hidden by `warning: false` |
| `hidden-then-plot` | figure numbering keyed to executed-cell position |
| `markdown-only` | no cell at all: an empty record |
| `twenty-cells` | the size measurement of Phase 3 |
| `ceded-ojs` | a ceded `{ojs}` block is not a record cell |
