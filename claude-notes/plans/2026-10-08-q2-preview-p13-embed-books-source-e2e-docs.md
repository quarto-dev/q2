# Plan 13: book scope in the embed, end-to-end checks, and close-out docs

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E3, E6, E11, E12); the execution model and landing procedure are there.

> **Superseded in part by [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md):** book scope is now the `book-scope` metadata key (a render overlay carries it), `--book-scope`, the `scope` request fields and the `Option<BookScope>` plumbing described below are gone, and `Page` is `none`. This plan's text and Handoff log are history and are left as written.

## Execution

- **Lane:** A (`workspace-1`), binding: one agent per plan, in this order: Plan 3, Plan 5, Plan 10, Plan 13. This plan is number 4 of 4 in the lane.
- **Waits on:** Plans 7 (import), 8 (static viewer), 10 (book downloads, server), 11 (SPA PDF pane) and 12 (embed client and harness) merged into `feature/q2-preview-hub-parity`; by transitivity that includes Plans 1-6 and 9. Nothing inside this plan waits on another plan. (8 and 11 are documented in 13.7 and the epic checklist is ticked in 13.8, so they must have landed.)
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p07-embed-import' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p08-static-pdf-viewer' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p10-book-downloads-server' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p11-spa-pdf-pane' && git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p12-embed-client' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p13-embed-close-out`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** nothing; this plan closes the epic's checklist.
- **Next in lane:** none; this is the last plan in the lane.

## Goal

Hub-client offers, and the embed does not yet: **book scope** ("Download book as X" and "This chapter only", the menu Plan 5 made explicit) for typst, pdf and epub. Then verify everything end to end in a real `q2 preview --ui editor` session (book downloads, the `.typ` download Plan 12 added, import and `--join`), and write the user documentation for `q2 preview`.

## Background facts

- `useDownloadAs.ts:53-55` gates book info off in the embed (`!isPreviewEmbed()`); the info comes from `resolvePandocFormats(path)`; Plan 5 changed the scope type to `'book' | 'chapter'` and made the existing paired menu entries send it explicitly (a plain entry of a consolidating format sends `'chapter'`, so the embed never downloads a whole book by accident before this plan). `runNative` must set `book` from `stats.book`. `renderNatively` turns a 409 into `{kind:'error', message:'{"busy":true}'}` (`nativeRender.ts:63-64`); Plan 12 added the distinct busy variant. The embed controller never overlaps downloads of one path: a second click or Cancel aborts the fetch client-side (`downloadController.ts:~296`, `downloadService.ts:196`) while the blocking render keeps running (server-side busy guard: Plan 10).
- Import writes through Automerge; any non-GET/HEAD request tunnels through `--join` (`join_frontend.rs:333-365`), so no frontend change is expected; the tunnel is a raw `copy_bidirectional` with no body limit (`quarto-p2p/src/host.rs:206`). The idle-timeout worry is closed by reading: iroh 1.2.0 sets `keep_alive_interval(5s)` by default (`endpoint/quic.rs:156`) and `quarto-p2p` sets none.
- Share trust: any join-string holder can already POST native renders (`quarto-p2p/src/host.rs:191-208`); a `--join` guest's upload runs pandoc on the host.

## Tasks

Per task: `tsc` plus `vitest` in `hub-client`. Workspace nextest at the end of the plan. Playwright specs skip with a clear reason without `typst` or `pandoc`, or when the embed is the placeholder.

### Part A: client

- [x] 13.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [x] 13.2 Remove the `!isPreviewEmbed()` gate on `bookInfoFor`; `runNative` (which already forwards `scope`, Plan 12.3) sets `book` from `stats.book` and shows the busy outcome as a visible notice separate from `superseded`; the menu's paired entries then appear in the embed through the same code as hub-client.
- [x] 13.3 Vitest: menu entries and requests in embed mode; file names from `Content-Disposition`; busy handling; `book` status from stats; a non-member page downloads standalone with the notice from `X-Q2-Diagnostics`.
- [x] 13.4 hub-client changelog.

### Part B: end to end

