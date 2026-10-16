# Plan 5: explicit book scope in the wasm option and hub-client

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E11); the execution model and landing procedure are there.

> **Superseded in part by [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md):** book scope is now the `book-scope` metadata key (a render overlay carries it), `--book-scope`, the `scope` request fields and the `Option<BookScope>` plumbing described below are gone, and `Page` is `none`. This plan's text and Handoff log are history and are left as written.

## Execution

- **Lane:** A (`workspace-1`), binding: one agent per plan, in this order: Plan 3, Plan 5, Plan 10, Plan 13. This plan is number 2 of 4 in the lane.
- **Waits on:** Plan 3 merged into `feature/q2-preview-hub-parity` (`BookScope`, `resolve_book_scope`, the wasm shim). Nothing inside this plan waits on another plan.
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p03-book-scope-core' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p05-book-scope-clients`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 12 (embed client reuses the controller types and the menu).
- **Next in lane:** Plan 10 (`2026-10-08-q2-preview-p10-book-downloads-server`); the next agent starts only when its own start gate passes.

## Goal

The client side of explicit book scope: the wasm option and every TypeScript type use `'book' | 'chapter'` (absent means "not chosen"); the Download as menu sends the scope explicitly for the formats that consolidate a book; the resolved scope `'page'` is typed; and a click whose scope no longer holds when it lands is delivered standalone with the notice Rust returns, instead of being silently downgraded.

## Background facts

(Line numbers from the 2026-10-08 audit; re-check.)

