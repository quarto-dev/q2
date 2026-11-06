---
title: 'EXR-1b: move the stored-notebook replay engine out of the native-only module'
date: 2026-10-31
description: 'The trailing half of EXR-1, split out so the presenter extraction could finish: moves IpynbReplayEngine out of engine/jupyter so stored notebooks render their outputs on WASM, and adds the first quarto-core WASM tests.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Split from:** [EXR-1](2026-10-28-exr-01-presenter-extraction.md) (its former Phase 5), at Gordon\'s
direction on 2026-10-31, so that the dependency graph between EXR-1 and EXR-2 has no edge in both
directions.
**Depends on:** EXR-1 (landed) and [EXR-2](2026-10-28-exr-02-unit-scanner.md) **Phase 4**. The
epic table lists the plan-level edge, EXR-2; the start gate is Phase 4 only.
**Blocks:** [EXR-6](2026-10-28-exr-06-jupyter-flip.md), which rewrites `IpynbReplayEngine` into a
thin adapter and must find it at its final path.
**Lane:** D (`workspace-7`); run it under the epic\'s "Running the plans in four lanes" section.
It does not block the rest of the lane D queue.
**Estimated sessions:** 1 (more if the quarto-core WASM test harness is large; see Risks).

> **For implementers:** the presenter goldens from EXR-1 must stay unchanged. Run
> `scripts/check-presenter-goldens-unchanged.sh` at every phase exit.

## Goal

A stored `.ipynb`\'s outputs render on WASM, through the same presenter and figure sink as native,
with no engine-availability probe. Today the replay engine is excluded from the WASM build only
because it lives in `engine::jupyter`, so on WASM stored notebook cells are inert (bd-9zmpzw3w, replay
half). After this plan the replay engine is outside the native-only module and the stage constructs it
on every target.

## Background (state after EXR-1, 2026-10-31)

- The presenter, output types and figure sinks are already target-neutral and compile for `wasm32`:
  `crates/quarto-core/src/presenter/`. `describe_location` moved there in EXR-1 Phase 3 for this plan.
- `IpynbReplayEngine` is in `engine/jupyter/stored.rs`. It has an optional runtime:
  `IpynbReplayEngine::with_runtime(Arc<dyn SystemRuntime>)` writes figures through
  `RuntimeFigureSink`; `IpynbReplayEngine::new()` uses the filesystem. The native stage uses `new()`.
  This plan makes the WASM branch of the stage use `with_runtime(ctx.runtime.clone())`.
- The stage pushes the replay engine under `#[cfg(not(target_arch = "wasm32"))]`
  (`stage/stages/engine_execution.rs`, near line 429).
- `stored.rs` still imports `CodeBlock` and `parse_code_blocks` from the native-only `text_execute.rs`.
  EXR-2 Phase 4 replaces those with `scan_units`; moving first would be throwaway work. That is the
  start gate.
- **No `quarto-core` WASM test harness exists** (no `wasm_bindgen_test`, no `wasm-c-shim`
  dev-dependency, no CI runner). The model is `crates/pampa/tests/wasm_lua.rs`; see
  `dev-docs/wasm.md` for the run command. WASM tests cannot run on Windows; they run in Linux CI.
- Checked 2026-10-31: EXR-13 and EXR-14 reference only the presenter, not the replay engine, so they
  do not wait for this plan. EXR-6 does (it edits `IpynbReplayEngine`), and it already sits after
  EXR-4 and therefore after EXR-2.

## Phased checklist

### Phase 1: move the replay engine

- [x] Start gate: EXR-2 Phase 4 has landed on the integration branch; rebase onto it. Re-read
  `stored.rs`, whose imports EXR-2 Phase 4 changed.
- [x] Check that nothing earlier needs the move: EXR-13 and EXR-14 do not (checked 2026-10-31).
  Re-check if either plan changed since.
- [x] Test first, `replay_does_not_probe_engine_availability`: Jupyter registered but unavailable,
  `.ipynb` input without `execute.enabled`, replay succeeds and the engine\'s availability check is never
  called (confirm whether Plan 7c already has this and extend it).
- [x] Move `IpynbReplayEngine` (and what it needs from `stored.rs`) to `engine/ipynb_replay.rs`,
  outside the gated module; keep `engine::jupyter::IpynbReplayEngine` compiling for existing imports
  or update them.
