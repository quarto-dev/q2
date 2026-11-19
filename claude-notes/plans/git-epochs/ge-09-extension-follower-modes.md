---
title: 'GE-9: Extension follower modes: mirror and editor only'
date: 2026-11-18
description: Gives the extension an explicit per-folder mode and implements the two follower modes, mirror and editor only, which follow the project owner through the shared swap module instead of overwriting the working tree unconditionally.
status: done
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

- [x] `git switch -c ge/09-extension-follower-modes feature/git-epochs`; confirm GE-5, GE-6, and GE-8 are on the
  integration line and their gates pass.
- [x] Write test specs 1 and 2 and see them fail. Add `mode` to `ProjectMeta` (`project.ts:44-48` in GE-8\'s core
  `meta.ts`), default `mirror` when absent, and write `mode: 'mirror'` in `createProjectFolder` (today at
  `project.ts:123`). Implement the Binding mirror check in `core/text-binding.ts`; the pre-edit comparison uses the
  Binding\'s own mirror of the doc text, updated after every remote apply and every accepted local edit.
- [x] **Headless owner.** Extend `src/headless/peer.ts` with the owner operations, `flipPath(path, newDocId)`,
  `removePath(path)`, `relistPath(path, docId)`, and `createDocFor(path, text)` (each one index change, so a batch
  `applyBatch({ flips, removals, relists, additions })` is also one change), built with a plain Automerge change on the
  index and the GE-5 registry operations (register, move forward, retire). Test it against the test hub before spec 3
  uses it (a flip written by the owner reaches a second client). The owner is test code; GE-12 builds the real one.
- [x] Write test specs 3 and 4 against a headless owner. Implement the **follower** in `src/core/follower.ts`
  with `createSwapCoordinator` from the GE-6 package:
  `currentDocId(path)` from the tracked handles; `loadDoc` through the engine\'s `findDoc` with the abort signal;
  `rebind` untracks the old handle, tracks the new one, rebinds any open buffer, and (mirror only) writes the new
  doc to disk with `writeFileDoc`; `onReplaced` feeds a `NoticeAggregator` that the `Notifier` flushes once per batch.
  Replace the unconditional branch in `syncIndex` (`:322-330`) with `coordinator.indexChanged(path, docId)`; keep the
  creation of a first-time entry (no existing handle) as today.
- [x] Stranded-edit counting: the open Binding keeps its old handle during `pending`, so the typed characters land in
  the old doc; each local edit batch calls `coordinator.resolveWriteTarget(path, binding.handle.documentId)` and the
  Binding\'s `onStranded` callback increments the counter when it answers `retired`. Do not change where the edit
  goes; only count it.
- [x] Entry removal: a mirror deletes the disk file (as today, `project.ts:315-320`); editor only untracks and
  disposes the Binding without touching disk. Entry addition: a mirror writes the file; editor only does not.
- [x] Write test specs 5 and 6. Implement **editor-only** in `src/core/project-sync.ts` as a start-up branch on
  `meta.mode`: no `FileWatcher`, no `reconcileDisk`, no `createFromDisk`, no `onDocChange` disk writes, no
  `onDisk*` handlers. Wire the gate from the GE-5 package: its marker probe, resolved once with
  `git rev-parse --git-path` for the workspace repository; when the workspace is not inside a git repo the gate
  stays open (detail chosen by the plan author; confirm). While the gate is closed every bound Binding is
  suspended (its `onLocalEdit` drops edits; `applyRemote` still runs only for remote flips after resume). In a git repo it also adds `.quarto-hub/` to
  `.git/info/exclude` once (found with `git rev-parse --git-path info/exclude`; detail chosen by the second reviewer;
  confirm).
- [x] Resume rule: after the markers clear and the 1 s quiet window passes, compare each bound buffer with its
  current doc text; if all are equal, resume; otherwise wait up to 10 s for `indexChanged` to rebind (the swap
  coordinator\'s `done` ends the wait); on timeout show `Notifier.warn` with the actions "Keep waiting" (re-arm
  10 s) and "Dismiss" (the `EDITOR_ONLY_NO_FLIP` constant from GE-6), and stay suspended. A buffer that still differs from the new doc after the rebind is replaced
  by the doc text, as `Binding` already does at bind time; keystrokes typed while the gate was closed are dropped
  (detail chosen by the plan author; confirm), and when a bound buffer was dirty at the moment the gate closed one
  `Notifier.warn` per gate episode says "Edits made during a git operation were not kept." (G20). Timeouts are options of the core so tests can shorten them.
