---
title: 'GE-9: Extension follower modes: mirror and editor only'
date: 2026-11-18
description: Gives the extension an explicit per-folder mode and implements the two follower modes, mirror and editor only, which follow the project owner through the shared swap module instead of overwriting the working tree unconditionally.
status: approved
depends-on: [ge-5, ge-6, ge-8]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-5](ge-05-ts-logic-probe-controller.md), [GE-6](ge-06-swap-module.md),
[GE-8](ge-08-extension-foundation.md) ·
**Unblocks:** [GE-10](ge-10-extension-clone-mode.md) ·
**Branch:** `ge/09-extension-follower-modes` off `feature/git-epochs`; rebase and fast-forward when done; never pushed.

## Goal

A follower is a client with no repo of its own that shows whatever version the project owner has checked out. The
extension has two follower modes. A **mirror** is a folder the extension created from a project; it follows the
owner and rewrites its own disk when the owner switches versions. **Editor only** is VS Code opened on the directory a
hub is serving; the hub owns the disk, and the extension adds only live typing and cursors, so it never writes a file.
Today the extension follows a changed document id with an unconditional untrack, retrack, and disk write, which is
how an owner\'s checkout overwrites a peer\'s working tree. This plan replaces that with the swap contract the browsers
use, adds the explicit mode, and gives editor-only mode the git gate, so a checkout in the hub\'s directory is not
sent to the old doc as an edit.

## Decisions implemented

- **G19** The mode is explicit in `.quarto-hub/project.json`: `mirror`, `editor-only`, or `clone`. A folder with no
  mode is a mirror. (`clone` is implemented in GE-10.)
- **G20** A mirror follows the pointer and overwrites its disk on a flip; edits typed in the lag window stay in the
  old doc, counted, with the notice. Editor only follows the pointer and writes nothing; it runs the gate (its own
  marker probe, the 1 s quiet window, outbound edit suspension), waits up to 10 s for the hub\'s flip on resume,
  warns if none comes, and never reconciles a buffer back to the old doc.
- **G25** A peer that leaves a doc because of a switch sends a short ephemeral "moved" message on the old doc; the
  others show it briefly. The message type is the one GE-6 defines.
- **G26** The shared swap module serves followers: phases `pending`, `done`, `unavailable`; a 5 s timeout; retired
  handles kept; docId-bound writes with a stranded counter; one aggregate notice.
- **G27** The Binding keeps a mirror of the doc text and accepts an edit only when the pre-edit buffer equals it, so a
  late reload event cannot corrupt a doc. This applies to every mode.
- **G32** The numbers: quiet window 1 s, client timeout 5 s, editor-only wait for a flip 10 s.

## Context

- **What follows the pointer today.** `ProjectSync.syncIndex` (`vscode-sync-experiment/src/project.ts:313-341`):
  for an entry whose doc id differs from the tracked handle it calls `untrack` (which disposes the editor Binding,
  `:233-241`), loads the new doc, `track`s it, and writes its content to disk (`:322-330`), with no freeze, no
  stranded-edit accounting, and no notice. It also deletes the disk file when an entry disappears (`:315-320`).
- **How edits and reloads become doc changes.** `Binding.onLocalEdit` splices every buffer change into the doc using
  `rangeOffset` and `rangeLength` with no check of the pre-edit text (`src/binding.ts:54-62`); `applyRemote` rewrites
  the buffer from the doc (`:74-100`). VS Code has no "reload" reason on its change events, so a git rewrite of an open
  file arrives as an ordinary whole-document edit.
- **Presence.** `Presence` handles two ephemeral message types, `presence` and `leave`, on the file\'s doc handle
  (`src/presence.ts`); the hub-client wire shape is the same, and the moved message is a third type.
- **From earlier plans.** GE-8 split the extension into `src/core/` (the engine with injected `FileWatcher`,
  `BufferHost`, `Notifier`), a thin VS Code layer, a headless peer, and a Playwright harness with a local test hub.
  GE-6 provides `createSwapCoordinator`, `NoticeAggregator`, and the moved message type in
  `@quarto/quarto-git-epochs`. GE-5 provides the gate (the marker probe and the quiet window) in the same package.
- **No hub plan is needed here.** A headless owner plays the owner against the test hub: it writes
  `files[path]` flips with a plain Automerge change on the index and registers docs with the GE-5 registry
  operations. GE-8\'s headless peer has only `join` and `publish`, so this plan builds the owner side (see the
  checklist); GE-12, GE-14, and GE-17 reuse it.

## Test specifications

Written first and seen failing.

1. **Mode parsing.** A `project.json` with no `mode` reads as `mirror`; each of the three values round-trips; an
   unknown value is rejected with a clear message (detail chosen by the plan author; confirm).
2. **Binding mirror check** (pure, in the core): an edit whose pre-edit buffer equals the mirror applies; an edit
   whose pre-edit buffer differs (a late reload) is not spliced and the Binding re-reconciles by content; the mirror
   updates after each remote apply; a keystroke during an apply still converges.
