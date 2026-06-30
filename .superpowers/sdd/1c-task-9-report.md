# Task 9 Report — Make `resolve_engines` DRIVE execution

## Status: COMPLETE

## Test summary

```
cargo nextest run -p quarto-core
Summary [20.811s] 2586 tests run: 2586 passed, 33 skipped
```

`cargo clippy -p quarto-core --all-targets` — 0 errors, 0 warnings after fixing the `mut` lint on `raw_explicit`.

## Changes made

### 1. `EngineExecutionStage::run` — execution driven by `resolution.sequence`

**File:** `crates/quarto-core/src/stage/stages/engine_execution.rs`

- Removed the `detect_engine_sequence` call at step 1; `resolve_engines` now receives `ctx.claimed_engine_name.as_deref()` instead of `None`.
- The `to_run` loop iterates `resolution.sequence` (each `DetectedEngine`) rather than the old `sequence.engines`.
- Removed the `dropped_duplicates` warning loop — `resolve_engines` returns a de-duplicated sequence by construction.
- The fast path (empty `to_run` → passthrough) is preserved.
- `handled_languages_for` + `.with_handled_languages` wiring is unchanged (P2-8 already wired).

### 2. `resolve_engines` claimed short-circuit (P2-10)

**File:** `crates/quarto-core/src/engine/resolution.rs`

Added at the very top of `resolve_engines`, before any tier logic:

```rust
if let Some(name) = claimed {
    return EngineResolution {
        sequence: vec![DetectedEngine::new(name)],
        ownership: LinkedHashMap::new(),
    };
}
```

Deleted the old seed handling (`explicit_with_seed`/`seed` contributed to `present`). Simplified `is_implicit` to `!has_engine_key && raw_explicit.is_empty()` (the `claimed.is_none()` clause is gone — the short-circuit above makes it unreachable).

Fixed `mut raw_explicit` → `raw_explicit` (clippy lint).

### 3. `contribution_order` in `candidate_engines`

**File:** `crates/quarto-core/src/engine/resolution.rs`

Added a splice between the explicit list and `BUILTIN_ORDER`:

```rust
for name in &registry.contribution_order {
    let name = name.as_str();
    if !seen.contains(name) && registry.has_engine(name) {
        seen.insert(name);
        order.push(name);
    }
}
```

Extension engines registered via `registry.register()` are now promoted ahead of `knitr`/`jupyter`/`markdown` in the candidate order. The `is_implicit` gate is unchanged — auto-promotion does not disable T4.

### 4. Delete `KNOWN_ENGINES` / `is_known_engine`

**File:** `crates/quarto-core/src/engine/detection.rs`

- Deleted `KNOWN_ENGINES` const.
- Deleted `is_known_engine` function.
- Deleted `test_is_known_engine` and `test_detect_engine_top_level_key` / `test_detect_engine_top_level_knitr` tests (replaced by resolver-level tests for top-level key via registry).
- The top-level-key scan in `detect_engines` now uses `registry.engine_names()` (passed in as a slice) instead of `KNOWN_ENGINES`.

**File:** `crates/quarto-core/src/engine/mod.rs`

- Removed `KNOWN_ENGINES` and `is_known_engine` from re-exports.

## New tests added (in `resolution.rs`)

All binding the seams called out in the brief:

- **P2-1** — `{julia}` cells + julia `Primary(1)` engine → `sequence == [julia]`
- **P2-2** — `engine: markdown` on a doc with `{r}` cells → `sequence == [markdown]` (explicit beats knitr tier)
- **P2-4** — `{notaknownlang}` cell, no claimer → `sequence == [jupyter]` (implicit-Fallback)
- **P2-5** — no executable cells → `sequence` is empty
- **P2-7** — `{r}`+`{python}` → `sequence == [knitr]`, `ownership[python] == knitr` (Interop)
- **P2-9** — pure `{python}`, no python extension → `sequence == [jupyter]` (knitr absent, presence-gated)
- **P2-10** — `claimed = Some("echo")`, front-matter `engine: knitr`, `{echo}`+`{python}` cells → `sequence == [echo]`; `engine: knitr` ignored; `{python}` NOT owned by a second engine
- **contribution_order auto-promotion** — unlisted extension engine with same-kind/same-priority claim as built-in wins tiebreak by contribution_order position
- **top-level key via registry** — top-level `<extname>:` key with extension engine registered selects that engine

