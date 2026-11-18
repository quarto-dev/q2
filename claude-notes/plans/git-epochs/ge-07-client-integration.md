---
title: 'GE-7: Browser clients: sync client swap, consumers, indicator, MCP'
date: 2026-11-18
description: 'Wires the swap module into quarto-sync-client and every consumer that holds a file handle or writes by path, adds the replaced-file notice and the paused-for-git banner to the web surfaces, and covers the branch service and the MCP edit tool.'
status: approved
depends-on: [ge-6]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-6](ge-06-swap-module.md) ·
**Unblocks:** [GE-13](ge-13-hub-e2e.md) (with GE-4) · **Branch:** `ge/07-client-integration` off `feature/git-epochs`,
merged by rebase and fast-forward; nothing is pushed.

## Goal

When the project owner flips `files[path]` to a different doc, a browser client must freeze that file, load the new doc,
swap, say so, and keep every edit that was made against the old doc in the old doc instead of corrupting the new one.
Today the client ignores the flip. This plan makes `quarto-sync-client` do the swap through the GE-6 module, then audits
every consumer that holds a handle or writes by path, and adds the two web-surface signals: the replaced-file notice and
the "paused for git" banner. Nothing here needs the git logic on the hub; the hub side is GE-3 and GE-4, and the real
end to end is GE-13.

## Decisions implemented

