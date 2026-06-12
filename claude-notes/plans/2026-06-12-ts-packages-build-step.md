# ts-packages build step for xtask build-all / verify (bd-6rczoll3)

## Overview

`cargo xtask build-all` documents itself as the fresh-clone source of truth,
but no step in it (nor in `cargo xtask verify`) ever builds `ts-packages/*`.
This goes unnoticed because hub-client consumes ts-packages **from source**:
every ts-package's exports map points `types`/`source` at `./src/index.ts`,
hub-client's `tsc -b` has no project references into ts-packages, and Vite
bundles from source. The one runtime consumer of ts-package `dist/` output is
the **quarto-hub-mcp server**, which Node executes directly and which resolves
workspace imports through the `"import": "./dist/index.js"` export condition.
A missing/stale dependency dist (observed: `@quarto/quarto-sync-client` had no
`dist/` at all) kills the MCP at startup with `ERR_MODULE_NOT_FOUND`, which
the MCP harness surfaces as a `-32000` error.

Observed tree state at diagnosis time (2026-06-12): 7 of 9 ts-packages had no
`dist/` (`annotated-qmd`, `pandoc-types`, `preview-renderer`,
`preview-runtime`, `quarto-sync-client`, `sync-test-harness`,
`wasm-js-bridge`); the two that did had been built by hand.

## Design

1. **New shared helper** `crates/xtask/src/ts_packages.rs`:
   - `package_dirs(project_root) -> Vec<PathBuf>`: sorted list of
     `ts-packages/*` subdirectories containing a `package.json`; empty when
     `ts-packages/` is absent (old branches). Unit-tested with tempdir
     fixtures.
   - The build invocation is one root-level npm run:
     `npm run build --if-present -w ts-packages/<a> -w ts-packages/<b> ...`
     `--if-present` covers packages without a `build` script
     (`sync-test-harness`, `wasm-js-bridge`). Build **order doesn't matter**:
     since types resolve via `src/`, each package's `tsc` compiles without its
     dependencies' `dist/` present.
2. **build-all**: new step between `npm install` and the hub-client build,
   with a `--skip-ts-packages-build` flag.
3. **verify**: new step 6 (before the hub-client build; `TOTAL_STEPS` 12→13,
   later steps renumbered), same `--skip-ts-packages-build` flag. The step
   builds ts-packages, then **smoke-checks the MCP server**:
   `node ts-packages/quarto-hub-mcp/dist/index.js --help` must exit 0. The
   `--help` path runs after the whole ESM graph links, prints usage, and
   exits 0 — no hang, no network, no auth env needed. A broken module graph
   exits 1 with `ERR_MODULE_NOT_FOUND` before any code runs.
4. **Cross-platform fix**: quarto-hub-mcp's build script is
   `tsc && chmod +x dist/index.js`, which fails on Windows (no `chmod`).
   Replace with a portable node one-liner
   (`node -e "fs.chmodSync(...)"`-style, no-op semantics on Windows) so the
   new verify/build-all step doesn't break Windows.
5. **Docs**: update step lists in `build_all.rs` / `verify.rs` module docs,
   the `Verify` / `BuildAll` doc comments in `main.rs`, and the verify
   description in `CLAUDE.md`.

Out of scope (deliberately): TypeScript project references between
ts-packages and hub-client; building hub-client/trace-viewer/q2-preview-spa
from the root `npm run build --workspaces` script (they keep their dedicated
steps, which include WASM and embedding concerns the root script doesn't
know about).

## Work Items

### Phase 1: Red (failing checks first)

- [x] Reproduce the failure mechanically: `node
      ts-packages/quarto-hub-mcp/dist/index.js --help` exits 1 with
      `ERR_MODULE_NOT_FOUND` for `@quarto/quarto-sync-client/dist/index.js`
      (recorded below)
- [x] Write unit tests for `ts_packages::workspace_paths` (tempdir fixtures:
      sorted result, skips dirs without package.json, missing ts-packages/
      → empty, ignores plain files)

### Phase 2: Implementation

- [x] Add `crates/xtask/src/ts_packages.rs` helper + tests
- [x] Make quarto-hub-mcp's `build` script cross-platform (drop bare `chmod`;
      now `node -e "import('node:fs').then(fs => fs.chmodSync(...))"`)
- [x] build-all: add ts-packages step + `--skip-ts-packages-build` flag
- [x] verify: add step 6 (build + MCP smoke check), renumber steps 6–12 →
      7–13, add `--skip-ts-packages-build` flag
- [x] Update doc comments in `main.rs`, module docs, `CLAUDE.md`

### Phase 3: Verification

- [x] `cargo nextest run -p xtask` — 4 new unit tests pass
- [x] End-to-end: verify with all other steps skipped → ts-packages step
      builds 7 packages (2 have no build script, covered by `--if-present`)
      and the MCP smoke check passes; same for the build-all step
- [x] End-to-end: `node ts-packages/quarto-hub-mcp/dist/index.js --help`
      exits 0 and prints usage (the original failing command from the bug
      report)
- [x] `cargo build --workspace` (clean) + `cargo nextest run --workspace`
      (9967 passed, 196 skipped)
- [x] `cargo xtask verify --skip-hub-build` (also exercises the new step
      for real) — all steps passed, exit 0
- [ ] Close bd-6rczoll3

## Evidence

### Red state (before fix, 2026-06-12)

```
$ node ts-packages/quarto-hub-mcp/dist/index.js --help
Error [ERR_MODULE_NOT_FOUND]: Cannot find module
'/Users/cscheid/repos/github/quarto-dev/q2/node_modules/@quarto/quarto-sync-client/dist/index.js'
imported from .../ts-packages/quarto-hub-mcp/dist/connection-manager.js
exit code: 1
```

(Tree state: 7 of 9 ts-packages had no `dist/`. The `--help` flag still
fails because ESM module resolution happens before any code runs — which is
exactly why it works as a smoke check.)

### Green state (after fix)

```
$ cargo xtask verify --skip-rust-build --skip-rust-tests --skip-hub-build \
    --skip-hub-tests --skip-trace-viewer-build --skip-trace-viewer-tests \
    --skip-treesitter-tests --skip-shared-package-tests --skip-q2-preview-spa-build
━━━ Step 6/13: Building ts-packages workspaces ━━━
> @quarto/annotated-qmd@0.1.1 build ... (7 packages built via
  npm run build --if-present -w ts-packages/<pkg> ...)
  ↳ Smoke-checking quarto-hub-mcp module graph (node dist/index.js --help)...
Usage: quarto-hub-mcp --server <url> [--read-only] [--redirect-port <N>]
  ✓ quarto-hub-mcp module graph loads
✓ ts-packages build complete
...
✓ All verification steps passed!

$ node ts-packages/quarto-hub-mcp/dist/index.js --help
Usage: quarto-hub-mcp --server <url> [--read-only] [--redirect-port <N>]
MCP --help exit code: 0
```

Output inspected: usage text printed, exit 0, and
`ts-packages/quarto-sync-client/dist/index.js` (the originally missing
module) now exists.
