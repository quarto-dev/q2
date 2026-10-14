# Replace the embedded esbuild binary with rollup run under Node

> **Status (2026-10-13): implemented on `feature/rollup-ts-bundler`, one commit
> stacked on `feature/node-ts-engine-extensions` (#829).**

Strand: bd-which-ts-bundler-l3iebero. Predecessor attempts: embedded esbuild (P6 of
`2026-10-13-ts-engines-deno-to-node.md`), rolldown linked into q2 (branch
`feature/rolldown-ts-bundler`; abandoned, `serde_json` feature unification broke 18
`quarto-core` tests — its plan's "Outcome").

## Decision

`q2 call build-ts-extension` pipes an embedded, self-contained **rollup driver** to
`node -` (stdin) and passes the job as JSON in `Q2_TS_BUNDLE_JOB`. Node is the Node 24
q2 already needs for TS engine extensions (`quarto-node-discovery`). No new Rust
dependency, no embedded executable, nothing extracted to disk. Slower than esbuild
(measured below); simpler is the point.

### Choices (with the evidence)

- **Rollup flavour: `@rollup/wasm-node` 4.61.1**, not `rollup` (native per-platform
  `@rollup/rollup-*` packages). Pure JS + one 560 KB `.wasm` (used only for parsing and
  xxhash), so Windows, musl and darwin-x64 behave identically and none of them needs a
  native package. The `.wasm` is read via `__dirname`, so a build plugin
  (`esbuild.config.mjs`, `inline-rollup-wasm`) replaces that read with the bytes as base64
  and **fails the build if the lines it rewrites change**. Result: one 1.3 MB ESM file.
- **One file, run from stdin** instead of the `quarto-mcp-launcher` extract-to-a-hashed-cache
  pattern. The launcher extracts because its bundle is a directory (native addons, WASM);
  a single file needs no extraction, cache, hash, lock or cleanup. Windows command lines
  are too short for `-e`, so the script goes over stdin; the small job goes in an env var
  (no argv quoting questions). Deviation from "follow the launcher pattern" — flagged
  for Gordon.
- **TS stripping: Node's built-in `module.stripTypeScriptTypes`** (no dependency).
  - Real sources checked: host entry (`node-main.ts`), echo/content-claim/synth-echo
    fixtures, julia-engine, marimo (`q2/node-port`). All build, no enums etc.
  - Prints an ExperimentalWarning once → the driver is spawned with
    `--disable-warning=ExperimentalWarning`.
  - Byte stability across Node versions (the real risk of using the runtime's stripper):
    output is **identical on Node 24.0.2 (amaro 0.5.2) and 24.5.0 (amaro 1.1.1)** for all
    five real sources. Strip mode replaces types with whitespace, so offsets don't move.
    A future amaro that changes this shows up as a freshness-gate diff, not silently.
  - Non-erasable syntax (enum, namespace, parameter properties) is rejected by strip mode;
    the plugin retries that file in `transform` mode, so such sources still build
    (esbuild accepted them).
  - Known gaps vs esbuild: a *value-style* import of a type (`import { T }` without
    `type`) is a rollup `is not exported by` error (hint added in `explain_failure`);
    `.tsx`, tsconfig `paths`, `.js`→`.ts` specifier rewriting and JSON imports are not
    supported (no real source uses them).
  - Stripped regions leave whitespace-only lines in the output (committed fixture
    bundles will carry trailing whitespace).
- **Plugins**: `@rollup/plugin-node-resolve` 16.0.3 (already in the root lock) and
  `@rollup/plugin-commonjs` 29.0.3 (new), exact pins. Aliases are a ~12-line plugin with
  esbuild semantics (name or `name/…`, longest first) rather than `@rollup/plugin-alias`.
  `exportConditions: ['source','import']`, `.ts` in `extensions`, `preferBuiltins`.
- **Unresolved imports fail**: rollup's `UNRESOLVED_IMPORT` warning is turned into
  `Could not resolve "<specifier>" (imported by <file>)`, so the existing `jsr:` / `npm:` /
  `@std/` → `node:` hint and the `@quarto/*`-in-shipped-tier → `--workspace` hint still key
  off the quoted specifier. Output file is not written. `CIRCULAR_DEPENDENCY` warnings are
  dropped (esbuild is quiet about cycles); others go to stderr.
- **Byte-stable**: rollup single-file ESM output has no paths, timestamps or hashes.
  Verified two builds identical, and from different directories (vitest + Rust e2e).
- **Where the driver lives**: `ts-packages/quarto-ts-bundle` (`@quarto/ts-bundle`),
  built by esbuild (as the engine-host bundle is; esbuild stays a dev dependency of the
  TS packages) to a committed `dist/ts-bundle.mjs` (minified, `legalComments: eof`),
  `cargo xtask build-ts-bundle`, `build-all` step, CI freshness gate in
  `ts-test-suite.yml` next to the engine-host one. Embedded in `quarto-core`
  (`extension/build.rs`, `include_str!`) — like the host bundle; ~1.3 MB.
- **`build.rs` spawns Node directly**; the injected-function seam is gone (no heavy Rust
  dependency to keep out of quarto-core). `BuildOptions` lost `esbuild_binary` and
  `embedded_esbuild`.
- Licences: `resources/extension-build/ROLLUP-LICENSE.md` replaces `ESBUILD-LICENSE.md`
  (the 19 packages inlined into the driver, from the esbuild metafile).

## Not changed

- The extension config file is still named `esbuild.json` (format is bundler-neutral).
  **Follow-up for Gordon**: rename (e.g. `bundle.json`, accepting both for a while)?
- Host bundle (`engine-host-node.mjs`), hub MCP bundle, marimo/julia fork builds keep
  esbuild: they are build-time tools, not shipped, and moving them changes every committed
  byte.

## Tasks

- [x] Cherry-pick the esbuild-machinery removals from `135a53a38` (drop its
  `quarto-ts-bundle` crate and Cargo changes)
- [x] `ts-packages/quarto-ts-bundle`: driver, vitest suite (9), esbuild config, committed bundle
- [x] `extension/build.rs`: spawn Node with the embedded driver; hints; unit tests
- [x] Callers (`q2` bin, quarto-core fixture helper, quarto-preview capture driver)
- [x] New/updated Rust e2e tests (unresolved import, CJS dep runs, enums, missing Node,
  byte-stable across dirs) gated on `node_available_for_tests`
- [x] xtask `build-ts-bundle` + `build-all` step; `.gitignore`; CI freshness + vitest steps
- [x] Release-pipeline: verify step reworded (runs under the runner's Node 24: this is
  what exercises Windows/musl); staging step removed
- [x] Docs (`ts-engines-node.qmd`), licences (the TEMP Windows smoke script lives in #829, not here)
- [x] Verification: clippy clean for quarto-core, quarto, quarto-preview, xtask;
  `nextest -p quarto-core -p quarto -p quarto-preview -p xtask`: 7012 run, 7012 pass,
  35 skipped (no live baseline run at the integration tip; workspace-wide run and
  direct dependents not run — recommended before push)
- [x] Measurements: host entry ~0.4 s (esbuild ~0.1 s), small entries ~0.1 s; release
  `q2` (`cargo build --release -p quarto`, this machine) 102.4 MB raw. NOT comparable
  to the rolldown plan's table (different profile/flags) and the integration-tip build
  was not repeated; the embedded driver itself is 1.3 MB. Byte comparison: julia
  28.9 -> 42.1 KB, marimo v0.6.0 21.3 -> 25.5 KB (whitespace left by stripping,
  comments kept); fork freshness gates pin esbuild's bytes and would need to adopt
  this driver or re-pin.
- [x] Rebased onto the rewritten #829 tip (`9d4c930c1`) as one squashed commit; kept
  upstream's `strip_verbatim_prefix`, applied to the driver's alias targets. Re-ran clippy
  and `nextest -p quarto-core -p quarto -p xtask`: 6815 passed, 34 skipped
- [ ] Not verified: Windows/musl (release-pipeline verify step only);
  `cargo tree -p quarto-core -e features -i serde_json` vs the tip