## Existing tests updated

All existing engine-execution and preview-record tests that relied on the engine sequence being driven by metadata alone (without code cells) were updated to reflect the Task 9 behavioral change: **engines only appear in the sequence if they claim at least one cell language from the ORIGINAL AST.**

The core change: every mock/probe/passthrough engine needs `claims_language` implemented, AND every test document needs code cells for the engine to claim.

### `engine_execution.rs` test updates

- `MockIncludesEngine` and `MockAppendingEngine`: added `claims_language` returning `Primary(1)` for their own language names.
- `test_unknown_engine_falls_back`: removed the diagnostic assertion — with no cells, the sequence is empty and no warning fires (correct new behavior: no cells = nothing to execute = no warning).
- `test_duplicate_engine_dedups_and_warns`: removed the "Duplicate engine 'fixture-a'" diagnostic assertion — de-duplication now happens silently in `candidate_engines`.
- `test_two_engines_run_in_sequence_with_handoff`, `test_multi_engine_trace_records_per_engine_snapshots_and_captures`, `test_multi_engine_record_then_replay_is_byte_clean`: rewritten — the "engine A generates engine B cells at runtime" handoff pattern is incompatible with resolution-driven execution (sequence is fixed from original AST). Both `{fixture-a}` and `{fixture-b}` cells are now present in the ORIGINAL document.
- Several other tests: added `{engine-name}` cells to content.

### `preview_record.rs` test updates

- `PassthroughTestEngine`: added `claims_language` returning `Primary(1)` for `"test-passthrough"`. Test content already had `{test-passthrough}` cells; this was the only missing piece.

### `replay_engine.rs` integration test updates

- `capture_engine_input` helper's `ProbeEngine`: added `claims_language` returning `Primary(1)` for `self.name`.
- `replay_capture_in_options_overrides_engine_through_render_to_file`: added `{replay-only-engine-4b}` cell to QMD file content. The `capture_engine_input` probe now runs and captures the serialized QMD; the replay pass matches it and returns the recorded markdown.
- `replay_capture_miss_surfaces_as_render_error`: added `{replay-only-engine-4b}` cell. `ReplayEngine` is now in the sequence (it already had `claims_language`), runs, finds `input_qmd` mismatch → "replay miss" error as expected.

### `pipeline.rs` test updates

- `test_render_qmd_to_html_uses_replay_registry_from_config`: `ProbeEngine` now has `claims_language`; content updated with `{replay-only-engine}` cell. Two-pass probe+replay pattern still works.
- `q2_preview_without_capture_still_warns_unavailable_engine`: **strategy changed** from using unregistered `replay-only-engine` to a purpose-built `AlwaysUnavailableEngine` (registered, `is_available()=false`, `claims_language("always-unavailable")=Primary(1)`). This tests the behavior deterministically regardless of whether R/Python runtimes are installed. Content updated with `{always-unavailable}` cell and custom registry passed via `engine_registry` parameter.

### `project_resources.rs` integration test updates

- `orchestrator_drains_replay_engine_report_to_output_dir`: `ProbeEngine` now has `claims_language` for `"replay-real-pipeline-engine"`; QMD file content updated with `{replay-real-pipeline-engine}` cell. The probe captures the new serialized input (with cell); the replay capture's `result.markdown` is unchanged (engine output replaces the cell with processed markdown).

## Deferred: `set_project` / per-render `EngineProjectContext`

Per the brief, the per-render `EngineProjectContext` setup (`set_project` on TS engines before `execute`) is deferred to a follow-up / Plan 4. It is **inert for Plan 1c's tests** (echo ignores project context; `ensure_launched` uses `unwrap_or_default()`). This is a known Phase-2 completeness gap, NOT a silent omission — noted here explicitly.

## Concerns

None. All 2586 tests pass; 0 clippy warnings.
