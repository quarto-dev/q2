---
title: 'Misleading "engine not available in this build" warning in `q2 preview` with spliced captures'
date: 2026-06-10
description: 'Stops `q2 preview` from warning that knitr is unavailable and not executing when its output was already captured server-side and spliced in, by passing spliced engine names to the execution stage.'
---

**Date:** 2026-06-10
**Status:** Draft — awaiting review before implementation
**Strand:** bd-sauc9iiq

## Overview

In `q2 preview`, documents are rendered through the **WASM** pipeline,
which does not register the `knitr` (or `jupyter`) engines. For a
document with `engine: knitr`, the native preview host runs the engine
out-of-band, records the result as a **trace engine capture**, and ships
it to the WASM preview. There, `CaptureSpliceStage` splices the recorded
post-engine output into the AST *before* `EngineExecutionStage` runs, so
the user **does** see real execution results.

Despite that, `EngineExecutionStage` still emits:

> Engine 'knitr' not available in this build, using markdown (no execution)

This warning is **technically true for the WASM engine registry** but
**misleading to the user**: their code did execute (server-side) and the
results are visible. The parenthetical "(no execution)" is the part that
is actively wrong in this path.

## Root cause

Two stages share no information about each other:

1. **`CaptureSpliceStage`** (`crates/quarto-core/src/stage/stages/capture_splice.rs`)
   holds the ordered `Vec<EngineCapture>` and, when non-empty, replaces
   each engine's code cells with the recorded output. It runs immediately
   before `EngineExecutionStage`.

2. **`EngineExecutionStage`** (`crates/quarto-core/src/stage/stages/engine_execution.rs`)
   re-detects the engine sequence from document metadata (still
   `engine: knitr`), looks each engine up in its registry, and — when the
   engine is not registered — emits the warning via
   `get_engine_with_fallback()` (lines 102–127, warning at line 120–123).

The pipeline builder wires both stages together:

- `build_q2_preview_pipeline_stages(engine_registry, captures)`
  (`crates/quarto-core/src/pipeline.rs:387`) receives the `captures`
  vector, constructs `CaptureSpliceStage::new().with_captures(captures)`,
  and inserts it before the existing `EngineExecutionStage`.

So at build time the set of engine names that were spliced is **known**
(`captures.iter().map(|c| &c.engine_name)`), but it is never handed to
`EngineExecutionStage`. The execution stage therefore cannot tell the
difference between:

- **(a)** "knitr unavailable AND nothing executed" — a genuine
  no-execution fallback (e.g. WASM preview of a doc that was *never* run
  server-side), where the current warning is appropriate; and
- **(b)** "knitr unavailable in this build, but its output was already
  spliced in from a server-side capture" — where the warning is
  misleading and should be suppressed or reworded.

### Relevant code locations

| Concern | File:line |
| --- | --- |
| Warning string ("not available in this build") | `crates/quarto-core/src/stage/stages/engine_execution.rs:120` |
| Other fallback warning ("runtime not found") | `crates/quarto-core/src/stage/stages/engine_execution.rs:114` |
| `get_engine_with_fallback()` | `crates/quarto-core/src/stage/stages/engine_execution.rs:102` |
| Engine sequence resolution loop | `crates/quarto-core/src/stage/stages/engine_execution.rs:199` |
| Registry cfg-gating (knitr/jupyter native-only) | `crates/quarto-core/src/engine/registry.rs:52` |
| Capture splice stage | `crates/quarto-core/src/stage/stages/capture_splice.rs` |
| q2-preview pipeline builder (has `captures`) | `crates/quarto-core/src/pipeline.rs:387` |

## Proposed approach (for discussion)

**Thread the spliced engine names into `EngineExecutionStage`** so it can
distinguish case (a) from case (b).

Sketch:

1. Add an optional field to `EngineExecutionStage`, e.g.
   `spliced_engines: HashSet<String>` (engine names whose output was
   already provided via capture), with a builder
   `EngineExecutionStage::new().with_spliced_engines(names)`.
2. In `get_engine_with_fallback()` (or its caller), when an engine is
   unavailable/unregistered **and** its name is in `spliced_engines`,
   **suppress** the "(no execution)" warning. Optionally emit a
   `trace_event!(Debug, …)` noting the output came from a server-side
   capture, so the information is still discoverable in `-v` traces.
3. In `build_q2_preview_pipeline_stages`, collect
   `captures.iter().map(|c| c.engine_name.clone()).collect()` and pass it
   to the `EngineExecutionStage` the builder already produces. Because the
   builder owns both stage constructions, no plumbing through
   `StageContext` is needed.

Why this shape:

- **Localized.** No changes to `StageContext`, the observer channel, or
  AST markers. The HTML pipeline (`q2 render`) doesn't set
  `spliced_engines`, so its behavior is identical.
- **Honors existing semantics.** The genuine no-execution case (no
  capture for the engine) still warns exactly as today.
- **Native render unaffected.** `q2 render` never goes through
  `CaptureSpliceStage`, so nothing changes there.

### Open questions for review

- **Suppress vs. reword?** Should the warning be fully suppressed, or
  replaced with an informational message like "Engine 'knitr' executed
  server-side; results spliced into preview"? A reworded message keeps the
  user informed that the preview path differs from a native render.
  Recommendation: suppress the *warning level* (it's not a problem the user
  can act on) and emit an `Info`/`Debug` trace instead — but happy to make
  it a visible info note if you prefer.