- [x] Remove the cfg at the stage push. The WASM branch builds the engine with
  `IpynbReplayEngine::with_runtime(ctx.runtime.clone())`; the native branch keeps `new()`.
- [x] The WASM check passes (recipe in EXR-1\'s Background section).
- Phase 1 landed 2026-10-31. Notes: the test already held on native (the probe test is a regression guard; it passes before and after). `CodeBlock`, `legacy_cells` and `code_block_of` moved from `text_execute.rs` to `engine/units.rs` so the engine no longer imports the native-only module; the engine is re-exported as `engine::IpynbReplayEngine` and still as `engine::jupyter::IpynbReplayEngine`. Gate: clippy clean, nextest 6630/6630 (6629 + the new test), goldens unchanged, WASM check passes.
- Exit: goldens-unchanged check passes. Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings`
  plus `cargo nextest run -p quarto-core`.

### Phase 2: WASM tests

- [x] Add the first `quarto-core` WASM test target in the style of `crates/pampa/tests/wasm_lua.rs`
  (integration-test layout rule: one `integration` binary per crate, unless the WASM target cannot share
  it; decide and state why).
- [x] WASM test: a stored notebook renders its outputs (drive the engine directly; the `.ipynb` file
  claim on WASM is out of scope).
- [x] WASM test (moved from EXR-1 Phase 2): a stored notebook\'s figures are written to the VFS through
  `WasmRuntime`, not degraded to `[Image output]`.
- [x] CI runs the new target, or the PR says why not.
- Phase 2 notes: the target is `crates/quarto-core/tests/wasm_replay.rs`, a top-level file, not a module of
  `tests/integration/`, because that binary needs native-only dev-dependencies (tokio, tempfile, socket2, ...) and
  cannot build for wasm32 (same reason as pampa's `wasm_lua.rs`; it compiles to an empty binary on native).
  quarto-core's `[dev-dependencies]` moved wholesale into a `cfg(not(wasm32))` table, with `wasm-bindgen-test` and
  `wasm-c-shim` under `cfg(wasm32)`. The harness took under a session. CI step added to the WASM job in
  `test-suite.yml`. Both tests pass locally on wasm32 (macOS).
- Closed 2026-10-31: `cargo xtask verify --skip-hub-build --skip-hub-tests` green; `cargo nextest run --workspace` 17605/17605 passed (200 skipped) before the final rebases, and `-p quarto-core` 6742/6742 before the last rebase (onto EXR-4 Phase 5); on the merged tip clippy, 890 targeted tests (`ipynb`, `engine::`, `engine_execution`), the WASM tests and the goldens check were re-run green. No workspace baseline run exists on the integration tip, so the delta is accounted per crate: quarto-core +1 native test (`replay_does_not_probe_engine_availability`); the two WASM tests run only on wasm32. A rebase onto EXR-2 Phase 5 merged textually but broke the build (`text_execute.rs` imports), fixed in the Phase 1 commit.
- Exit: Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`; the WASM tests pass in a Linux or macOS run. Plan exit criteria:
  all phases green; `cargo xtask verify` green; the workspace test delta reported against the live
  baseline and accounted for. bd-9zmpzw3w\'s replay half is done; the file-claim half stays open in the
  strand.

## Out of scope

- The `.ipynb` file claim on WASM (this epic uses `.ipynb` as a record, not an input format).
- Presenting from records (EXR-5 and EXR-6); making `IpynbReplayEngine` a thin adapter (EXR-6).

## Risks and open questions

- **The WASM harness may be bigger than the move.** If standing up a `quarto-core` WASM test target
  takes more than a session, land Phase 1 first (it is independently valuable) and raise the harness
  with Gordon rather than blocking EXR-6.
- **Figure paths on WASM:** preview currently materializes embedded capture bytes instead
  (`capture_files.rs`); the VFS sink and that path must not both write the same files. EXR-13 and EXR-14
  settle it.

## References

- [EXR-1](2026-10-28-exr-01-presenter-extraction.md), [EXR-2](2026-10-28-exr-02-unit-scanner.md),
  [EXR-6](2026-10-28-exr-06-jupyter-flip.md)
- `crates/pampa/tests/wasm_lua.rs`, `dev-docs/wasm.md`
- [Plan 7c](2026-07-08-plan7c-ipynb-content-processor.md), bd-9zmpzw3w
