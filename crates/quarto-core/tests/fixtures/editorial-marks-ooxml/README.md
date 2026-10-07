# editorial-marks-ooxml fixtures

`01-…08-*.qmd` are copied unchanged from the `examples/` directory of Gordon's
extension `quarto-ooxml-editorial-marks` (MIT, Copyright (c) 2026 Gordon Woodhull;
`github.com/gordonwoodhull/quarto-ooxml-editorial-marks`), source commit `9ae840d`.

They exercise the editorial-marks export transform (document import epic, P6, I23):
`[++ ]`, `[-- ]`, `[!! ]` and `[>> ]` marks, inline and `:::` block forms, rendered to
docx (01-05, 07, 08) and pptx (06). The counts the integration test asserts are ported
from the extension's `tests/run-tests.py` `EXPECTATIONS`; see
`tests/integration/editorial_marks_ooxml.rs` for the table and the one deliberate change
(02's comment ids are numeric, not `comment-id`).

`extension/` is a copy of the extension itself (`_extensions/quarto-ooxml-editorial-marks/`,
same source commit, with its MIT `LICENSE`), used by the coexistence test: a project that
still lists it must render the same docx as one that doesn't, because the built-in transform
has already renamed the `quarto-*` classes the extension's filter looks for.