3. **Mirror follower** against the test hub with a headless owner: a flip while no buffer is open writes the new text
   to disk and clears the retired doc from the listing; a flip while a buffer is open and dirty keeps the typed
   characters in the old doc, increments the stranded count, and the buffer converges on the new doc after `done`; two
   flips before the first swap finishes skip to the final state; an unavailable new doc fires `unavailable` after
   5 s, the file stays on the old doc, and a retry swaps once the doc arrives; the owner removing an entry deletes the
   mirror\'s file; the owner adding an entry creates it.
4. **Notices.** One aggregate message per batch, using the strings GE-6 exports (never restated here), with
   the stranded sentence appended only when the counter is nonzero.
5. **Editor-only follower:** it never calls `fs` write or remove, even when a flip changes a bound file; the file
   watcher and `reconcileDisk` are not started; a flip rebinds the open buffer to the new doc.
6. **Editor-only gate** (fake probe and clock): while a marker exists no outbound edit is sent from any bound buffer;
   after the markers clear the quiet window must pass; if every bound buffer equals its current doc, editing resumes
   at once (a plain commit); otherwise it waits up to 10 s for a flip, rebinds on a flip, and on timeout stays
   suspended and warns; it never writes the old doc\'s text into the buffer; one `GIT_OP_EDITS_DROPPED` warning (GE-6\'s
   constant) per gate episode appears when a bound buffer was dirty as the gate closed, and none when no
   buffer was dirty.
7. **Moved message:** a swap that completes sends it on the old handle before the old Binding is disposed; receiving
   it shows the transient status text naming the sender, and a missing or malformed message is ignored.
8. **Legacy folder:** an existing folder created before this change starts as a mirror and behaves as in GE-8\'s
   characterization tests except for the flip handling above.

## Checklist

- [ ] `git switch -c ge/09-extension-follower-modes feature/git-epochs`; confirm GE-5, GE-6, and GE-8 are on the
  integration line and their gates pass.
- [ ] Write test specs 1 and 2 and see them fail. Add `mode` to `ProjectMeta` (`project.ts:44-48` in GE-8\'s core
  `meta.ts`), default `mirror` when absent, and write `mode: 'mirror'` in `createProjectFolder` (today at
  `project.ts:123`). Implement the Binding mirror check in `core/text-binding.ts`; the pre-edit comparison uses the
  Binding\'s own mirror of the doc text, updated after every remote apply and every accepted local edit.
- [ ] **Headless owner.** Extend `src/headless/peer.ts` with the owner operations, `flipPath(path, newDocId)`,
  `removePath(path)`, `relistPath(path, docId)`, and `createDocFor(path, text)` (each one index change, so a batch
  `applyBatch({ flips, removals, relists, additions })` is also one change), built with a plain Automerge change on the
  index and the GE-5 registry operations (register, move forward, retire). Test it against the test hub before spec 3
  uses it (a flip written by the owner reaches a second client). The owner is test code; GE-12 builds the real one.
- [ ] Write test specs 3 and 4 against a headless owner. Implement the **follower** in `src/core/follower.ts`
  with `createSwapCoordinator` from the GE-6 package:
  `currentDocId(path)` from the tracked handles; `loadDoc` through the engine\'s `findDoc` with the abort signal;
  `rebind` untracks the old handle, tracks the new one, rebinds any open buffer, and (mirror only) writes the new
  doc to disk with `writeFileDoc`; `onReplaced` feeds a `NoticeAggregator` that the `Notifier` flushes once per batch.
  Replace the unconditional branch in `syncIndex` (`:322-330`) with `coordinator.indexChanged(path, docId)`; keep the
  creation of a first-time entry (no existing handle) as today.
- [ ] Stranded-edit counting: the open Binding keeps its old handle during `pending`, so the typed characters land in
  the old doc; each local edit batch calls `coordinator.resolveWriteTarget(path, binding.handle.documentId)` and the
  Binding\'s `onStranded` callback increments the counter when it answers `retired`. Do not change where the edit
  goes; only count it.
- [ ] Entry removal: a mirror deletes the disk file (as today, `project.ts:315-320`); editor only untracks and
  disposes the Binding without touching disk. Entry addition: a mirror writes the file; editor only does not.
- [ ] Write test specs 5 and 6. Implement **editor-only** in `src/core/project-sync.ts` as a start-up branch on
  `meta.mode`: no `FileWatcher`, no `reconcileDisk`, no `createFromDisk`, no `onDocChange` disk writes, no
  `onDisk*` handlers. Wire the gate from the GE-5 package: its marker probe, resolved once with
  `git rev-parse --git-path` for the workspace repository; when the workspace is not inside a git repo the gate
  stays open (detail chosen by the plan author; confirm). While the gate is closed every bound Binding is
  suspended (its `onLocalEdit` drops edits; `applyRemote` still runs only for remote flips after resume). In a git repo it also adds `.quarto-hub/` to
  `.git/info/exclude` once (found with `git rev-parse --git-path info/exclude`; detail chosen by the second reviewer;
  confirm).
