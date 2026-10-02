# Plan: Typst worker (pandoc-host H7)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (PDF section, T1-T5)
**Depends on:** H0 only (independent of the pandoc host otherwise), except the budget measurement and the prior-art reading, which can start before H0; can run in parallel with H1-H5. **Unblocks:** H8.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H0's Close-out (including its STOP) is ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H8 (its Start gate decides whether it can begin).

## Overview

Measure the PDF download budget, then build a standalone typst compiler worker from typst.ts 0.7.0: lazy load, virtual filesystem, fonts, package registry, PDF export, tested against the real wasm.

## Decisions

- Budget set before measuring: a first-use PDF download of at most 40 MB gzipped in total (pandoc ~16 MB + typst ~11 MB + fonts 4-8 MB + the 2.3 MB package `index.json` + the `pdfjs-dist` pair (~0.5 MB gzipped; H9's stock viewer is a lazy fetch on first PDF view, measured there), four wasm modules in play: Rust, Automerge, pandoc, typst). If the measurement exceeds it, report to a human; nothing is auto-trimmed. The typst wasm is served like pandoc's, from `public/typst/` outside `assets/`, so the service worker's `wasm-cache` route (`maxEntries: 8`) is untouched.
- The typst compiler follows D2: its wasm is compiled on the main thread and the `Module` posted to one short-lived worker per compile, with the same wall timeout and idle drop as pandoc, so an abort terminates the worker without discarding the compile. typst.ts 0.7.0 accepts one: its init options' `getModule()` may return a `WebAssembly.Module` (`wasm.mts`), which the worker's init returns.

- The package-registry callback is synchronous (typst's `World` resolves packages synchronously), so the host prefetches the document's package set into memory before compile.

## Checklist

### Tests first
- [x] Valid PDF and correct page count on typst fixtures (the native `.typ` recordings from R0, with page-count assertions made against the fonts the host loads, since typst.ts ships none).
- [x] A missing package yields a diagnostic naming it; a package fetch failure (offline) likewise.
- [x] The pinned typst.ts version assertion (T2).

### Tasks
- [x] **Measure:** gzip size of the typst.ts wasm and the real default-font payload against the budget. Pin `packages.typst.org` tarballs by version/hash (T3) and note the CSP `connect-src` it needs.
- [x] **Read prior art** first: pandoc's own web app and changelog (3.9 onward) already run wasm pandoc with a wasm Typst to produce PDF and embed images for it (`wasm/index.js`).
- [x] **Worker:** lazy load; `map_shadow` virtual filesystem for the `.typ`, project files and vendored packages; fonts (typst.ts ships none: defaults from `typst-assets`, vendored Font Awesome, brand/project fonts); PDF export. Under Node a precompiled `Module` passed through `getModule` works without a custom loader (verified, including inside a `worker_threads` Worker).
- [x] **Package registry:** prefetch the document's package set (the vendored five plus any `@preview` imports found in the `.typ`) into memory before compile, caching tarballs in the Cache API at that point. Packages that those packages import are found by retrying: compile, prefetch the package named in each not-found diagnostic, recompile, with caps on count, bytes and time. The registry callback's context supplies `untar` (the default `FetchPackageRegistry` passes it the `.tar.gz` bytes it fetched synchronously from `packages.typst.org/preview/<name>-<version>.tar.gz`; `untar` takes the gzipped bytes and calls back `(path, bytes, mtimeMs)`, confirmed; a tarball made with `tar -C dir .` has `./` prefixes that break path resolution, so strip them), so no tar reader is needed; the registry is wired with `withPackageRegistry({resolve})` from `@myriaddreamin/typst.ts/options.init` plus `withAccessModel(new MemoryAccessModel())`; `resolve` writes each file under `/@memory/…` and returns that directory, is called about seven times per compile for one package so it caches, and returns `undefined` for an unknown package, which yields a diagnostic naming it (a throw loses the message); `index.json` is fetched only if a use is shown (import specs carry exact versions), otherwise it leaves the budget.
- [x] **Fonts API:** init passes `loadFonts(bytes, {assets: false})` explicitly (otherwise typst.ts fetches 17 fonts from jsdelivr, an older typst-assets, or loads none, and with none a compile "succeeds" with a text-less PDF and no diagnostic); a test asserts the default fonts are loaded. `typst_available_fonts` comes from `createTypstFontBuilder().getFontInfo(bytes)`, which returns family names (`{info: [{family, variant, …}]}`, verified; how its names, such as "Font Awesome 6 Free Solid", compare with native `typst fonts` is not), computed once per font file at load. `get_loaded_fonts()` (reachable as `driver.compiler.get_loaded_fonts()`, untyped) lists only fonts a compile has already used, so it is no source for the pre-pandoc list. The default fonts come from a pinned, checksummed download in the same script as pandoc's (the typst-assets 0.14.2 set is 8.8 MB raw, 5.8 MB gzip). The budget also counts the typst-assets export (2.5 MB of packages and fonts, R4).

**Measured (2026-10-02, evidence §14):** pandoc 16,652,658 + typst wasm 11,068,415 + fonts 5,857,677 + vendored export ~862,794 = 34,441,544 gzip bytes (32.85 MiB) against the 40 MiB budget; `index.json` is not fetched. The CSP `connect-src` for `packages.typst.org` is allowed (Gordon).

## Verification

Vitest against the real typst wasm; the measurement recorded against the budget.

## Exit

A standalone typst worker compiles `.typ` fixtures to PDF in Node and a browser. H8 chains it after pandoc.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h7-typst-worker` in workspace-7 (cut from `feature/pandoc-wasm` at bbe5c3a2c); landed by fast-forward into `feature/pandoc-wasm` (2026-10-02).
- Done: `ts-packages/typst-host` (TypstSession, packages, fonts, protocol; 39 Node tests on the real wasm), `resources/typst-wasm.json` + the assets in `scripts/fetch-pandoc-wasm.mjs`, hub-client `src/typst/` (loaders, TypstRunner, worker shell, tests), `e2e/typst-worker.harness.spec.ts` (11 pass in Chromium and WebKit), and `e2e/typst-measure.harness.spec.ts` (opt-in). Measure and prior-art done and written up (evidence §14, `dev-docs/pandoc-wasm-host.md`); H8 plan has a "From H7" note. All task boxes ticked.
- Gates (2026-10-02): hub-client `test:ci` unit 1384 / integration 155 / wasm 282 (baseline at H6's tip 1366 / 155 / 274: +18 and +8 are exactly `src/typst` unit and `typstHost.wasm.test.ts`; integration unchanged). The wasm suite first failed one R4 test (`Q-20-10` still emitted) because `crates/wasm-quarto-hub-client/pkg` predated R8; `npm run build:wasm` fixed it, no code change (the build also dirties `crates/wasm-quarto-hub-client/Cargo.lock` with R8's `css-inline`, which I reverted). `typst-host`: typecheck and 39 tests pass. Lint: one error in this phase's code (`prefer-const` in `typstRunner.ts`), fixed; the rest are the pre-existing ones elsewhere (203 errors in other files, not just the Editor/PreviewRouter pair the H6 note listed). `cargo clippy -p xtask` clean, `cargo nextest run -p xtask` 189/189; no other Rust changed, so the workspace nextest was **skipped**. The typst browser tests pass again in Chromium and WebKit after the fix.
- Gotchas: typst.ts's wasm remembers a failed package resolution for the instance's life, so the retry builds a fresh compiler. WebKit loses Cache API writes made by a soon-terminated worker, so tarball caching goes through the main thread (`cache-get`/`cache-put` messages). Vite needs `worker.rollupOptions.output.inlineDynamicImports` for typst.ts. Pass `{module_or_path}` to getModule to avoid a deprecation warning. A server that adds Content-Encoding to `*.gz` hands over decoded bytes (`isRaw`). The hub CSP (`quarto-hub/src/server.rs`) now allows `connect-src https://packages.typst.org` (Gordon, 2026-10-02); `wasm-unsafe-eval` is unverified and unchanged (evidence §14).
- Next step: H8 (its Start gate: H5's demo STOP, H7 and R4 ticked). Close-out was: reconcile the checklist, rebase onto `feature/pandoc-wasm` (a `package-lock.json` conflict is resolved by restoring the original and adding only the typst-host and `@myriaddreamin` entries, never a bare `npm install`), `git -C ../workspace-3 merge --ff-only pandoc-wasm/h7-typst-worker`, report the landing as unblocking H8, retire to `parking/workspace-7`.
