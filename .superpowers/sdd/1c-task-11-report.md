# Task 11 Report — P2-12 + P2-13

## Summary

Both P2-12 and P2-13 are implemented, tested GREEN, and all 2595 quarto-core
tests pass. Clippy reports zero warnings.

---

## P2-13 (implemented earlier in this session)

### What changed

`partition_cells` in `crates/quarto-core/src/engine/jupyter/text_execute.rs`
gained a `multi_engine: bool` third parameter. When `false` (single-engine
sequence), owned-but-unrunnable cells are passed through unexecuted rather than
raising `NoHandlerForLanguage`. When `true` (multi-engine), the existing loud
error fires.

`ExecutionContext` gained `multi_engine: bool` (default `false`) and
`with_multi_engine(bool)`. `engine_execution.rs` computes `let multi_engine =
to_run.len() > 1` before the `into_iter()` move and passes it via
`.with_multi_engine(multi_engine)`.

### Tests (P2-13)

| Test | File | Result |
|------|------|--------|
| `test_partition_cells_owned_unrunnable_fails_loudly` | `text_execute.rs` | GREEN (updated to `multi_engine=true`) |
| `test_partition_cells_single_engine_owned_unrunnable_passthrough` | `text_execute.rs` | GREEN (new, `multi_engine=false` → Ok) |
| `test_partition_cells_cede` | `text_execute.rs` | GREEN (updated to pass `false`) |
| `test_partition_cells_execute` | `text_execute.rs` | GREEN (updated to pass `false`) |
| `test_partition_cells_mixed` | `text_execute.rs` | GREEN (updated to pass `false`) |

---

## P2-12 — Registered owning engine unavailable → loud error

### What changed

`get_engine_with_fallback` in `engine_execution.rs` return type changed from
`Arc<dyn ExecutionEngine>` to `Result<Arc<dyn ExecutionEngine>, PipelineError>`.

New behaviour matrix:

| Registered? | `is_available()` | In `spliced_engines`? | Result |
|-------------|------------------|----------------------|--------|
| Yes | true | — | `Ok(engine)` |
| Yes | false | No | **`Err(PipelineError::stage_error(...))`** ← P2-12 |
| Yes | false | Yes | `Ok(markdown)` silently (capture replay) |
| No | — | No | `Ok(markdown)` + warning |
| No | — | Yes | `Ok(markdown)` silently |

`run()` now propagates the error via `?` at the call site.

### Blast-radius analysis

One test asserted the now-obsolete silent-fallback contract:

| Test | File | What changed | Why |
|------|------|-------------|-----|
| `q2_preview_without_capture_still_warns_unavailable_engine` | `pipeline.rs` | Renamed to `q2_preview_without_capture_errors_unavailable_engine`; assertion changed from "Ok + `not available` warning" → "Err + engine name in message" | Was asserting the old silent-fallback behaviour P2-12 intentionally removes |

Three unit tests call `get_engine_with_fallback` directly (all test the
UNREGISTERED path, which still returns `Ok`):

| Test | Change | Why |
|------|--------|-----|
| `test_engine_fallback_with_unavailable_engine` | Added `.expect()` | Return type changed to `Result` |
| `test_spliced_engine_suppresses_fallback_warning` | Added `.expect()` | Return type changed to `Result` |
| `test_unspliced_engine_still_warns_when_sibling_spliced` | Added `.expect()` (both calls) | Return type changed to `Result` |

These tests still exercise unregistered engines (else branch), which return
`Ok(markdown)` both before and after P2-12 — their semantics did not change.

### New P2-12 tests

| Test | File | RED → GREEN |
|------|------|-------------|
| `test_p2_12_owning_engine_unavailable_fails_loudly` | `engine_execution.rs` | RED (result.is_ok(), expected Err) → GREEN |
| `test_p2_12_spliced_unavailable_engine_still_silent` | `engine_execution.rs` | Was already PASS (spliced path unchanged); GREEN from start |

### Vacuity check

Vacuity revert described in test comment: removing the `is_available()` gate
(falling back to markdown for ALL registered engines) causes
`test_p2_12_owning_engine_unavailable_fails_loudly` to fail with "got Ok (old
silent-fallback behaviour)". Confirmed at RED run before implementation.

---

## Test counts

- Tests added: 2 (P2-12: `test_p2_12_owning_engine_unavailable_fails_loudly`,
  `test_p2_12_spliced_unavailable_engine_still_silent`)
- Tests modified: 1 renamed + 1 assertion changed (`q2_preview_without_capture_*`)
  + 4 `.expect()` additions + 5 P2-13 arg updates
- Total quarto-core: **2595 passed, 0 failed**
- Clippy: **0 warnings**