- [ ] Resume rule: after the markers clear and the 1 s quiet window passes, compare each bound buffer with its
  current doc text; if all are equal, resume; otherwise wait up to 10 s for `indexChanged` to rebind (the swap
  coordinator\'s `done` ends the wait); on timeout show `Notifier.warn` with the actions "Keep waiting" (re-arm
  10 s) and "Dismiss" (the `EDITOR_ONLY_NO_FLIP` constant from GE-6), and stay suspended. A buffer that still differs from the new doc after the rebind is replaced
  by the doc text, as `Binding` already does at bind time; keystrokes typed while the gate was closed are dropped
  (detail chosen by the plan author; confirm), and when a bound buffer was dirty at the moment the gate closed one
  `Notifier.warn` per gate episode says "Edits made during a git operation were not kept." (G20). Timeouts are options of the core so tests can shorten them.
- [ ] Write test spec 7. Add the moved message: in the follower\'s `rebind`, call `oldHandle.broadcast(moved)` before
  the old Binding is disposed; extend `Presence.receive` to accept the `moved` type and surface it through
  `Notifier.status` for about 3 s ("NAME switched versions"; text chosen by the plan author; confirm).
- [ ] Event log: `logEvent` (`project.ts:247`) writes the append-only `.quarto-hub/events.jsonl` with a closed set of
  event names (`created`, `deleted`, `edited-offline`). Widen the type and add `flipped` for a follower swap (path, old
  and new docId), with a test (detail chosen by the epic reviewer; confirm).
- [ ] Status bar: a persistent item showing the mode ("Quarto Hub: mirror" or "Quarto Hub: editor only") through
  `Notifier.status`; GE-10 adds the clone mode and the detached count.
- [ ] Write test spec 8 and check a pre-change folder (copy a fixture created by GE-8\'s smoke test B) opens as a
  mirror.
- [ ] **Playwright tests** in `vscode-sync-experiment/e2e/` (the GE-8 harness, a local hub, a headless owner):
  a mirror folder with an open dirty buffer receives a flip (assert the notice text, the stranded count in it, the
  disk and editor content after `done`); an editor-only folder inside a temp git repo: create `.git/index.lock`,
  rewrite the open file on disk the way git would, assert that no splice reached the hub doc, remove the lock, then
  have the owner flip and assert the buffer shows the new doc; the same without a flip, assert the timeout warning
  (with the shortened option); the moved message appears in a second window; a legacy folder opens as a mirror; and a timing test, the permanent
  form of Gate 0 item 12: on a checkout of a few thousand files, the last buffer reload event arrives within the
  quiet window after `index.lock` is released (the test records the measured gap and fails above the window, which
  triggers the experimental fallback in G20).
- [ ] Mark which of these Playwright tests are candidates for the permanent subset (no network, deterministic) in a
  comment at the top of each file; GE-14 decides. The timing test is not a candidate (it fails by design when the
  machine is slow); give it its own script, `test:e2e:timing`, run locally and by GE-17\'s `c2` on Windows, where its
  measured gap is recorded.
- [ ] Reconcile this checklist with what landed, update "Notes from the run", and commit.

## Gates

```bash
npm run test -w quarto-hub-vscode                       # core, follower, gate, binding, mode
npm run test -w @quarto/quarto-git-epochs               # unchanged, but prove the shared package still passes
npm run typecheck && npm run build                      # from the repo root
npm run test:e2e -w quarto-hub-vscode                   # the Playwright tests above, with a display
cargo xtask lint
```

Follow `.claude/rules/hub-client-tests.md`: wait for the creator\'s heads on the hub before the follower reads, and
await every promise the code under test starts.

## End-to-end verification

Run the Playwright flows against a real VS Code and the local hub. Record the commands, the VS Code version, and
snippets of the observed notices and file contents in "Notes from the run", and say they were inspected. Say plainly
that no person watched the window, and that the real `q2 preview` hub case (a hub on the directory) is exercised in
GE-14, not here.

## Exit criteria

- A folder with no mode starts as a mirror; `mode` round-trips; `createProjectFolder` writes it.
- A mirror follows a flip through the swap module: new text on disk, stranded edits counted and announced once, a
  late reload does not corrupt the doc, an unavailable doc is retried, removals and additions follow the owner.
- Editor-only mode writes nothing to disk, suspends edits while a marker exists, waits for the flip, warns on
  timeout, and never writes the old text into a buffer.
- The moved message is sent and shown; the status bar shows the mode.
- All gates pass and the tests marked as permanent candidates are listed.

## Out of scope

- Clone mode, attach, the registry, arrival, and detach (GE-10).
- The Join command and server binding (GE-11); the owner writing the pointer (GE-12). The flips here come from a
  headless owner.
- A real hub with `--git track` and browsers (GE-13, GE-14).
- Choosing the permanent Playwright subset (GE-14).

## Notes from the run

Not started.
