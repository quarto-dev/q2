# Plan 1: one child-process runner with deadline and cancel (quarto-core)

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E7); the execution model and landing procedure are there.

## Execution

- **Lane:** C (`workspace-3`), binding: one agent per plan, in this order: Plan 1, Plan 4, Plan 7, Plan 8, Plan 11. This plan is number 1 of 5 in the lane.
- **Waits on:** nothing. Nothing inside this plan waits on another plan.
- **Start gate:** none; start immediately.
- **Branch:** `q2-preview/p01-child-runner`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic (rebase, workspace nextest, `git -C ../workspace-7 merge --ff-only`).
- **Unlocks:** Plan 7 (import), Plan 9 (preview routes: sets the deadline and cancel), Plan 10 (book downloads, via the merge tail's pandoc and typst children).
- **Next in lane:** Plan 4 (`2026-10-08-q2-preview-p04-pdfjs-packaging`); the next agent starts only when its own start gate passes.

## Goal

Every native child process the preview server starts (typst, pandoc for downloads, pandoc for import) has a wall-clock timeout and can be cancelled, through one helper in `quarto-core`. Today both render children use `Command::output()`, so a pathological document pins a blocking-pool thread forever and its path's slot with it.

## Background facts

(Line numbers are from the 2026-10-08 review; re-check when starting.)

- Both render children are `Command::output()`: typst at `typst_compile.rs:243-255` (`typst compile --root <ctx.project.dir> ...`), pandoc at `pandoc_write.rs:1436`. Both sites have a std `Command` and the stage context or request in scope. Three more `.output()` calls exist (`typst_compile.rs:327,505`, `pandoc_write.rs:97`); task 1.3 routes each through the helper or records why it stays.
- `quarto_core::stage::Cancellation` (a tokio `CancellationToken`, `stage/cancellation.rs`) already exists and is on `StageContext` (`context.rs:179`, `with_cancellation` at `:473`), but nothing sets it: `stage_context_from_render_context` (`pipeline.rs:1105`) takes a `RenderContext`, which has no cancellation or deadline field (10 `RenderContext {` literal sites), and `RenderToFileOptions` has none either. `PandocWriteStage::execute(&self, request, pandoc_bin)` (`pandoc_write.rs:1376`) is `pub`, takes no stage context, and has six external two-argument callers (`quarto-core/tests/integration/pandoc_request_execute.rs:189,225`, `pandoc_request_extension_filters.rs:279`, `pandoc_request_books.rs:266,412`) plus one in-crate caller (`pandoc_write.rs:1509`).
- `resolve_and_gate_pandoc` is a private fn in the stage (`pandoc_write.rs:89`) returning `PipelineError`; it runs `pandoc --version` on every call. Plan 7's route lives in another crate and needs it public.
- `quarto-preview`\'s tokio has no `process` feature (`Cargo.toml:12`); the helper therefore uses `std::process` and is driven from `spawn_blocking`.
- Native `typst` (>= 0.8, `QUARTO_TYPST` then `PATH`; missing is `Q-21-1`) and `pandoc` (missing `Q-20-1`, too old `Q-20-2`, `pandoc_filters/version.rs:76-100`) are external; tests that need them skip with a clear reason when absent.

## Tasks

Per task: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus `cargo nextest run -p quarto-core`. Workspace nextest at the end of the plan, against the baseline taken at the start. New integration tests go in `crates/<crate>/tests/integration/<name>.rs` plus `pub mod` in `main.rs`.

- [x] 1.1 Baseline `cargo nextest run --workspace` on the integration tip; record pass/skip counts. (Baseline 2026-10-08: 15990 passed, 202 skipped, clean tip 135a42b69.)
- [x] 1.2 The helper, `quarto_core::child_runner` (new module, `#[cfg(not(target_arch = "wasm32"))]`): `run_child(cmd: std::process::Command, limits: ChildLimits) -> Result<ChildOutput, ChildError>` with `ChildLimits { deadline: Option<Duration>, cancellation: Option<Cancellation> }` (`Cancellation` is the existing `quarto_core::stage::Cancellation`, which has `is_cancelled`), `ChildOutput { status, stdout, stderr }` and `ChildError::{TimedOut, Cancelled, Io}`. It spawns the child, drains stdout and stderr on threads (so a full pipe cannot deadlock), polls `try_wait` against the deadline and the cancellation, and on timeout or cancel kills the child (the whole process group on unix, so a pandoc `--filter` grandchild cannot hold the pipes open) and does not wait on the drain threads beyond a short grace period. Plans 7, 9 and 10 call exactly this signature. Tests use a fake child script behind a `#[cfg(unix)]` helper with a `#[cfg(not(unix))]` counterpart (the repo's cross-platform rule; see `spawn_long_lived_child` in `engine/ts_process.rs`): completes, times out, is cancelled, produces output larger than a pipe buffer.
- [x] 1.3 Thread it into both render sites. `RenderToFileOptions` gains `cancellation: Option<Cancellation>` and `child_deadline: Option<Duration>` (default `None` for both, so `q2 render` never times out a long typst or pandoc run); `RenderContext` gains the same two fields (update the 10 literal sites); `stage_context_from_render_context` copies them onto `StageContext` (it already has `with_cancellation`; add a deadline field). `typst_compile.rs:243` calls the helper with the stage's limits. `PandocWriteStage` gains a sibling `execute_with(&self, request, pandoc_bin, limits)` that `execute` delegates to with no limits, so the six external callers are unchanged; `pandoc_write.rs:1436` goes through it. Timeouts and cancels map to distinct stage errors. The three other `.output()` calls (`typst_compile.rs:327,505`, `pandoc_write.rs:97`) are version probes or similar: route each through the helper, or leave it and say why.
- [x] 1.4 Make `resolve_and_gate_pandoc` public (or add a thin public wrapper) and re-export it from `quarto-core` for Plan 7.
- [x] 1.5 `cargo build -p quarto` and `cargo build --workspace` (struct-literal fallout from the new fields).
- [x] 1.6 Plan end: workspace nextest against 1.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- Killing the process group is unix-only; on Windows a pandoc filter grandchild can outlive a kill, so the helper's post-kill grace period (rather than an unbounded join) is what keeps the caller from hanging.

## Handoff log

Landed on `q2-preview/p01-child-runner` (two commits on top of `135a42b69`, no rebase needed; no conflicts).

**Interfaces (as shipped)**
- `quarto_core::child_runner` (native-only): `run_child(cmd: std::process::Command, limits: ChildLimits) -> Result<ChildOutput, ChildError>`; `ChildLimits { deadline: Option<Duration>, cancellation: Option<Cancellation> }` (`Default` = unbounded, like `Command::output()`); `ChildOutput { status, stdout: Vec<u8>, stderr: Vec<u8> }`; `ChildError::{TimedOut, Cancelled, Io(io::Error)}` (unit variants for the first two). stdin is null. With either limit set the child gets its own process group (unix, via `libc`, new unix-only dep of quarto-core) and a timeout or cancel kills the group; the deadline and cancel are also honoured while draining pipes after the child has exited (a grandchild holding the pipes). Post-kill drain grace is 500 ms. A nonzero exit is `Ok`.
- `RenderToFileOptions { cancellation: Option<Cancellation>, child_deadline: Option<Duration> }` and the same two fields on `RenderContext`, copied by `render_document_to_file` and then by `stage_context_from_render_context` onto `StageContext::cancellation` (only when `Some`) and the new `StageContext::child_deadline`. `StageContext::child_limits()` builds the `ChildLimits` (native only). Both options default to `None`; `q2 render` sets them to `None`.
- `PandocWriteStage::execute_with(&self, request, pandoc_bin, limits: ChildLimits)`; `execute` delegates with default limits. `TypstCompileStage` and the pandoc stage pass `ctx.child_limits()`.
- Timeout and cancel are stage errors with distinct messages (`typst timed out and was killed`, `pandoc was cancelled`, ...) from `child_runner::stage_error` (crate-private). No error-catalog codes were added.
- `quarto_core::resolve_and_gate_pandoc(stage_name: &str, runtime: &dyn SystemRuntime) -> Result<PathBuf, PipelineError>` is public (also `quarto_core::stage::stages::resolve_and_gate_pandoc`), native only.

**Deviations**
- The plan said `RenderContext` has 10 literal sites; it has one constructor (`RenderContext::new`), so only that changed. The only `RenderToFileOptions` literal needing the new fields was `crates/quarto/src/commands/render.rs`.
- The three other `.output()` calls (`--version` probes at `typst_compile.rs`/`pandoc_write.rs`, `typst fonts`) now go through `child_runner::run_probe`, a fixed 30 s deadline, no cancellation, failures mapped to the old `.ok()` behaviour. This bounds a hung probe without threading limits into their signatures.
- Line numbers in the plan had drifted (typst site is at ~255, pandoc at ~1439); premises otherwise held.

**Tests:** `tests/integration/child_runner.rs` (9: completes, nonzero exit, spawn failure, timeout, cancel, pre-cancelled, 1 MB on both pipes, two unix-only grandchild cases) and two in `pandoc_request_execute.rs` (`execute_with` deadline and cancel via the fake pandoc, which gained `FAKE_SLEEP`).

**Nextest (workspace):** baseline on the clean integration tip 15990 passed / 202 skipped; final 16001 passed / 202 skipped. Delta +11 passed = 9 + 2 new tests; skipped unchanged.

**For later plans:** Plan 9 sets `cancellation` and `child_deadline` on `RenderToFileOptions`; Plan 7 can call `quarto_core::resolve_and_gate_pandoc` and `run_child` directly. `run_child` puts bounded children in their own process group, so a terminal Ctrl-C no longer reaches them; the caller's cancellation is what stops them. Windows kills only the child, not grandchildren.
