# Plan: PDF chain (pandoc-host H8)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (PDF section, D8.6)
**Depends on:** H5 (the control and status channel), H7 (typst worker), request R4 (typst-source request); typst documents with remote images also need R6. **Unblocks:** H9.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H5's demo STOP, H7 and R4 are ticked (R6 too for typst documents with remote images). If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H9 (its Start gate decides whether it can begin).

## Overview

"Download as PDF": the pandoc worker produces the `.typ`, the typst worker compiles it. This phase adds the compile-side request additions and the orchestration, with one progress and diagnostic channel across both stages.

## Decisions

- The PDF request is the typst-writer request from request R4 with `output_path = *.typ` and a `post: compile_typst` step.
- The PDF chain is font-list → request → pandoc → typst compile: the typst compiler is loaded (fonts known) before pandoc runs, and `render_pandoc_request` takes the optional `typst_available_fonts` that request R2's export reserved.

- **From R4 (landed):** the `pdf` request is `render_pandoc_request(path, 'pdf')`: the typst request with `post: "compile_typst"` and `output_path` a `.typ`. Before compiling, prepend `typst_date_prelude(Number(request.env.SOURCE_DATE_EPOCH))` to the `.typ` (typst.ts has no date option; with it two compiles are byte-identical). Compile inputs: `get_typst_assets()` (packages under `packages/`, fonts under `fonts/`; its own `typst_assets_version`, not the share tree) plus the request's `resource_refs` (the AST's images and the brand's `source: file` fonts and logos, mounted only for `pdf`). Pass `typst_available_fonts` (from the loaded fonts) to the render call; it feeds the filter param. The `pdf` row of `getPandocFormats()` has `hidden: true`; drop that flag in Rust when this phase wires the menu, and then the resolver classes `pdf` as `download`.

