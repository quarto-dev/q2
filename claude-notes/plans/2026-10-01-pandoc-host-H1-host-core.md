# Plan: Host core (pandoc-host H1)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (Contracts, D2, D4, D6, Who owns what)
**Depends on:** H0 (command-mode result); request R1's first task (the published `pandoc-request.schema.json` and golden). **Unblocks:** H2, H4.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H0's Close-out (including its STOP) and R1's "Publish the contract" task with its schema-review STOP are ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H2 (its Start gate decides whether it can begin).

## Overview

The DOM-free host package: a WASI command-mode runner over a compiled `WebAssembly.Module`, mounting of the share tree and request files with limits and path rules, the download-by-checksum script, and the vitest/CI wiring that runs the real wasm. Uses the captured H0/R0 directories as disk fixtures until request R2's exports exist.

## Decisions

- Command mode per the H0 result; the host drops the spike's reactor model (`hs_init`/`convert`, `fileSystem.clear()`, the `stdin`/`stdout`/`warnings` magic files). `spike/host-patched.js` provides the nested-directory idea but cannot be reused as-is (it drives the reactor exports).
- The host takes no part in UI policy (supersession, classification).
- Limits (decided; kept in `resources/pandoc-wasm.json`, shared with `prepare()`): images ≤ 25 MB each, `reference-doc` ≤ 50 MB, total mounted payload ≤ 300 MB. **Open:** the decompression-ratio guard has no number yet; H3 sets it from measurement. **Open:** how the limits reach the host (request fields in the R1 schema, or a Vite virtual module like `attributionViewerCssPlugin`; `rootDir ./src` and `?raw` outside the project root rule out a direct import of the repo-root JSON); choose in the scaffolding task.

## Checklist

### Tests first
- [ ] The captured fixtures through this host produce the native recorded output byte for byte for docx and typst (equal under `SOURCE_DATE_EPOCH`, R0), and equal under the extractor for pptx and epub.
- [ ] A project `filters/main.lua` cannot shadow the vendored one: a `resource_refs` path under the share root is rejected, `request.share_root` equals the constants file's value, and every argv path is under the share root or `project_root` (which the request carries, R1). Every limit is enforced. Each mount-rule violation (design: Contracts) is rejected with a diagnostic naming the path, with nothing overwritten: a path in two of the share tree/`files`/`resource_refs`/`dirs`, a file/directory collision, `resource_refs` under `/__q2_share__` or `/tmp`, `files` under `/tmp`. A `resource_refs` path that normalizes outside the allowed root (design D3) is rejected. Parent directories of files are created implicitly, and `/tmp` is mounted only because `dirs` names it.
- [ ] Host behaviours: `+RTS -M5m` exits 251; the stderr `Fd` keeps an unterminated last line; no console output during a run (`{debug:false}`); a filter echoing `os.getenv('QUARTO_FILTER_PARAMS')` sees the request env; a changed `share_tree_version` remounts; a non-ASCII document name works in argv and paths.
- [ ] A pure path-normalizer test runs `C:\proj\a.qmd` on every OS.

