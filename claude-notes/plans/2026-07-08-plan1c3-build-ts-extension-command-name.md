# Plan 1c3: `q2 call build-ts-extension` rename + extracted build lib + hermetic self-regenerating fixtures

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** (1) Rename `q2 build-ts-extension` → `q2 call build-ts-extension` (Q1 parity); (2) extract the extension-bundle build logic out of the bin-only `quarto` crate into a `quarto-core` library so both the CLI and the tests can call it in-process; (3) stop committing the synth engine `dist/*.js` — delete them, `.gitignore` them, and have the e2e suites **regenerate each bundle at test time** hermetically via that shared library.

**Architecture:** The build logic (deno.json resolution + `deno bundle` spawn) currently lives in `crates/quarto/src/commands/build_ts_extension.rs`, unreachable from `quarto-core`'s tests (bin-only crate; `CARGO_BIN_EXE_q2` is same-package-only; no lib target). Move it to `quarto-core::extension::build` (native-gated, like `engine::ts_process`), make the CLI command a thin wrapper, and call the same function in-process from the tests. The 8 synth engines + `echo-engine`/`echo-legacy` import only `@quarto/api/claims` (pure, type-only transitive deps) and `@quarto/types` (erased); after switching the 7 barrel-importing engines to the `@quarto/api/claims` subpath, `deno bundle` resolves their graph from local `ts-packages/` with no npm/jsr/network and no `deno.lock` (~2 KB, ~10 ms — validated). `julia-engine`/`marimo` pull jsr/`deno.land` deps and stay committed.

**Tech Stack:** Rust (clap 4 derive; `anyhow`, `serde_yaml`, `tempfile` — all already quarto-core deps), Deno 2.9 `deno bundle`, `resources/extension-build/deno.workspace.json` import map, nextest.

## Global Constraints

- **Base branch:** `feature/ts-engine-extensions`. Runs after plan6 (already on the branch), before plan9 (`q2 call engine`); plan9 adds an `Engine` variant to the `CallCommands` enum introduced here.
- **Q1-authoritative command** (`external-sources/quarto-cli/src/command/call/cmd.ts:1-17`): `quarto call build-ts-extension`. We match the `call build-ts-extension` **path** only; not Q1's `--check`/`--init-config` flags.
- **quarto-core compiles to wasm32** (hub-client). The extracted build module spawns `deno` via `std::process`, unavailable on wasm — it MUST be `#[cfg(not(target_arch = "wasm32"))]` gated (mirror `engine::ts_process`). Final verification runs **full `cargo xtask verify`** (not `--skip-hub-build`) because this touches quarto-core's wasm leg.
- **Hermeticity:** test-time regeneration must not hit the network, fetch npm/jsr, or write into any git-tracked dir. Validated: subpath-fixed fixtures build with 0 external modules, no `deno.lock`, into a tempdir. The build config `deno.workspace.json` sets `nodeModulesDir: auto`; its `node_modules/` sibling under `resources/extension-build/` is **already git-ignored**, and the claims-subpath build writes **nothing** there (re-confirmed 2026-07-22: `alpha` via `@quarto/api/claims` → `Bundled 2 modules`, 2.03 KB, 0 md5 refs, no `deno.lock`, 0 new `node_modules/` files).
- **Deno is a hard dep** only for the *executing* engine suites (they build+load bundles); they gate on `deno_available()` and skip when absent — preserve that. `engine_registry_build` is NOT such a suite (see Task 5).
- **TDD:** adjust/author the failing test first, watch it fail, implement, watch it pass. `cargo nextest run` (never `cargo test`), run directly (never piped to `tail`).

## Background (corrected diagnosis)

An earlier reading called the committed synth `dist/*.js` "ad-hoc esbuild, not deno-bundle output." **Empirical spikes disproved this:** deno 2.9's `deno bundle` *is* esbuild internally (same prelude, no `// deno:` markers); a canonical rebuild of `alpha` was byte-near-identical. `julia`/`marimo`'s `// deno:` markers just mean an older deno built them. So the bundles were valid and `synth_engines_e2e.rs`'s "real deno bundle output" docstring was accurate — the "fix false docstring / rebuild esbuild→deno" tasks were dropped.