- **Granularity.** Should suppression be keyed per-engine-name (knitr
  spliced ⇒ suppress only knitr's warning) so that a *second*, genuinely
  unavailable engine with no capture still warns? The `HashSet<String>`
  approach gives this for free; confirm that's the desired behavior.
- **Mismatch safety.** If a capture is present but the splice failed
  (parse error / unmatched cell — `CaptureSpliceStage` is fail-soft and
  leaves raw source), suppressing the warning could hide a real
  "no execution happened" situation. Do we want suppression keyed on
  "capture was *attempted*" (current sketch) or "splice *succeeded*"? The
  latter is more honest but requires the splice stage to report success
  back, which reintroduces cross-stage plumbing. Leaning toward the
  simpler "capture present" key, with the fail-soft splice warnings
  (already emitted by `CaptureSpliceStage`) covering the failure case.

## Decisions (resolved before implementation)

- **Suppress, not reword.** The warning is dropped at the *warning level*
  for spliced engines — it's not actionable, and the user saw real output.
  (Discoverability via trace is already covered by the existing
  "resolved to markdown (no-op) — skipping" Debug `trace_event!` in the
  resolution loop.)
- **Per-engine granularity** via `HashSet<String>` — a sibling engine with
  no capture still warns.
- **Keyed on "capture present"**, not "splice succeeded". `CaptureSpliceStage`
  is fail-soft and emits its own `Warn` trace events on splice failure, so
  the failure case stays visible without cross-stage plumbing.
- **Both fallback branches suppressed.** The suppression covers both
  "not available in this build" (unregistered, WASM) *and* "runtime not
  found" (registered but unavailable), since both are equally misleading
  once output was spliced.

## TDD plan

Tests first (per project TDD policy):

- [x] Unit test constructing an `EngineExecutionStage` with a spliced engine
      name + a registry lacking it; assert **no** warning
      (`test_spliced_engine_suppresses_fallback_warning`, plus end-to-end
      through `run`: `test_spliced_engine_no_diagnostic_through_run`).
- [x] Companion test with **empty** `spliced_engines`: existing
      `test_unknown_engine_falls_back` and
      `test_engine_fallback_with_unavailable_engine` guard the un-suppressed
      path.
- [x] Two-engine granularity test: one spliced, one not — assert only the
      un-spliced engine warns
      (`test_unspliced_engine_still_warns_when_sibling_spliced`).
- [x] q2-preview builder wiring test: run the pipeline with a synthetic
      capture and assert no diagnostic
      (`q2_preview_capture_suppresses_engine_unavailable_warning`), plus a
      negative companion
      (`q2_preview_without_capture_still_warns_unavailable_engine`).

## Implementation checklist

- [x] Write failing tests (above) and confirm they fail.
- [x] Add `spliced_engines` field + builder to `EngineExecutionStage`.
- [x] Suppress the warning when the engine name is spliced (both branches).
- [x] Pass capture engine names from `build_q2_preview_pipeline_stages`
      (also preserves the caller's engine registry — previously the
      replay-registry path; reconstructs the stage rather than discarding it).
- [x] Make tests pass (`-p quarto-core`: 2311 passed).
- [x] `cargo nextest run --workspace` — 9942 passed (1 leaky), 196 skipped.
- [x] `cargo xtask verify --skip-rust-tests` — all steps passed, including
      the WASM rebuild + hub-client build + hub tests.
- [x] End-to-end: `q2 preview` a `engine: knitr` fixture in a live browser
      (see below).

## End-to-end verification (2026-06-10)

**Invocation:**

```bash
# fixture .tmp-knitr-preview/test.qmd: engine: knitr, two R chunks
#   summary(cars)   and   1 + 1
cargo build --bin q2                # re-embed freshly-rebuilt SPA/WASM
cargo run --bin q2 -- preview .tmp-knitr-preview/test.qmd --no-browser
# → http://127.0.0.1:61723/?page=test.qmd
```

The native host ran knitr (`processing file: test.rmarkdown` /
`output file: test.knit.md` in the preview log), producing the capture.
Loaded the preview URL in Chrome and inspected the rendered DOM.

**Observed (inspected, not inferred):**

- The iframe rendered the **real R execution output** spliced into the
  WASM preview:
  - `summary(cars)` → the full stats table (`Min. : 4.0`, `Mean : 15.4`,
    `Max. : 25.0`, dist column, etc.).
  - `1 + 1` → `[1] 2`.
- The misleading warning is **gone**: searching the entire parent
  document *and* the iframe for `"not available in this build"`,
  `"no execution"`, and `"not available (runtime not found)"` returned
  **zero** matches, and there were **zero** diagnostics/warning overlay
  elements.

This confirms the fix on the exact path a user runs: real `q2 preview`
binary → WASM render → spliced knitr output visible, misleading warning
suppressed. (The pre-fix behaviour — warning present — is locked in by
the negative tests `test_unknown_engine_falls_back` and
`q2_preview_without_capture_still_warns_unavailable_engine`.)

## References

- `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md` —
  capture/splice architecture rationale (bd-lucp, bd-5yff4, bd-45yw).
- `crates/quarto-core/src/engine/capture_splice.rs` — AST-level splice.