### Tasks
- [ ] **Package scaffolding.** Host unit tests (path normalizer, limits, mount) run in the package; real-wasm tests are `hub-client/src/**/*.wasm.test.ts` (that is what `vitest.wasm.config.ts` includes) and run in the `test-suite` job after the download step, not in `workspace-ts-suites` (no wasm or pandoc there). The R0 recordings are the fixtures (location per R0). A DOM-free host package in `ts-packages` (`execute(request, shareTree, { module, fault? }) -> result`; a worker-agnostic **core module** so node-environment vitest can test it; a thin `self.onmessage` shell lives in hub-client) and a typed message protocol (init with the `Module`, run, progress, result); request bytes are deduplicated by buffer in the transfer list. `WebAssembly` needs the `WebWorker` lib or a local declaration (`@types/node` has none). It needs a `package.json` with a `build` script (tsc to dist; see `crates/xtask/src/ts_packages.rs` and the "Build ts-packages workspaces" step in `ts-test-suite.yml`), aliases in both `hub-client/vitest.config.ts` and `vitest.wasm.config.ts` (hand-listed), lockfile entries for `@bjorn3/browser_wasi_shim` and `fflate`, and a step in the `workspace-ts-suites` job (`ts-test-suite.yml`, lists packages explicitly) if it has its own tests. The `PandocRequest` TS type is generated from or checked against `pandoc-request.schema.json`. `tsconfig.app.json` has `lib: ["ES2022","DOM"]`, so the worker file declares `self` as `DedicatedWorkerGlobalScope` (or has its own tsconfig); `erasableSyntaxOnly` bans enums and parameter properties in the protocol types.
- [ ] **WASI host** in command mode: takes a compiled `WebAssembly.Module`, real argv and env, runs `_start`, catches `WASIProcExit`, captures stderr and stdout with a custom `Fd`, reads the output file back from the shim's directory tree; the shim is created with `{debug: false}`, with `args_sizes_get` overridden to count UTF-8 bytes, and absolute paths resolved against a `/` preopen (the shim's `Path.from` rejects a leading `/`, but `new PreopenDirectory('/', …)` resolves absolute paths directly). `_start` runs once per instance (a second call traps), so every render is a fresh instance. Includes the typed fault-injection option from the design (OOM via `+RTS -M`, crash by throwing from an import, hang by a never-returning `poll_oneoff`; the shim's `poll_oneoff` busy-waits, so hang tests run the core in a terminable `worker_threads` Worker). Defines `ExecuteResult` and its failure kinds (success = exit 0 and output present); taxonomy diagnostics that originate in TS use a TS-owned shape with no `Q-` code; one `Diagnostic` union covers these and the Rust classify export's output, with the optional `stage: pandoc|typst` tag H8 uses.
- [ ] **Mounting.** Share tree + `files` + `resource_refs` at the request's absolute paths, plus `dirs` (including `/tmp`); parent directories of every file are created implicitly, and the mount rules (design: Contracts) are checked first, so a violation rejects the request and order does not matter. The main thread reads the share tree from the Rust export **once** per `share_tree_version`, keeps it, and posts it to each worker by structured clone (~1.35 MB); `execute(request, shareTree)` takes it as a second argument, and only `files[].bytes` are transferred. Security invariant: pandoc's filesystem holds only the share tree, `files`, `resource_refs` and `dirs`, with no host passthrough. Limits and path rules as above; over-limit is a diagnostic naming the file. Until request R2's export exists, tests mount the `$QUARTO_SHARE_PATH` copy inside the R0 recordings (the layout is `filters/`, `pandoc/datadir/`, `formats/docx/`; `resources/pandoc-filters/` alone lacks the callout PNGs).
- [ ] **Download-by-checksum script** (Node; extracts the official zip with `fflate`, not `unzip`, per `cross-platform.md`; repacks to the opaque `.gz` asset) into a gitignored cache dir (the decompressed wasm, which vitest reads) and `hub-client/public/pandoc/pandoc.wasm.gz` (the served asset; the script adds `public/pandoc/` to `hub-client/.gitignore`), reading the shared constants (`resources/pandoc-wasm.json`); verifies the upstream zip SHA and the decompressed-wasm SHA; with a `--require` mode (fails when the asset cannot be fetched or verified) and a default mode that skips with a message; called by vitest, CI, `cargo xtask verify`, the hub-client `build:all` chain (H4) and the entry points that bypass it (`test:e2e`, `test:harness`, `scripts/build-local-prod.sh`); documented in `dev-docs/`. A cache miss during a GitHub outage fails CI (it never silently skips).
- [ ] **Tests and CI wiring.** Real-wasm vitest in `vitest.wasm.config.ts` (`*.wasm.test.ts`; `vitest.config.ts` excludes them), already part of `test:ci`. The exnref flag goes in top-level `test.execArgv` (vitest 4; confirm against the installed version) of that config with `pool: 'forks'` asserted (V8 flags are not reliable under threads), not `NODE_OPTIONS`, which rejects it. Locally a missing wasm skips with an explicit message; in CI (`CI` set) it fails. `ts-test-suite.yml` has no `paths:` filter (ubuntu + macos, Node 24; "Run hub-client tests" is at `:187`; `PANDOC_VERSION: "3.11"` already appears at `:18`); add the download/cache step before it.

## Verification

Hub-client vitest and the wasm vitest config pass locally and in CI with the downloaded wasm; the package builds.

## Exit

The host runs a request on disk fixtures in Node. H2 adds the browser loader and worker lifecycle.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