Real findings driving this plan:
1. **Command name** — Q1 parity is `quarto call build-ts-extension`; q2 shipped it top-level (`plan1c` line 470 vs the same item's line 481).
2. **`@quarto/api` barrel drags crypto→`blueimp-md5` into every barrel import** — `q2 build-ts-extension --workspace` *fails* for the 7 synth engines importing `@quarto/api`. `primary`/`interop`/`fallback` all live in `@quarto/api/claims/index.ts` (only `import type { LanguageClaim }`, zero crypto path). Switching to the `@quarto/api/claims` subpath makes the build hermetic — **validated:** `Bundled 2 modules in 10ms`, 2 KB, 0 md5 refs, no lock.
3. **The build logic is trapped in a bin-only crate**, so quarto-core's tests can't invoke it — hence the extraction (Task 2).

**Hermeticity scope** (from a runtime-import scan): regenerable = the 8 synth (`alpha, beta, behave, mismatch, content-claim, fallback-univ, interop-r, whenclass-marimo`) + `echo-engine`, `echo-legacy` — all under `.../dist/`. NOT hermetic (stay committed, under `.../_extensions/`): `julia-engine` (`@std/path`, `fs/exists`, `encoding/base64`) and `marimo` (`path`, a `https://deno.land/...` URL).

## File map

| File | Change |
|---|---|
| `crates/quarto/src/main.rs` | Remove top-level `BuildTsExtension` variant + dispatch; add `CallCommands { Test, BuildTsExtension }` subcommand group; rewrite `Call` dispatch. |
| `crates/quarto-core/src/extension/build.rs` (new, native-gated) | Moved build logic + `pub fn build_ts_extension(BuildOptions) -> anyhow::Result<PathBuf>` + moved unit tests. |
| `crates/quarto-core/src/extension/mod.rs` | `#[cfg(not(target_arch = "wasm32"))] pub mod build;` |
| `crates/quarto/src/commands/build_ts_extension.rs` | Reduce to a thin wrapper: `BuildTsExtensionArgs` → `quarto_core::extension::build::build_ts_extension`. Doc `//! q2 call build-ts-extension`. |
| `crates/quarto/src/commands/call/mod.rs` | Doc mentions `build-ts-extension`. |
| `crates/quarto/tests/integration/build_ts_extension_e2e.rs` | Invoke `["call","build-ts-extension"]`; fix prose strings (lines 4/6/51/100/112). |
| `crates/quarto-core/src/extension/read.rs` | Hint (:415) + doc (:1409) → `q2 call build-ts-extension`; tighten assertions (:1427/:1451/:1475 `.contains`, messages :1428/:1452/:1476). |
| `crates/quarto-core/src/project/mod.rs` | Hint (:748). |
| `crates/quarto-core/tests/integration/engine_registry_build.rs` | Tighten `:674` assertion + fix `:675` panic msg; **write a stub `.js`** where it used committed `dist/{alpha,beta}.js` (:112/:117) — these tests only need existence. |
| synth fixtures `src/*.ts` (7) | `"@quarto/api"` → `"@quarto/api/claims"`. |
| `crates/quarto-core/tests/integration/engine_fixture_build.rs` (new) | `pub fn build_bundle(ext_dir)` (in-process lib call) + `HERMETIC_FIXTURES` allowlist + validation test. Register in `main.rs`. |
| executing suites: `synth_engines_e2e`, `echo_engine_e2e`, `behave_engine_e2e`, `capture_splice_seam` | Build hermetic fixtures via the helper after copy. (`marimo_engine_e2e` installs only the non-hermetic `marimo` → no change; see Task 5 Step 3.) |
| `.gitignore` | `crates/quarto-core/tests/fixtures/extensions/*/dist/`, `resources/extension-build/deno.lock`. |
| deleted | The 10 committed `.../dist/*.js`. |

---

## Test Seam Spec (frozen — prevalidated 2026-07-22, `feature/ts-engine-extensions`)

Bound against the real tree at `afaed2c96`. **Frozen:** once a row is GREEN its
harness + assertions are fixed — fix production or the spec, never the test.
The line citations below (and in the refreshed task bodies) are the *current*
lines; plan6's `read.rs`/`project/mod.rs` edits shifted the numbers the task
bodies originally cited (`read.rs` `:405→:415`, `:1274→:1409`,
`:1292/:1316/:1340→:1427/:1451/:1475`; `project/mod.rs` `:710→:748`, existence
check `:707→:745`).

| # | Test (file::fn) | Tier | Real unit exercised | Seam: mount → trigger → assertion surface | Mock boundary | Named revert → RED |
|---|---|---|---|---|---|---|
| T1 | `build_ts_extension_e2e` P2-18 (rewired to `["call","build-ts-extension"]`) | e2e-rs (real `q2`, deno-gated) | main.rs clap → `commands::build_ts_extension::execute` → `quarto_core::extension::build::build_ts_extension` → `deno bundle` | temp-copy echo-engine; **delete** `dist/echo-engine.js`; run `q2 call build-ts-extension <dir>`; assert exit 0 **and** `.js` exists, non-empty, contains `"echo"` + `"export"` | none (real deno) | (a) remove `CallCommands::BuildTsExtension` variant/dispatch (main.rs) → clap "unrecognized subcommand" → `status.success()` RED; (b) comment out the `run_deno_bundle` call in the extracted lib → no `.js` → `js_path.exists()` RED |
| T2 | `build_ts_extension_e2e::top_level_build_ts_extension_removed` **(NEW — missing-test #1)** | e2e-rs (real `q2`, no deno) | main.rs `Commands` enum — *absence* of a top-level `BuildTsExtension` | run `q2 build-ts-extension <dir>` (no `call`); assert `!status.success()` + stderr names an unrecognized subcommand | none | re-add the top-level `#[command(name="build-ts-extension")] BuildTsExtension{…}` variant → command parses → `!status.success()` RED |
| T3 | `read.rs::{test_engine_ts_path_rejected, _uppercase_js_rejected, _mjs_path_rejected}` (`:1427/:1451/:1475`) | unit-rs | `read_extension` `.js`-extension validation (read.rs `~:407-419`) + hint `:415` | write `_extension.yml` w/ engine `path:` non-`.js`; `read_extension(..).unwrap_err()`; assert `err.contains("call build-ts-extension")` | none (tempdir) | (a) bypass the `!= Some("js")` `return Err` block → Ok → `.unwrap_err()` panics RED (primary/`.js`-validation); (b) drop the `call ` prefix from hint `:415` → `.contains("call build-ts-extension")` RED (rename) |
| T4 | `engine_registry_build.rs` bundle-**missing** (`:674`) | integration-rs (no deno) | registry-build existence guard `project/mod.rs:745` + hint `:748` | install extension declaring `path: dist/x.js` **without** writing the `.js`; build registry; assert `err.contains("call build-ts-extension")` | none | (a) revert `if !path.exists()` guard (`:745`) → no error → `.unwrap_err()` RED; (b) drop `call ` from hint `:748` → `.contains` RED |
| T5 | `engine_registry_build.rs` bundle-**present** (combo, `:108-140`) | integration-rs (**must stay deno-free**) | registry-build path resolution + Ok side | copy fixture src + **write a stub** `dist/{alpha,beta}.js` (placeholder) into the tempdir copy (was: committed bundle, now deleted); build; assert Ok / both engines registered | stub `.js` (existence only — never loaded/executed) | revert `:745` guard to always-error → combo build errors → Ok-assert RED |
| T6 | `extension::build::{resolve_build_config_*, find_workspace_root_*, materialize_shipped_config_*}` (moved verbatim) | unit-rs | the pure fns in their new home | call w/ crafted inputs; assert precedence/path | `shipped_config` FnOnce double | resolve: swap tier-1/tier-2 order → `explicit_wins` RED; find_workspace_root: change the `ts-packages/quarto-api` marker → `detects_ts_packages` RED; materialize: skip the temp-write → round-trip RED. **After the move, run `test(extension::build)` to confirm no stray `#[cfg]` silently excludes them.** |
| T7 | `engine_fixture_build::build_helper_produces_bundle` (Task 4 linchpin) | integration-rs (deno-gated) | in-process `build_ts_extension` via the helper + `deno.workspace.json` | recursively copy `alpha` (`src/`+`_extension.yml`) to tempdir; `build_bundle(&dst)`; assert `dst/dist/alpha.js` exists | none (real deno) | comment out `run_deno_bundle` → no `.js` → `exists()` RED. Also binds the `../../resources/…` path depth (wrong depth → build errors → RED) |
| T8 | executing suites regenerate: `synth_engines_e2e`, `echo_engine_e2e`, `behave_engine_e2e`, `capture_splice_seam` (existing load+render assertions) | e2e-rs (deno-gated) | `ensure_bundle`/`build_bundle` + full engine load+execute | after per-test copy (**post `deno_available()` gate**), call `ensure_bundle`; keep existing render assertions (e.g. `smoke_alpha_registers_and_loads`) | none | make `ensure_bundle` a no-op **with committed bundles deleted (post-Task 6)** → `smoke_alpha…` cannot load bundle → RED |

### Check 2 — refactor-induced vacuity (the one real trap here)

The rename migrates `err.contains("build-ts-extension")` → `err.contains("call
build-ts-extension")` in **T3** and **T4**. **The specific string is
load-bearing — require `"call build-ts-extension"`, never the bare
`"build-ts-extension"`.** The new hint ("… `q2 call build-ts-extension` …")
*contains* the old substring, so a bare-`"build-ts-extension"` assertion would
survive reverting the `call ` prefix — passing whether or not the rename
happened (the skill's breadcrumb-collapse trap, verbatim). Migrating to the full
string keeps the discriminator alive: revert the `call ` prefix → RED. Each row
keeps a second, distinct revert hunk (the `.unwrap_err()`/existence guard binds
the underlying `.js`-rejection independent of the hint text).

### Check 3 — missing-test pass

- **T2 (specced above):** removal of the top-level `q2 build-ts-extension`.
  Without it, re-adding the old variant reddens nothing — Task 1 Step 6 only
  checks this *manually*. Cheap, deno-free, directly bound; author it alongside
  T1 in `build_ts_extension_e2e.rs`.
- **T8 ordering (specced above):** the regenerate mechanism only binds *after*
  Task 6 deletes the committed bundles. Between Task 5 (wire `ensure_bundle`,
  bundles still committed) and Task 6, an `ensure_bundle`→no-op revert does
  **not** redden — the stale committed bundle masks it. Bind the regenerate seam
  with an explicit fail-on-revert of `ensure_bundle`→no-op **at Task 6 Step 3**,
  not at Task 5.
- **`q2 call test` (accepted-untested):** the refactor moves `test` from a raw
  positional under `Call{function,args}` to a typed `CallCommands::Test`
  (gaining `allow_hyphen_values`). No `call test` e2e exists today, and the
  dispatch target `commands::call::execute(Some("test"), …)` → `test::execute`
  is unchanged, so this is not a regression from a covered state. **Accepted
  untested.** If the executor is already adding e2e infra, a one-line smoke
  (`["call","test"]` → stderr `"Usage: quarto call test"`; revert the
  `CallCommands::Test` arm → RED) is cheap and welcome.
- **wasm gate (build-gated, not a `#[test]`):** the `#[cfg(not(target_arch =
  "wasm32"))]` on `extension::build` is proven by Task 6's full `cargo xtask
  verify` (revert the gate → quarto-core wasm build fails). Logged as
  build-gated, not a test row.

---

## Task 1: Rename `q2 build-ts-extension` → `q2 call build-ts-extension`

**Files:** `main.rs`; `commands/build_ts_extension.rs` (doc only); `commands/call/mod.rs`; `build_ts_extension_e2e.rs`; `read.rs`; `project/mod.rs`; `engine_registry_build.rs`.

- [x] **Step 1 (red):** In `build_ts_extension_e2e.rs`, `.arg("build-ts-extension")` → `.args(["call","build-ts-extension"])`; update its failure-message string (this is seam **T1**). In `read.rs`, change the three `.contains("build-ts-extension")` (`:1427/:1451/:1475`) + messages (`:1428/:1452/:1476`) to the **full** `"call build-ts-extension"` — *not* the bare `"build-ts-extension"` substring, which would survive the rename revert vacuously (seam **T3**, Check 2). In `engine_registry_build.rs:674`, same full-string tightening; update the `:675` panic message text (seam **T4**). **Also author seam T2** (missing-test #1) in `build_ts_extension_e2e.rs`: a test that runs `q2 build-ts-extension <dir>` (no `call`) and asserts `!status.success()` — guards the top-level command's removal (Step 3).

- [x] **Step 2 (verify red):**
  ```bash
  cargo nextest run -p quarto-core -E 'test(extension::read)'
  ```
  Expected: read.rs assertions FAIL (current hint lacks the `call ` prefix). *(The `build_ts_extension_e2e` red is a **runtime** failure: pre-conversion `Call { function, args }` accepts any positional, so `q2 call build-ts-extension <dir>` parses and fails at runtime with `Unknown function: build-ts-extension` → the `status.success()` assert fails. Not a clap error.)*

- [x] **Step 3:** In `main.rs`: delete the top-level `#[command(name="build-ts-extension")] BuildTsExtension {…}` variant (~295-315) + its dispatch arm (~706-716). Replace `Call` with `Call { #[command(subcommand)] command: CallCommands }`. Add after `Commands`:
  ```rust
  /// Subcommands under `quarto call` — the Q1-parity `call` group.
  /// Q1's group is `{ engine, build-ts-extension, typst-gather }`; q2 ships
  /// `test` + `build-ts-extension` (plan9 adds `engine`).
  #[derive(clap::Subcommand)]
  enum CallCommands {
      /// Run embedded document tests
      Test {
          #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
          args: Vec<String>,
      },
      /// Build TypeScript execution engine extensions
      #[command(name = "build-ts-extension")]
      BuildTsExtension {
          path: Option<PathBuf>,
          #[arg(long)] config: Option<PathBuf>,
          #[arg(long)] workspace: bool,
      },
  }
  ```
  Replace the `Commands::Call` dispatch arm with:
  ```rust
  Commands::Call { command } => match command {
      CallCommands::Test { args } => commands::call::execute(Some("test".to_string()), args),
      CallCommands::BuildTsExtension { path, config, workspace } =>
          commands::build_ts_extension::execute(
              commands::build_ts_extension::BuildTsExtensionArgs { path, config, workspace }),
  },
  ```
  *(`PathBuf` stays imported. `Test` gains `allow_hyphen_values` — minor desirable improvement. `call/mod.rs`'s `Some(other)`/`None` arms become unreachable — harmless; leave them.)*

- [x] **Step 4:** Hints/docs: `read.rs:415` + `read.rs:1409`; `project/mod.rs:748`; `build_ts_extension.rs:1` doc; `call/mod.rs` doc adds `build-ts-extension`; `build_ts_extension_e2e.rs` prose at lines 4/6/51/100/112. *(The `read.rs:415` + `project/mod.rs:748` hint edits are the `call ` prefix that reddens T3/T4's rename assertion — do them here, not before Step 1's assertion change.)*

- [x] **Step 5 (sweep):**
  ```bash
  grep -rn "q2 build-ts-extension\|'q2 build-ts" crates/ | grep -v 'call build-ts-extension'
  ```
  Expected: only the clap leaf `#[command(name = "build-ts-extension")]` (correct) remains; fix `behave_engine_e2e.rs:379`'s regeneration comment too.

- [x] **Step 6 (green + real binary):**
  ```bash
  cargo nextest run -p quarto-core -E 'test(extension::read) | binary(integration) & test(engine_registry_build)'
  cargo nextest run -p quarto -E 'binary(integration) & test(build_ts_extension_e2e)'
  cargo run --bin q2 -- call build-ts-extension --help | tail -20
  cargo run --bin q2 -- build-ts-extension --help; echo "exit=$? (must be non-zero)"
  ```
  Record both help outputs in the commit body. **Commit.**

---

## Task 2: Extract build logic into `quarto-core::extension::build` (native-only)

**Files:** create `crates/quarto-core/src/extension/build.rs`; edit `crates/quarto-core/src/extension/mod.rs`; rewrite `crates/quarto/src/commands/build_ts_extension.rs` as a wrapper.

**Interface produced (quarto-core public API):**
```rust
pub struct BuildOptions {
    pub ext_dir: Option<PathBuf>, // None = cwd
    pub config: Option<PathBuf>,  // tier-1 explicit override
    pub workspace: bool,
}
/// Build the extension's `dist/*.js` bundle; returns the output path.
pub fn build_ts_extension(opts: BuildOptions) -> anyhow::Result<PathBuf>;
```

- [x] **Step 1:** Create `extension/build.rs` and **move verbatim** from `crates/quarto/src/commands/build_ts_extension.rs`: `resolve_build_config`, `find_workspace_root`, `SHIPPED_DENO_JSON` + `materialize_shipped_config`, `find_entry_ts`, `find_output_path`, `run_deno_bundle`, and the whole `#[cfg(test)] mod tests`. The `include_str!("../../../../resources/extension-build/deno.json")` path is **unchanged** — both old and new files sit exactly 4 dirs below the repo root (`crates/quarto/src/commands/` vs `crates/quarto-core/src/extension/`). Convert the old `execute(args)` body into `pub fn build_ts_extension(opts: BuildOptions) -> anyhow::Result<PathBuf>` returning the resolved `output_js` (add the return; the old fn returned `Ok(())`). (`resolve_extension_dir`, an internal helper used by `execute`/`build_ts_extension` and exercised by 3 of the moved tests, moved too — kept private to the module, not in the plan's explicit fn list but implied by "move the build logic + the whole tests module".)

- [x] **Step 2:** In `extension/mod.rs`, add `#[cfg(not(target_arch = "wasm32"))] pub mod build;` (mirrors `engine::ts_process`'s native gate). No flat re-export added: the Interface-produced block in this plan documents only the nested `quarto_core::extension::build::{BuildOptions, build_ts_extension}` path, and Task 4's helper hard-codes that same full path — adding a flat re-export would be an unrequested extra surface with no consumer, so it was left out to keep the two in sync as specced.

- [x] **Step 3:** Rewrite `crates/quarto/src/commands/build_ts_extension.rs` to a thin wrapper — keep `BuildTsExtensionArgs` (the CLI struct) and:
  ```rust
  pub fn execute(args: BuildTsExtensionArgs) -> anyhow::Result<()> {
      quarto_core::extension::build::build_ts_extension(
          quarto_core::extension::build::BuildOptions {
              ext_dir: args.path,
              config: args.config,
              workspace: args.workspace,
          },
      )?;
      Ok(())
  }
  ```
  Delete the moved fns and their tests from this file. Keep the module doc `//! q2 call build-ts-extension`.

- [x] **Step 4 (unit tests move with the code):**
  ```bash
  cargo nextest run -p quarto-core -E 'test(extension::build)'
  ```
  Expected: the moved `resolve_build_config`/`find_workspace_root`/`materialize_shipped_config` unit tests pass in their new home. **Actual: 14/14 passed** (all 14 tests that were in the original `#[cfg(test)] mod tests` block moved and re-ran green — no stray `#[cfg]` excluded any).

- [x] **Step 5 (CLI still works end-to-end):**
  ```bash
  cargo build -p quarto
  cargo nextest run -p quarto -E 'binary(integration) & test(build_ts_extension_e2e)'
  ```
  Expected: PASS — proves the wrapper drives the extracted lib through the real binary (dogfoods the full path on `echo-engine`). **Actual: 2/2 passed** (`top_level_build_ts_extension_removed`, `build_ts_extension_produces_bundle`) against real `deno 2.9.0`.

- [x] **Step 6 (wasm gate holds):** confirm the new module is excluded from wasm — a full `cargo xtask verify` runs in Task 6; here just confirm `cargo build -p quarto-core` is clean and the module carries the `#[cfg(not(target_arch = "wasm32"))]` gate. **Commit** (`refactor(quarto-core): extract extension-bundle build lib; CLI becomes a wrapper`). **Actual: `cargo build -p quarto-core` clean; gate confirmed in `extension/mod.rs` (`#[cfg(not(target_arch = "wasm32"))] pub mod build;`). Full wasm verify deferred to Task 6 per plan.**

---

## Task 3: Switch the 7 barrel-importing synth engines to `@quarto/api/claims`

**Files:** `alpha, beta, behave, mismatch, fallback-univ, interop-r, whenclass-marimo` `src/<name>.ts`.

- [x] **Step 1:** In each of the 7, change the runtime import specifier `"@quarto/api"` → `"@quarto/api/claims"` (imported symbols unchanged). Leave `content-claim`/`echo-engine`/`echo-legacy` (they import only `@quarto/types`).

- [x] **Step 2 (prove hermetic; bundles reverted after):**
  ```bash
  for fx in alpha beta behave mismatch fallback-univ interop-r whenclass-marimo; do
    cargo run --bin q2 -- call build-ts-extension --workspace \
      crates/quarto-core/tests/fixtures/extensions/$fx \
      && echo "OK $fx md5=$(grep -c 'blueimp\|md5' crates/quarto-core/tests/fixtures/extensions/$fx/dist/$fx.js)" \
      || echo "FAILED $fx"
  done
  git checkout -- crates/quarto-core/tests/fixtures/extensions/*/dist/   # revert; regen happens at test time
  git status --short resources/extension-build/   # expect clean (no deno.lock)
  ```
  Expected: every `OK <fx> md5=0`; no `deno.lock`. **Commit** the 7 `src/*.ts` edits only. *(Note: committed `dist/*.js` are now stale vs their `src` until Task 6 deletes them; benign — the committed bundles still contain the same `primary()` logic and all tests keep passing; test-time regen supersedes them in Task 5.)*

---

## Task 4: Shared in-process build helper

**Files (new):** `crates/quarto-core/tests/integration/engine_fixture_build.rs`; register `pub mod engine_fixture_build;` in `tests/integration/main.rs` (alphabetized).

- [x] **Step 1:** Author it — an **in-process** call to the Task-2 lib (no subprocess to `q2`, no `CARGO_BIN_EXE`):
  ```rust
  use std::path::Path;

  /// Fixtures whose bundles are hermetically regenerable (pure/type-only imports).
  pub const HERMETIC_FIXTURES: &[&str] = &[
      "alpha","beta","behave","mismatch","content-claim",
      "fallback-univ","interop-r","whenclass-marimo","echo-engine","echo-legacy",
  ];

  pub fn deno_available() -> bool {
      std::process::Command::new("deno").arg("--version").output()
          .is_ok_and(|o| o.status.success())
  }

  /// Build `<ext_dir>/dist/<name>.js` in place via the workspace import map.
  /// Hermetic (no network/lock). Caller gates on `deno_available()`.
  pub fn build_bundle(ext_dir: &Path) {
      let cfg = Path::new(env!("CARGO_MANIFEST_DIR"))
          .join("../../resources/extension-build/deno.workspace.json");
      quarto_core::extension::build::build_ts_extension(
          quarto_core::extension::build::BuildOptions {
              ext_dir: Some(ext_dir.to_path_buf()),
              config: Some(cfg),
              workspace: false,
          },
      ).expect("build fixture bundle");
  }

  /// Build only if `name` is hermetic; else no-op (committed bundle used as-is).
  pub fn ensure_bundle(ext_dir: &Path, name: &str) {
      if HERMETIC_FIXTURES.contains(&name) { build_bundle(ext_dir); }
  }
  ```
  *(`env!("CARGO_MANIFEST_DIR")` here is `crates/quarto-core` — the test's own crate — so `../../resources/...` reaches the repo root. This is a quarto-core-internal API call, so there is no cross-crate binary problem.)*

- [x] **Step 2 (validate the mechanism):** a throwaway test copies `alpha`'s source into a tempdir and builds:
  ```rust
  #[test]
  fn build_helper_produces_bundle() {
      if !deno_available() { eprintln!("SKIP: no deno"); return; }
      let tmp = tempfile::tempdir().unwrap();
      let dst = tmp.path().join("alpha");
      // recursively copy the committed fixture (src/ + _extension.yml) into dst
      build_bundle(&dst);
      assert!(dst.join("dist/alpha.js").exists());
  }
  ```
  Run it; expect PASS. This is the linchpin (in-process lib + `--config` tempdir build + the `../../` path depth). **Commit.**

---

## Task 5: Wire the helper into consumers; stub for `engine_registry_build`

- [x] **Step 1 (enumerate — guard against a missed site):**
  ```bash
  grep -rln 'fixtures/extensions\|fixture_ext_dir\|dist/' crates/quarto-core/tests/integration/*.rs
  ```
  Reconcile against the classification below.

- [x] **Step 2 (executing suites — build hermetic installs):** In each suite that *loads and executes* a bundle, call `crate::engine_fixture_build::ensure_bundle(&copied_ext_dir, name)` right after copying each fixture into the tempdir, **after** the `deno_available()` gate. Known sites:
  - `synth_engines_e2e.rs` `setup_project` (loop over `ext_names`).
  - `echo_engine_e2e.rs` `setup_project`, **plus** the inline `echo-wrong` copy (~:242): it renames `echo-engine`→`echo-wrong` with a custom `_extension.yml`; call `build_bundle` on that dir (name-convention entry lookup falls back to the sole `src/*.ts`; output path comes from the custom yml).
  - `behave_engine_e2e.rs` `setup_project`.
  - `capture_splice_seam.rs` inline `echo-engine` copy (~:132).

- [x] **Step 3 (explicitly leave unwired — do NOT build):**
  - `marimo_resolution.rs` — installs only `marimo` (non-hermetic); it's a pure resolver test (no load, no deno, no render). No change.
  - `julia_engine_e2e.rs` — installs committed `julia-engine` (non-hermetic). No change.
  - `marimo_engine_e2e.rs` — installs only `marimo` (non-hermetic, via `setup_marimo_project` + the dynamic-path `_extension.yml` rewrite); no hermetic fixture is copied (verified 2026-07-22 by Step 1's grep). No change. *(If a future grep ever shows a hermetic fixture installed here, wire it per Step 2.)*
  - `pass1_engine_resolution_pipeline.rs` — installs the committed hand-written `legacy-python` **stub** bundle (non-hermetic: it has no `src/*.ts`, is not regenerable, and is never loaded/executed). No change. *(An 11th committed bundle the plan's original inventory omitted; it stays committed and is correctly left unwired.)*

- [x] **Step 4 (`engine_registry_build` — stub, not build; seam T5):** These tests only assert the bundle `.js` **exists** (static registry build + the `if !path.exists()` guard at `project/mod.rs:745`); they never execute it and have **no** deno gate. Where it used committed `dist/{alpha,beta}.js` (~:112/:117), have it **write a placeholder `.js`** (e.g. `std::fs::write(dist_dir.join("alpha.js"), "// stub for registry existence check\n")`) into a tempdir copy instead. No `build_bundle`, no deno dependency — preserves no-deno coverage (T5's revert: relax the `:745` guard → Ok-assert RED).

- [x] **Step 5 (run consumers, committed bundles still present):**
  ```bash
  cargo nextest run -p quarto-core -E 'binary(integration) & (test(synth_engines_e2e) | test(echo_engine_e2e) | test(behave_engine_e2e) | test(capture_splice_seam) | test(marimo_engine_e2e) | test(engine_registry_build))'
  ```
  Expected: all PASS (executing suites build into tempdirs; `engine_registry_build` uses stubs; the stale committed copies are ignored). **Commit.**
  **Actual: 49/49 passed** (1 pre-existing `LEAK` diagnostic on `capture_splice_seam::cell_wrapped_capture_splices`, unrelated to this task — not a failure).

---

## Task 6: Delete committed bundles, gitignore, docstring, full verify

- [x] **Step 1:** `git rm crates/quarto-core/tests/fixtures/extensions/{alpha,beta,behave,mismatch,content-claim,fallback-univ,interop-r,whenclass-marimo,echo-engine,echo-legacy}/dist/*.js` (leave `julia-engine`/`marimo` `_extensions/**.js`). **Actual: 10 bundles removed; `legacy-python/dist/legacy-python.js` correctly retained.**

- [x] **Step 2 (`.gitignore`):** add
  ```
  crates/quarto-core/tests/fixtures/extensions/*/dist/
  resources/extension-build/deno.lock
  ```
  (the `*/dist/` glob matches every extension's `dist/` dir; the 10 hermetic bundles are `git rm`'d (Step 1) so their regenerated copies stay ignored, while `legacy-python/dist/legacy-python.js` remains a tracked committed stub (a gitignore entry cannot untrack an already-tracked file); `julia-engine`/`marimo` live under `_extensions/`, not `dist/`, so are unaffected. `deno.lock` is defensive — hermetic builds produce none. `resources/extension-build/node_modules/` — created by `deno.workspace.json`'s `nodeModulesDir: auto` — is **already** git-ignored and stays clean under hermetic builds, so it needs no new rule here.)

- [x] **Step 3 (prove regeneration from clean):**
  ```bash
  find crates/quarto-core/tests/fixtures/extensions -path '*/dist/*.js' | sort   # expect empty
  cargo nextest run -p quarto-core -E 'binary(integration) & (test(synth_engines_e2e) | test(echo_engine_e2e) | test(behave_engine_e2e) | test(capture_splice_seam) | test(marimo_engine_e2e) | test(engine_registry_build))'
  git status --short   # expect NO untracked dist/*.js or deno.lock (built in tempdirs)
  ```
  Expected: all PASS with zero committed bundles; working tree clean. **Actual: 49/49 PASS; only `legacy-python/dist/legacy-python.js` remains; no untracked `dist/*.js`/`deno.lock`.**

  **Bind T8 here (fail-on-revert — the regenerate seam only binds now):** with the committed bundles deleted, stub `ensure_bundle` to a no-op (`if false { … }`), re-run the executing suites → `synth_engines_e2e::smoke_alpha_registers_and_loads` (and siblings) must go **RED** (no bundle to load) → restore → GREEN. Record the RED verbatim. *(Before this step the committed bundle masks the no-op, so this proof is impossible at Task 5 — it is deliberately deferred to here.)* **Actual (T8 RED verbatim): `ensure_bundle`→`if false && …` no-op ⇒ `Summary 7 tests run: 0 passed, 7 failed` in `synth_engines_e2e` (incl. `smoke_alpha_registers_and_loads`, `smoke_beta…`, `smoke_interop_r…`, `smoke_fallback_univ…`, `smoke_whenclass_marimo…`, `b10…`, `b11…`) → restored ⇒ 7/7 PASS. Binding proven.**

- [x] **Step 4:** Update `synth_engines_e2e.rs` docstring (~:35): bundles are **regenerated at test time** via `crate::engine_fixture_build` (fixtures import `@quarto/api/claims` → hermetic). Keep "real deno bundle output" (accurate). **Done.**

- [x] **Step 5 (full verification — WASM leg included):**
  ```bash
  cargo build --workspace
  cargo nextest run --workspace
  cargo xtask verify            # FULL (not --skip-hub-build): confirms the native-gated build module doesn't break the wasm hub-client leg
  ```
  Expected: clean build, all tests pass, verify green (incl. wasm build of quarto-core).

  **PLAN GAP found here (Task 5's consumer enumeration was scoped to `crates/quarto-core/tests/integration/*.rs` only, so it missed committed-`echo-engine`-bundle consumers in OTHER crates). Full-workspace `cargo nextest run --workspace --no-fail-fast` surfaced 2 more failures beyond the quarto-core suites:**
  - **`crates/quarto/src/commands/render.rs::classify_echo_file_admitted_by_extension_discovery_not_rejected`** — deno-free, existence-only (only hits `build_engine_registry`'s bundle-exists guard, never executes). **Fix: write a stub `dist/echo-engine.js` after `copy_dir` (the same deno-free T5 stub pattern used for `engine_registry_build`).**
  - **`crates/quarto-preview/src/capture_driver.rs::{p2_14_eager_capture_runs_extension_engine, p2_15_on_edit_re_execution_keeps_extension_engine}`** — deno-gated and actually *execute* echo-engine (assert `ECHO_EXECUTED`), so a stub is insufficient. **Fix: regenerate the real bundle in `build_ctx_with_echo_extension` via the public `quarto_core::extension::build::build_ts_extension` lib (the tests are already `deno_available()`-gated; the `../../resources/…` config-path depth matches because quarto-preview and quarto-core are both one level under `crates/`).** This faithfully extends the plan's "regenerate at test time" design to the missed consumer rather than re-committing echo-engine's bundle. *(Note: `cargo xtask verify`'s nextest aborts on first failure — it only reported render.rs; the authoritative full failure set came from a separate `--no-fail-fast` workspace run.)*

- [x] **Step 6:** Reconcile checklist; **commit** (`test(plan1c3): delete committed synth bundles; regenerate at test time; gitignore dist/`). Report removed-file count + confirm clean tree after a test run. **Done: committed `52a207602`; 10 bundles removed; working tree clean of tracked changes after a full-workspace test run.**

---

## Follow-up strands (out of scope — file in braid)

1. **Publish `@quarto/api` + `@quarto/types` to jsr** — the shipped tier-4 `deno.json` maps `@quarto/api → jsr:@quarto/api`, so installed-binary authors (no `--workspace`) can't build until publication. Sizes: types 65 KB / 0 deps; api 417 KB / 2 npm deps (`blueimp-md5`, `yaml`).
2. **`@quarto/api` barrel drags crypto→`blueimp-md5`** into any barrel import — breaks `build-ts-extension` for a real author who imports the convenience barrel. Decide: drop `export * from "./crypto"` from the aggregate, lazy md5, or map `blueimp-md5`/`yaml` as `npm:` in the shipped configs.
3. **Make `julia-engine`/`marimo` fixtures hermetic** (vendored deno deps) so they can self-regenerate too, or keep committed by decision.

## Self-review notes

- **Extraction risk retired first:** Task 2 lands the lib + keeps the CLI e2e green *before* any test depends on it (Task 4). The `include_str!` path is unchanged (verified: both files 4 dirs below repo root).
- **wasm:** the build module spawns `deno` (`std::process`) → native-gated like `engine::ts_process`; Task 6's full verify confirms the wasm leg.
- **Race-free:** executing suites build into per-test tempdirs; `engine_registry_build` writes stubs into tempdirs — no shared-tree writes across nextest's per-test processes.
- **Coordination with plan9:** introduces `CallCommands` (typed subcommand group). plan9 must add a typed `Engine` variant to `CallCommands`, not a bare clap string-dispatch arm — **plan9's doc has been reconciled** (2026-07-22) from its old "clap string-dispatch arm" wording (Tech Stack + Task 7.4) to the typed variant. The `Some("engine")` string arm in `call/mod.rs` stays valid because the new `CallCommands::Engine` variant routes through `commands::call::execute(Some("engine"), args)`, mirroring how this plan routes `CallCommands::Test`.
- **Consumer classification is explicit** (Task 5 Steps 2–4) so no suite is silently missed or wrongly wired: executing→build hermetic; `engine_registry_build`→stub; `marimo_resolution`/`julia_engine_e2e`→untouched.
