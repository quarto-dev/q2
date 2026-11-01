# Jupyter markdown goldens (EXR-6)

The markdown the Jupyter engine and the stored-notebook replay produced **before the
flip** (EXR-6 Phase 0), frozen here as the oracle for everything after it: once
`present_document` replaces the two splice loops, these files are what "byte-identical"
means. They are never regenerated in CI and otherwise never regenerated at all (epic
decision 28); a golden that has to change is a behavior change, not a refactor, so stop
and raise it.

Driven by `tests/integration/jupyter_markdown_goldens.rs`:

- `<case>.md`: the live markdown for `tests/fixtures/jupyter-record-shadow/<case>.qmd`,
  rendered through the engine stage against a real Python kernel (ipykernel; the image
  cases use `IPython.display.Image`, so nothing else is needed). Checked only when a
  Python kernel is on `PATH`.
- `<case>.preview.md`: the markdown the preview capture (`record_capture_run`, the
  legacy `EngineCapture` payload) produced for the same fixture before the flip. Its
  input differs from the engine stage's (`format: q2-preview`, a trailing newline), so
  it is its own golden. Checked only with a kernel.
- `<case>.record.json`: the execution record the same run built (the coalesced record
  notebook, as `write_record` writes it). It lets a kernel-free test present the record
  and compare the result with `<case>.md`.
- `ipynb-<case>.ipynb` and `ipynb-<case>.md`: the `ipynb_stored_replay.rs` notebooks and
  the markdown the engine stage's stored-replay route produces for them. Checked on every
  run, no kernel needed. The execute-demanded notebook is not here: it routes to a live
  engine and has no replay markdown.

`every_golden_file_is_consumed` fails on any file in this directory a test does not read
(this README excepted) and on any golden a test expects but does not find.

## Regenerating

Never, in practice: see `claude-notes/instructions/jupyter-markdown-goldens.md` for why and
for the one command. For the record, it is run with the kernel on `PATH`:

```bash
JUPYTER_MARKDOWN_GOLDENS_BLESS=1 \
JUPYTER_PATH=~/.virtualenvs/positron/share/jupyter PATH=~/.virtualenvs/positron/bin:$PATH \
cargo nextest run -p quarto-core -E 'test(jupyter_markdown_goldens::jupyter_markdown_matches_goldens) | test(jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip)'
```

Without the kernel the command rewrites only the `ipynb-*.md` goldens.
