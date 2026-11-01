# Replay Engine (bd-45yw)

The replay engine reproduces a previously-recorded engine execution in
pure Rust. It exists to make engine-channel tests cheap to run (no R,
Python, or Jupyter required) and bug reports cheap to reproduce (a
trace file pinned to a single document).

It is a debugging / QA tool — not a substitute for the real engines.

Two replay paths (EXR-6, 2026-11-01):

- **Markdown engines** (knitr, marimo, any engine that returns markdown):
  the captured `ExecuteResult` is returned verbatim, byte-identical to
  what the recording saw.
- **Jupyter**: the capture carries the engine's execution record on
  `engine_capture.record` (EXR-5), and replay *re-presents* it: the
  record comes back byte for byte on `ExecuteResult.record`, and the
  markdown is `present_document(record, input_qmd, units, execute
  scope, target format)` computed now. So replayed Jupyter markdown
  follows the current presenter and the render's target format (one
  trace replayed to `html` and to `docx` differs where the presenter's
  mime choice differs), and a presenter bug that lived at recording
  time no longer reproduces from the trace. The engine-produced record
  is what stays fixed. A Jupyter capture with no `engine_capture.record`
  was recorded before EXR-5 and fails with a "re-record this trace"
  error; there is no fallback to its stored markdown.

## When to use it

- **Regression tests** for engine-emitted features (resources, filters,
  `ExecuteResult.includes`, etc.) that would otherwise require a real
  engine install in CI.
- **Reproducing user bug reports** where the user can attach a trace
  file produced by their failing render. You replay that trace
  locally; the recorded engine output is byte-identical to what the
  user saw.
- **Pinning a flaky engine result** for repeatable debugging.

It is **not** intended for general use during normal renders.

## Recording a trace

Add `trace: true` to the document's metadata and run a normal render:

```yaml
---
title: My Document
trace: true
---
```

```bash
q2 render path/to/doc.qmd
```

Quarto writes `path/to/.quarto/trace/<doc-stem>/latest.json`. When the
document declared a non-markdown engine that actually executed, the
trace's top-level `engine_capture` block carries the engine name, the
verbatim QMD that was passed to `engine.execute()`, and the full
`ExecuteResult` (markdown, supporting files, includes, filters,
post-process flag).

For a Jupyter run the capture also carries `record`, the engine's
execution record (a one-engine record notebook), which is what replay
presents from. The trace observer always asks the stage for records,
so every trace recorded since EXR-5 has it.

The markdown engine is a no-op passthrough — `engine_capture` is
absent for documents that did not run a real engine. That is correct
behavior; replay against such a trace fails loudly rather than
silently degrading (see "Miss policy" below).

## Replaying a trace

```bash
q2 render path/to/doc.qmd --replay path/to/trace.json
```

Or via env var (useful in scripted CI):

```bash
QUARTO_REPLAY=path/to/trace.json q2 render path/to/doc.qmd
```

The flag wins over the env var. Both surface the same
`load_replay_capture` path.

The replay reads the trace, extracts `engine_capture`, builds an
`EngineRegistry` with `ReplayEngine` substituted under the recorded
engine's name, and hands it to `EngineExecutionStage` via
`HtmlRenderConfig.engine_registry`. Everything else in the pipeline
(parse, metadata-merge, transforms, render, template-apply) runs
against the real implementations.

## Miss policy: hard fail, no fallback

The replay engine compares the QMD `EngineExecutionStage` hands to
`execute()` against `engine_capture.input_qmd` byte-for-byte. On any
mismatch (one byte difference, different metadata, a renamed
heading), `execute()` returns
`ExecutionError::ExecutionFailed` with a "replay miss" diagnostic and
the byte counts of recorded vs. observed inputs.

This is intentional. Quiet fallbacks on a debugging tool send
investigators on wild-goose chases — when replay fails, we make sure
they know.

If the document genuinely changed since the trace was recorded,
re-record the trace.

A Jupyter capture without a record (recorded before EXR-5 landed)
fails the same way, with a message that says to re-record. A record
whose cell ids a fresh scan of `input_qmd` cannot reproduce (a sugared
cell, whose id came from the stage's author text) still presents: the
presenter falls back to the cell's content hash.

## Source-info caveat

Replay does **not** restore the original engine's source provenance
for engine-emitted content. Diagnostics that map source positions
into engine output (e.g. Jupyter cell line numbers in error messages,
knitr line attribution into `_files/` figures) will not match between
a real engine run and its replay.

This is a documented v1 limitation. If a use case for source-info
parity surfaces, the trace already carries the recorded `SourceInfo`
on engine-emitted blocks; restoring it requires plumbing during
replay, not a format change.

## Authoring regression-test fixtures

For checked-in CI fixtures that exercise engine-channel behavior:

1. On a development machine with R/Python/Jupyter installed, render
   the fixture once with `trace: true`.
2. Copy the produced `latest.json` into the test fixture tree (e.g.
   `crates/<your-crate>/tests/fixtures/<feature>/`).
3. In your test, use `RenderToFileOptions.replay_capture` (set from
   `quarto_trace::read::read_trace(...).engine_capture`) and assert
   on the rendered output.

For an example, see
`crates/quarto-core/tests/project_resources.rs::orchestrator_engine_channel::orchestrator_drains_replay_engine_report_to_output_dir`,
which uses the probe-then-replay technique to fabricate a capture
without needing R/Python at test-write time.

## Trace size

Recorded traces can be large because they capture the full pipeline
state (per-stage data + the engine capture). On-disk size optimization
is bd-5qnj's concern; the in-memory representation stays as-is.

For checked-in fixtures, prefer the smallest possible reproducer
(small QMD, minimal supporting files). For user-attached bug reports,
size is whatever the user's failing render produced; we accept the
cost in exchange for byte-faithful reproduction.

## Activation surfaces summary

| Surface | Effect |
|---------|--------|
| `trace: true` in metadata | Records `engine_capture` into the trace. |
| `q2 render --replay <trace>` | Replays the trace's `engine_capture`. |
| `QUARTO_REPLAY=<trace>` | Same as `--replay` (CLI flag wins). |
| `RenderToFileOptions.replay_capture` | Library-level activation. |
| `RenderToFileOptions.engine_registry_override` | Test escape hatch (takes precedence over `replay_capture`). |

## Code references

- `crates/quarto-trace/src/lib.rs` — `EngineCapture`, `TraceDocument.engine_capture`
- `crates/quarto-core/src/engine/replay.rs` — `ReplayEngine`
- `crates/quarto-core/src/presenter/document.rs` — `present_document`, what a Jupyter replay runs
- `crates/quarto-core/src/engine/registry.rs` — `EngineRegistry::with_replay`
- `crates/quarto-core/src/stage/stages/engine_execution.rs` — `ENGINE_CAPTURE_KIND` and the recording emit
- `crates/quarto-core/src/stage/trace.rs` — `JsonTraceObserver::on_auxiliary_data` routing
- `crates/quarto-core/src/render_to_file.rs` — `RenderToFileOptions.replay_capture` and `engine_registry_override`
- `crates/quarto/src/commands/render.rs` — `--replay` / `QUARTO_REPLAY` plumbing

Plan: `claude-notes/plans/2026-05-03-replay-engine.md`.
