---
title: 'GE-7: Browser clients: sync client swap, consumers, indicator, MCP'
date: 2026-11-18
description: 'Wires the swap module into quarto-sync-client and every consumer that holds a file handle or writes by path, adds the replaced-file notice and the paused-for-git banner to the web surfaces, and covers the branch service and the MCP edit tool.'
status: done
depends-on: [ge-6]
lane: C
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
8. **Offline, tabs, and late joiners** (detail chosen by the epic reviewer; confirm): a client offline during two
   flips reconnects and lands on the final doc only, with its retired handle loaded from local storage and an edit it
   made offline against the old doc routed to that retired doc, never to the new one. Two hub-client tabs on the same
   project each swap independently, and each clears only its own remote cursors. A client that joins during a pause
   or after a swap loads the current doc and shows no replacement notice (it never saw the old one); this is accepted
   and stated in the docs.

## Checklist

### Sync client

- [x] Write test spec 8 (offline reconnect, two tabs, late joiner) first; where today\'s behavior already satisfies a
  clause, say so in the notes; make the rest pass with the swap module.
- [x] Confirm GE-6 is complete; add `@quarto/quarto-git-epochs` as a dependency of `quarto-sync-client` and check the
  build order of the workspaces (detail chosen by the plan author; confirm). Import only the package\'s browser-safe
  `.` entry, never `./probe`.
- [x] Write tests 1 and 2; see them fail.
- [x] `types.ts`: add an optional `docId?: string` (the bare doc id) to `TextFilePayload` and `BinaryFilePayload`, an
  optional `onFileReplaced?: (event: ReplacedEvent) => void` to `SyncClientCallbacks`, and an optional `baseDocId`
  parameter to the `SyncClient` signatures of `updateFileContent` and `applyEditorOperations`.
- [x] Set `docId` at every `onFileAdded` site: `subscribeToFile` (`client.ts:1205-1223`), file creation (`:1944`,
  `:1987`, `:2049`), rename (`:2240-2242`), and `createNewProject` (`:2553`, `:2567`).
- [x] Create the coordinator in `createSyncClient` with a `SwapHost`: `currentDocId` from `state.fileHandles`;
  `loadDoc` from `findDoc`; `rebind` detaches the old change handler, replaces `state.fileHandles`, drops `astCache` and
  `binaryFiles` entries, and calls `subscribeToFile` so `onFileAdded` re-fires; `onReplaced` calls `onFileReplaced`;
  on `unavailable` call `markFileUnavailable` and `startUnavailableRetry`. `disconnect` calls `dispose`.
- [x] In `syncWithFiles`, for an existing path whose index docId differs from the bound handle, call `indexChanged`
  and do not await it; new paths keep the inline path; the repair of an unavailable entry stays separate.
- [x] Route `updateFileContent` and `applyEditorOperations` through `resolveWriteTarget(path, baseDocId)`.
- [x] Implement the host\'s `unconfirmedLocalEdits(path, oldHandle)` from GE-6 on the sync client: the number of this
  peer\'s changes to the old doc whose heads the server has not yet confirmed (from the sync info and `remote-heads`
  state the client already tracks for `drainOutbound`, `client.ts:~1665`), so that an edit typed while offline is
  counted as stranded when the flip arrives (GE-13 S3). Add a test: go offline, edit, flip, reconnect; the `done`
  event carries `stranded` of at least 1.
- [x] Tests 1 and 2 pass; the whole sync-client suite passes.

### Preview runtime and the preview SPA

- [x] `preview-runtime/src/automergeSync.ts`: pass `docId`, `baseDocId`, and `onFileReplaced` through; expose a notice
  subscription built on `NoticeAggregator` (detail chosen by the plan author; confirm). Update `mockSyncClient.ts` to
  the new signatures.
- [x] `PreviewApp.tsx` block editor under `--allow-edit`: capture the docId at the time the diff base is read
  (`:660-675`) and pass it as `baseDocId`; freeze the editor on `pending` (skip the write path and show the editor as
  read-only); render the aggregate notice in a `role="status"` region. The default viewer renders no notice.

### hub-client

