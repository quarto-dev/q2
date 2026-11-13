---
title: 'TS engines (julia, marimo): deliver the execute environment (bd-x3h4zjhl)'
date: 2026-11-12
description: 'Give julia and marimo cells the per-document variables (`QUARTO_DOCUMENT_*`, `QUARTO_EXECUTE_INFO`, `QUARTO_FIG_*`, `QUARTO_PROJECT_ROOT`) and the project environment (`_environment` files, `QUARTO_PROFILE`) through one optional `executeEnv` field on the TS execute protocol.'
status: session-1-done  # Decisions final 2026-11-12 (Gordon), after a design/implementation review
braid:
  strand: bd-x3h4zjhl
---

**Branch:** `feature/bd-x3h4zjhl-ts-engines-execute-env`, checked out in
`.worktrees/workspace-4`.
**Owner:** Gordon Woodhull, who also owns the julia-engine and
quarto-marimo forks. Carlos Scheidegger wrote the first investigation.
This plan was rewritten on 2026-11-12 after a two-reviewer design
review; all decisions below are final.

## Overview

q2 never changes its own process environment. Quarto 1 did
(`Deno.env.set`), so every child process inherited two sets of
variables. q2 instead computes them as data and passes them to each
process that runs user code:

- **`project_env`**, the same for the whole project. It holds the
  values from `_environment`, `_environment.local`,
  `_environment.required` and `_environment-<profile>`, for keys the
  real environment does not already set, plus `QUARTO_PROFILE`
  (`crates/quarto-core/src/project/environment.rs`;
  `env_for_subprocess`; `engine_execution.rs` around line 707).
