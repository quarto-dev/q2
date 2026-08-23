# CI test census — what we don't run (GH #250)

**Date:** 2026-08-22
**Branch:** `bugfix/250` (worktree `.worktrees/workspace-5`), base `e3b3d7d4a` (v0.26.0)
**Issue:** [#250 — Wire most workspace TS test suites into CI](https://github.com/quarto-dev/q2/issues/250)

Issue #250 was filed 2026-06-02 against a much smaller tree. This is a fresh
census across **both** Rust and TypeScript, taken by actually running every
suite rather than reading scripts.

## 1. What CI runs today

| Workflow | Trigger | Runs |
| --- | --- | --- |
| `test-suite.yml` | push/PR to `main`, `kyoto` | `cargo fmt --check`, `cargo xtask lint`, `cargo clippy --workspace --all-targets -D warnings`, `tree-sitter test` (**qmd grammar only**), `cargo nextest run --tests --cargo-profile ci`; separate `wasm-tests` job runs `cargo test -p pampa --test wasm_lua --target wasm32-unknown-unknown` |
| `ts-test-suite.yml` | push/PR to `main`, `kyoto` | `hub-client npm run build:all`, `hub-client npm run test:ci`, `@quarto/engine-host-deno` vitest, 3 × `deno test` (deno-host, control-transport, wire-parity), engine-host-deno bundle-freshness gate |
| `hub-client-e2e.yml` | push/PR + nightly schedule | builds WASM + ts-packages + hub-client, `cargo build --bin hub`, Playwright: `hub-client` custom specs (31), smoke-all (nightly/opt-in), visual |
| `build-wasm.yml` | `workflow_dispatch` only | WASM artifact build, no tests |
| `release.yml` | tags | preflight + signed builds |

`cargo xtask verify` (local-only) covers a **superset** of the TS side: it also
runs trace-viewer build+tests, `preview-renderer` unit+integration,
`preview-runtime` unit, `preview-*` `typecheck:tests`, ts-packages build + the
`quarto-hub-mcp` ESM smoke check, `quarto-sync-client` + `quarto-hub-mcp` tests,
`q2-preview-spa` build (E2E behind `--e2e`), and a tree-sitter **CRLF parity**
check. None of those legs exist in CI. Anything only `verify` catches is
enforced by developer discipline, not by the merge gate.

## 2. TypeScript census

All numbers from a local run on macOS at base `e3b3d7d4a` (logs in the session
scratchpad). "In CI" = gated by a push/PR workflow.

| Package | Script | Files | Tests | Result | In CI |
| --- | --- | --- | --- | --- | --- |
| `hub-client` | `test:ci` (unit+integration+wasm) | — | — | green | **yes** |
| `@quarto/engine-host-deno` | `test` + 3 deno tests + bundle gate | 6 + 3 | — | green | **yes** |
| `trace-viewer` | `test` | 3 | 10 | green | no (verify only) |
| `q2-demos/kanban` (`hub-kanban`) | `test` | 1 | 35 | green | no |
| `q2-demos/kanban` | `test:integration` | 1 | 20 | green | no |
| `q2-preview-spa` | `test` | 5 | 46 | green | no |
| `q2-preview-spa` | `test:integration` | 6 | 76 | green | no |
| `@quarto/preview-renderer` | `test` | 42 (2 skipped) | 585 (549 pass / 36 skip) | green | no (verify only) |
| `@quarto/preview-renderer` | `test:integration` | 50 | 580 | **1 real failure** (578 pass / 1 skip, WASM built) | no (verify only) |
| `@quarto/preview-runtime` | `test` | 8 | 77 | green | no (verify only) |
| `@quarto/api` | `test` | 26 | 369 (368 pass / 1 skip) | green | no |
| `@quarto/quarto-automerge-schema` | `test` | 2 | 36 | green | no |
| `@quarto/quarto-sync-client` | `test` | 21 | 137 | green **after dist build** | no (verify only) |
| `@quarto/hub-mcp` | `test` | 22 | 249 (246 pass / 3 skip) | green **after dist build** | no (verify only) |
| `@quarto/wasm-js-bridge` | `test` | 3 | 19 | green | no |
| `@quarto/annotated-qmd` | `test` (node:test) | 16 | 156 | **2 failures** (bd-1d6io, `in_progress`) | no |
| `@quarto/sync-test-harness` | `test` | 2 | 11 (8 pass / 3 skip) | **1 suite fails** — needs `external-sources/` | no |
| `q2-preview-spa` | `test:e2e` (Playwright) | 17 specs | — | not measured | no (verify `--e2e` only) |

**Roughly 2,070 TypeScript assertions currently sit outside the merge gate**
(~1,620 excluding `preview-renderer`'s integration tier).

### 2.1 Build-order prerequisites (not test bugs)

Three suites fail from a cold `npm ci` and pass once the workspace `dist/`
outputs exist, because they resolve siblings through the `"import":
"./dist/index.js"` export condition:

- `@quarto/quarto-sync-client` → needs `@quarto/quarto-automerge-schema` dist.
  Cold: 14 of 21 files fail with *"Failed to resolve entry for package"*. After
  build: **21 files / 137 tests, all green.**
- `@quarto/hub-mcp` → needs `quarto-sync-client` dist. Cold: 13 files fail +
  `symlink-invocation.test.ts` fails (it spawns `dist/index.js`). After build:
  **22 files / 249 tests, green.**
- `@quarto/annotated-qmd` → needs `@quarto/pandoc-types` dist; cold it crashes
  with `ERR_MODULE_NOT_FOUND` mid-run.

So CI must run the equivalent of `verify`'s step 6 (ts-packages build in
dependency order) **before** these suites. This is #250's "a couple of deps not
installed in my local checkout" — it is a build-order requirement, not flake.

`@quarto/preview-renderer`'s integration tier additionally needs the **WASM**
package present (`wasm-quarto-hub-client`): 26 of its 27 file-level failures
in a WASM-less tree are `Failed to resolve import "wasm-quarto-hub-client"`.
With WASM built the tier reports **49 of 50 files, 578 passed / 1 failed / 1
skipped**. CI's `ts-test-suite.yml` already builds WASM before hub-client
tests, so ordering this suite after that step is enough.

### 2.2 Genuinely red today

- `@quarto/annotated-qmd` — 154/156. `div-attrs.json - Div with attributes
  conversion` and `substring invariant - links.qmd: inline code` (an off-by-one:
  got `' \`x = 5\`'`, expected `'\`x = 5\`'`). Tracked by **bd-1d6io**
  (`in_progress`). Unchanged since #250 was filed.
- `@quarto/preview-renderer` `test:integration` — one real assertion failure in
  `custom-components.integration.test.tsx > Equation > appends \tag{N} to the
  LaTeX when plain_data.order is set` (`expect(tagEl).not.toBeNull()` at
  `custom-components.integration.test.tsx:664`). **Confirmed real**: re-run
  after a full `npm run build:wasm` still fails, with every other file green
  (49/50 files, 578 pass / 1 fail / 1 skip). Since `verify` step 11 runs this
  suite, **`cargo xtask verify` is red on `main` today** — which is itself
  evidence that a verify-only gate does not hold.
- `@quarto/sync-test-harness` — the `ts-sync-server` describe block times out
  after 30 s. Cause is structural, see below.

### 2.3 `sync-test-harness` depends on `external-sources/` — policy violation

`ts-packages/sync-test-harness/src/server-manager.ts:152` spawns
`node src/index.js` in `external-sources/automerge-repo-sync-server`. That
directory is not version-controlled, so the `ts-sync-server` tier **can never**
run in CI, and it violates the repo's External Sources Policy ("Test fixtures
that depend on external-sources/" is listed as prohibited). Its sibling
`hub` tier passes: 8 tests green, including the three reconnect-delay cases
#250 suspected of flake. **Caveat on that measurement:** the hub tier spawns
`cargo run --bin hub` (`server-manager.ts:96-117`) with a 120 s readiness
timeout, so "green" here means green against a *warm* local `target/`. On a CI
runner with no Rust cache that is a cold workspace build and would time out —
which is why the fix plan gates this suite in `hub-client-e2e.yml`, the workflow
that already pre-builds that binary, rather than in `ts-test-suite.yml`.

Options: skip the tier when the directory is absent, vendor the sync server, or
delete the tier. Needs a decision — it is not a wiring problem.

### 2.4 Dead / vestigial test scripts

- `@quarto/preview-runtime` `test:wasm` → `vitest run --config
  vitest.wasm.config.ts`, but **that config file does not exist**. The script
  always fails. A `src/userGrammar/Highlight.wasm.test.ts` exists and is run by
  nothing.
- `@quarto/preview-runtime` `test:integration` → config includes
  `src/**/*.integration.test.ts`; **zero files match**.
- `q2-demos/kanban` `test:wasm` → include `src/**/*.wasm.test.ts`; **zero files
  match**, so `test:ci` has a no-op leg.
- `editors/vscode-quarto-rust` `test` → `node ./out/test/runTest.js`; the
  package has no test sources (`src/extension.ts` only) and is **outside the npm
  workspace** (it has its own `package-lock.json`). Dead.

### 2.5 Never type-checked in CI

`npm run typecheck --workspaces --if-present` exists at the repo root and runs
in **no** workflow. `verify` type-checks only `preview-renderer` and
`preview-runtime` test files (`typecheck:tests`, added after bd-ddaqjb91 —
test harnesses silently drifting from the interfaces they stub).

## 3. Rust census

### 3.1 Doctests: not run anywhere, and currently red

`cargo nextest` cannot execute doctests and there is no `cargo test --doc` step
in CI or in `verify`. Running `cargo test --doc --workspace --no-fail-fast`
locally: **44 crates, 20 passed, 5 failed, 68 ignored** — the tier is red today.

Source shape: **422 fenced blocks** in doc comments across 31 crates (211
blocks), heavily tagged so they never compile or run — `ignore` (46),
`rust,ignore` (24), `text` (38), `yaml` (24), plus `json`, `html`,
`javascript`, `markdown`, `bash`, `qmd`, `xml`, `sh`, `r`, `lua`. Only ~26 are
live Rust doctests, and only 20 of those pass.

The 5 failures:

| Crate | Doctest | Cause |
| --- | --- | --- |
| `quarto-core` | `crossref::codeblock_shorthand` (lines 19, 34) | prose treated as Rust — smart quotes, backticks, em-dashes; `error: prefix \`cell\` is unknown`, `expected one of ! or ::, found Div` |
| `quarto-core` | `engine::jupyter::text_execute::render_cell` (lines 535, 541) | same, plus a real `E0308` mismatched types |
| `quarto-sass` | `bundle::assemble_themes` (`bundle.rs:769`) | **stale API**: `ThemeContext::new` gained a `runtime: &dyn SystemRuntime` parameter; the doctest still calls it with one argument (`E0061`) |

`quarto-core` alone emits 70 compile errors across its 4 failing blocks
(45 + 17 + 1 + 7); nearly all are `unknown start of token` from untagged prose.

So wiring doctests in is **not** free. It needs a triage pass (tag prose blocks
as `text`, fix the two real breaks) before a `cargo test --doc` gate can go
green. The `quarto-sass` one is the interesting case — it is exactly the class of
rot a doctest gate is supposed to catch, and it went unnoticed because nothing
compiles that block.

### 3.2 `#[ignore]`d tests

67 `#[ignore]` attributes (a few are in comments/`build.rs` codegen), by crate:
`quarto-core` 38, `comrak-to-pandoc` 15, `quarto-hub-provider` 4,
`qmd-syntax-helper` 4, `quarto-citeproc` 2, `pampa` 2, `quarto-preview` 1,
`quarto` 1. They fall into distinct buckets:

- **Needs a tool CI doesn't have** — knitr/R (19 in `engine/knitr/`),
  ipykernel (6 in `jupyter_integration.rs`).
- **Needs network** — `quarto-hub-provider/sync_probe.rs` (4, `PROBE_DOC_ID` /
  `PROBE_SERVER`), `quarto/bootstrap_sh.rs` (1, needs a published release).
- **Known behavioural gaps** — `comrak-to-pandoc/differential.rs` +
  `debug.rs` (15, documented pampa/comrak differences), `pampa`
  `incremental_writer_tests.rs` (2, lossy definition-list roundtrip),
  `quarto-core/pipeline.rs` (1, "parser is too forgiving").
- **Environment-specific bug** — `quarto-preview/staleness.rs` (bd-9brz,
  FSEvents starved on macOS).
- **Deliberately never-run repro** — `julia_engine_e2e.rs` `pc4a_*` (gated on
  `QUARTO_PC4A_LIVE` *and* `#[ignore]`).
- **Unexplained** — `qmd-syntax-helper/attribute_ordering_test.rs` (4, bare
  `#[ignore]` with no reason string).

### 3.3 Tests that run in CI but pass vacuously

This is the biggest Rust finding, and it is *not* in #250. **80 silent-skip
sites** early-return with an `eprintln!` when a tool is missing:

| File | Sites | Gate |
| --- | --- | --- |
| `quarto-core/tests/integration/engine_visibility.rs` | 19 | jupyter, knitr |
| `quarto-core/src/engine/knitr/mod.rs` | 19 | Rscript |
| `quarto-core/tests/integration/engine_error_policy.rs` | 9 | jupyter, knitr |
| `quarto-sass/tests/integration/parity_test.rs` | 8 | dart-sass |
| `quarto-core/tests/integration/jupyter_integration.rs` | 6 | ipykernel |
| `quarto-core/tests/integration/engine_output_parity.rs` | 5 | both engines, matplotlib |
| `quarto-system-runtime/src/sass_native.rs` | 3 | dart-sass |
| `quarto-core/src/engine/knitr/subprocess.rs` | 3 | Rscript |
| others (hub-provider, engine/mod, pampa, jupyter cleanup ×2, capture-splice) | 8 | mixed |

Plus availability-gated e2e suites with no message: `julia_engine_e2e.rs` (8
tests, `julia_available()`/`deno_available()`), `marimo_engine_e2e.rs` (8,
`uv`/`Rscript`/`knitr` R package/`deno`), `behave_engine_e2e.rs` (7,
`deno_available()`).

`test-suite.yml` installs **pandoc, tree-sitter, minisign, deno** — and *no*
Python/ipykernel, R/knitr, Julia, uv, or dart-sass. So every one of those tests
reports green in CI while asserting nothing. Only deno is protected: `QUARTO_CI=1`
turns the deno skip into a hard failure (`deno_available_when_quarto_ci`) —
exactly the pattern the other engines lack.

### 3.4 Crates never compiled or tested in CI

Excluded from the workspace in `Cargo.toml`: `wasm-quarto-hub-client` (0 test
attrs), **`wasm-qmd-parser` (4 test attrs — never built or run)**,
`crates/pampa/fuzz` (0), `tree-sitter-language-wasm-shim` (0),
`crates/experiments/piccolo-test` (0). `crates/experiments/reconcile-viewer` is
in `default-members`, so its 2 tests do run.

Also never run: `crates/wasm-qmd-parser/tests/web.rs` (4 `wasm_bindgen_test`s)
— the crate is workspace-excluded, and the `wasm-tests` job only builds
`pampa --test wasm_lua`.

No orphaned Rust test modules: every `crates/*/tests/integration/*.rs` is
declared in its crate's `main.rs`, and no test module is hidden behind a
`#[cfg(feature = ...)]` gate.

### 3.5 Feature combinations never compiled

- `pampa --no-default-features` (the WASM shape) is only built by the
  `wasm-tests` job with `--features lua-filter`; the `filters`-off /
  `json-filter`-off / `template-fs`-off combinations aren't compiled anywhere.
- `quarto-system-runtime/js-bridge` — off by default, never enabled in CI.
- `quarto-core/clap`, `quarto/vendored-openssl` (release-only),
  `quarto-config/span-assert` (enabled transitively by quarto-core dev-deps).
- No `--all-features` build exists.

### 3.6 Grammar and other gaps

- `tree-sitter test` runs for **`tree-sitter-qmd`** only (55 corpus files).
  **`tree-sitter-doctemplate/grammar` has a 215-line corpus that runs
  nowhere** — not in CI, not in `verify`.
- The tree-sitter **CRLF parity** check is `verify`-only.
- `cargo nextest run --tests` (not `--all-targets`): the comment cites
  `quarto-yaml`'s `harness = false` benches, but that crate is now external and
  **no in-tree crate has a `benches/` dir**, so the distinction is now moot.
- `crates/quarto-hub-provider/tests/integration/auth_bridge.rs` has 2 skip
  sites; hub auth paths are thinly covered in CI.

## 4. Gap classes (what a fix has to handle)

1. **Pure wiring** — green as-is, just needs a CI step: `trace-viewer`,
   `kanban` (unit+integration), `q2-preview-spa` (unit+integration),
   `preview-renderer` unit, `preview-runtime` unit, `quarto-api`,
   `quarto-automerge-schema`, `wasm-js-bridge`. (~1,200 assertions.)
2. **Wiring + build ordering** — `quarto-sync-client`, `quarto-hub-mcp`
   (ts-packages dist build first), `preview-renderer` integration (WASM first).
3. **Red, needs a fix** — `annotated-qmd` (bd-1d6io), `preview-renderer`
   integration `Equation \tag{N}`.
4. **Structurally un-CI-able** — `sync-test-harness` `ts-sync-server` tier
   (`external-sources/`).
5. **Dead scripts to delete or implement** — `preview-runtime` `test:wasm` +
   `test:integration`, `kanban` `test:wasm`, `editors/vscode-quarto-rust` `test`.
6. **Vacuous passes** — 80+ engine-gated Rust tests that skip silently on CI
   runners. Either install the engines in CI or extend the `QUARTO_CI=1`
   hard-fail pattern beyond deno.
7. **Never-run tiers** — Rust doctests (currently red), `tree-sitter-doctemplate`
   corpus, `q2-preview-spa` Playwright e2e, root `typecheck --workspaces`,
   `wasm-qmd-parser`.

Classes 1–2 are the literal scope of #250. Classes 6–7 are what the census
turned up beyond it and need scoping decisions before they become work items.