- [x] 13.5 Playwright (extends Plan 12's `embed` project): download book as PDF and chapter-only on a small book fixture; a non-member page downloads standalone with the notice; the `.typ` download on a book chapter is the chapter alone.
- [x] 13.6 Import end to end (Plan 7): import a docx with an image and a comment into a `--ui editor` session; the qmd and `<stem>_media/` appear; a `--join` guest sees them. Confirm over a real tunnel with a near-25 MB upload and a slow import (the transport's 5 s keep-alive should keep it open).

### Part C: documentation and close-out

- [x] 13.7 User docs for `q2 preview`: runtime requirements (typst, pandoc, network for non-vendored `@preview` packages); a writable project for PDF preview; what `--static` does for PDFs (Plan 8); the viewer SPA's PDF pane (Plan 11); the embed's downloads, import and books; the unsaved-chapter-edits note (only the active chapter's text is sent; other chapters come from disk without `--allow-edit`); pre/post-render scripts not run for book downloads; a book's own `output-ext` ignored; time expectations; the output-identity note (E4); share trust (a `--join` guest's request runs native renders and pandoc on the host); import and the `--allow-edit` interaction (check the VFS-created-file write path `72ef5c84f`).
- [x] 13.8 Reconcile the epic: tick its checklist against what landed, update the gap table, and record the final workspace nextest delta against 13.1 and against the integration branch's first baseline.
- [x] 13.9 Plan end: workspace nextest; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- Book renders can be slow; cancellation is limited to chapter boundaries and the child timeouts.
- A reviewer comparing hub and embed output side by side will see the E4 differences.

## Handoff log

**Headline.** The embed offers "Download book as" and "This chapter only" (typst source, PDF, EPUB); the embed's books, import (including through `--join`) and typst downloads are covered end to end; `q2 preview` has user docs; the epic is reconciled. Branch `q2-preview/p13-embed-close-out`, six commits on `368636cb1` (the integration tip when Plan 12's agent reported Plans 1-12 landed): `08b17fdc8` (13.2-13.3), `23bbfd486` (13.4), `61938234a` (resolver fix, below), `ddf47ff33` (13.5), `2e4110385` (13.6), `ae2887867` (13.7), plus this checklist/epic commit.

**What changed in the client (13.2-13.3).** `useDownloadAs` no longer gates `bookInfoFor` off in the embed. `nativeRender.ts`: the `ok` outcome carries `book?: { chapters }`, set only when `X-Q2-Stats.book.scope === 'book'` (a chapter or a page is one document, as in the wasm path). `DownloadController.runNative` puts it on the `done` status, so "Book downloaded (N chapters)" shows. The busy outcome was already a visible `native-error` (Plan 12); a test now pins its message. The non-member notice (`Q-5-41`) arrives in `X-Q2-Diagnostics` and shows among the warnings.

**Deviation: a Rust fix found by the e2e (not in the plan).** `quarto_core::pandoc_request::formats::resolve_document_formats` compared the active page against the project's absolute file list, but the hub client (embed and browser alike) asks with the project-relative path it shows in its file tree, so `book.chapter` was always `false` and the book entries never appeared. It now canonicalizes the path through the runtime first, as `render_pandoc_request` already did. Pinned by a new case in `hub-client/src/pandoc/downloadBook.wasm.test.ts` (relative and absolute agree). The existing wasm tests all passed absolute paths, which is why this was never caught. This also affects the browser hub-client menu; I did not check whether it was visible there before.

**Tests.** hub-client: typecheck clean, `npm test` 1635, `npm run test:integration` 223, `npm run test:wasm` 390 passed / 28 skipped (was 389). `q2-preview-spa` Playwright `--project=embed`: 14 passed (Plan 12's 4, plus `embed-books.spec.ts` 3 and `embed-import.spec.ts` 7, described below). `cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings` clean; `nextest -p quarto-core` 5852 passed / 32 skipped; `-p quarto` 632 / 2.

- `embed-books.spec.ts`: the menu pairs on a chapter; the book `.typ` holds every chapter and is named `Small-Book.typ`; the chapter-only `.typ` is `two.typ` with only that chapter; the book PDF and chapter PDF are real PDFs (needs network for `@preview/orange-book`, skips when `packages.typst.org` is unreachable); a non-member page downloads standalone, and a stale `scope: "book"` request for it returns that page with `Q-5-41` and a non-book stat.
- `embed-import.spec.ts`: docx with an image (qmd opens, `<stem>_media/` holds the PNG); docx with comments and tracked changes (`[++`, `[--`, `[>>`); `--allow-edit` (without it nothing reaches disk; with it the qmd and its media are written); a guest sees what the host imported; a guest imports a 21 MB docx through the real `--join` tunnel (relay) and the host gets the qmd and all three images; a slow import (a wrapper `pandoc` sleeping 12 s) completes through the tunnel, which settles the keep-alive question empirically, not only by reading iroh.
- The harness gained `share`, `joinTicket()` and `startJoinGuest(ticket)` in `q2-preview-spa/e2e/helpers/previewServer.ts`.

**Things learned.**
- hub-client stores an imported image only up to 10 MB (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`; larger ones are skipped), in the browser too. The near-25 MB test therefore uses three 7 MB images, not one 22 MB image (the route itself returns the 22 MB media: verified with `curl`).
- The viewer SPA's e2e and the pdf.js tree: a stale gitignored `q2-preview-spa/dist` (no `pdfjs/`) makes the embed's PDF pane fail with "The PDF viewer did not start". `npm run build` in `q2-preview-spa` (its `prebuild` fetches pdf.js), then `cargo build --bin q2`, fixes it. Same cause as the `pdfjs_serving` failures Plans 6 and 10 recorded.
- `--static`\'s `--help` text still said "Only html and revealjs"; it now names typst (docs table fixed too).
- Not reproduced by me: Plan 10's note that a whole-book download fails when a chapter `embed`s another chapter's executed cell (typst `Q-21-3`). It is documented as a limitation in the user docs (13.7); no plan covers splicing a recorded capture into an embed.
- The `--join` e2e cases need network to iroh's relay (they do not skip without it); CI has network.

**Workspace nextest (13.1, 13.9).** Baseline: ledger entry `ccd32719c: 16134 passed, 202 skipped` (the tip `368636cb1` differs from it only by plan notes, so no baseline run). Final run on this tree (`--no-fail-fast`, Gordon approved it): 16134 tests run, 16134 passed (2 slow, 2 leaky), 202 skipped, 562 s. Delta 0 passed, 0 skipped, expected: this plan adds no Rust test (its coverage is TypeScript, wasm and Playwright). Against the integration branch's first baseline (`135a42b69: 15990 / 202`) the epic added 144 passed, 0 skipped.

Ledger: ae2887867: 16134 passed, 202 skipped