- `wasm-quarto-hub-client/src/lib.rs:1602-1772`: `options.scope` accepts only `'auto' | 'chapter'`; `:2388-2460` (the wasm preview render) hard-codes `RenderMode::ActivePage`, takes no scope option at all, and is chapter-alone by construction. Plan 3 added a shim mapping `'auto'` to `None` and made the `ResolvedBookScope` match (`:1561-1564`) handle `Page`.
- hub-client: `downloadController.ts:91` (resolved scope type `'book' | 'chapter'`) and `:117-183` (`scope?: 'auto'|'chapter'`), with the capture selection at `:403-412` branching on `scope === 'auto'` (whole-book capture maps); `downloadService.ts:131-206` (`wasmDeps('chapter')` default; the PDF preview controller is "chapter-alone for good" at `:189`); `useDownloadAs.ts:30,53-55,66-86` (the click's scope is silently downgraded to `'chapter'` if the chapter list went stale; the info comes from `resolvePandocFormats(path)`; book info is gated off in the embed by `!isPreviewEmbed()` until Plan 13); `ts-packages/preview-runtime` `wasmRenderer.ts:669-682` and `src/wasm-quarto-hub-client.d.ts:108,122-130`.
- **The menu is hub-client's existing Download as menu** (`DownloadAsControl.tsx:140-156`, a `Menu` of `MenuItem`s). For a chapter of a book and a format in `BOOK_FORMATS` (typst, typst-pdf, epub) it already shows two entries per format, "Download book as \<label>" and "This chapter only" (`strings.ts:92-94`); every other format and non-chapter page shows one plain entry. That structure stays: no new widget.
- `book_resolution` (`pandoc_request/formats.rs:280-310`) feeds the menu: `chapter: bool` derived from membership.
- Hub test (2026-10-08, ran): a page outside `book.chapters` has `book.chapter = false`, so the menu shows only plain entries; its download is a chapter-alone document whatever the scope, with no notice. A plain entry clicked on a page that has since become a chapter sends no scope, `start()` re-resolves only for `'auto'`, Rust defaulted to auto, and the download was the whole book. Strand `bd-j6qvbtbj` tracks the degraded standalone output (book template, inherited `crossref.chapters` numbering, bare crossrefs); this plan fixes the undefined-scope staleness only.
- Files carrying `'auto'` or the scope type besides the sources above: `hub-client/src/pandoc/downloadBook.wasm.test.ts:96-135`, `pdfChain.wasm.test.ts:45`, `hub-client/src/services/pandocRequest.wasm.test.ts:60,670`, `pandocBook.wasm.test.ts:38,496,522` (`:496` asserts the scope parser's error text), `hub-client/src/components/DownloadAsControl.integration.test.tsx:49`, `DevHarness.tsx:852`, `hub-client/src/pandoc/downloadController.test.ts` (about 12 sites), and the Playwright specs `hub-client/e2e/download-as.harness.spec.ts` and `e2e/pandoc-download.spec.ts` (they exercise the book menu entries; `tsc` and `vitest` do not run them).

## Decisions

- **P5-D1 What a click sends.** "Download book as X" sends `'book'`; "This chapter only" sends `'chapter'`; the plain entry of a consolidating format (typst, typst-pdf, epub; shown on non-chapter pages) sends `'chapter'` (it means "this page", so a page that became a chapter meanwhile still downloads alone); entries for formats that do not consolidate (docx, odt, pptx, html) send no scope. The PDF preview always sends `'chapter'`.
- **P5-D2 Rust owns the notice.** After Plan 3, a `'book'` click on a page that is no longer a member returns a standalone document plus the notice (resolved scope `'page'`). The client no longer downgrades; `useDownloadAs.ts:71-79` is removed and the controller surfaces the notice from the render result, so it cannot appear twice.
- **P5-D3 Captures follow the resolved scope.** The whole-book capture map is selected only when the scope sent is `'book'`; absent or `'chapter'` selects the active chapter's captures (`downloadController.ts:403-412`).

## Tasks

Per task: `tsc` plus `vitest` in `hub-client` and `ts-packages/preview-runtime`; `cargo clippy -p wasm-quarto-hub-client --all-targets -- -D warnings` plus `cargo nextest run -p wasm-quarto-hub-client` for the Rust side. Workspace nextest at the end of the plan.

- [x] 5.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [x] 5.2 Wasm option: `options.scope` becomes `'book' | 'chapter'` and optional in `parse_request_options` (absent is `None`); the shim's `'auto'` is removed and the parser's error text updated; the wasm preview render is unchanged (it takes no scope option and is chapter-alone). Update the wasm tests and the option docs.
- [x] 5.3 TypeScript types follow: `downloadController.ts` (`scope?: 'book' | 'chapter'`, resolved scope `'book' | 'chapter' | 'page'`, P5-D3 at `:403-412`), `downloadService.ts` (the default and the comment at `:189`), `useDownloadAs.ts`, `ts-packages/preview-runtime` (`wasmRenderer.ts` and the `.d.ts` at `:108,122-130`, including `'page'`), `DevHarness.tsx:852`, and every straggler listed above; grep `'auto'` and the scope type across `hub-client/src`, `hub-client/e2e`, `ts-packages` and `q2-preview-spa` and list what was found in the checklist. *Found and changed:* `downloadController.ts`, `downloadService.ts`, `useDownloadAs.ts`, `DownloadAsControl.tsx`, `DevHarness.tsx`, `wasmRenderer.ts`, `wasm-quarto-hub-client.d.ts`, `downloadBook.wasm.test.ts`, `pdfChain.wasm.test.ts`, `pandocRequest.wasm.test.ts`, `pandocBook.wasm.test.ts`, `downloadController.test.ts`, `useDownloadAs.integration.test.tsx`, `DownloadAsControl.integration.test.tsx`. *Nothing found* in `hub-client/e2e`, `q2-preview-spa` or other `ts-packages` (the Playwright specs name no scope value; `downloadService.embed.integration.test.ts` and `downloadService.test.ts` carry no scope).
- [x] 5.4 `DownloadAsControl.tsx`: the existing paired entries stay; "Download book as X" calls `onSelect(f, 'book')` (was `'auto'`), "This chapter only" `onSelect(f, 'chapter')`, and the plain entry of a consolidating format `onSelect(f, 'chapter')` (P5-D1); the plain entries of other formats pass no scope.
- [x] 5.5 `useDownloadAs.ts`: remove the client-side stale-list downgrade (P5-D2); the controller shows the notice that `render_pandoc_request` returns in the result's diagnostics for resolved scope `'page'` ("this page is not part of the book, so only this page was downloaded"), for a stale click in either direction, never as an error.
- [x] 5.6 Vitest and Playwright: the menu shows the paired entries only for chapters of consolidating formats and sends the values in P5-D1; a stale `'book'` click yields the page plus the notice once; a plain typst click on a page that became a chapter does not download the whole book; docx sends no scope and gets no diagnostic; capture selection per P5-D3; wasm option parsing. Tests that change: `downloadService.test.ts`, `downloadController.test.ts`, `useDownloadAs.integration.test.tsx`, `DownloadAsControl.integration.test.tsx`, `downloadService.embed.integration.test.ts` (scope types only; embed behaviour is Plan 12), the four wasm tests above, and the two Playwright specs (run them locally where a browser is available; otherwise they are verified in CI).
- [x] 5.7 hub-client changelog entry.
- [x] 5.8 Plan end: workspace nextest against 5.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- The wasm option's values change; any other caller of the wasm renderer (the SPA's `wasmRenderer` use) must be found with the 5.3 grep before the shim is removed.

## Handoff log

**Headline:** the wasm `options.scope` is `'book' | 'chapter'` (absent means not chosen; `'auto'` is now rejected), every TS type follows, the Download as menu sends the scope per P5-D1, and a stale click is delivered by Rust with the `Q-5-41` notice. Landed as `4482cf93f` (code and tests) and `c44019369` (changelog), on top of integration tip `67a1471d2`; no rebase was needed. The plan-end commit follows them.

**Nextest.** Baseline (5.1, branched from `67a1471d2`, no ledger entry then): 16013 run, 16013 passed, 202 skipped. After rebasing onto Plan 2 (`3b5fb2fc7`) the workspace run is 16025 passed, 202 skipped (`--no-fail-fast`, 2 slow), which equals Plan 2's own ledger entry (16025/202): the +12 are Plan 2's tests, and this plan adds none to the workspace (its changes are TypeScript, wasm-crate Rust excluded from the workspace, and docs). Delta of this plan alone: 0.

Ledger: f2910fcc6: 16025 passed, 202 skipped

Before the rebase (tip `67a1471d2`) also green:  `tsc` and `vitest` in `hub-client` (708 tests in `src/pandoc`, `src/components/DownloadAs*`, `src/services`) and `ts-packages/preview-runtime` (79), `npm run test:wasm` after `npm run build:wasm` (389 passed, 28 skipped), and `cargo check --lib --target wasm32-unknown-unknown` for the wasm crate. The two Playwright specs (`download-as.harness.spec.ts`, `pandoc-download.spec.ts`) name no scope value and were not run locally; CI covers them.

**Interfaces as they ended up.**

- Wasm `render_pandoc_request(..., options)`: `options.scope?: 'book' | 'chapter'`; any other string errors with "options.scope must be 'book' or 'chapter'". `stats.book.scope` is `'book' | 'chapter' | 'page'`.
- TS: `StartOptions.scope?: 'book' | 'chapter'`, `BuildRequestExtra.scope` the same, `RequestEnvelope.stats.book.scope` includes `'page'`, `DownloadAsControlProps.onSelect(format, scope?: 'book' | 'chapter')`, `DownloadAs.start(format, scope?)`, `renderPandocRequest` options in `ts-packages/preview-runtime`.
- `wasmDeps(scope: 'book' | 'chapter' | undefined, runners)`: the app-wide download controller now has **no default scope** (`wasmDeps(undefined)`); the PDF preview controller still defaults to `'chapter'`. A caller of `controller.start` for typst, typst-pdf or epub that passes no scope therefore gets the whole book on a chapter (Rust's "not chosen"); the menu always passes one.
- Menu (P5-D1): "Download book as X" sends `'book'`; "This chapter only" and the plain entry of typst, typst-pdf and epub send `'chapter'`; docx, odt, pptx, html send nothing.
- Capture selection (P5-D3): whole-book capture maps only for `scope === 'book'`; any other scope fetches the active page's capture. For a stale `'book'` click on a non-chapter, the page is rendered without its own capture (the controller skips `captureDocId` for `'book'`).

**Deviations.**

1. The plan says the controller "surfaces the notice"; no new code was needed. Request diagnostics already flow into the done status's `warnings`, so the `Q-5-41` info diagnostic appears once there. A test (`downloadController.test.ts`) pins this and that a `'page'` result is named after the page, not the book.
2. The default scope of the app-wide controller changed from `'chapter'` to none (see above), so a docx click sends no scope and gets no `Q-5-42`.
3. `crates/wasm-quarto-hub-client/Cargo.lock` picks up a `libc` line whenever the crate is built; it is not part of this plan and was left out of the commits.
4. **Plan 2 broke the wasm32 build, found after the rebase.** `cargo check --lib --target wasm32-unknown-unknown` in `crates/wasm-quarto-hub-client` now fails: `quarto-core/src/transforms/resource_collector.rs:90` calls `crate::render_to_file::output_in_place`, and `render_to_file` is `#[cfg(not(target_arch = "wasm32"))]` (`lib.rs:75-76`). This plan's own wasm tests and `npm run test:wasm` passed before the rebase; they cannot be rerun on the new tip until that is fixed (move `output_in_place` out of the native-only module, or gate the call). Not fixed here: it is Plan 2's code.

**What the next plans must know.** Plan 12 (embed client) reuses `DownloadAsControl` and the controller types as above; the embed's `bookInfoFor` gate (`!isPreviewEmbed()`) is untouched (Plan 13). The native routes\' `scope` field (Plans 9, 10) should take the same `'book' | 'chapter'` values. The known link difference noted in Plan 3's log (`#link("../one.pdf")` natively vs `../one.qmd` in the request path) is still undecided.
