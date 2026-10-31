---
title: 'EXR-1: extract the presenter (cell outputs to markdown) into a target-neutral module'
date: 2026-10-28
description: 'Moves the Jupyter output types, figure writing and cell presentation out of the native-only Jupyter module so one Rust presenter can run natively and in WASM, with golden parity tests written before anything moves.'
status: in progress (Phases 0-4 done except the Phase 2 WASM VFS test; Phase 5 waits on EXR-2 Phase 4)
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** nothing (can start immediately).
**Blocks:** EXR-5, EXR-8 (parity suite), EXR-13. Soft prerequisite of EXR-3
(`render_inline` should be born in the new module, not moved later) and of EXR-4
(it reuses the nbformat reader `stored_result_for_cell` that this plan moves). Per the dependency table in the epic, it also blocks EXR-6, EXR-14.
**Lane:** D (`workspace-7`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2-3 (the goldens are most of the risk).

> **For implementers:** TDD, task by task. Phase 0 (goldens on the *unmodified*
> code) comes first and its files must not change in any later phase.

**Sequencing with EXR-2 (epic decision 37).** Both lanes start now, so this is a phase-order
gate, not a plan-level edge (the epic dependency table is unchanged). Phase 3 here (the move
of the presenter functions) lands **before** EXR-2 Phases 1, 4 and 5, because both plans edit
the same hunks of `text_execute.rs` (`parse_code_blocks`, the imports, `execute_blocks_inner`)
and of `stored.rs`. EXR-2 Phases 0, 2 and 3 do not collide and may run in parallel with this
plan. The replay-engine move is the trailing Phase 5 here, gated on EXR-2 Phase 4 (see there).

## Goal

One Rust presenter turns kernel-shaped outputs (nbformat mime bundles, streams,
errors) into the Quarto cell markdown, and it compiles for `wasm32`. Today the
presenter lives in a module that is excluded from WASM for reasons unrelated to
presentation. After this plan the live Jupyter path, the stored-notebook replay
and (later) record presentation all call the same functions, and presenting
never touches engine availability.

This is a pure refactor: no behavior change, proven by goldens.

## Background (verified 2026-10-28, workspace-1 at `2ee0bd966`)

**The gate.** `engine/mod.rs:87-88` is `#[cfg(not(target_arch = "wasm32"))] pub mod
jupyter;`. The presenter is excluded from WASM only because it lives inside it.

**What the presenter is, and where.** All in `crates/quarto-core/src/engine/jupyter/`:

- Output types `CellOutput` (`execute.rs:52`), `ExecuteStatus` (`:37`), the kernel
  result struct (`:26`) and `MimeBundle` (`:75`). Same file imports
  `jupyter_protocol` and `tokio::time::timeout` (`execute.rs:13-18`) because the
  kernel session code lives beside the types. The conversion from
  `jupyter_protocol::Media` (`media_to_mime_bundle`, `:242`) belongs with the
  kernel code, not the presenter.
- `text_execute.rs`: `render_cell` (`:562`), `format_outputs` (`:728`),
  `FigureWriter` (`:623`; `write_image` `:670` calls `std::fs::create_dir_all` at
  `:701` and `std::fs::write` at `:705`), `ticks_for_code` (`:518`),
  `echoed_source_fence` (`:597`), `fenced_output_div` (`:605`), `format_error_text`
  (`:830`), `strip_ansi_codes` (`:856`), `extract_text_content` (`:842`),
  `CellVisibility` (`:425`), `is_warning_output` (`:475`), `resolve_cell_options`
  (`:390`), `resolved_flag` (`:406`). The move also needs `extract_text_content` and
  `is_warning_output`, which the replay code and `format_outputs` call.
- `render_cell` takes `(language, code, &result, &mut FigureWriter,
  CellVisibility)`: no `ExecutionContext`, no kernel. That is why the move is
  feasible.

**Consumers.** `text_execute.rs` (live path) and `stored.rs`
(`IpynbReplayEngine`, `:51`; imports at `:33-36`: `CellVisibility`, `CodeBlock`,
`FigureWriter`, `describe_location`, `extract_text_content`, `parse_code_blocks`,
`render_cell`, `resolve_cell_options`, all from the native-only `text_execute.rs`, which
imports the kernel daemon at `:21`). The stage pushes the replay
engine under `#[cfg(not(target_arch = "wasm32"))]` (`stage/stages/engine_execution.rs:429-431`),
so on WASM stored notebook cells are inert: that is bd-9zmpzw3w, still open.

**Correction to the epic and handoff.** bd-5t6wvu7m (image outputs rendered as
placeholders) is **closed**: `FigureWriter` landed in PR #412. The only
placeholder left is the fail-soft fallback at `text_execute.rs:792-794` for
payloads that cannot be decoded. No placeholder fix belongs in this plan.

**Known presenter gaps** (documented in `format_outputs`, `text_execute.rs:726-727`):
`text/markdown`, `text/latex` and widget payloads are unhandled. Not fixed here.

**The second presenter.** `ts-packages/quarto-api/src/jupyter/to-markdown.ts`
(1069 lines) is a TypeScript rewrite of Q1 `jupyterToMarkdown` that the Julia
engine calls through the Quarto API; `display-data.ts:80` has `displayDataMimeType`.
Its header lists simplifications versus Q1 (no ANSI colorization, no retina
sizing, no slide delimiters). This plan does **not** unify the two. It records
that nbformat-to-markdown now exists twice; the parity measurement is EXR-8.

**Existing tests we can build goldens from.**
`stored.rs` has kernel-free tests over nbformat fixtures (`STDOUT_NB`,
`HTML_OUTPUT_NB`, `ERROR_OUTPUT_NB`, `LITERAL_FENCE_NB`, `PNG_OUTPUT_NB`,
`MARKDOWN_ONLY_NB`, `ECHO_FALSE_NB`) driven by a `ctx_for` helper that mirrors
production source-context registration. Live-path tests need `ipykernel` and are
`#[ignore]`d (`tests/integration/jupyter_integration.rs`). The cross-engine
parity suite (`engine_output_parity.rs`) needs both R and Python.

**WASM VFS.** `SystemRuntime` has `file_write` (`quarto-system-runtime/src/traits.rs:341`)
and `dir_create` (`:399`): the seam for a WASM figure sink. Today's `FigureWriter::write_image`
uses `std::fs::create_dir_all` (`:701`) and `std::fs::write` (`:705`): that compiles on
`wasm32` but fails at runtime, and the fail-soft path then emits `[Image output]`
(`:792-794`). The planned `RuntimeFigureSink` needs a `SystemRuntime`, but `ExecutionContext`
has no runtime field (no match for "runtime" in `context.rs`) and `IpynbReplayEngine` is a
unit struct built as `Arc::new(IpynbReplayEngine)` (`engine_execution.rs:431`); Phase 2
decides how the sink gets its runtime.

**Name collision.** `engine::context::ExecuteResult` already exists (`text_execute.rs:23`
aliases it `KernelExecuteResult` for that reason). Moving the kernel result struct into
`presenter/output.rs` would create a second `ExecuteResult`, so it is renamed when it moves
(Phase 1).

**WASM check recipe (verified).** `cargo check -p quarto-core --target wasm32-unknown-unknown`
fails from the workspace root (`tree-sitter-doctemplate` cannot find `stdlib.h`, then
`panic_unwind` lacks the unwind strategy). The working recipe runs from
`crates/wasm-quarto-hub-client`, whose `.cargo/config.toml` sets build-std and the rustflags,
with the two variables that mirror `hub-client/scripts/build-wasm.js:149-157`:

```bash
cd crates/wasm-quarto-hub-client
CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
CFLAGS_wasm32_unknown_unknown="-isystem $PWD/wasm-sysroot" \
cargo check --target wasm32-unknown-unknown
```

About 1 minute cold, a few seconds warm. Phases 1, 2 and 5 name it as "the WASM check".

**Not verified:** whether the moved function bodies pull in other native-only
items (expected: no, they use `regex`, `serde_json`, `base64`, `tracing`); whether
the `ConfigValue` dependency of `CellVisibility` is WASM-clean (quarto-core already
builds for WASM, so very likely).

## Design decisions (from the epic; do not re-litigate)

- Additive migration: this is step 1. The live path switches to the moved
  functions but its output must be byte-identical.
- The presenter is Rust long term; the TS presenter is not touched here.
- Presenting a stored or recorded notebook never needs engine availability.
- The `.cell` wrapper shape is preserved exactly
  (`2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`).
- Output types are defined without `jupyter_protocol` or `tokio`.

## Proposed layout

A new target-neutral module `crates/quarto-core/src/presenter/`, a sibling of `engine/`
(epic decision 31):

- `output.rs`: `CellOutput`, `MimeBundle`, `ExecuteStatus`, the result struct (renamed on
  the move, see Background, so it does not collide with `engine::context::ExecuteResult`).
  `jupyter/execute.rs` re-exports them so existing imports keep compiling.
- `figures.rs`: a `FigureSink` trait (`begin_cell`, `write_image`, `wrote_any`,
  `files_dir`), `FsFigureSink` (today's behavior, native) and
  `RuntimeFigureSink` (writes through a `SystemRuntime`, so it works on the WASM VFS).
- `cell.rs`: `render_cell`, `format_outputs` and the fence/escape helpers.
- `visibility.rs`: `CellVisibility`, `resolve_cell_options`, `resolved_flag`.
- `nbformat.rs`: `stored_result_for_cell` and `mime_bundle` from `stored.rs:229,300`
  (nbformat JSON to output types); EXR-4 reuses this reader.
- `IpynbReplayEngine` moves to `engine/ipynb_replay.rs`, outside the gated module, as the
  trailing Phase 5 after EXR-2 Phase 4.

## Out of scope

- Unifying with the TS presenter; text/markdown, latex and widget handling.
- New figure behavior (naming stays `cell-<n>-output-<m>.<ext>`).
- Presenting from records (EXR-5/EXR-6); the `.ipynb` file claim on WASM (this
  epic uses `.ipynb` as a record, not as an input format), so bd-9zmpzw3w stays
  open for that half.
- Moving `engine_cell_lang` out of `engine/capture_splice.rs`: EXR-2 does it, early,
  so the legacy splice module holds only legacy code and can be deleted later (EXR-17).

## Phased checklist

### Phase 0: golden characterization (on unmodified code)

- [x] Build a presenter corpus of kernel-free inputs: each stored.rs notebook plus
  new fixtures for stderr stream, `execute_result` text/plain, `display_data`
  html, valid and invalid-base64 png/jpeg/svg, error with ANSI traceback,
  an empty-output cell, `ExecuteStatus::Error` with no outputs (built by hand from Rust
  results, because `stored.rs` always sets `ExecuteStatus::Ok`, `:293-296`; the corpus
  README says so), output text
  that needs a four-backtick fence (`ticks_for_code`), `echo: false`,
  `include: false`, `output: false`, `warning: false` dropping stderr only, and
  a multi-cell notebook where a hidden cell still advances figure numbering. Also cases
  for the behaviors the stored-notebook fixtures do not cover: the live-path splice loop
  (`text_execute.rs:284-371`), document `execute:` scope and cell-over-scope precedence
  (`resolve_cell_options`), multi-output figure numbering (`cell-N-output-M`), nbformat
  array-form text (the `extract_text_content` array arm), and unhandled mimes
  (`text/markdown`, `text/latex`, widgets), whose silent drop is pinned today only by a
  comment at `text_execute.rs:725-727`.
- [x] Test `presenter_goldens`: drive today's `IpynbReplayEngine` and
  `render_cell` on each fixture and compare the markdown **and** the written
  figure file names and bytes against checked-in files under
  `crates/quarto-core/tests/fixtures/presenter-goldens/`. The harness is a unit test under
  `engine/jupyter`, because `render_cell`, `FigureWriter` and `CellVisibility` are
  `pub(super)`; Phase 3 re-points it at the new paths. Directory comparison is sorted, so
  file order cannot matter (output priority is explicit in `format_outputs`, `:733-796`,
  so the output is deterministic).
- [x] Generate the goldens from the unmodified tree in the first commit; the
  commit message says so. Goldens are never regenerated in later phases.
- [x] Negative control: perturb one byte of `format_outputs` output locally and confirm the
  goldens fail (as EXR-5 does), then revert.
- [x] A mechanical check that the goldens directory is unchanged since the first commit
  (`git diff --exit-code <first-commit> -- crates/quarto-core/tests/fixtures/presenter-goldens/`
  in a script or test); it runs at every later phase exit.
- Exit: goldens committed and green on unmodified code. Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 1: target-neutral output types

- [x] Move the types to `presenter/output.rs`; re-export from `jupyter/execute.rs`. Rename
  the kernel result struct on the move (it would otherwise be a second `ExecuteResult`
  beside `engine::context::ExecuteResult`); `text_execute.rs` drops its
  `KernelExecuteResult` alias.
- [x] The kernel code (`KernelSession`, `media_to_mime_bundle`) now produces the
  neutral types.
- [x] The WASM check (recipe in Background) compiles the types.
- Exit: goldens green and the goldens-unchanged check passes. Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 2: figure sink

- [x] Tests first, with an in-memory sink: numbering across cells and outputs,
  hidden cells advance numbering, base64 newlines stripped, literal `<svg`
  written as text, write failure returns `None` (fail-soft, then the placeholder).
- [x] `FigureSink` trait; `FsFigureSink` (the current code verbatim).
- [x] Goldens green with `FsFigureSink`.
- [x] Decide and test how `RuntimeFigureSink` obtains its `SystemRuntime`. Pick one: a
  field on `IpynbReplayEngine` supplied by the stage when it builds the engine
  (`engine_execution.rs:431`), or a runtime field on `ExecutionContext`. Write the test
  first: a mock `SystemRuntime` records `dir_create` and `file_write` calls, and the sink
  writes every figure through it with no `std::fs` access.
- [x] `RuntimeFigureSink` over that `SystemRuntime`, passing the mock-runtime test.
- [ ] WASM test (style of `crates/pampa/tests/wasm_lua.rs`) that a stored notebook's
  figures are written to the VFS, not degraded to `[Image output]`. **Moved to Phase 5.** `quarto-core` has
  no WASM test harness yet (no `wasm_bindgen_test`, no `wasm-c-shim` dev-dependency), and the stored-notebook
  half cannot run on WASM until the replay engine leaves the native-only module. One Phase 5 WASM test
  covers both. Native coverage today: `presenter::figures` mock-runtime tests and
  `stored::png_output_with_a_runtime_writes_through_it_not_the_filesystem`; `RuntimeFigureSink` compiles
  under the WASM check. Decision: the runtime is a field on `IpynbReplayEngine` (`with_runtime`); the native
  stage uses `IpynbReplayEngine::new()` (filesystem), and Phase 5 passes `ctx.runtime` on WASM.
- Exit: Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`; the WASM check passes.

### Phase 3: move the presenter functions

> **Landed 2026-10-31 ahead of the rest of Phase 2** (lane A's EXR-2 Phases 1, 4, 5 were waiting on it). Phase 2's
> `FigureSink`/`FsFigureSink` core was done first because `render_cell` needs it. Phase 0 corpus is 58 notebook
> cases plus 6 direct cases; the live-path splice loop needs a kernel and is not pinned (see the goldens README).

- [x] Move `render_cell`, `format_outputs`, the helpers, `CellVisibility` and
  `describe_location` (`text_execute.rs:494`); adjust visibility from `pub(super)`.
  `describe_location` is not a presenter function, but `stored.rs:33-36` imports it and
  Phase 5 needs it outside the native-only module; it takes only a `SourceInfo`, an offset and an
  `ExecutionContext`, so it moves with the presenter.
- [x] `text_execute.rs` and `stored.rs` call the new paths.
- [x] Re-point the Phase 0 golden harness at the new paths; the goldens are untouched.
- Exit: goldens byte-identical with zero edits (the goldens-unchanged check passes). Gate:
  `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. This phase must land before EXR-2 Phases 1, 4 and 5.

### Phase 4: record the second presenter

- [x] Add a short note to `claude-notes/instructions/` listing the two
  nbformat-to-markdown implementations (Rust here, TS in `quarto-api`) and the
  rule that EXR-8 owns their parity.
- Exit: Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

### Phase 5: replay engine out of the native-only module (trailing; gated on EXR-2 Phase 4)

Do not start this phase until EXR-2 Phase 4 has landed. `stored.rs:33-36` imports
`CodeBlock`, `parse_code_blocks`, `describe_location`, `FigureWriter` and more from the
native-only `text_execute.rs`; EXR-2 Phase 4 replaces the scanner pieces with `scan_units`,
so moving them first would be throwaway work. (Epic decision 37.)

- [ ] Check, before starting, that nothing earlier needs the move. UNVERIFIED: the review
  says EXR-13 and EXR-14 need the presenter (Phase 3), not the replay-engine move. If one of
  them does need it, raise that with Gordon rather than moving early.
- [ ] Test first, `replay_does_not_probe_engine_availability`: Jupyter registered but
  unavailable, `.ipynb` input without `execute.enabled`, replay succeeds and the
  engine's availability check is never called (confirm whether Plan 7c already has
  this and extend it).
- [ ] Move `IpynbReplayEngine` to `engine/ipynb_replay.rs`; remove the cfg at
  `engine_execution.rs:429-431`.
- [ ] The WASM check passes; add a WASM-side test, in the style
  of `crates/pampa/tests/wasm_lua.rs`, that a stored notebook renders its outputs
  (drive the engine directly; the `.ipynb` file claim is out of scope).
- Exit: bd-9zmpzw3w's replay half is done; the claim half stays open in the strand.
  Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`. Exit criteria for the plan: all phases green;
  `cargo xtask verify` green; the workspace test delta reported against the live baseline
  and accounted for.

## Risks and open questions

- **Visibility churn** is mechanical but wide; keep moves and edits in separate
  commits so golden diffs stay empty.
- **Figure paths on WASM:** the sink writes into the VFS; preview currently
  materializes embedded capture bytes instead (`capture_files.rs`). The two must
  not both write the same files. Out of scope here; EXR-13/EXR-14 settle it.
- **Placement (decided, epic decision 31).** A top-level `presenter/` beside `engine/`,
  not `engine/presenter/`. `engine/` is not WASM-gated as a whole (`lib.rs:58` declares it
  with no `cfg`; its native-only submodules are gated one by one in `engine/mod.rs`), so
  either place would compile, but the presenter serves every engine and the preview, and a
  sibling keeps its WASM-clean surface explicit.

## References

- `crates/quarto-core/src/engine/jupyter/{execute,text_execute,stored}.rs`
- `crates/quarto-core/src/stage/stages/engine_execution.rs:429`
- `ts-packages/quarto-api/src/jupyter/{to-markdown,display-data}.ts`
- [Plan 7c](2026-07-08-plan7c-ipynb-content-processor.md), bd-9zmpzw3w, bd-5t6wvu7m
