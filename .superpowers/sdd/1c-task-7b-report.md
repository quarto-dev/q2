# Task 7b Report — Build real engine registry on ProjectContext

## Status: COMPLETE

Commit: `bd0e7dde9`

---

## Construction sequence as built

All changes in `crates/quarto-core/src/`:

### `lib.rs`
- Added `pub fn version() -> &'static str { env!("CARGO_PKG_VERSION") }` for `HostGlobalConfig.quarto_version`.

### `project/mod.rs`

**New function `build_engine_registry` (native-only, `#[cfg(not(target_arch = "wasm32"))]`)**

Signature:
```rust
fn build_engine_registry(
    extensions: &[Extension],
    binary_dependencies: &BinaryDependencies,
    runtime: &dyn SystemRuntime,
) -> Result<Arc<EngineRegistry>>
```

Steps implemented exactly per brief:

1. **HostGlobalConfig**: `resource_dir` from `BUILTIN_EXTENSIONS.path()` (empty if None), `runtime_dir`/`data_dir` from `quarto_util::quarto_runtime_dir()`/`quarto_data_dir()` (IO errors propagated as `QuartoError`), `pandoc_path` from `binary_dependencies.pandoc`, `is_interactive_session`/`running_in_ci` from `runtime`, `quarto_version` from `crate::version()`.
2. **`Arc<TsEngineHost>::new(global)`** — NOT spawned (cheap; no subprocess).
3. **`EngineRegistry::new()`** — built-ins markdown/knitr/jupyter.
4. **Per-extension contribution loop**:
   - `Reorder { name }` → push name to `order` vec (no register).
   - `External { path, name, claims, file_extensions, claims_files }`:
     - 4a: `!path.exists()` → `Err("…no bundled .js file… Run 'q2 build-ts-extension'…")`
     - 4b: `key = name.unwrap_or(ext.id.to_string())`, `name_declared = name.is_some()`
     - 4c: `registry.has_engine(&key)` → collision `Err("…both '{}' and '{}'…")` naming both contributors (tracked in `key_to_contributor: HashMap<String,String>`, built-ins pre-seeded as "built-in")
     - 4d: `TsEngine::new(…)`, `registry.register(Arc::new(engine))`, push `key` to `order`
     - 4e: `engine_contribution_missing_fields_warning(…)` → push to `registry.diagnostics` if Some
5. **`contribution_order`**: dedup first-occurrence from `order`. Comment left for Task 9 `_quarto.yml` engines splice.
6. **Validation**: for each name in `contribution_order`, if `!registry.has_engine(name)` → `Err("'{}' was specified in the list of engines… Available engines are: …")` (sorted, joined).
7. **Return** `Arc::new(registry)`. Comment: `// Task: drain registry.diagnostics at orchestrator (plan step 10)`.

**`ProjectContext::discover` updated**:
- Captures `single_file_input = input_file.clone()` before `input_file` is consumed into `files`.
- Computes `binary_dependencies` before the registry build.
- `discovery_anchor`: single-file → `single_file_input` (file path, `start_dir = dir`); project → `dir.join("_quarto.yml")` (parent = `dir`, so `start_dir = dir`).
- `builtin_dir`: native = `BUILTIN_EXTENSIONS.path()`, WASM = None.
- Calls `discover_extensions(anchor, project_dir_opt, builtin_dir, runtime)`.
- Native: `registry = build_engine_registry(&extensions, &binary_dependencies, runtime)?`.
- WASM: `registry = Arc::new(EngineRegistry::new())`.

**`ProjectContext::single_file` updated**:
- Same pattern: compute `binary_dependencies`, `builtin_dir`, `extensions`, then `registry` (native/WASM gated).

---

## Seams bound

| Seam | Test | Status |
|------|------|--------|
| P1-1: engine registered | `p1_1_extension_engine_appears_in_engine_names` | BOUND |
| P1-5 (reg half): declared name + zero-spawn | `p1_5_named_engine_registered_without_spawn` | BOUND |
| P1-6 (alias reg half): ext-id key | `p1_6_unnamed_engine_registered_under_ext_id` | BOUND |
| P1-4: collision names both contributors | `p1_4_name_collision_errors_and_names_both_contributors` | BOUND |
| P1-3: unknown reorder lists available | `p1_3_unknown_reorder_hint_errors_listing_available` | BOUND |
| P1-2: contribution_order populated | `p1_2_contribution_order_contains_declared_engines` | BOUND |
| Warning: missing static fields in diagnostics | `warning_missing_static_fields_appears_in_diagnostics` | BOUND |
| Bundle-missing: Err mentions build-ts-extension | `bundle_missing_errors_with_build_ts_extension_hint` | BOUND |