See the [epic decisions](../2026-11-18-git-epochs-epic.md#decisions).

- **G26** the shared swap module in the browser clients: phases, freeze, docId-bound writes, retired handles, one
  aggregate notice per batch, shown on editing surfaces only; the default read-only viewer shows none (its
  `onFileAdded` re-render is enough).
- **G25** the moved message: a peer that leaves a doc because of a switch says so on the old doc, and the others show
  \"this person switched versions\" briefly.
- **G33** the paused indicator: a banner only, polled from `GET /api/preview/git-status`; no read-only mode.
- **G34** the branch service: additive; reset the active branch to main on a swap; branch edits are docId-bound; no
  orphan UI. Attribution: a switched doc is lump-attributed and the reactji feature does not see older contributions.
- **G15, G32** the wording says \"project owner\"; the 5 s timeout and 8 retired handles per path.

## Context

What exists, with locations as read on 2026-11-18:

- **Sync client.** `syncWithFiles` compares path sets and ignores a changed docId for an existing path; its loop awaits
  `findDoc` inline for each new path (`ts-packages/quarto-sync-client/src/client.ts:1393-1440`). `subscribeToFile`
  sets the handle and re-fires `onFileAdded` (`:1195-1252`); `detachFileChangeHandler` is at `:1186`;
  `markFileUnavailable` at `:1297` and `startUnavailableRetry` at `:1368`; `findDoc` with its retry at `:829`;
  `recoverFileDocument` (`:1086`) is the precedent for swapping a handle in place. Writes by path:
  `updateFileContent` (`:1881-1895`) and `applyEditorOperations` (`:1907-1923`), both resolving
  `state.fileHandles.get(path)`. `getFileHandle` is exported (`:2261`, listed at `:2679`). Types:
  `TextFilePayload` and `BinaryFilePayload` (`types.ts:41-58`), `SyncClientCallbacks` (`types.ts:87`).
- **Wrapper.** `ts-packages/preview-runtime/src/automergeSync.ts` re-exports the client: `setSyncHandlers` (`:72`),
  `updateFileContent` (`:221`), `applyEditorOperations` (`:231`), `getFileHandle` (`:376`). The test double is
  `src/test-utils/mockSyncClient.ts` (`:51-52`, `:172-181`).
- **Preview SPA.** The block editor writes through `getFileContent` and `applyEditorOperations`
  (`q2-preview-spa/src/PreviewApp.tsx:660-675`); `setSyncHandlers` is at `:800`; an existing status region is at
  `:1409` (`role="status"`).
- **hub-client.** `App.tsx` sets handlers at `:837` and writes through `handleContentOperations` (`:923-925`);
  `Editor.tsx` already freezes the editor for replay with `updateOptions({ readOnly, domReadOnly })` (`:662-663`);
  `hooks/useReplayMode.ts:150-160` takes `getFileHandle(filePath)` into a replay session that **holds the handle**;
  `hooks/usePresence.ts:225-245` resolves remote cursors against `getFileHandle(path).doc()`;
  `services/presenceService.ts` keeps `state.currentHandle` and its `setCurrentFile` **returns early when the path is
  unchanged** (`:214-217`), so after a swap of the same path it would keep listening and broadcasting on the old
  handle; it already has a `leave` message (`:49-53`, `broadcastLeave`); `services/attribution-store.ts` extends cached
  incremental state against whatever handle it gets (`:44`, `:156`); also `pandoc/downloadAttribution.ts:30`,
  `services/debugAutomerge.ts:217-220`, `components/SyncStatusBadge.tsx:98`, `components/ConnectionStatusDialog.tsx:226`
  (each resolves the handle at call time).
- **Branch service.** `services/branchService.ts`: `applyBranchEdits` returns silently when it cannot resolve the doc or
  the branch doc (`:311-326`); `setActiveBranch` (`:235`); tests in `branchService.test.ts`, `BranchBar.test.tsx`, and
  `useDocBranches.test.tsx` (34 passing, behind the default-off `documentBranches` preference).
- **MCP.** The edit tool writes with `state.client.updateFileContent` after an `expected_hash` check
  (`ts-packages/quarto-hub-mcp/src/tools.ts:2250-2296`); tests use `test-hub.ts` and `in-memory-fixture.ts`.

## Test specifications

Written first and seen failing. Tests follow `.claude/rules/hub-client-tests.md`: nothing outlives a test, every wait is
on the state asserted, and before a second client reads what the first wrote, wait for the writer\'s heads on the hub
(`hub.hubHasHeadsOf`).

1. **Sync client with two real clients through the test hub** (`quarto-sync-client/src/swap.test.ts`): the owner
   creates doc B and flips `files[path]`; the guest sees `pending` then `done`, `onFileAdded` re-fires with the new text
   and `docId`, and `getFileHandle(path).documentId` is B. Two quick flips reach only the final doc. A flip to a doc
   the hub does not have fires `unavailable` after 5 s and marks the path unavailable. A write with the old `baseDocId`
   during `pending` lands in doc A, not B. A flip back reuses the retired handle.
2. **Write routing:** `updateFileContent` and `applyEditorOperations` with a stale `baseDocId` go to the retired doc; an
   unknown `baseDocId` is dropped with a console warning; no `baseDocId` behaves as today (the existing tests pass
   unchanged).
3. **Consumers:** presence rebinds and clears remote cursors on `done`; the replay session on the path closes; the
   attribution store resets its incremental state; the other handle resolvers return the new handle after `done`.
4. **Notice:** one aggregate per batch from `NoticeAggregator`; rendered in a region with `role="status"` and
   `aria-live`; absent in the read-only viewer; present on the block editor under `--allow-edit` and in the hub-client
   editor; the stranded line appears only when the counter is nonzero.
5. **Branch service with the flag on:** a swap resets the active branch to main; `applyBranchEdits` with a stale
   `baseDocId` reaches the branch doc of that docId or warns, never silently returns; the 34 existing tests stay green.
6. **Paused banner** against a stubbed `/api/preview/git-status` (the hub route lands in GE-4): shows while `paused`,
   hides when not, hides when the route answers 404 or `tracking` is false.
7. **MCP:** after a swap, the `edit` tool with the pre-swap `expected_hash` is refused and returns the current content
   and hash.

## Checklist

### Sync client

- [ ] Confirm GE-6 is complete; add `@quarto/quarto-git-epochs` as a dependency of `quarto-sync-client` and check the
  build order of the workspaces (detail chosen by the plan author; confirm).
- [ ] Write tests 1 and 2; see them fail.
- [ ] `types.ts`: add an optional `docId?: string` (the bare doc id) to `TextFilePayload` and `BinaryFilePayload`, an
  optional `onFileReplaced?: (event: ReplacedEvent) => void` to `SyncClientCallbacks`, and an optional `baseDocId`
  parameter to the `SyncClient` signatures of `updateFileContent` and `applyEditorOperations`.
- [ ] Set `docId` at every `onFileAdded` site: `subscribeToFile` (`client.ts:1205-1223`), file creation (`:1944`,
  `:1987`, `:2049`), rename (`:2240-2242`), and `createNewProject` (`:2553`, `:2567`).
- [ ] Create the coordinator in `createSyncClient` with a `SwapHost`: `currentDocId` from `state.fileHandles`;
  `loadDoc` from `findDoc`; `rebind` detaches the old change handler, replaces `state.fileHandles`, drops `astCache` and
  `binaryFiles` entries, and calls `subscribeToFile` so `onFileAdded` re-fires; `onReplaced` calls `onFileReplaced`;
  on `unavailable` call `markFileUnavailable` and `startUnavailableRetry`. `disconnect` calls `dispose`.
- [ ] In `syncWithFiles`, for an existing path whose index docId differs from the bound handle, call `indexChanged`
  and do not await it; new paths keep the inline path; the repair of an unavailable entry stays separate.
- [ ] Route `updateFileContent` and `applyEditorOperations` through `resolveWriteTarget(path, baseDocId)`.
- [ ] Tests 1 and 2 pass; the whole sync-client suite passes.

### Preview runtime and the preview SPA

- [ ] `preview-runtime/src/automergeSync.ts`: pass `docId`, `baseDocId`, and `onFileReplaced` through; expose a notice
  subscription built on `NoticeAggregator` (detail chosen by the plan author; confirm). Update `mockSyncClient.ts` to
  the new signatures.
- [ ] `PreviewApp.tsx` block editor under `--allow-edit`: capture the docId at the time the diff base is read
  (`:660-675`) and pass it as `baseDocId`; freeze the editor on `pending` (skip the write path and show the editor as
  read-only); render the aggregate notice in a `role="status"` region. The default viewer renders no notice.

### hub-client

- [ ] `App.tsx` and `Editor.tsx`: pass `baseDocId` from `handleContentOperations`; on `pending` set the editor read-only
  with the mechanism replay already uses (`Editor.tsx:662-663`); on `done` restore it; show the aggregate notice
  (editing surfaces only) with `role="status"` and `aria-live="polite"`.
- [ ] `hooks/useReplayMode.ts`: close any replay session on the path when a swap completes (the session holds its
  handle).
- [ ] `services/presenceService.ts`: add an explicit rebind entry point that broadcasts the moved message on the old
  handle, stops listening, clears remote presences for the path, and listens on the new handle; call it from
  `done`. Extend the `leave` message with a reason so a receiver can show \"this person switched versions\" for a
  short time. `hooks/usePresence.ts` re-resolves the doc after the rebind.
- [ ] `services/attribution-store.ts`: reset the cached incremental state (and any run list) when the path\'s docId
  changes, because histories are unrelated.
- [ ] `pandoc/downloadAttribution.ts`, `services/debugAutomerge.ts`, `components/SyncStatusBadge.tsx`,
  `components/ConnectionStatusDialog.tsx`: confirm each resolves the handle at call time; add a test that each returns
  the new handle after `done`; fix any that caches.
- [ ] Tests 3 and 4 pass.

### Branch service

- [ ] Flag-on tests (test 5) first. On `done`, `setActiveBranch(path, null)`; `applyBranchEdits` gains
  `baseDocId` and uses the branch doc of that docId, warning (not returning silently) when none exists; if the path had
  branches, add one line to the notice: \"N branches of this file stay with the previous version\" (from D16; detail
  chosen by the plan author; confirm). No orphan UI.