- **`execute_env`**, computed for each document by `prepare_execute_env`
  (`engine_execution.rs`; values from `engine/execute_info.rs`). It
  holds `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`,
  `QUARTO_DOCUMENT_FILE`, `QUARTO_EXECUTE_INFO` (a JSON file in the
  render's temp directory) and `QUARTO_FIG_WIDTH`/`HEIGHT`/`DPI`/`FORMAT`.

knitr and Jupyter are started from Rust and receive both, with
`project_env` applied first and `execute_env` on top (bd-8fmuyh4d,
PR #1161). TS engines receive neither, because `build_execute_options`
(`crates/quarto-core/src/engine/ts_engine.rs`) drops both, and the TS
execute protocol has no field to carry them. So julia and marimo cells
see none of these variables. For `project_env` this is a regression from
Quarto 1, where both engines inherited the `_environment` values.

This plan adds one optional field, `executeEnv`, to the TS execute
protocol. It carries both maps, merged. The julia and marimo engines
pass it to the processes that run cells.

## Decisions

1. **One field, `executeEnv`, carrying both maps.** Rust merges the
   filtered `project_env` pairs (with `QUARTO_PROFILE` applied as in
   `engine_execution.rs`) and then `execute_env` on top, the same
   precedence knitr uses. The filter runs in Rust against q2's
   environment, which is correct for engine children because the Node
   host inherits it. The name matches Rust's `execute_env`. Rejected
   names: `env`, `cellEnv`, `documentEnv`, `quartoEnv`.
2. **Wire shape: a JSON object of strings, left out when empty.** In
   Rust a `BTreeMap<String, String>` on `TsExecuteOptions` with
   `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`. A
   `BTreeMap` keeps the byte-compared parity fixture stable. The default
   lets an absent key deserialize. The variable names are distinct, so
   a map loses nothing.
3. **Version skew is harmless in every direction.** Nothing rejects
   unknown fields: serde has no `deny_unknown_fields` in `engine/`, and
   the host casts parsed requests (`framing.ts`) and copies options field
   by field (`host.ts` around line 670). A new q2 with an old host or
   engine drops the feature; an old q2 with a new host sends nothing.
4. **No `process.env` mutation anywhere.** The host runs executes for
   different engines concurrently (`perEngineQueue`), so a process-wide
   variable would leak between documents. The host copies `executeEnv`
   into a fresh object for each execute.
5. **Typed and backward-compatible in `@quarto/types` (D-c).**
   `executeEnv?: Readonly<Record<string, string>>` on `ExecuteOptions`.
   It is optional, so existing engines and any code that builds
   `ExecuteOptions` literals compile unchanged. The doc comment says:
   merge these into the environment of the processes that run user
   code, and never cache them across executes. The `host.ts` comment
   that says q2-only fields are read by cast is updated. The julia
   fork mirrors the field in its vendored `types/quarto-types.d.ts`.
6. **Only `execute` gets it.** No other host method runs user code
   (`dependencies` calls `target()` and the engine's stub
   `dependencies`).
7. **marimo:** pass `executeEnv` to the `extract.py` compiler process
   only, the one process that runs cells (`src/engine/process.ts`,
   `runMarimoCompiler`). The `uv run ... command.py` dependency resolver
   runs no cells and does not get it.
8. **julia: use QNR's per-run `env`; no QNR change.** In the julia
   engine's `qnrRunOptions` (`src/record-wire.ts`), set `env` from
   `executeEnv` and drop `executeEnv` from the forwarded object. QNR
   0.18.6 (pinned rev `ffe5dab7`) replaces the values on every run and
   sets them only around cell and inline evaluation (see Verified facts).
9. **Accept QNR re-running package-loading hooks each render (D-b).**
   `QUARTO_EXECUTE_INFO` changes every render, so the worker sees
   changed options and calls `run_package_loading_hooks()` each time
   (`QuartoNotebookWorker.jl:147`). The only hooks are the Plots and
   RCall setup functions, which are cheap and idempotent. The worker
   is not restarted.
10. **Third-party engines opt in; no fallback.** An engine that ignores
    `executeEnv` keeps working. A fallback would need `process.env`
    mutation, which decision 4 rules out. q2-ojs runs no user code and
    needs nothing.
11. **Fork workflow.** Engine changes are made in the forks, never in
    q2's copies: commit on `q2/node-port`, push after Gordon's OK, then
    sync into q2 as a separate commit naming the fork commit.
    - julia: `external-sources/quarto-julia-engine` →
      `cargo xtask pull-extension-subtree julia-engine`.
    - marimo: `external-sources/quarto-marimo` →
      `scripts/sync-marimo-fixture.sh external-sources/quarto-marimo`.
12. **No logging of execute payloads.** `executeEnv` can carry secrets
    from `_environment.local`. Today neither q2 nor the host logs
    request payloads, and QNR keeps these values out of its worker
    status printout (`worker_setup.jl:117-121`). Keep it that way.

## Work plan: two sessions

Session 1 is q2 only and ends green and committed, with the e2e tests
red on purpose. Session 2 does the forks, the syncs and the docs, and
turns the e2e tests green.

## Checklist

### Session 1, Phase 0: tests first

The Rust tests below name `TsExecuteOptions::execute_env`, so until
Phase 1 adds the field they fail as compile errors. That counts as
"red" here.

- [x] Protocol parity. In `render_wire_parity_fixture`
      (`crates/quarto-core/src/engine/ts_protocol.rs`, next to
      `exec_opts.units = ...`), set `exec_opts.execute_env` to a
      non-empty map; leave `make_execute_options` unchanged so it stays
      the empty case. In
      `ts-packages/quarto-engine-host-node/src/wire-parity.test.ts`,
      add `"executeEnv"` to `EXR7_OPTIONS_KEYS` (a contains-check, not
      set equality). Once the field exists (Phase 1), regenerate
      `crates/quarto-core/tests/fixtures/ts_wire_parity.json` with
      `QUARTO_REGEN_WIRE_FIXTURES=1 cargo nextest run -p quarto-core -E 'test(ts_protocol::tests::test_ts_wire_parity_fixture)'`.
- [x] Rust unit test in `ts_protocol.rs`: an empty map leaves the
      `executeEnv` key out, and a payload without the key deserializes
      (model: `presented_by_is_absent_by_default_and_roundtrips_as_core`).
- [x] Rust unit tests for `TsEngine::build_execute_options` in the
      `ts_engine.rs` tests module, building the context with
      `ExecutionContext::new(..).with_project_env(..).with_execute_env(..)`
      (existing examples near the `staged_ctx` tests): the result holds
      `project_env` then `execute_env`; `execute_env` wins on a shared
      key; a `QUARTO_PROFILE` pair in `project_env` passes through; both
      empty gives an empty map. (Profile activation itself is covered by
      the marimo e2e.)
- [x] Host tests (`host.test.ts`), using the EXR-7 recorder pattern (a
      custom `instance.execute` that records `options`; `makeFakeEngine`
      does not record): the engine's `execute` receives `executeEnv`;
      no key when the request omits it; two executes queued on one
      engine and two running concurrently on different engines (the T5
      `makeExecMsg`/`makeDeferred` pattern, with a loader that returns a
      different engine per path) each see their own values. No
      "mutation does not leak" test (review P2, decided 2026-11-13):
      each request is parsed separately, so such a test could never
      fail.
- [x] Types: add
      `ts-packages/quarto-engine-host-node/src/__type-tests__/execute-env.type-test.ts`
      (pattern: `b2-conformance.type-test.ts`), checked by
      `npm run typecheck -w @quarto/engine-host-node`. Assert that
      `Readonly<Record<string, string>>` is assignable to
      `ExecuteOptions["executeEnv"]` (fails before Phase 2) and that an
      `ExecuteOptions` literal without `executeEnv` type-checks.
- [x] CLI e2e through the real binary (review P5, decided 2026-11-13),
      in `crates/quarto/tests/integration/execute_env_cli_e2e.rs`, next
      to the knitr and Jupyter tests and reusing that file's `render`,
      `read_json`, `canonical` and `assert_paths`. Each test builds a
      temp project: `_quarto.yml` with `profile:` / `default: prod`, an
      `_environment` value and an `_environment-prod` value. Never set
      `QUARTO_PROFILE` on the test process. As in the knitr test, the
      cell writes the variables it saw to `env.json` and copies the
      `QUARTO_EXECUTE_INFO` file to `info.json` next to the document;
      the test asserts on every variable, the `_environment` and
      profile values, `QUARTO_PROFILE`, and `document-path` in
      `info.json`, and `eprintln!`s `env.json` so a `--no-capture`
      run shows what the cells saw.
  - [x] `marimo_cells_see_the_execute_environment`: copy
        `_extensions/marimo` from
        `crates/quarto-core/tests/fixtures/extensions/marimo/` (the
        same cross-crate pattern as `engine_diagnostics_cli.rs`). The
        Python cell writes `env.json` with `json` and `shutil.copy`.
        Skip unless `node_available_for_tests()` and `uv` are present.
        It runs in CI.
  - [x] `julia_cells_see_the_execute_environment`: `execute: daemon:
        false`. The julia cell writes `env.json` by hand (no JSON
        package) and copies the info file with `cp`. Gate it with a
        `julia_available()` copy, as `julia_targets_cli.rs` does. It
        is local-only.

  This replaces a marimo e2e in `quarto-core`. The CLI test also
  covers the embedded host bundle and command-line project discovery.
- [x] julia e2e (`crates/quarto-core/tests/integration/julia_engine_e2e.rs`,
      skipped by `julia_available()`, not run in CI), each case
      asserting every variable through `println` in the rendered HTML.
      Cells have no JSON package: check `QUARTO_EXECUTE_INFO` with
      `isfile` and `occursin` on `read(path, String)`.
  - [x] the same document rendered twice against a live daemon worker,
        with `fig-width` changed in between; the second render shows the
        new `QUARTO_FIG_WIDTH` and a different `QUARTO_EXECUTE_INFO`.
        Set `execute: daemon: true` explicitly (the default is off when
        there is no interactive session), and isolate the server as j7
        does (`seed_isolated_julia_runtime`, `HOME` override,
        `IsolatedJuliaServerGuard`). No existing test uses
        `daemon: true`;
  - [x] two documents with `julia:` / `share_worker_process: true` in
        both front matters (same isolation), each seeing its own
        `QUARTO_DOCUMENT_FILE`;
  - [x] a single-file document outside a project
        (`setup_julia_project()`, `daemon: false`; QNR sets nothing
        here today);
  - [x] a document path containing a space
        (`tmp.path().join("my doc.qmd")`).
- [x] Mark every new e2e test (the CLI tests and the julia cases) with
      `#[ignore = "bd-x3h4zjhl: needs <engine> fork sync (Session 2)"]`,
      so that the Session 1 gate and CI stay green (review P1, approved
      2026-11-13).
- [x] Confirm every new test fails before implementation; compile errors
      count. The type test's "literal without the field" half passes
      already and is exempt. Prove the e2e tests red with
      `cargo nextest run -p quarto -p quarto-core --run-ignored only -E 'binary(integration) & (test(execute_env_cli_e2e::) | test(julia_engine_e2e::))'`
      and record the failures here. (The knitr and Jupyter tests in
      that file are not ignored and still pass.) They stay red until their fork sync
      lands in Session 2.

### Session 1, Phase 1: Rust

- [x] Add `execute_env: BTreeMap<String, String>` to `TsExecuteOptions`
      (decision 2) and update `make_execute_options`.
- [x] Build the merged map in `TsEngine::build_execute_options`
      (`engine/ts_engine.rs`): extend a `BTreeMap` with
      `ctx.project_env`, then with `ctx.execute_env`. No helper is
      needed, because `ctx.project_env` already holds the filtered pairs
      with `QUARTO_PROFILE` applied (set at
      `stage/stages/engine_execution.rs:707-722`, before any engine
      runs), which is also what knitr consumes
      (`knitr/subprocess.rs:395-396`).

### Session 1, Phase 2: host and types

- [x] `quarto-engine-host-node/src/types.ts`: add
      `executeEnv?: Record<string, string>` to the wire type.
- [x] `host.ts`, execute path: in the field-by-field `executeOptions`
      literal (the `ExecuteOptions` assembly before the
      `Object.assign` that adds `signal`/`units`), add
      `...(opts.executeEnv !== undefined ? { executeEnv: { ...opts.executeEnv } } : {})`.
      Keep the copy even though each request is parsed separately: a
      one-line comment says it protects decision 4 if the host ever
      reuses request objects. Leave the dependencies path alone. Rewrite the comment saying the
      vendored `@quarto/types` stays a parity copy of Q1 (already untrue
      for `ownedLanguages`).
- [x] `ts-packages/quarto-types/src/execution.ts`: add the optional,
      `Readonly` field to `ExecuteOptions` with its doc comment
      (decision 5), marked as a q2 extension Q1 does not send, like
      `ownedLanguages`.
- [x] Rebuild the embedded host bundle: `cargo xtask
      build-engine-host-bundle` (or `npm run bundle -w
      @quarto/engine-host-node`) and commit
      `ts-packages/quarto-engine-host-node/dist/engine-host-node.mjs`.
      q2 embeds it with `include_str!` (`engine/ts_process.rs:59-62`),
      so without this the e2e tests run against a host that drops
      `executeEnv`. CI's bundle-freshness step
      (`ts-test-suite.yml`, `git diff --exit-code`) also fails on a
      stale bundle.
- [x] Wire the host typecheck into CI and `verify` in one commit (review
      P4, approved 2026-11-13). In `.github/workflows/ts-test-suite.yml`,
      add a "Typecheck engine-host-node" step running
      `npm run typecheck -w @quarto/engine-host-node`, next to the
      existing "Run engine-host-node tests (vitest)" step. In
      `crates/xtask/src/verify.rs`, add the counterpart
      `run_command("npm", &["run", "typecheck"], ...)` on
      `ts-packages/quarto-engine-host-node`, beside the
      pandoc-host/typst-host `typecheck:tests` calls. Gate it with
      `cargo clippy -p xtask --all-targets -- -D warnings` and
      `cargo nextest run -p xtask`.
- [x] Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings`;
      `cargo nextest run -p quarto-core`;
      `npm run test -w @quarto/engine-host-node`;
      `npm run typecheck -w @quarto/engine-host-node` (covers the type
      test and `@quarto/types` through its source path);
      `cd ts-packages/quarto-types && npx tsc --noEmit -p .`;
      after the bundle rebuild, `git status` shows no other change to
      `dist/engine-host-node.mjs`. Commit.

#### Session 1 evidence (2026-11-13)

**Red before implementation.**

- Rust unit tests (`ts_protocol::tests::execute_env_is_absent_when_empty_and_roundtrips`,
  the parity-fixture change, and the three `ts_engine` `execute_options_*` env
  tests): `cargo nextest run -p quarto-core -E 'test(ts_protocol::) | test(ts_engine::)'`
  failed to compile, six times ``error[E0609]: no field `execute_env` on type `ts_protocol::TsExecuteOptions` ``.
  A compile error counts as red here.
  failures: the three `executeEnv` host tests that expect a value (the one
  that expects the key to be absent passes already) and `TsExecuteOptions
  carries the EXR-7 keys`.
- Type test: `npx tsc --noEmit` gave
  `execute-env.type-test.ts(21,65): error TS2339: Property 'executeEnv' does not exist on type 'ExecuteOptions'.`
- e2e tests, run after the Rust field existed (Phase 1) but before the host
  and the forks changed, with
  `cargo nextest run -p quarto -p quarto-core --run-ignored only --no-fail-fast --no-capture -E 'binary(integration) & (test(execute_env_cli_e2e::) | test(julia_engine_e2e::))'`:
  `7 tests run: 1 passed, 6 failed, 2699 skipped` (the one pass is
  `pc4a_abandoned_worker_close_busy`, which prints a skip line and returns).
  The six failures are the new tests, each for the expected reason:
  - `julia_cells_see_the_execute_environment`: render error
    `KeyError: key "QUARTO_DOCUMENT_PATH" not found` from the cell;
  - `marimo_cells_see_the_execute_environment`: `read .../proj/sub/env.json: No such file or directory`
    (the cell could not read the variables, so it never wrote the file);
  - the four `julia_engine_e2e` cases (the daemon rerender, the path with a
    space, the shared worker and the single file outside a project): the
    cell printed `QUARTO_DOCUMENT_PATH=` followed by the word unset.

**Green after implementation.**

- `cargo clippy -p quarto-core --all-targets -- -D warnings`,
  `cargo clippy -p quarto --all-targets -- -D warnings` and
  `cargo clippy -p xtask --all-targets -- -D warnings`: clean.
- `cargo nextest run -p quarto-core`: 8393 passed, 40 skipped.
- `cargo nextest run -p xtask`: 268 passed, 0 skipped.
- `cargo nextest run -p quarto -E 'binary(integration) & test(execute_env_cli_e2e::)'`:
  2 passed (knitr, Jupyter), 607 skipped; the new tests are ignored.
- `npm run test -w @quarto/engine-host-node`: 181 passed (11 files).
  `npm run typecheck -w @quarto/engine-host-node` and
  `npx tsc --noEmit -p .` in `quarto-types`: clean.
- The bundle rebuild changed only `dist/engine-host-node.mjs`.

**Environment note.** An earlier `quarto-core` run failed because the
`pandoc` on `PATH` was 3.12.1 (`Q-20-2`, minimum 3.13). The pandoc in
`~/.local/bin` was swapped for 3.13, and the 3.12.1 binary was kept as
`pandoc-3.12.1.bak`.

### Session 2, Phase 3: marimo (fork, then sync)

- [x] In `external-sources/quarto-marimo` (check that the branch is
      `q2/node-port`, `git status` is clean, and `git fetch` shows it
      level with `origin/q2/node-port`):
  - in `src/engine/index.ts`, add `executeEnv?: Record<string, string>`
    to the local `Quarto2ExecuteOptions` interface and read it in the
    `const { ownedLanguages, units } = options as ...` line of
    `execute`;
  - pass it as a new optional `env` in the `runMarimoCompiler({...})`
    options;
  - in `src/engine/process.ts`, pass it to the `extract.py`
    `executeProcess(quarto, command, args, options.source)` call.
    `env` is the sixth positional parameter, so pass
    `compilerTimeoutMs()` explicitly as the fifth. Do not change the
    `uv run ... command.py` call.
- [x] Fork test in `tests/engine/process.test.ts` with the existing
      `withScript(...)` helper (`externalEnv: true`, a script that echoes
      a variable into its JSON payload); run `npm test` in the fork
      (Node 24).
- [x] Commit; push after Gordon's OK. Then run
      `scripts/sync-marimo-fixture.sh external-sources/quarto-marimo`
      from the q2 worktree. It needs a clean tracked tree in the fork,
      runs `npm run build` itself, and records the fork HEAD in
      `FIXTURE-SOURCE`. Run
      `QUARTO_MARIMO_SRC=/Users/gordon/src/q2/external-sources/quarto-marimo cargo nextest run -p quarto-core -E 'binary(integration) & test(marimo_fixture_fresh::)'`;
      commit the sync naming the fork commit.
- [x] Remove the `#[ignore]` from `marimo_cells_see_the_execute_environment`;
      it passes:
      `cargo nextest run -p quarto -E 'binary(integration) & test(execute_env_cli_e2e::marimo)'`.

### Session 2, Phase 4: julia (fork, then sync)

- [x] Clean the fork's working tree: the stray `.gitignore` edit and
      `deno.lock` from an Oct 13 smoke-test run were discarded at
      Gordon's request (2026-11-12).
- [x] Bring the fork checkout up to date first. On 2026-11-13 the local
      `q2/node-port` was at `ea4dcee`, five commits behind the pushed
      `4937625` (the subtree split q2 vendors, and the commit range that
      pins QNR `ffe5dab7`), with a stale `origin/q2/node-port` ref. Run
      `git fetch origin && git merge --ff-only origin/q2/node-port`, and
      confirm that HEAD equals the `version` of `subtree-julia-engine`
      in `resources/vendored.toml`.
- [ ] In `external-sources/quarto-julia-engine`: map `executeEnv` to
      `env` in `qnrRunOptions` (`src/record-wire.ts`), dropping
      `executeEnv`; add the field to `ExecuteOptions` in
      `types/quarto-types.d.ts` (repo root).
- [ ] Fork unit tests in `tests/unit/record-wire.test.ts` (it already
      imports `qnrRunOptions` and has `baseOptions()`): the mapping, and
      no `env` key when `executeEnv` is empty or absent. Run `npm test`
      and `npm run typecheck`; add a `CHANGELOG.md` line.
- [ ] Rebuild `_extensions/julia-engine/julia-engine.js` (`npm run
      build`, deterministic; the bundle is committed); commit; push after
      Gordon's OK.
- [ ] Sync: in a clean q2 tree, run
      `cargo xtask pull-extension-subtree julia-engine`. It fetches the
      pushed branch from GitHub (never the local clone), then runs
      `git subtree pull --squash`, which creates the squash commit
      (with a `git-subtree-split:` trailer naming the fork commit) and
      the merge commit itself. Then set `version` to the new split sha
      and `last_reviewed` in the `subtree-julia-engine` entry of
      `resources/vendored.toml`, run `cargo xtask lint` (the registry
      rule fails on a stale version), and commit that.
- [ ] Remove the `#[ignore]` from the julia e2e tests and from
      `julia_cells_see_the_execute_environment`; they pass (the CLI one:
      `cargo nextest run -p quarto -E 'binary(integration) & test(execute_env_cli_e2e::julia)'`;
      the quarto-core ones:
      `cargo nextest run -p quarto-core -E 'binary(integration) & test(julia_engine_e2e::)'`
      (the subtree is embedded with `include_dir!`, so the test binary
      rebuilds)).

### Session 2, Phase 5: docs

- [ ] `docs/guides/authoring/execution-context.qmd`: remove the line
      saying julia and marimo do not receive these variables; widen the
      "knitr engine and Jupyter kernels" sentence; note that
      `QUARTO_FIG_*` and the richer execute-info file are new for julia
      and marimo compared with Quarto 1; keep the "read it from a cell"
      caveat.
- [ ] `docs/guides/extensions/ts-engines-node.qmd`: in a new section
      after "Environment variables are replaced, not merged", document
      `executeEnv`, what it contains, and the opt-in contract for
      engines that run user code.
- [ ] `docs/guides/projects/environment.qmd` (the "Executed code"
      bullet, which names only the Jupyter and knitr engines) and
      `docs/guides/projects/profiles.qmd` (the "code cells and render
      scripts see `QUARTO_PROFILE`" item): name julia and marimo too
      (review P3, approved 2026-11-13).
- `cargo xtask build-agents-docs` is not needed. No gate runs it; it
  only refreshes the local `q2 docs llms` embed.

### Session 2, wrap-up

- [ ] clippy and nextest for `quarto-core` and `quarto`; the host
      package tests; recommend a workspace nextest run to Gordon.
- [ ] End to end through the real binary: with both syncs landed, run
      `cargo nextest run -p quarto -E 'binary(integration) & test(execute_env_cli_e2e::)'`
      with `--no-capture`, on a machine with julia, Node and uv. The
      test binary embeds the current host bundle and julia subtree.
      Confirm that no test printed a skip line. Record the command, the
      pass lines, and the marimo and julia `env.json` contents the tests
      print, and mark it "output inspected" here.
- [ ] `q2 render --strict` on the edited docs pages and this plan.
- [ ] Reconcile this checklist with what landed.

## Verified facts (review, 2026-11-12)

Checked against q2, the forks and QNR at `ffe5dab7`. Both installed
0.18.6 copies (`~/.julia/packages/QuartoNotebookRunner/cQERI` and
`Cuv1k`) are byte-identical to it in the files below.

- **QNR path of a run `env`.** The socket message is parsed into a
  `Dict{String,Any}`. `_extract_relevant_options` copies `env`
  (`options.jl`, default an empty dict). `_exeflags_and_env` calls
  `Base.byteenv(options["env"])` (`worker_setup.jl:122`). The result
  becomes `NotebookInitRequest.env_vars`, which the worker copies onto
  the per-file context on every run (`QuartoNotebookWorker.jl:140,151`).
  `RenderRequest` evaluation runs inside `NotebookState.with_env_vars`,
  which sets and restores `ENV`. `Base.byteenv` accepts a
  `Dict{String,Any}`; checked on Julia 1.12.7, including a value with a
  space and one with backslashes. A list of `K=V` strings would not
  match its method.
- **QNR scope.** The variables are set around cell and inline
  evaluation only, not during worker setup or package loading
  (`Pkg.activate`, hooks). Contexts are per file and dispatch to a
  worker is serialized, so shared workers keep values separate. A
  top-level `env` wins over frontmatter `julia.env`. In projects QNR
  also appends `QUARTO_PROJECT_ROOT` from `projectDir`
  (`worker_setup.jl:126-129`); q2 derives both from `ctx.project.dir`,
  so they agree. The restart check (`refresh!`, `worker_lifecycle.jl`)
  ignores `env`.
- **QNR stale copy.** A daemon worker also has the first run's values in
  its real process environment, from spawn. Cells never see them,
  because `with_env_vars` overrides them, but background tasks could.
  The same is true of a long-lived Jupyter kernel today.
- **Execute-info file lifetime.** It lives in `ctx.temp_dir()`, owned by
  the `StageContext` (`stage/context.rs`), so it outlives every engine
  call; each render gets a new path. Notebook replay passes an empty
  env, and the all-markdown fast path skips `prepare_execute_env`. One
  `execute_env` is computed per document and shared by every engine in
  a mixed document.
- **marimo spawn paths.** `executeProcess` is called twice: the
  `extract.py` compiler, which runs cells for both html and static
  modes, and the `uv run ... command.py` resolver. `executeProcess`
  merges `{...process.env, ...pythonUtf8Environment, ...env}`, so the
  `env` argument wins; its names cannot collide with `PYTHONUTF8`.
- **Quarto 1.** `dotenvSetVariables` (`quarto-cli/src/quarto-core/dotenv.ts`)
  and `project-profile.ts` set `_environment` values and
  `QUARTO_PROFILE` on `Deno.env`, and `environment.ts` sets
  `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE`
  and `QUARTO_EXECUTE_INFO` before each execute. marimo children
  inherited current values. The QNR server inherited the values of the
  render that started it, so later renders saw stale document values;
  only `QUARTO_PROJECT_ROOT` was refreshed. Neither engine had
  `QUARTO_FIG_*`.
- **q2-ojs** only reads `opts.target` (`ojs-engine.ts`), so an optional
  field leaves it unchanged.

## Implementability review (2026-11-13)

Two read-only reviewers checked whether a fresh agent can carry out each
checklist item as written: one for Session 1 (q2), one for Session 2
(forks, syncs, e2e, docs). The orchestrator re-checked every blocking or
surprising claim against the tree before accepting it. No decision
(1-12) needs to change.

### Fixed in place (wording, names, commands, missing mechanical steps)

- **Embedded host bundle (was blocking).**
  `ts-packages/quarto-engine-host-node/dist/engine-host-node.mjs` is
  tracked in git and embedded by `include_str!`
  (`engine/ts_process.rs:59-62`). CI rebuilds it and fails on a diff
  (`ts-test-suite.yml:212-216`). Phase 2 now rebuilds and commits it.
  Without that, the e2e tests could never turn green.
- **Stale julia fork checkout (was blocking).** The local
  `q2/node-port` is at `ea4dcee`. The pushed and vendored split is
  `4937625` (`resources/vendored.toml`, `subtree-julia-engine`), five
  commits later (`git log ea4dcee..4937625`), including the QNR pin. A
  first step to fast-forward was added to Phase 4.
- **julia sync bookkeeping.** `pull-extension-subtree` fetches
  `https://github.com/gordonwoodhull/quarto-julia-engine.git`, not the
  local clone, and makes its own squash and merge commits. The
  `vendored.toml` `version` must then be bumped, or the `cargo xtask
  lint` registry rule fails. Both are now spelled out.
- **No shared helper needed.** `ExecutionContext.project_env` is set
  at `stage/stages/engine_execution.rs:707-722`, already filtered by
  `env_for_subprocess` and with `QUARTO_PROFILE` applied, before
  `TsEngine::execute` calls `build_execute_options`. Phase 1 now says
  to merge `ctx.project_env` then `ctx.execute_env`. So the unit test
  can only check that a `QUARTO_PROFILE` pair passes through.
- **Parity fixture.** The instance is built in
  `render_wire_parity_fixture` (`exec_opts`), not in
  `make_execute_options`. The TS side is the `EXR7_OPTIONS_KEYS`
  contains-check. The exact regen command was added.
- **Type check.** `ts-packages/quarto-types` has no test or typecheck
  script. The existing pattern is
  `quarto-engine-host-node/src/__type-tests__/`, checked by
  `npm run typecheck -w @quarto/engine-host-node` (`tsc --noEmit`,
  which passes today). A positive assertion was added so the test can
  fail first.
- **Gate commands** named: `npm run test -w @quarto/engine-host-node`,
  `npm run typecheck -w @quarto/engine-host-node`, `npx tsc --noEmit`
  in `quarto-types`. No other crate, hub-client or wasm code consumes
  `TsExecuteOptions` or the host wire types.
- **marimo call site.** `executeProcess`\'s `env` is the sixth
  positional parameter after a defaulted `timeoutMs`
  (`src/engine/process.ts:127-134`), so the implementer must pass
  `compilerTimeoutMs()` explicitly. Named the `Quarto2ExecuteOptions`
  interface, the `withScript` test helper, the sync script's
  preconditions, and the `QUARTO_MARIMO_SRC` nextest invocation.
- **julia e2e harness.** No existing julia e2e uses `daemon: true`
  (every document sets `daemon: false`). The engine defaults the daemon
  off without an interactive session (`julia-engine.ts:209-213`), so
  the daemon and shared-worker cases must set it and isolate the server
  with j7's pieces (`seed_isolated_julia_runtime`, `HOME` override,
  `IsolatedJuliaServerGuard`). Julia cells have no JSON package. CI has
  no julia; `julia_available()` skips.
- **marimo e2e harness.** Activate the profile through `_quarto.yml`
  (`profile: default:`), never the process environment. `render_html`
  discovers the project (`render_to_file.rs:355`), and `extract.py` runs
  cells in html mode too, so "static mode" was dropped. Paths need
  canonicalizing.
- **Wrap-up e2e** made concrete. q2 embeds the julia subtree
  (`include_dir!`) and the host bundle, but not marimo, so the scratch
  project needs `_extensions/marimo/`. Nothing rendered is committed.
- `ts-engines-node.qmd`: section placement named.
  `build-agents-docs` is not needed (no gate runs it).

### Gordon's decisions (2026-11-13)

P1 approved (`#[ignore]` with a reason), P3 approved, P4 approved
(add the typecheck to both CI and `verify`). P2: drop the mutation
test and keep the copy, with a comment. P5: the marimo e2e moves into
`crates/quarto/tests/integration/execute_env_cli_e2e.rs`, with a
basic julia case beside it. The julia edge cases stay in
`quarto-core`, and the wrap-up runs the CLI tests instead of a manual
render. All five are now in the checklist.

### Raised for decision

- **P1. Red e2e tests vs. a green Session 1.** The marimo e2e tests
  run in CI: `test-suite.yml` installs uv and runs nextest with
  `QUARTO_CI=1`, and `node_available_for_tests` panics rather than
  skips there. The julia e2e tests run on Gordon's machine. Committing
  failing tests would turn the pushed branch red in CI and break the
  local `cargo nextest run -p quarto-core` gate. Proposal: commit them
  with `#[ignore = "bd-x3h4zjhl: needs <engine> fork sync (Session 2)"]`,
  prove them red with `--run-ignored only`, and remove the `#[ignore]`
  in Phases 3 and 4. Alternative: keep them uncommitted until Session 2.
- **P2. Host "mutation" test is vacuous.** Each request is
  `JSON.parse`d separately (`framing.ts:119`), so a mutation in one
  execute cannot reach another even without the host's copy. The test
  cannot fail first. Drop it, or keep it as a documented guard that
  is exempt from fail-first.
- **P3. Two more stale docs pages.**
  `docs/guides/projects/environment.qmd:52-54` says only Jupyter and
  knitr cells see the variables, and `profiles.qmd:18-19` says code
  cells see `QUARTO_PROFILE` without naming the engines. Both become
  incomplete or stale once julia and marimo receive the variables. Add
  them to Phase 5?
- **P4. Host typecheck is not in CI.** `npm run typecheck -w
  @quarto/engine-host-node` (and so the new type test) runs neither in
  `ts-test-suite.yml` nor in `cargo xtask verify`. Add it to both
  (AGENTS.md: verify mirrors CI), or accept a local-only gate?
- **P5 (optional).** A permanent CLI e2e through `CARGO_BIN_EXE_q2`,
  like `crates/quarto/tests/integration/execute_env_cli_e2e.rs`, would
  meet AGENTS.md's real-binary rule better than a recorded manual
  render. It would replace the wrap-up's manual render with a test.

## Notes

- **TS-only documents** get the execute-info file too, since
  `prepare_execute_env` runs whenever any engine executes code. It
  costs about 0.3 ms per document (PR #1161 measurements); accepted.
- **Related:** bd-i7yz08uh (cells of different documents interleaving
  on a shared Jupyter kernel) is the cautionary example: anything shared
  across documents in a long-lived process must not hold per-document
  state globally.