- [x] `App.tsx` and `Editor.tsx`: pass `baseDocId` from `handleContentOperations`; on `pending` set the editor read-only
  with the mechanism replay already uses (`Editor.tsx:662-663`); on `done` restore it; show the aggregate notice
  (editing surfaces only) with `role="status"` and `aria-live="polite"`.
- [x] `hooks/useReplayMode.ts`: close any replay session on the path when a swap completes (the session holds its
  handle).
- [x] `services/presenceService.ts`: add an explicit rebind entry point that broadcasts the moved message on the old
  handle, stops listening, clears remote presences for the path, and listens on the new handle; call it from
  `done`. Receivers handle the shared `MovedMessage` from GE-6 (`src/moved.ts`, the same type the extension sends; do
  not add a reason to `leave`) and show \"this person switched versions\" for a short time. `hooks/usePresence.ts` re-resolves the doc after the rebind.
- [x] `services/attribution-store.ts`: reset the cached incremental state (and any run list) when the path\'s docId
  changes, because histories are unrelated.
- [x] `pandoc/downloadAttribution.ts`, `services/debugAutomerge.ts`, `components/SyncStatusBadge.tsx`,
  `components/ConnectionStatusDialog.tsx`: confirm each resolves the handle at call time; add a test that each returns
  the new handle after `done`; fix any that caches.
- [x] Tests 3 and 4 pass.

### Branch service

- [x] Flag-on tests (test 5) first. On `done`, `setActiveBranch(path, null)`; `applyBranchEdits` gains
  `baseDocId` and uses the branch doc of that docId, warning (not returning silently) when none exists; if the path had
  branches, add one line to the notice, the `BRANCHES_STAY` constant from GE-6 (from D16). No orphan UI.

### Paused indicator

- [x] Test 6 first. Poll `GET /api/preview/git-status` every 2 s only while the response says `tracking` is true; stop
  polling on a 404 (detail chosen by the plan author; confirm). Show a banner, the `PAUSED_BANNER` constant from GE-6,
  in `role="status"`. The response shape is the one in GE-1\'s contract.
  The surfaces: the preview SPA and the hub-client when served by a preview. No read-only mode. Style the banner and
  the notice with solid colors (`.claude/rules/hub-client-theme.md`) and run `npm run lint:css -w hub-client`.

### MCP and rebuilds

- [x] Test 7 first: after a swap the MCP `edit` tool with the stale `expected_hash` is refused. Run it before the bundle
  step.
- [x] `cargo xtask build-hub-mcp-bundle`; check `q2 mcp --launcher-info` after `cargo build --bin q2`.
- [x] Preview SPA chain: `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed` (the `--ui editor`
  end-to-end check needs it), then `cargo build --bin q2` (run
  `cd hub-client && npm run build:wasm` only if the WASM artifact is missing or stale; no Rust changes here).

### Changelog and wrap-up

- [x] Do this last, after the final rebase onto `feature/git-epochs`, because a rebase rewrites the hash: commit the
  hub-client changes, then a second commit adding the entry to `hub-client/changelog.md` under today\'s date, with the
  first commit\'s hash (the hub-client two-commit workflow in `AGENTS.md`). One sentence, user-facing: a replaced file
  shows a notice, and a paused banner appears while the owner is mid-operation.
- [x] Update this plan\'s checkboxes and fill in the notes below.

## Gates