- **From H7 (landed):**
  - Use `runner.listFonts()` (`TypstRunner`, from the font builder's `fontFamilies()`) for `typst_available_fonts`; load the compiler before the pandoc request is built.
  - The typst job's `vendoredPackages` and `fonts` are `splitTypstAssets(get_typst_assets().files)` (`hub-client/src/typst/typstAssetSplit.ts`).
  - `typstUiStateFor(outcome)` can return `typst-error` and `package-error`, which have **no copy yet**. Add both to `FailureState` and `strings.ts` and update the exhaustive test, as in `DownloadAsControl.integration.test.tsx`.
  - The registry origin `https://packages.typst.org` is allowed by the hub CSP already. The first-use total is measured: 34,441,544 gzip bytes for pandoc + typst + fonts + the vendored export (evidence §14); use that, not "~16 + ~11 + fonts", for the size hint.
  - Pandoc's own web app works around pandoc#11584 (images extracted to a temp dir in the wasm) with a Lua filter that turns mediabag images into `data:` URIs; our shared-tree mount should make that unnecessary, but it is the fallback if the chain's image test fails on mediabag images.

## Checklist

### Tests first
- [x] A typst fixture goes through pandoc then typst to a valid PDF with the expected page count. (`pdfChain.wasm.test.ts`: the production `DownloadController` over the real Rust wasm, pandoc.wasm and typst in worker threads; 2 pages; browser twin `e2e/pandoc-pdf-chain.harness.spec.ts`, Chromium and WebKit.)
- [x] A typst fixture with an image and a template import compiles (the compile sees the same tree pandoc did). (Image via `resource_refs`; a user template partial; a callout that imports the vendored `@preview` packages and Font Awesome with no registry fetch. Mutation-checked: dropping `resource_refs` or `vendoredPackages` from the compile fails the image and callout tests. A raw `#import "part.typ"` of another project file is NOT mounted, see the handoff log.)
- [x] An abort during either stage terminates the right worker and reports once. (Fake-runner tests for the pandoc stage, the typst stage and a superseding click; real workers: cancel at `typst-compiling` in `pdfChain.wasm.test.ts` and in the browser spec, which then completes a second click.)

### Tasks
- [x] **Host side of the compile additions** (request R4's last task supplies the `pdf` format entry, `post: compile_typst`, the packages/fonts in the typst-assets export and `typst_available_fonts`): the typst worker mounts the pandoc request's own tree (share tree, `files`, `resource_refs`, at the same absolute paths) plus the produced `.typ`, with compile root `/`, so images and template imports resolve; hand the vendored `TYPST_PACKAGES_DIR` + Font Awesome fonts to the typst worker; brand `source: file` font bytes from the request's `resource_refs`; `typst-available-fonts` from the font builder's family names for the fonts the typst worker loaded (H7).
- [x] **Chain:** pandoc worker → typst worker, "Download as PDF" in the H5 menu, progress across both stages; each stage's `diagnostics[]` is tagged `stage: pandoc|typst` and concatenated in stage order on one channel, an error from either blocks the download and warnings from both are shown; one-in-flight and cancel semantics from D8.5.
- [x] First-use download totals shown in the size hint (pandoc ~16 MB + typst ~11 MB + fonts). (`download.sizeHintPdf`: about 33 MB in all, the measured 34,441,544 gzip bytes.)

## Verification

Vitest for the chain in Node, Playwright for "Download as PDF" on a typst fixture.

## Exit

PDF download works. H9 adds the viewer and parity checks.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS). (`cargo nextest run --workspace`: 15616 passed, 202 skipped. The Rust diff adds and removes no test (one test renamed, assertions changed for the un-hidden `pdf` row), so the delta against the cut is 0 by construction, not by a base run. `test:ci` against the cut's measured 1384 / 155 / 282: unit 1395 (+11, the chain's controller tests), integration 157 (+2, the control's PDF hint and typst copy), wasm 290 (+8, `pdfChain.wasm.test.ts`). clippy `-D warnings` on quarto-core clean; ESLint clean on every touched file, the 195 errors elsewhere pre-exist. Browser: `e2e/pandoc-pdf-chain.harness.spec.ts` 3/3 on Chromium and on WebKit.)
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed. (re-read 2026-10-02; the tick on the "same tree pandoc did" test is qualified: see the gap in the handoff log.)
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h8-pdf-chain`, workspace-7 (cut from `feature/pandoc-wasm` at `7be5d1fe0`).
- Last commit; tasks ticked: `cb47c9a48` (the chain); all Tests-first and Tasks boxes, then Close-out.
- State and gotchas:
  - The chain lives in `DownloadController.runWasm` (`format.key === 'pdf'`), with `DownloadDeps.typst` (`runner`, `assets`, `datePrelude`). `buildRequest` gets `typstAvailableFonts` as a fifth argument for `pdf` only.
  - Both runners transfer (detach) the buffers they are given: the controller copies `request.files` and `resource_refs` before the pandoc run, copies the share tree for the compile, and asks `assets()` once per typst job (`listFonts` and `run` each need their own font buffers).
  - Diagnostics from the two stages share the type `Diagnostic = PandocDiagnostic | TypstDiagnostic` (exported from `downloadController.ts`); `DownloadAsControl` shows a typst diagnostic as its message plus `path range`.
  - `pdf` is no longer hidden in the Rust table, so a `format: pdf` document is download-class; `MENU_FORMATS` is `['docx', 'pdf']`. `pdf` is not in `NATIVE_FORMATS`: the embed does not offer it.
  - **Gap (not fixed, outside H8):** the compile mounts what pandoc saw (the share tree, `files`, `resource_refs`). A raw typst block that `#import`s or `#include`s another project file fails with "cannot read file outside of project root", where the native renderer reads the file from disk. That needs the request builder (Rust) to mount such files; it is a parity item for H9 or a request-lane follow-up. Gordon decides.
  - The H0 spike dir, `.cache/pandoc-wasm/`, `hub-client/public/pandoc/` and `hub-client/public/typst/` stay untracked and intentional. Running the workspace nextest rewrites rendered `.typ` files under `crates/quarto/tests/smoke-all`; revert them (`git checkout` / `git clean` on that path) before committing.
  - Real-Safari and CSP (`wasm-unsafe-eval`) checks from H5/H7 remain open for Gordon.
- Next step: H9 (its Start gate decides whether it can begin).
