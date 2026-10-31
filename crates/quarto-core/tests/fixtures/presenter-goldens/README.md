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

Known pinned oddities (documented, not fixed): `text/markdown`, `text/latex` and widget
payloads are silently dropped; an undecodable image with no `text/plain` renders
`[Image output]`.
