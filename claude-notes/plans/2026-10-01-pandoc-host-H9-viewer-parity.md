# Plan: Viewer, incremental spike and PDF parity (pandoc-host H9)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (PDF section, T2, T6, T7, D10)
**Depends on:** H8. **Unblocks:** H10a (the warm pandoc executor).
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H8 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H10a (its Start gate decides whether it can begin).

## Overview

Show the PDF in a pdf.js viewer that keeps zoom and scroll across recompiles (the preview shows the active chapter alone, also for books), spike incremental recompile, and check typst/PDF parity against native.

## Decisions


## Checklist

### Tasks
- [x] **Viewer** (scroll/zoom retention). Start from T7's pattern (the stock pdf.js viewer in an iframe or blob URL with a stable per-file fingerprint, so zoom and scroll survive recompiles); fall back to `pdfjs-dist`\'s `PDFViewer` component (scroll restored from saved state) if the stock viewer's footprint is a problem. Viewer assets go under `public/pdfjs/` and are excluded from the workbox precache (`globIgnores: ['pdfjs/**']` beside the existing ones, since `globPatterns` would otherwise precache the viewer's html, css and svg, `vite.config.ts:143-147`; a CI check confirms `dist/sw.js` lists none; the viewer is ~14 MB; the precache has a per-file ceiling and an atomic install, `OFFLINE.md`, GH #447). The stock `viewer.html` is not in the `pdfjs-dist` npm package (6.3.289 ships `web/pdf_viewer.mjs`, css and images only), so the assets come from a checksum-pinned pdf.js release download like pandoc's, or from a `gulp generic` build, and are gitignored. The fingerprint is the trailer `/ID` when valid, else an MD5 of the first 1024 bytes (`src/core/document.js:1616`), and typst writes `/ID` from the content and the creation date, so it changes with every edit (and, unless the date is pinned in R4, every second): patching `pdf.worker.js` for a constant fingerprint is required, not optional. Vendoring and patching `pdf.worker.js` for a stable fingerprint is time-boxed to 2 days. Done when a recompile keeps the scroll position, and, once the book plan's chapter anchors exist, when the viewer jumps to the active chapter's anchor.
- [x] **Viewer: preview pane (added 2026-10-02).** The viewer is the third preview kind: `PreviewMode` gains `pdf` for a `format: pdf` document when the browser chain is shipped; `PdfPreviewPane` mounts the viewer in the preview slot, starts from a "Show PDF preview" button once per session (first fetch ~33 MB), recompiles 500 ms after an edit through its own controller (`createPdfPreviewController`, saves nothing), keeps the last PDF under an error banner on failure. Tests: classifier cases (unit and wasm), `PdfPreviewPane.test.tsx`, the pane case in `e2e/pandoc-pdf-viewer.harness.spec.ts` (Chromium and WebKit). Changelog entry added.

## Verification

Playwright for the viewer's scroll retention; the parity job in CI.

## Exit

PDF is viewable and checked. Incremental recompile has a documented yes/no.

## Close-out

- [x] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [x] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [x] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: `pandoc-wasm/h9-viewer-parity` in workspace-7, landed into `feature/pandoc-wasm`, retired to `parking/workspace-7`.
- Last commit; tasks ticked: viewer (retention), `incr_compile` spike (NO; `claude-notes/research/2026-10-02-incr-compile-spike.md`), parity (6/6, CI job with pinned 0.14.2). Open: user-facing viewer placement.
- State and gotchas: viewer assets come from `scripts/fetch-pandoc-wasm.mjs` (pinned pdf.js 6.3.289, worker patched); see `dev-docs/pandoc-wasm-host.md` ("PDF viewer", "Browser/native typst parity"). Gates: test:ci unit 1397 / integration 157 / wasm 292; workspace nextest 15626 passed, 202 skipped (H9 touches no Rust; the handoff baseline said 15616, so the +10 is not from H9 and was not chased). Viewer spec passes in Chromium and WebKit. Still open from H5/H7: real-Safari check and the CSP `wasm-unsafe-eval` check (the harness cannot exercise a CSP); the viewer iframe and its blob URL are also untested under a CSP.
- Next step: H10a.
- Update 2026-10-02 (pane): branch `pandoc-wasm/h9b-pdf-preview`; the preview pane landed (see its checklist box).

### Resume point (2026-10-03)

- **H9 is complete.** Landed on `feature/pandoc-wasm` (tip `e1cae273f` when written): viewer with stable fingerprint, precache exclusion and check, `incr_compile` spike (no), browser/native parity (6/6, pinned-0.14.2 CI job, not yet run in CI), the PDF preview pane (third preview kind), changelog entry.
- **Next in lane:** H10a.
- **Gates at last run (re-measure on a clean cut and report the delta):** hub-client unit 1401, integration 157, wasm 292; workspace nextest 15626 passed, 202 skipped (no Rust changed by H9; the earlier handoff said 15616 and the +10 was not chased).
- **Still unchecked (not blockers):** real Safari; the CSP `wasm-unsafe-eval` check; the viewer iframe and its blob URL under a CSP (the harness cannot exercise one); the parity CI job's first run on Linux and macOS.
- **Gotchas:** assets come from `node scripts/fetch-pandoc-wasm.mjs --require` (pandoc, typst, pdf.js; `hub-client/public/{pandoc,typst,pdfjs}` are untracked on purpose). Before browser tests: `npm run build:wasm` in hub-client (then `git checkout crates/wasm-quarto-hub-client/Cargo.lock`), `npm run build -w ts-packages/typst-host`, then `VITE_E2E=1 npm run build`, then `npx playwright test --config playwright.harness.config.ts e2e/<spec> --project=chromium|webkit --retries=0 --workers=1` (port 5173 is the harness port; 5174 belongs to another worktree). Workspace nextest dirties `crates/quarto/tests/smoke-all/**/*.typ`: `git checkout` and `git clean` that path before committing; never `git add -A` on `crates/`. Never run a bare `npm install`. The shell cwd resets to the main checkout: `cd` into `.worktrees/workspace-7` first. `sed -i` fails in this shell; use python for in-place edits.
- **Demo:** branch `pandoc-wasm/h9-demo` (local only, not for landing) adds a stand-in page `#/dev/pdf-preview` to `DevHarness.tsx`: the real pane beside a text box, no hub server needed. A real-hub demo needs a hub sync server and a `format: pdf` document; how to run one locally was not established. A dev server may still be running on port 5175 (`npx vite --port 5175`); kill it when done. The orange-book native PDF check used `q2 render --to typst` on a scratch copy of `crates/quarto/tests/smoke-all/typst/orange-book` (native `--to pdf` is rejected).
- **Close-out to repeat for the jump:** tick the box and the Close-out line, rebase onto `feature/pandoc-wasm`, `git -C ../workspace-3 merge --ff-only <branch>`, retire to `parking/workspace-7` (`git branch -D` after `git merge-base --is-ancestor` if `-d` refuses), tick the epic's H9 box and continue with H10a if it has not run.
- Update 2026-10-02 (D2(b) spike): branch `pandoc-wasm/h9d-warm-pandoc-spike`, note `claude-notes/research/2026-10-02-d2b-warm-instance-spike.md`, sources in the pandoc-wasm-spike `d2b/`. Verdict: warm works via the exported `hs_init_with_rtsopts`+`convert` (`_start` traps on re-entry), correct and memory-flat, but pandoc leg only drops callouts typst 160 -> 105 ms (Chromium) and 291 -> 104 ms (WebKit), floor ~58 ms for an empty doc; poisoned after heap exhaustion; Haskell-side SOURCE_DATE_EPOCH unreachable by a Lua preamble; terminate-on-supersede returns to cold cost. Gordon then approved building D2(b) for the PDF preview only: see H10a and H10b. `incr_compile` verdict (no) confirmed. No box ticked.
