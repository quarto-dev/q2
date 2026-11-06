# Jupyter markdown goldens

`crates/quarto-core/tests/fixtures/jupyter-markdown-goldens/` holds the markdown the
Jupyter engine and the stored-notebook replay produced **before** EXR-6 flipped them
onto the record presenter (`presenter::document::present_document`), one golden per
EXR-5 shadow fixture and per stored-replay notebook, plus the preview capture's own
markdown for each shadow fixture and the execution record each shadow run built. They
are the oracle for "byte-identical": `jupyter_markdown_matches_goldens`,
`stored_records_present_to_the_goldens` and
`legacy_capture_payload_is_byte_identical_after_flip` in
`crates/quarto-core/tests/integration/jupyter_markdown_goldens.rs` compare against
them, and `every_golden_file_is_consumed` rejects a stray or missing file.

## The goldens are never regenerated

Epic decision 28 (`claude-notes/plans/2026-10-28-execution-records-epic.md`): the goldens
freeze the pre-flip output and are kept after EXR-17 removes the legacy splice. A golden
that has to change is a behavior change, not a refactor. Stop and raise it; do not
re-bless. The one command below exists for the record of how the files were made, and
for the case where Gordon decides a behavior change is intended.

## The one regeneration command

Run from the repository root, with a Python kernel (ipykernel) on `PATH`; it rewrites
every golden from the current tree and must be followed by a review of the diff:

```bash
JUPYTER_MARKDOWN_GOLDENS_BLESS=1 \
JUPYTER_PATH=~/.virtualenvs/positron/share/jupyter PATH=~/.virtualenvs/positron/bin:$PATH \
cargo nextest run -p quarto-core -E 'test(jupyter_markdown_goldens::jupyter_markdown_matches_goldens) | test(jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip)'
```

Without the kernel it rewrites only the `ipynb-*.md` goldens (the notebook half needs
no kernel). The pipeline's `listing-item: date-modified:` is normalized to `<date>`
before a golden is written or compared, so a bless on another day produces the same
files.

The preview goldens (`<case>.preview.md`) were blessed from the pre-flip tree (commit
`ab4fa2ca8`, with the flip's `text_execute.rs` and `inline.rs` checked out at that
commit during the bless); the command above, run today, would regenerate them from the
post-flip presenter, which is only acceptable because the two are proven equal.

See the fixtures directory's `README.md` for what each file is.

## Intended exceptions (open, recorded 2026-11-02)

Lane A (EXR-2) asked where a note belongs when a golden or a CI check has an *intended*
exception (a place where the presenter output is meant to differ from a golden, such as a
span the shared scanner claims differently from the old engine path). EXR-2 is done, so its
plan is not the place, and no answer was given. Until the epic picks one, record each
intended exception here, with the golden it affects and the plan that decided it, and keep
the rule above: a golden changes only by the one command, and only by a decision written
down. The question itself (where the CI script should carry such a note) is still open; its
text is in lane A\'s EXR-2 message, which this note summarizes and does not replace.