### Paused indicator

- [ ] Test 6 first. Poll `GET /api/preview/git-status` every 2 s only while the response says `tracking` is true; stop
  polling on a 404 (detail chosen by the plan author; confirm). Show a banner, \"Paused for git: edits to files the
  project owner rewrites may be set aside\", in `role="status"`. The response shape is the one in GE-1\'s contract.
  The surfaces: the preview SPA and the hub-client when served by a preview. No read-only mode.

### MCP and rebuilds

- [ ] Test 7 first: after a swap the MCP `edit` tool with the stale `expected_hash` is refused. Run it before the bundle
  step.
- [ ] `cargo xtask build-hub-mcp-bundle`; check `q2 mcp --launcher-info` after `cargo build --bin q2`.
- [ ] Preview SPA chain: `cargo xtask build-q2-preview-spa`, then `cargo build --bin q2` (run
  `cd hub-client && npm run build:wasm` only if the WASM artifact is missing or stale; no Rust changes here).

### Changelog and wrap-up

- [ ] Do this last, after the final rebase onto `feature/git-epochs`, because a rebase rewrites the hash: commit the
  hub-client changes, then a second commit adding the entry to `hub-client/changelog.md` under today\'s date, with the
  first commit\'s hash (the hub-client two-commit workflow in `AGENTS.md`). One sentence, user-facing: a replaced file
  shows a notice, and a paused banner appears while the owner is mid-operation.
- [ ] Update this plan\'s checkboxes and fill in the notes below.

## Gates

```bash
npm test -w @quarto/quarto-sync-client
npm test -w @quarto/preview-runtime
npm test -w @quarto/hub-mcp
npm test -w q2-preview-spa
npm run test:integration -w q2-preview-spa
npm run typecheck
cd hub-client && npm run test:ci && npm run build:all
cargo xtask build-hub-mcp-bundle
cargo xtask lint
```

`npm run build:all` in `hub-client` is required: the production build is stricter than `tsc --noEmit` and vitest.

## End-to-end verification

Passing tests is not enough for a user-visible change. After the SPA and bundle rebuilds, run the real binary:
`q2 preview --ui editor --allow-edit <fixture project>`. Drive a flip with a scratch script that acts as the owner: it
connects a sync client to the preview\'s `/ws`, creates a second doc with different text, and writes
`files["index.qmd"]` to point at it (the script is not committed; GE-13 adds the real harness). Open the page with
Playwright, and record in the notes: the exact invocations, the notice text observed, that the editor was read-only
during `pending`, and that an edit typed just before the flip stayed in the old doc. Repeat on the default viewer and
record that it re-rendered with no notice. Say that the output was inspected. If any of this cannot be run, say so.

## Exit criteria

- Tests 1 to 7 pass; the whole suites in the gates are green; `build:all` succeeds.
- The consumers audit is complete: every handle holder is either rebound, reset, or shown to re-resolve.
- The two end-to-end runs are recorded in the notes.
- The MCP bundle and the preview SPA are rebuilt, and the changelog entry is committed.

## Out of scope

- The hub side of the flip and the git logic (GE-3, GE-4); the real owner and the git operations (GE-13).
- The extension\'s own swap (GE-9).
- A read-only mode while paused, and any orphaned-branch UI.
- A viewer for retired versions.

## Notes from the run

Not started.
