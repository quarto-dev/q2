# Task 6 Report — Registry primitives: `contribution_order` + engine-shutdown machinery

## Status: COMPLETE

## Commit

`9191eb1b0` on branch `feature/ts-engine-extensions`

## Changes

Three files touched:

### `crates/quarto-core/src/engine/traits.rs`
Added `ExecutionEngine::shutdown()` default method (no-op, returns `Ok(())`) with full
idempotency contract documented in the doc comment. Placed after `quarto_required()`,
before the trait's closing `}`.

### `crates/quarto-core/src/engine/ts_engine.rs`
Added `TsEngine::shutdown()` override near the `quarto_required()` override (~line 728):
```rust
fn shutdown(&self) -> Result<(), ExecutionError> {
    self.host.shutdown()
}
```
Delegates directly to `TsEngineHost::shutdown()`, which is already idempotent via
`Option::take()` guards on all subprocess handles.

### `crates/quarto-core/src/engine/registry.rs`
- Added `use super::ExecutionError;` import.
- Added `pub contribution_order: Vec<String>` field to `EngineRegistry` struct (with doc
  comment explaining consumer intent).
- Initialized `contribution_order: Vec::new()` in all three struct-literal constructors:
  `new()`, `empty()`, and `with_replay_many()`.
- Added `shutdown_all(&self) -> Result<(), ExecutionError>` method: best-effort iteration
  over all engines, returns first error, continues through the rest.

## Tests (TDD — RED then GREEN)

Three tests added to `engine::registry::tests`:

| Test | Gate | Result |
|------|------|--------|
| `test_shutdown_all_noop_on_builtins` | always | PASS |
| `test_contribution_order_roundtrip` | always | PASS |
| `test_shutdown_all_kills_ts_engine` | `deno_is_available()` | PASS (Deno ran) |

Command:
```
cargo nextest run -p quarto-core -E 'test(engine::registry) or test(engine::ts_engine::tests::shutdown)'
```
Output: `16 tests run: 16 passed, 2587 skipped`

The Deno-gated test (`test_shutdown_all_kills_ts_engine`) ran and passed. It:
1. Spawned a real subprocess via `TsEngineHost::start_with_command(sh -c 'cat >/dev/null', ...)`
2. Asserted `host.is_alive() == true` (exercised-guard)
3. Wrapped the host in a `TsEngine`, registered it in a `EngineRegistry::empty()`
4. Called `registry.shutdown_all()`
5. Asserted `host.is_alive() == false`

Build verification:
- `cargo build -p quarto-core` — clean (no errors, no warnings)
- `cargo build -p quarto-core --tests` — clean (no errors, no warnings)

## Concerns

None. Implementation is exactly as specified in the brief. No scope creep.