- [x] Write test spec 7. Add the moved message: in the follower\'s `rebind`, call `oldHandle.broadcast(moved)` before
  the old Binding is disposed; extend `Presence.receive` to accept the `moved` type and surface it through
  `Notifier.status` for about 3 s ("NAME switched versions"; text chosen by the plan author; confirm).
- [x] Event log: `logEvent` (`project.ts:247`) writes the append-only `.quarto-hub/events.jsonl` with a closed set of
  event names (`created`, `deleted`, `edited-offline`). Widen the type and add `flipped` for a follower swap (path, old
  and new docId), with a test (detail chosen by the epic reviewer; confirm).
- [x] Status bar: a persistent item showing the mode ("Quarto Hub: mirror" or "Quarto Hub: editor only") through
  `Notifier.status`; GE-10 adds the clone mode and the detached count.
- [x] Write test spec 8 and check a pre-change folder (copy a fixture created by GE-8\'s smoke test B) opens as a
  mirror.
- [x] **Playwright tests** in `vscode-sync-experiment/e2e/` (the GE-8 harness, a local hub, a headless owner):
  a mirror folder with an open dirty buffer receives a flip (assert the notice text, the stranded count in it, the
  disk and editor content after `done`); an editor-only folder inside a temp git repo: create `.git/index.lock`,
  rewrite the open file on disk the way git would, assert that no splice reached the hub doc, remove the lock, then
  have the owner flip and assert the buffer shows the new doc; the same without a flip, assert the timeout warning
  (with the shortened option); the moved message appears in a second window; a legacy folder opens as a mirror; and a timing test, the permanent
  form of Gate 0 item 12: on a checkout of a few thousand files, the last buffer reload event arrives within the
  quiet window after `index.lock` is released (the test records the measured gap and fails above the window, which
  triggers the experimental fallback in G20).
- [x] Mark which of these Playwright tests are candidates for the permanent subset (no network, deterministic) in a
  comment at the top of each file; GE-14 decides. The timing test is not a candidate (it fails by design when the
  machine is slow); give it its own script, `test:e2e:timing`, run locally and by GE-17\'s `c2` on Windows, where its
  measured gap is recorded.
- [x] Reconcile this checklist with what landed, update "Notes from the run", and commit.

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

**Session 1 (lane A, 2026-11-18 to 2026-11-19).** Branch `ge/09-extension-follower-modes` off `feature/git-epochs` at
`3c6c7fb87` (GE-1, GE-5, GE-6, GE-8 landed). The gates for the three dependencies passed at the start: the extension's
92 tests and the package's tests.

**Results.** `npm test -w quarto-hub-vscode`: 12 files, 131 tests (92 before this plan, 39 new), all passing.
`npm test -w @quarto/quarto-git-epochs`: 12 files, 205 tests, unchanged. Root `npm run typecheck`: exit 0 (every
workspace, the extension and its `e2e/` project included). `npm run build -w quarto-hub-vscode`: green.
`cargo xtask lint`: "All checks passed! (1736 files checked)". The root `npm run build` was not run in full (it builds the
hub-client, which needs the WASM build); the extension and package builds were. `cargo xtask verify` is Gordon\'s call.

**Test-first record.** Specs 1 and 2 (`meta.test.ts`, `text-binding.test.ts`) ran red on the missing
`InvalidProjectMetaError` and the missing mirror check before the code. The follower tests (specs 3, 4, 7, 8 in
`follower.test.ts`, `presence.test.ts`) and the editor-only tests (specs 5, 6 in `editor-only.test.ts`) were written
before `follower.ts` and `editor-gate.ts`. As a mutation check I made `EditorGate.isClosed()` return false: 3 of the 8
editor-only tests failed, then the code was restored.

**What landed.**

- `meta.ts`: `ProjectMode`, `mode` defaults to `mirror` on read, an unknown value throws `InvalidProjectMetaError`
  naming the value and the choices (the extension reports it as a sync failure). `createProjectFolder` writes
  `mode: 'mirror'`. `ProjectSync` refuses `clone` for now (GE-10).
- `text-binding.ts`: the Binding mirror (G27). **Formulation:** a VS Code change event arrives after the buffer has
  changed, so "the pre-edit buffer equals the mirror" is checked as "the mirror with the event\'s changes applied equals
  the buffer now". An edit that fails is not spliced and the buffer is reconciled to the doc by content. The old
  `splices local edits` test did not update its fake buffer, so it was changed to do what an editor does.
- `follower.ts` wraps `createSwapCoordinator` and `NoticeAggregator`. A batch is the run of `done` events with no swap
  still outstanding; one `Notifier.info` follows. `unavailable` is a `warn` with `NOTICE_UNAVAILABLE`.
- `ProjectSync.syncIndex` calls `follower.indexChanged` for every entry that already has a handle; a new entry is
  created as before. `rebind` sends the moved message on the old Binding, untracks, tracks, and (mirror) writes the disk.
- **Unavailable docs.** The test hub (and a real server) answers "unavailable" at once for a doc it has not received, so
  the follower\'s `loadDoc` keeps asking every 150 ms until the swap\'s signal aborts; the 5 s timeout then decides
  `unavailable`. A later index change that lists the doc starts a fresh load, as GE-6 specifies.
- `editor-gate.ts` (the editor-only gate) on the GE-5 `createController`. Details chosen (all "confirm" items took the
  plan\'s choice): the gate stays open outside a git repository (the controller goes `off`); `.quarto-hub/` is added to
  `.git/info/exclude` once (`git-exclude.ts`, tested against a real repo, a repo below its root, and a non-repo). The
  gate also polls the cheap marker state every 250 ms so the start of an episode is seen before the next keystroke.
  **"Dirty when the gate closed"** is `BufferHost.isDirty` (VS Code\'s `TextDocument.isDirty`) at that moment; the one
  `GIT_OP_EDITS_DROPPED` warning is shown when the episode ends (resume), not when it starts. A buffer whose doc was
  swapped during the episode counts as settled and takes the new doc\'s text on resume; a buffer that differs from a doc
  that did not swap keeps waiting, so the old doc\'s text is never written into a buffer.
- Interface changes (GE-10 and GE-12 build on them): `BufferHost.isDirty`; `BufferHost.bind` takes `BindHooks`
  (`onLocalEdit`, `suspended`, `onMoved`); `BufferBinding` gains `sendMoved` and `reconcile`; `Presence` takes an
  `onMoved` callback and sends/receives the `moved` type of GE-6\'s `MovedMessage`; `TextBindingCore` gains
  `suspended` and `onLocalEditBatch`; `findDoc` takes an optional signal; `RecordingNotifier` records actions and
  answers warnings from a `choices` queue; test support gains `ManualClock` and `FakeGitProbe`.
- The status bar shows the mode as a persistent item (`VscodeNotifier.status(text)` without a timeout now owns one
  `StatusBarItem`; before, each call leaked a message).
- The event log has `flipped` with `from` and `to` doc ids.
- **Headless owner** (`src/headless/owner.ts`): `create`, `open`, `createDocFor`, `flipPath`, `removePath`,
  `relistPath`, `applyBatch` (one index change), `editText`, `broadcast`, `flush`, plus `createDocOffline` and
  `publishOffline` (a doc the hub has not seen, to hold a swap `pending`). **It has no registry operations**: the
  projects here are plain, there is no sidecar to write, so the GE-5 register/move-forward/retire calls are GE-12\'s.
  Its flips reach a second client (`owner.test.ts`).

**Things GE-10, GE-12, and GE-14 need to know.**

1. **Bundle and automerge.** `@quarto/quarto-git-epochs` imports the full `@automerge/automerge`. In the extension
   bundle that made activation fail with `ENOENT … dist/automerge_wasm_bg.wasm` (found by the Playwright smoke test
   A going red). `esbuild.mjs` now resolves the bare specifier to `@automerge/automerge/slim` and `vitest.config.ts`
   aliases it the same way, so the bundle and the tests share one wasm instance with the slim entry `ensureWasm`
   initializes. A registry function can therefore take a doc from the repo.
2. **`noUncheckedIndexedAccess`.** The extension compiles the package from source with that flag; `paths.ts`,
   `registry.ts`, `swap.ts`, and `probe.ts` needed `!` and `?? ''` in 14 places. No behavior change; 205 package tests
   pass.
3. The extension\'s `package.json` now depends on `@quarto/quarto-git-epochs`; the matching two lines in
   `package-lock.json` were added by hand (a plain `npm install` prunes platform packages, as GE-1 and GE-8 found).
   `npm ci --dry-run` accepts the lockfile.
4. `engines.vscode` is still `^1.90.0` although sync needs Node 22 (GE-8\'s finding); not changed here. GE-11 should
   decide.

**Checkout-reload timing (the permanent Gate 0 item 12, G20).** `npm run test:e2e:timing -w quarto-hub-vscode`, VS Code
1.101.2, macOS arm64, a 3000-file checkout (`git switch` between two branches that differ in every file), the last open
buffer being the last file git rewrites. Four runs measured the gap from `index.lock` release to the last reload at
**1211, 1143, 975, and 727 ms** (the slowest editor query was 22 to 27 ms, so the numbers are real). GE-1 measured 151 ms
on a small checkout. Two of four runs exceed the 1 s quiet window, so by the plan\'s own rule the test fails and the G20
fallback applies: editor-only ships as experimental with a longer window and a stated warning. **I did not change the
window**: it is a G32 number in the shared package and the contract, and the content guard (drop an edit that replaces
most of a file with its disk text) is "added only if the measurement shows a problem", which is Gordon\'s call. What the
gate does today at a late reload: if the buffers still equal their docs when the window ends, it resumes, and a reload
that arrives after that is an ordinary-looking edit; the mirror check passes it when the buffer was the old text. See the
handoff.

**End to end (real VS Code 1.101.2, macOS arm64, the in-process test hub, a headless owner; nobody watched the window).**
The VS Code build was copied from lane C\'s `.vscode-test` (same pinned version); no download. Invocation, from the
repo root after building the ts-packages and `npm run build -w quarto-hub-vscode`: `npm run test:e2e -w quarto-hub-vscode`.
Result: 7 tests (5 here and GE-8\'s two smoke tests), all passing; the moved-message test was re-run alone after a fix.
Observed output I read:

```text
mirror notice: "Quarto Hub: This file was replaced because the project owner's checked-out version changed. 3 of your recent edits stayed with the previous version."
editor-only: new doc text after resume: "A2\ntyped"
editor-only warning: "… Quarto Hub: The project owner has not switched versions yet. Keep waiting? … Keep waiting / Dismiss"
```

The mirror test typed `abc` while the swap was pending (3 edits, counted), then published the new doc: the editor shows
`A2`, the disk holds `A2\n`, the old doc keeps `abc`. The editor-only test took `.git/index.lock`, rewrote `a.qmd` the
way git does, saw the buffer reload to `A2`, asserted the hub\'s old doc heads did not move (no splice), removed the
lock, had the owner flip, and saw the buffer on the new doc with editing resumed (`A2\ntyped`, not duplicated). The
no-flip test saw the warning with its two actions and the buffer still on the git text. The legacy test opens a folder
whose `project.json` has no `mode` and sees `Quarto Hub: mirror` in the status bar, then a flip on disk.

**Deviations and what did not run.**

- The Playwright moved-message test sends `moved` from the test peer (after a `presence` message so the name is
  known) instead of from a second VS Code window: with two real mirrors the receiver may swap first and miss the message
  on the old doc, so it would be flaky. The sending side (before the Binding is disposed) is a unit test; the
  wire shape is a unit test with two repos; the display is the Playwright test.
- The 10 s wait and the 5 s swap timeout have test-only environment variables in the VS Code layer
  (`QUARTO_HUB_TEST_FLIP_WAIT_MS`, `QUARTO_HUB_TEST_SWAP_TIMEOUT_MS`), as GE-8 did for the sync server.
- Not run: the Playwright tests on Linux or Windows (Windows is GE-17; the workflow is still manual); a Windows
  cross-clippy (no Rust changed); the root `npm run build`; `cargo xtask verify`.
- Permanent-subset candidates: every test in `follower-modes.e2e.ts` (marked in its header comment). The timing test is
  not one.
