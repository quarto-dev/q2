# Plan: "Download as" UI (pandoc-host H5)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D8, D11, Failure taxonomy)
**Depends on:** H3, H4; request R2 (resolver and format table), R3 (cell count; the control works without it), R4/R5 for the typst/pptx/epub entries, R7 for project downloads; the embedded-hub item also needs H4b. **Unblocks:** H6, H8.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H3 and H4 are fully ticked, and so is R2. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H6 once the demo STOP passes; the menu-wiring items in Close-out stay open until R3, R4, R5 and R7 land, and the agent returns to them then.

## Overview

The product surface: a "Download as" control next to the print button, a download-status channel, the three-class format classification with a `PreviewRouter` mode, accessibility, E2E and the changelog. docx first; the menu grows as request phases land.

## Decisions

- The menu comes from the Rust-owned format table; a click renders with the format overridden, on click only (D8.1, D8.3).
- Documents the preview cannot render get no preview, only a "Download <type>" button for the document's own format, rendering only on click (D8.2); three classes with `aria-disabled` explanation for "neither" (D8.4).
- A document with errors does not download; warnings still download (D8.5).

- **From R4 (landed):** `getPandocFormats()` rows have a `hidden` flag (`pdf` is hidden until H8): filter on it when building the menu. A typst request can carry two warnings the menu should show: `Q-20-9` (a remote image; the run will fail until R6's prefetch) and `Q-20-10` (a styled raw HTML table is not CSS-inlined until R8). The `.typ` download names no images or brand assets, so say it may have dangling resource references (D8.6).


## Checklist

### Tests first
- [ ] The download-name sanitizer handles bidi controls, control characters and `../`.
- [ ] Two quick clicks leave one live worker and the first is reported as superseded; a stale `render_pandoc_request` response from an earlier click is dropped (click-id tagging, D8.5).
- [ ] A document with an error diagnostic or non-zero exit produces no Blob and no download; a warnings-only run does download.
- [ ] axe harness (`e2e/*.harness.spec.ts`, `e2e/helpers/axe-baseline.json`): add the control, progress and disabled states to a `DevHarness.tsx` page (the baseline is a characterization test: regenerate with `AXE_BASELINE_WRITE=1`), and build the menu on the shared `Menu.tsx`.

### Tasks
- [ ] **Order of work:** (a) control, download, name sanitizer and status channel for docx; (b) progress, cancel, supersession, accessibility; (c) classifier and router mode. (a) needs no classifier; (c) needs R2's resolver and touches `PreviewRouter.tsx`/`Editor.tsx`, so it is serial.
- [ ] **"Download as" control** in the same `DocumentTopBar` slot as the print button (print is shown only when `canOpenPrintable` and not in fullscreen, `DocumentTopBar.tsx:67-72,115`, so for a docx-format document there is no print button to sit next to; the control renders regardless) (menu from the Rust-owned format table, skipping entries flagged hidden such as `pdf` until H8; docx first): render with format overridden, on click only → `render_pandoc_request` → main-thread loader → worker → Blob download. The file is named `<doc-stem>.<ext>`, sanitized.
- [ ] **Download-status channel.** The editor's diagnostics array is replaced on every live-preview render and cleared on file switch (`Editor.tsx:370,583,569,991`), so pandoc diagnostics must not live there (the main thread calls the classify export on each `execute` result and feeds the shared `Diagnostic` union, H1); unlocated ones can reuse the banner (`:1611`). Every classified pandoc warning (`Q-11-1`, which covers all pandoc stderr warnings, including the missing-resource alt-text substitutions) and the unexecuted-cell count (D11) are summarized visibly; a failed run (`Q-20-3`, a host failure kind, or a load error) shows its diagnostic here too.
- [ ] **Progress and cancel.** No progress surface exists (closest: `DocumentTopBar`'s print button, `DocumentTopBar.tsx:65-82`), so the byte-progress UI, the first-use size hint (~16 MB), the cancel control, the supersession policy and the live region are separate tasks. Progress updates are throttled.
- [ ] **D8 classification** (needs request R2's resolver and format table): the three-class classifier replacing `getQ2Format`'s `null` (`getQ2Format.ts:29-41`; `PreviewRouter.tsx:~174-186` currently mounts the HTML `Preview` for docx/pdf), the third `PreviewRouter` mode (an enum, so a later "pdf-preview" mode fits), and the "Download <type>" button for unsupported document formats rendering only on click. `Preview.tsx`/`ReactPreview.tsx` render on edit (20 ms debounce), so the "neither" state mounts neither. The menu and the classifier both touch `PreviewRouter.tsx` and `Editor.tsx` and are implemented serially.
- [ ] **Accessibility:** keyboard/ARIA for the menu, `aria-disabled` plus a described explanation (not a tooltip only) for the disabled button, progress and errors in a live region. Tab-kill risk on low-memory mobile devices is accepted (no `deviceMemory` gate); the size hint is the mitigation.
- [ ] **Embedded hub:** in the embed the control routes to the native render mechanism built in H4b. H4b landed `isPreviewEmbed()` (`VITE_PREVIEW_EMBED=1`, `src/pandoc/featureFlag.ts`) and `renderNatively({path, format, content})` (`src/pandoc/nativeRender.ts`: outcome `ok` with blob, file name and warnings / `failed` for HTTP 422 with diagnostics / `error`), over `POST /api/preview/render`, which accepts docx, pptx and epub only: native `format: typst` compiles to PDF, so the `.typ`-only download needs a pipeline option that stops before `TypstCompileStage` (no phase owns it yet; hide typst in the embed until one does). Pass the editor's current text as `content`. In the embed `pandocWasmEnabled()` is false, so gate the menu on `pandocWasmEnabled() || isPreviewEmbed()`.
- [ ] Playwright E2E for docx; changelog entry (`hub-client/changelog.md`) and a short user-facing note on what filters and extensions can do in the browser (design D9).

## Verification

hub-client unit tests, the axe harness, Playwright E2E for docx in CI; a manual check that the downloaded docx opens in Word.

## Exit

**STOP:** the H5 demo is the first human checkpoint after the slice: review before H6 and before the request epic's post-slice phases (R3-R8) are wired into the menu.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Manual: the downloaded docx opens in Word.
- [ ] **STOP:** the H5 demo, human review before H6 and before R3-R8 are wired into the menu.
- [ ] After the STOP, as each request phase lands on `feature/pandoc-wasm` (lane H meanwhile does H6 and H7 and comes back): the unexecuted-cell status (R3) is shown; the typst entry (R4) is wired; the pptx and epub entries (R5) are wired; project downloads (R7) are wired. Each is its own landing.
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
