# Plan 13: book scope in the embed, end-to-end checks, and close-out docs

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E3, E6, E11, E12); the execution model and landing procedure are there.

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

- [ ] 13.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [ ] 13.2 Remove the `!isPreviewEmbed()` gate on `bookInfoFor`; `runNative` (which already forwards `scope`, Plan 12.3) sets `book` from `stats.book` and shows the busy outcome as a visible notice separate from `superseded`; the menu's paired entries then appear in the embed through the same code as hub-client.
- [ ] 13.3 Vitest: menu entries and requests in embed mode; file names from `Content-Disposition`; busy handling; `book` status from stats; a non-member page downloads standalone with the notice from `X-Q2-Diagnostics`.
- [ ] 13.4 hub-client changelog.

### Part B: end to end

- [ ] 13.5 Playwright (extends Plan 12's `embed` project): download book as PDF and chapter-only on a small book fixture; a non-member page downloads standalone with the notice; the `.typ` download on a book chapter is the chapter alone.
- [ ] 13.6 Import end to end (Plan 7): import a docx with an image and a comment into a `--ui editor` session; the qmd and `<stem>_media/` appear; a `--join` guest sees them. Confirm over a real tunnel with a near-25 MB upload and a slow import (the transport's 5 s keep-alive should keep it open).

### Part C: documentation and close-out

- [ ] 13.7 User docs for `q2 preview`: runtime requirements (typst, pandoc, network for non-vendored `@preview` packages); a writable project for PDF preview; what `--static` does for PDFs (Plan 8); the viewer SPA's PDF pane (Plan 11); the embed's downloads, import and books; the unsaved-chapter-edits note (only the active chapter's text is sent; other chapters come from disk without `--allow-edit`); pre/post-render scripts not run for book downloads; a book's own `output-ext` ignored; time expectations; the output-identity note (E4); share trust (a `--join` guest's request runs native renders and pandoc on the host); import and the `--allow-edit` interaction (check the VFS-created-file write path `72ef5c84f`).
- [ ] 13.8 Reconcile the epic: tick its checklist against what landed, update the gap table, and record the final workspace nextest delta against 13.1 and against the integration branch's first baseline.
- [ ] 13.9 Plan end: workspace nextest; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- Book renders can be slow; cancellation is limited to chapter boundaries and the child timeouts.
- A reviewer comparing hub and embed output side by side will see the E4 differences.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
