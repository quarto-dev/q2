# Task 5 Report — Infra leaves

## Status: COMPLETE

## Files changed

- `crates/quarto-util/src/data_dir.rs` — new module: `data_dir_from()` pure helper + `quarto_data_dir()` IO wrapper + 5 tests
- `crates/quarto-util/src/lib.rs` — added `pub mod data_dir;` + `pub use data_dir::quarto_data_dir;`
- `crates/quarto-system-runtime/src/traits.rs` — added `is_interactive()` (default `false`) + `running_in_ci()` (default reads `env_get("CI")`) to `SystemRuntime` trait; added 5 tests with `CiMockRuntime` inline mock
- `crates/quarto-system-runtime/src/native.rs` — added `is_interactive()` override (`std::io::stdin().is_terminal()`); added `is_interactive_native_false_under_nextest` test

## Verification commands and output

```
cargo nextest run -p quarto-util -p quarto-system-runtime
```

136 tests run: **136 passed, 0 skipped**

New tests by file:
- `quarto-util data_dir::tests::data_dir_from_both_none_returns_none` PASS
- `quarto-util data_dir::tests::data_dir_from_data_dir_branch_last_component_is_quarto` PASS
- `quarto-util data_dir::tests::data_dir_from_falls_back_to_data_dir_with_quarto_suffix` PASS
- `quarto-util data_dir::tests::data_dir_from_override_wins_and_is_used_as_is` PASS
- `quarto-util data_dir::tests::quarto_data_dir_returns_existing_directory` PASS
- `quarto-system-runtime traits::tests::running_in_ci_true_for_nonempty_value` PASS
- `quarto-system-runtime traits::tests::running_in_ci_true_for_one` PASS
- `quarto-system-runtime traits::tests::running_in_ci_false_for_empty_string` PASS
- `quarto-system-runtime traits::tests::running_in_ci_false_when_not_set` PASS
- `quarto-system-runtime traits::tests::is_interactive_default_is_false` PASS
- `quarto-system-runtime native::tests::is_interactive_native_false_under_nextest` PASS

```
cargo build -p quarto-core
```

Clean — no existing `impl SystemRuntime` required updating (both new methods have defaults).

## Design decisions

**`QUARTO_DATA_DIR` override semantics**: honored as-is (no `quarto` suffix appended). The `quarto` suffix is only appended to the `dirs::data_dir()` fallback branch. This mirrors Q1's `quartoDataDir()` which treats `QUARTO_DATA_DIR` as the quarto data root directly. Documented in the `data_dir_from` doc-comment and asserted in `data_dir_from_override_wins_and_is_used_as_is`.

**`running_in_ci` test isolation**: used a minimal inline mock (`CiMockRuntime`) that controls only `env_get("CI")`. All other required methods are `unimplemented!()`. This avoids reading or mutating the real process environment — no parallel-test races.

**`is_interactive` true-path not unit-tested**: `NativeRuntime::is_interactive()` delegates to `std::io::stdin().is_terminal()`. The `true` path requires an actual PTY, which nextest does not provide. The test asserts `false` under nextest (correct — no TTY) and notes the true path is not covered by unit tests. This is acceptable: the implementation is a one-liner with no logic to test beyond the bool flip.

## Out of scope

`HostGlobalConfig` construction (Task 7) — not touched. `QUARTO_DATA_DIR` is not consumed anywhere except `quarto_data_dir()`.