```bash
npm test -w @quarto/quarto-sync-client
npm test -w @quarto/preview-runtime
npm test -w @quarto/hub-mcp
npm test -w q2-preview-spa
npm run test:integration -w q2-preview-spa
npm run typecheck
npm run lint:css -w hub-client
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

- Tests 1 to 8 pass; the whole suites in the gates are green; `build:all` succeeds.
- The consumers audit is complete: every handle holder is either rebound, reset, or shown to re-resolve.
- The two end-to-end runs are recorded in the notes.
- The MCP bundle and the preview SPA are rebuilt, and the changelog entry is committed.

## Out of scope

- The hub side of the flip and the git logic (GE-3, GE-4); the real owner and the git operations (GE-13).
- The extension\'s own swap (GE-9).
- A read-only mode while paused, and any orphaned-branch UI.
- A viewer for retired versions.

## Notes from the run

**Session 1 (lane C, 2026-11-19).** The worktree had two untracked directories (`vscode-sync-experiment/.vscode-test/`,
383 MB of a downloaded VS Code, and `test-results/`); both are generated by GE-8's Playwright runs and neither is work,
so I left them alone and went on.

- **Sync client.** `swap.test.ts` (10 tests, two real clients through the test hub) and `swap-resilience.test.ts` (3)
  were written and run red before the code. Order differs from the checklist: test specification 8 was written after
  tests 1 and 2, because it needs the new API to exist. What today's behavior already satisfied: a late joiner
  (a fresh connect reads the index as it is, so there is nothing to replace). The sync client builds a coordinator per
  connection (`startSwap`, called after `disconnect()` in `connect` and `createNewProject`), `syncWithFiles` hands a
  changed docId to it without awaiting, the host rebinds through `subscribeToFile` (so `onFileAdded` re-fires with
  `docId`), and `retryUnavailableFilesOnce` retries a flipped path through the coordinator instead of a fresh
  subscription. `unconfirmedLocalEdits` counts the changes since the sync info's confirmed heads, or 1 when the doc
  has an undelivered local change and no baseline; my first version passed URL-encoded heads to Automerge raw and
  counted 0 (red test found it; `decodeHeads` fixed it). The stranded-edit test cuts only the old doc's outbound
  traffic (a gate on the guest's network adapter), because cutting all outbound traffic also stops the guest loading
  the new doc. The offline test cuts both directions, edits, flips twice on the hub, restores and forces a reconnect
  with the window `online` event; it asserts the final doc and that the offline edit stays in the old doc, and
  deliberately not the stranded count (whether the offline edit syncs before the swap starts is a race).
- **Dependency and build order** (detail chosen by the plan author; confirm; took it). `@quarto/quarto-git-epochs` is a
  dependency of `quarto-sync-client`, `preview-runtime`, `q2-preview-spa`, and `hub-client`; the lockfile got one line
  per package by hand (the `npm install` pruning incident in GE-1's notes happened again: it deleted about 260 lines of
  platform entries, which I restored). Only the browser-safe `.` entry is imported. Workspace builds run in
  alphabetical order, so `quarto-git-epochs` builds before `quarto-sync-client`; vitest in the dependents resolves the
  package's built `dist/`, so it must be built first (CI already builds it first).
- **Preview runtime** (detail chosen by the plan author; confirm; took it). The notice subscription is
  `createSwapStatusStore` (`swapStatus.ts`): frozen paths, one aggregate notice per batch published after a 150 ms
  quiet window, `appendToNextNotice` for extra lines, a `noticeSeq` so an identical notice is announced again.
  `automergeSync` feeds a singleton and also exposes the raw event through `setSyncHandlers({ onFileReplaced })`.
  `createGitStatusPoller` (`gitStatus.ts`) polls the route every 2 s while `tracking` is true and stops on a 404, on
  `tracking: false`, and on a 200 that is not JSON (an older preview answers every path with its page; found in the
  end-to-end run, because the route did not exist yet when I started: GE-4 landed mid-run).
- **Preview SPA.** The block-editor write captures `getFileHandle(path).documentId` with the diff base and passes it
  as `baseDocId`; the iframe's editing surface is disabled while the path is pending and a stray edit is dropped;
  `SwapNotice` renders on `--allow-edit` only; `PausedBanner` renders everywhere. `SwapNotice.tsx` and `PausedBanner.tsx`
  were written before their tests (an order slip), the tests then passed at once. The four strict runtime mocks in the
  SPA tests spread `test-utils/swapMock.ts`; my first mock returned a new snapshot object per call and looped
  `useSyncExternalStore`.
- **hub-client.** `services/swapEvents.ts` is a small bus; `App` forwards `onFileReplaced` to it and
  `services/swapConsumers.ts` subscribes: presence `rebindCurrentFile` (broadcasts the shared `MovedMessage` on the old
  handle, rebinds, clears remote cursors; receivers get `onPeerMoved` and show `movedNotice` in a 4 s toast), the branch
  service `handleDocSwapped` (back to main, returns how many branches stay, which becomes the `BRANCHES_STAY` line),
  `useReplayMode` closes its own session, and `AttributionStore` drops its state when the path resolves to a different
  doc id (the existing code already threw `HistoryCompactedError` for an unrelated doc; the explicit check also covers
  a fork that extends the old history). `applyBranchEdits` takes `baseDocId` and warns instead of returning silently.
  The editor is read-only while its file is pending, `handleContentOperations` and the branch edit path pass
  `baseDocId`. `SwapNotices` (tier-2 notification, `aria-live`) and `PausedForGitBanner` (tier-3 banner, preview
  sessions only, `previewSession` non-null) use the existing notification tiers and tokens. Handle resolvers
  (`SyncStatusBadge`, `ConnectionStatusDialog`, `downloadAttribution`, `debugAutomerge`): each already resolved at call
  time; a test per resolver confirms the new handle after a swap; no fix was needed. `usePresence` re-resolves the doc
  in its layout effect, which re-runs when the rebind notifies.
- **MCP.** Needed no code: after a swap `onFileAdded` updates the connection manager's file map and the existing
  compare-and-swap refuses the pre-swap hash with the current content and hash (`quarto-hub-mcp/src/swap.test.ts`,
  3 tests; they depend on the sync client change, since without it `files[path]` never changes). The bundle was rebuilt
  (`cargo xtask build-hub-mcp-bundle`, `q2 mcp --launcher-info` reports a bundle-hash and 19 files).
- **Two bugs the end-to-end run found, that no test had.** (1) The coordinator calls `clock.setTimeout(...)`, which
  makes the clock object `this`; `window.setTimeout` throws `Illegal invocation` then, so no swap ever started in a
  browser. The sync client, `swapStatus`, and the poller now pass arrow wrappers (regression tests stub the global timers
  to refuse any `this` but the global). GE-6's `swap.ts` is correct for hosts that pass wrappers, but its README tells
  hosts to pass the bare globals; the README line should change (not done here; see the handoff). (2) Monaco's
  `executeEdits` returns false on a read-only editor, so the new text never reached the frozen editor; the reconcile
  effect now falls back to `model.applyEdits` (test first).
- **End to end** (real binary, `target/debug/q2` built from this branch after `build-q2-preview-spa`,
  `build-hub-client-embed`, `cargo build --bin q2`; Google Chrome through Playwright, because the cached Playwright
  chromium was the wrong revision; the driver is a scratch script, not committed). Invocation:
  `q2 preview --ui editor --allow-edit --no-browser --port 4821 <fixture>` (index.qmd "Version A"), then a script acting as
  the owner (sync client on `/ws`; types text in the browser; flips `files["index.qmd"]` to a doc that does not exist
  yet, creates it 1.5 s later, and flips again). Observed, with the output inspected: before, the editor showed
  `title: Version A`; after, `title: Version B` with the notice text "This file was replaced because the project
  owner's checked-out version changed." in `[data-testid=swap-notice]`. Not observed: the read-only state during
  `pending` (the attribute I queried is absent in this Monaco; the unit test on `updateOptions` covers it), and the old
  doc holding the edit typed before the flip (the sync-client tests cover it: a write with the old `baseDocId` lands in
  the retired doc). Default viewer: `q2 preview --no-browser --port 4822 <fixture>`; with an immediate flip the
  rendered page changed from "Version A" to "Version V" within 1 s and no notice element exists. (The first driver run
  against the viewer, with the delayed second doc, still showed "Version A" after 4 s on a machine at load average
  100; the immediate-flip run is the one I rely on.)
- **Not done.** The paused banner was exercised only against stubs; `/api/preview/git-status` landed from GE-4
  while this plan ran, and I did not run `--git track` end to end. Windows cross-check: no Rust or platform API in this
  plan. A late joiner shows no notice; the docs statement is GE-18's.
- **Gates.** `npm test`: sync-client 211 passed (4 skipped), preview-runtime 112, hub-mcp 633 of 634 on a full run with
  a 60 s timeout (the one miss, `exit-drain`, passes alone; the default 5 s timeouts fail about a dozen edit tests when
  the machine's load average is near 100, and each passes alone), q2-preview-spa 95 plus 106 integration;
  hub-client unit and integration pass, `test:wasm` 481 passed (28 skipped) after `build:all` rebuilt the stale WASM,
  `build:all` succeeds, `lint:css` clean, root `npm run typecheck` clean after the second rebase, `cargo xtask lint`
  clean.