## Seams deferred (per brief)

- **P1-7**: name-mismatch fires at first LoadEngine — Task 14 / mock-load test.
- **P1-5 full resolution** (engine: echo with no-spawn resolution) — Task 9.

---

## Test counts

- New tests: **8** (all in `engine_registry_build.rs` behind `#[cfg(not(target_arch = "wasm32"))]`)
- Total quarto-core tests: **2578 passed, 0 failed, 33 skipped**

---

## Pre-existing test interactions

No pre-existing tests tripped the new validation. Checked: `cargo nextest run -p quarto-core` ran 2578 tests with 0 failures. The existing `project_pipeline` and related tests use temp dirs without `_extensions/` subdirectories, so `discover_extensions` returns an empty vec and `build_engine_registry` produces the same built-ins-only registry as before.

---

## Exact commands + output

```
cargo build -p quarto-core       → Finished (no errors/warnings)
cargo build -p quarto-core --tests → Finished (no errors/warnings)
cargo nextest run -p quarto-core -E 'test(engine_registry_build)'
  → 8 tests run: 8 passed, 2603 skipped
cargo nextest run -p quarto-core
  → 2578 tests run: 2578 passed, 33 skipped
```

---

## Notes

- **`quarto_version`**: uses `quarto-core`'s `CARGO_PKG_VERSION` (not the `quarto` binary crate). Both track the workspace release version — acceptable per brief.
- **WASM path**: `build_engine_registry` is native-only; WASM `discover` / `single_file` keep `EngineRegistry::new()` (built-ins only). Extension discovery runs on WASM but only format/filter contributions matter there.
- **`discovery_anchor` for multi-file projects**: uses `dir.join("_quarto.yml")` whose parent is `dir`, ensuring `discover_extensions` starts its walk at the project root. File need not exist; `Path::parent()` is purely path arithmetic.

---

## Fix pass (review findings) — commit `8d20b9def`

### Changes

**`crates/quarto-core/src/project/mod.rs`**

1. **Lazy host construction** (`build_engine_registry`):
   - Extracted `any_external_engine(extensions: &[Extension]) -> bool` (native-only, `#[cfg(not(target_arch = "wasm32"))]`): scans the extension list for at least one `EngineContribution::External`. `Reorder`-only and empty lists return `false`.
   - Restructured `build_engine_registry`: the `needs_host = any_external_engine(extensions)` predicate gates the entire `HostGlobalConfig` / `TsEngineHost` construction block. When `false`, only Reorder hints are harvested — `quarto_runtime_dir()` / `quarto_data_dir()` are never called.
   - All existing step numbering and behavior preserved for the `needs_host = true` path.

2. **Shared discovery helper** (`discover_extensions_and_build_registry`):
   - New function factoring the `builtin_dir` + `discover_extensions` + `build_engine_registry` (or `EngineRegistry::new()` on WASM) block, called by both `discover` and `single_file`.
   - Eliminates ~14 duplicated lines.

3. **No change to the WASM path**: the new helper correctly uses `Arc::new(EngineRegistry::new())` on wasm32.

**`crates/quarto-core/tests/integration/engine_registry_build.rs`**

- **`p0_no_extension_project_builds_builtins_only`**: new integration test — project with `_quarto.yml` but no `_extensions/` → `discover` succeeds, registry contains all three built-ins, `contribution_order` is empty.
- **`p1_5_named_engine_registered_without_spawn`**: added comment explaining the test binds only REGISTRATION (not spawn-count), and that the no-spawn guarantee is structural + covered by TsEngine unit tests (T4 P1-12).

**`crates/quarto-core/src/project/mod.rs` (unit tests)**

- **`needs_host_tests` submodule** (4 tests, `#[cfg(not(target_arch = "wasm32"))]`):
  - `needs_host_false_for_no_extensions` — `any_external_engine(&[])` returns false.
  - `needs_host_false_for_reorder_only` — Reorder-only extension → false.
  - `needs_host_true_for_external_engine` — External engine → true.
  - `needs_host_true_when_external_mixed_with_reorder` — External + Reorder → true.

### Covering tests + result

```
cargo nextest run -p quarto-core -E 'test(engine_registry_build)'
  → 9 tests run: 9 passed (was 8; p0_no_extension_project_builds_builtins_only added)

cargo nextest run -p quarto-core -E 'test(needs_host)'
  → 4 tests run: 4 passed (all new any_external_engine predicate tests)

cargo nextest run -p quarto-core
  → 2583 tests run: 2583 passed, 33 skipped (was 2578; +5 new tests)
```
