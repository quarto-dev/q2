# Presenter goldens (EXR-1)

Characterization of the cell presenter (kernel-shaped outputs to Quarto cell markdown).
Generated from the **unmodified** tree before EXR-1 moved any code, and never regenerated:
`scripts/check-presenter-goldens-unchanged.sh` fails if anything here (this README excepted)
differs from the commit that first added it. If a golden has to change, that is a behavior
change, not a refactor; stop and raise it.

Driven by `engine/jupyter/presenter_goldens.rs` (`presenter_goldens` test).

- `notebooks/<case>/notebook.ipynb`: replayed through `IpynbReplayEngine`.
  - optional `scope.json`: flat map of booleans used as the document `execute:` scope.
  - `expected.md`: the engine's markdown, or `ERROR: <message>` with the temp dir as `<dir>`.
  - `expected-supporting-files.txt`: reported supporting files, relative to the document.
  - `expected-files/`: every file written beside the notebook (figure names and bytes).
    Absent when nothing is written.
- `direct/<case>/expected.md`: results built by hand in the harness and passed to
  `render_cell`, for states the stored reader cannot produce (it always sets
  `ExecuteStatus::Ok`, `stored.rs:293-296`): `Error`/`Aborted` status with and without outputs.

Not covered: the live-path splice loop (`execute_blocks_inner`) needs a kernel. Its splice
discipline is the same as `replay_stored_outputs`, which the notebook cases cover; its
differences (error-abort policy, `begin_cell` ordering relative to execution) are not pinned
here and are covered by the `#[ignore]`d kernel tests in `tests/integration/jupyter_integration.rs`.

Known pinned oddities (documented, not fixed): widget payloads are silently dropped; an
undecodable image with no `text/plain` renders the escaped placeholder `\[Image output\]`.

Changed on purpose (EXR-8 B3, 2026-11-01): `text/markdown` and math `text/latex` were pinned
as silently dropped. The TS presenter emits them as markdown in a `.cell-output-markdown`
div, and the Julia flip needs the same, so `unhandled-markdown-mime`,
`unhandled-latex-mime` and `markdown-beats-nothing-plain-wins` were re-blessed (Gordon
approved). `scripts/check-presenter-goldens-unchanged.sh` reports these three files as
changed from now on; that is the intended exception.

Changed on purpose (bd-f63cqoke, 2026-11-11): `four-backtick-source` pinned a bug. A code
cell whose source has a line starting with ``` gets a four-backtick fence from the ipynb
converter (Quarto 1's rule), and the executor's legacy rule accepted only three-backtick
fences. The cell's stored `ok` output was dropped and only its source was shown. Quarto 1
opens a cell on any run of three or more backticks, so the cell now presents its output and
the golden was re-blessed. `scripts/check-presenter-goldens-unchanged.sh` reports this file as
changed from now on; that is the intended exception.
