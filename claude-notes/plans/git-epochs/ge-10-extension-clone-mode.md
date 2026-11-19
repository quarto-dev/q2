---
title: 'GE-10: Extension clone mode: attach, controller, arrival, detach, startup, rebind'
date: 2026-11-18
description: Implements the extension as a git-aware clone peer that attaches an existing git clone to a project, verifies it is the same repository, and then follows its own checkout through the registry, with detach, startup, and rebind behavior.
status: done
depends-on: [ge-9]
lane: A
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) ·
**Waits for:** [GE-9](ge-09-extension-follower-modes.md) ·
**Unblocks:** [GE-11](ge-11-join-command.md), [GE-12](ge-12-origination.md) ·
**Branch:** `ge/10-extension-clone-mode` off `feature/git-epochs`; rebase and fast-forward when done; never pushed.

## Goal

A clone peer is the one extension mode that is git-aware. The user already has a git clone of the project\'s repository
open in VS Code; attaching binds that folder to a project. From then on the extension watches git, and when git
rewrites a file it attaches to the doc that stands for the new version in the registry instead of merging git\'s
rewrite into a shared doc as if the user had typed it. Peers on the same version collaborate live; peers on other
branches are in other docs. This plan builds the attach flow, the controller wiring, arrival, detach, the startup rule,
and rebind, as functions in the vscode-free core plus interim commands so each can be driven end to end. Joining by
ticket or URL (GE-11) and originating a project (GE-12) reuse them.

## Decisions implemented

- **G9** Arrival, per changed file (disk text S, canonical doc text T, blob content B): no doc, create from S; already
  attached, do not merge disk into the doc; S equals T, attach silently; S differs from T and S equals B, join with a
  notice and an action to use my checkout; S differs from T and from B, do not overwrite, detach (inert) and ask.
  Binary docs compare byte hashes.
- **G10** The startup rule with a persisted record: HEAD unchanged, reconcile as today; HEAD changed, arrival for each
  changed blob and reconcile the rest; no record, arrival for every participating file and never merge disk into a doc.
- **G14** A tracked file uses blob-space; an untracked or ignored file follows the pointer, and only where the path
  already exists on disk. The pointer never creates an untracked file.
- **G13** (extension side) Clone peers hold or ignore the removal of a tracked file and never propagate a delete.
- **G19** The mode `clone` is written to `project.json`; rebinding a bound folder to a new session resets
  `.quarto-hub/` and the record after a confirm; the repo identity is re-verified at startup.
- **G21** Attach: preconditions, the identity check (root commits; blob overlap for shallow clones), path-prefix
  mapping, refuse and offer a mirror on failure, `.quarto-hub/` in `.git/info/exclude`, never create files the clone
  lacks.
- **G27** A detached file has an inert Binding.
- **G7, G8, G11, G12** (consumed) The canonical doc is the lowest `(generation, docId)`; a commit moves the doc
  forward; an unavailable canonical doc times out after 5 s; a doc is live under one path.

## Context

- **What exists after GE-8 and GE-9.** The vscode-free core with injected `FileWatcher`, `BufferHost`, `Notifier`; the
  headless peer; the Playwright harness; `mode` in `project.json` defaulting to mirror; the follower and the editor-only
  gate; the Binding mirror check; the status bar item; the moved message.
- **From GE-5** (`@quarto/quarto-git-epochs`): the classifier, `arrive`, `startup`, the registry operations on a
  sidecar doc (canonical, generation, move forward), the subprocess probe (HEAD, markers, reflog, blob map with a
  prefix, `cat-file --batch`), and the controller (gate, hold, quiescence, tick-driven). If the probe lacks
  `rootCommits()`, `isShallow()`, or `showPrefix()`, add them here: a small additive change (detail chosen by the plan
  author; confirm).
- **From GE-1:** the index keys (the sidecar pointer, `owner`, `repo`, version 4), the shared scenario step lists, the
  golden registry docs, and the contract for the repo entry (root commits and prefix).
- **What the engine does today that clone mode must change.** `syncIndex` follows any doc id change and writes to
  disk (`vscode-sync-experiment/src/project.ts:313-341`; GE-9 replaced this for followers); `onDocChange` rewrites a
  missing file from its doc (`:343-352`); `onDiskDelete` removes the entry for everyone (`:402-419`);
  `reconcileDisk` merges disk into docs and deletes entries for missing files (`:277-309`); `createFromDisk` publishes a
  new file (`:383-400`).
- **A hub will not be the only source of the registry.** Any peer creates the sidecar lazily and writes its pointer
  key into the index; two peers can do so at once. The index key resolves to one winner; the loser adopts the winner
  and re-registers its own attached docs (from the review\'s origination constraints; detail chosen by the plan author;
  confirm).

## Test specifications

Written first and seen failing. Real temporary git repositories, the shared scenario step lists, and a fake probe or
clock where timing matters.

1. **Attach preconditions:** refuses when the share has no repo entry, when the folder is not in a git repo, when the
   folder already has `.quarto-hub/project.json`; each refusal names the reason.
2. **Identity check:** a clone of the project\'s repo passes; an unrelated repo with an `index.qmd` fails; a repo with
   several roots passes on any intersection; a shallow clone (`--depth 1`) fails the root check and passes the blob
   overlap fallback at about half the tracked text files, and fails it below; a failed check offers a mirror.
3. **Prefix mapping:** the project in a subdirectory of the repo, opened as the repo root and as the subdirectory,
   gives the same project-relative paths; a different subdirectory refuses with a message.
4. **Exclude file:** `.quarto-hub/` is added to `.git/info/exclude` found with `git rev-parse --git-path
   info/exclude` (also in a linked worktree), once, idempotently.
5. **Arrival**, driven by `git switch` in a temp repo against a registry fixture: step 1 (no doc: created and
   registered with the right generation); step 2 (already attached: disk not merged into the doc; with S equal to B and T different, as after the clone\'s own
   `git reset --hard`, the entry is retired and a doc is created from disk and registered, and the pointer is left to
   the owner); step 3 (equal text,
   attached silently, including CRLF against LF); step 4 (clean, drifted doc: disk takes the doc text, one aggregate
   notice, and "use my checkout instead" restores the blob content with `cat-file` and detaches); step 5 (dirty and
   different: nothing overwritten, the file detached). The same for a binary file by hash. After each case the
   assertions cover the registry entries, the attached doc id, the disk content, and the notices.
6. **Move forward:** a plain commit registers the same doc under the new blob and removes the old entry in one change,
   keeping the generation; an amend does the same; no peer is disturbed.
7. **Removal and untracked files:** a tracked file git removes keeps its index entry (the clone writes no index
   change); the doc is never written back over a path absent at HEAD; an untracked path that exists on disk follows the
   pointer; the pointer does not create an untracked file that is absent.
8. **Detach:** a detached Binding neither sends edits nor applies remote changes; the status bar shows the count; the
   list opens from it; "adopt" asks first when the local file differs from its blob and then takes the doc; "stay
   detached" persists across restart; contents becoming equal re-attach automatically.
9. **Startup rule:** HEAD unchanged merges offline edits as today; HEAD changed after an offline `git switch` runs
   arrival for the changed blobs and merges nothing from disk into a doc; no record (first run) runs arrival for every
   participating file; an offline `git reset --hard` with an unchanged HEAD is the documented gap (a test pins what
   happens); the record file round-trips.
10. **Rebind:** a bound folder rebinds to a new project after a confirm, which deletes the Automerge store and the
    record; for clone mode the repo identity is re-verified, and a mismatch refuses.
11. **Sidecar creation race:** two peers create the sidecar at the same time; both end on the same doc and both
    re-register their attached docs; a loser whose disk differs from the canonical doc detaches (G7 through G9
    step 5) and a clean loser re-attaches.
12. **Late reload:** while the gate is closed a whole-file rewrite of an open buffer is not spliced into the doc.
13. **Retired and superseded:** canonical selection skips a retired doc; a file attached to a doc another peer retired
    stays attached and live, and the status bar list says "your doc was superseded"; the repair rule re-registers a
    doc that vanished from the registry and never a retired one.
14. **Moved message:** when arrival leaves a doc, the shared moved message goes out on the old handle before the
    Binding is disposed, and a second peer shows it.
15. **jj:** under colocated jj every changed file takes step 5 (detach); nothing is attached or registered by
    arrival.
16. **Hostile keys:** an index key naming `../x` or `.vscode/tasks.json` is skipped through GE-8\'s `safeJoin`.

## Checklist

- [x] `git switch -c ge/10-extension-clone-mode feature/git-epochs`; confirm GE-9\'s gates pass on the integration
  line.
- [x] **First, the git-enabled publish helper**, because specs 1 to 4 and the Playwright tests need a share that
  carries a `repo` entry. Add `publishGit(dir, serverUrl)` to the headless peer: an index at version 4 with the
  sidecar, `repo`, and a registry entry for each file under its HEAD blob at generation 0, built with the GE-1 schema
  helpers and the GE-5 registry operations. It is test code only (GE-12 builds the real origination). Test it by
  reading the result back with the GE-5 registry operations.
- [x] Add `joinAsClone(dir, serverUrl, indexDocId)` and `joinAsMirror(dir, serverUrl, indexDocId)` to the headless peer
  once attach and the mirror binding exist (wrappers over the core functions: the attach check and the binding
  writer), so that GE-12 spec 10, GE-14, and GE-17 `c1` step 3 can attach a headless clone without a VS Code window.
  They live here, not in GE-11, because GE-11 and GE-12 run in parallel and GE-12 needs them. Test them against
  `publishGit`.
- [x] Write test specs 1 to 4 and see them fail. Implement `src/core/clone/attach.ts` (preconditions, identity check,
  prefix mapping) and the exclude-file writer. Extend the GE-5 probe with the missing helpers if needed.
- [x] Add `mode: 'clone'` and the `repo` identity (roots, prefix) to `ProjectMeta`; write them at attach; re-verify
  roots (or blob overlap when shallow) at every startup and refuse to start sync on a mismatch, with a message.
- [x] Write test specs 5 and 6. Implement `src/core/clone/registry-client.ts`: find the sidecar from the index key
  (create lazily; write the key; on a lost race adopt the winner and re-register), read and write entries with the
  GE-5 registry operations, and write each move forward as one change. After each sidecar change re-evaluate every
  attached file (G7) and run the narrow repair rule: a clean file whose doc is no longer canonical re-attaches, a file
  whose disk differs goes to step 5, and a file attached to a doc another peer retired stays in it.
- [x] Implement `src/core/clone/controller.ts`: build the GE-5 controller on the subprocess probe for the project\'s
  repository (project prefix applied); on its `hold` and gate events suspend outbound edits from every bound Binding
  (G27) and skip disk-to-doc and doc-to-disk sync for held paths; on its decision run `classify` (G2) and then, per
  file, a move forward or `arrive`; resume sync for the held files. The controller runs only in clone mode. It passes `isOwnWrite(path)` (the engine\'s last-written content hash) so the
  engine\'s own disk writes do not restart the quiet window, and every file holds from the trigger if its disk differs
  from its last synced content.
- [x] Arrival side effects: a created doc is registered with generation max over the path plus one (the maximum of a path with no entry is -1, so its first doc gets 0); step 2 with
  S equal to B and T different retires the entry and creates a doc from disk (the owner also flips the pointer;
  GE-12);
  attach rebinds any open buffer to the chosen doc; step 4 writes the doc text to disk and queues the aggregate
  join notice; step 5 detaches, and so does every changed file under colocated jj (arrival is unsupported there). A failure on one
  file falls back to ordinary sync for that file and is logged (D14).
- [x] Send the shared moved message (GE-6) on the old handle, before the old Binding is disposed, whenever arrival
  leaves a doc (G25); GE-14 H4 expects peers to see it.
- [x] Write test spec 7. In clone mode change `syncIndex`: tracked files ignore the pointer and removals; untracked
  files follow the pointer only where the path exists on disk. Gate `onDocChange` so it does not recreate a file whose
  path is absent at HEAD or is held. Leave `onDiskDelete` for a user\'s own delete of a tracked file as it is today
  (a hand delete with no HEAD change behaves as today).
- [x] Add `Binding.setInert(bool)` in the core: inert drops local edits and skips `applyRemote`, keeping the handle for
  the re-attach comparison. Write test spec 8 and implement the detached set, the aggregated notices, and the actions
  (texts below).
- [x] **Notices.** Use GE-6\'s constants and never restate them: `FILES_DETACHED` (actions "Adopt shared version" and
  "Stay detached"), `FILES_JOINED_LIVE` (action "Use my checkout instead"), `ADOPT_CONFIRM` for the adopt confirm, and
  `DOC_SUPERSEDED` for the status bar line. Notices aggregate per batch, one message each.
- [x] **No "share mine".** A dirty file offers two actions, adopt and stay detached (decided 2026-11-18: a second doc
  under the current blob would lose to the canonical doc under G7, so it could never be joined). A detached file that
  is later committed goes through arrival again for its new blob; no doc is registered there, so one is created from
  the disk text and the file is shareable from then on. Add that as a test case: detach a dirty file, commit it, and
  assert a doc exists under the new blob and the file is attached.
- [x] **"Use my checkout instead"** (the action on the joined-live-edits notice, G9 step 4). It writes the blob's
  content (read with `git cat-file`, no filters) back to the file and detaches the file (inert); it never creates a
  second doc (detail chosen by the plan author; confirm). Test it: the file text equals the blob and the Binding is
  inert.
- [x] Event log: add `switched` (arrival attached a different doc), `detached`, `reattached`, `held`, `reset`, `retired`, and `moved-forward` (a commit-type decision) to the
  events GE-9 widened (`logEvent`, `project.ts:247`), each with the path, the doc ids, and the reason; the first
  plan listed these for the extension. Test that each decision writes its event (detail chosen by the epic
  reviewer; confirm).
- [x] Status bar: the mode plus the detached count (and a "your doc was superseded" line for a file attached to a doc
  another peer retired); clicking opens a quick pick listing the detached files with
  "Adopt" and "Stay detached" per file (detail chosen by the plan author; confirm). Automatic re-attach: on a watcher
  event or a doc change, compare normalized text and re-attach when equal.
- [x] Write test spec 9. Implement the persisted record `.quarto-hub/git-epochs.json` (`version`, `head`, `blobs` as a
  path-to-blob map for the participating files, `detached` as a list of paths) written atomically after each decision
  and on shutdown, and the startup rule over it. Reconcile as today for files whose blob is unchanged.
- [x] Write test specs 10 and 11. Implement `rebind(root, newMeta)` in the core (confirm, shut the repo and its storage
  down first, because on Windows deleting a store that is still open fails with EBUSY; remove the Automerge store and
  the record; rewrite `project.json`; re-run the first-run rule) and the sidecar race handling. Write the record
  atomically with a rename and retry a rename that fails with EPERM a few times (Windows, a watcher or scanner holding
  the target); GE-16 `b4` covers both on Windows.
- [x] Write test spec 12 if it is not already covered by GE-9\'s spec 2 and the gate: assert the whole-file reload
  during a closed gate is not spliced.
- [x] **Interim commands** so every flow is driveable end to end: "Quarto Hub: Attach Folder to Project" (asks for a
  server URL and a project id or share URL, parsed with the shared package) and "Quarto Hub: Rebind Folder to Project"
  (detail chosen by the plan author; confirm; GE-11\'s Join command calls the same core functions and then these two
  are removed or kept as shortcuts).
- [x] An owner hook: define this interface in `src/core/clone/pointer-writer.ts`, which the clone controller calls
  after each decision, with a no-op default; GE-12 supplies the owner implementation. Do not write the pointer here.

  ```ts
  export interface PointerBatch {
    flips: { path: string; docId: string }[];      // paths whose doc changed (checkout, or a doc created from disk)
    removals: string[];                            // tracked files git removed
    relists: { path: string; docId: string }[];    // files that returned, at the registry's doc
    additions: string[];                           // files git added; the owner publishes a fresh doc each
    resets: { path: string; retiredDocId: string; newDocId: string; discarded: number }[];
  }
  export interface PointerWriter {
    applyDecision(batch: PointerBatch): Promise<void>;   // one index change per call
  }
  ```
- [x] **Playwright tests** (GE-8 harness; a project published with the headless peer\'s `publishGit`, a temp git repo
  as the workspace; later steps use `git switch` run from the test): attach succeeds and the status bar says clone;
  attach to an unrelated repo is refused and a mirror is offered; a `git switch` to a branch another peer already
  created attaches to that doc and shows the join notice; a dirty file detaches, "Adopt" asks first, and then takes the
  doc; "Stay detached" survives closing and reopening the window; an offline `git switch` while the window is closed
  runs arrival at startup and nothing from disk reaches a doc.
- [x] List the tests that are candidates for the permanent subset in a comment at the top of each file.
- [x] Reconcile this checklist with what landed, update "Notes from the run", and commit.

## Gates

```bash
npm run test -w quarto-hub-vscode                        # core, clone, controller, record, rebind
npm run test -w @quarto/quarto-git-epochs                # shared package, if the probe was extended
npm run typecheck && npm run build                       # from the repo root
npm run test:e2e -w quarto-hub-vscode                    # attach, arrival, detach, startup, with a display
cargo xtask lint
```

Temporary git repositories use the helper from GE-8; no test depends on a global git configuration (set the identity
and `core.autocrlf` per repo).

## End-to-end verification

The Playwright flows exercise a real VS Code, a real git repository, and the local hub. Record the commands and
snippets of the notices, the registry entries read back from the sidecar, and the files on disk in "Notes from the
run", say they were inspected, and say no person watched the window. The case with a `q2 preview --git track` hub is
GE-14\'s.

## Exit criteria

- Attach verifies the repository, maps the prefix, excludes `.quarto-hub/`, and records `mode: clone` with the
  identity; a failed check refuses and offers a mirror.
- A checkout runs arrival per the five steps for text and binary files; a commit moves the doc forward; removals and
  untracked files behave as G13 and G14 say; no file the clone lacks is ever created.
- Detached files are inert, listed, persisted, and re-attach when equal; the actions work.
- The startup rule covers the three cases, with a persisted record, and the offline-checkout case merges nothing
  from disk.
- Rebind resets the folder after a confirm; the sidecar creation race converges.
- The owner hook exists and does nothing; all gates pass.

## Out of scope

- The unified Join command, tickets, `q2` spawn, and `project_root` detection (GE-11).
- Originating a project, the owner key, and the pointer writer (GE-12).
- A real `--git track` hub and browsers (GE-13, GE-14).
- Windows behavior (GE-15 to GE-17); the permanent Playwright subset (GE-14).

## Notes from the run

Worked by the lane A worker in `workspace-7` on `ge/10-extension-clone-mode`, unattended. Gates and what they showed:

- `npm run test -w quarto-hub-vscode`: 18 files, 184 tests, passing (131 before this plan). `npm run test -w @quarto/quarto-git-epochs`: 205 passing,
  unchanged (the probe already had `rootCommits()`, `isShallow()`, and `showPrefix()`, so the "extend the probe" step was not needed).
- `npm run typecheck -w quarto-hub-vscode` and `npm run build -w quarto-hub-vscode`: clean. `cargo xtask lint`: all checks passed.
- `npm run test:e2e -w quarto-hub-vscode`: 13 passed (6 new in `clone-mode.e2e.ts`, plus the GE-8 and GE-9 flows as a regression check).
  The Playwright flows drove a real VS Code 1.101.2 against the in-process test hub and real temporary git repositories. No person watched
  the window; assertions read the disk, the status bar, the toasts, the sidecar, and the hub\'s docs.

**End-to-end snippets (inspected).** From the "dirty file detaches" flow, after `git switch side` and an edit inside the quiet window,
`.quarto-hub/events.jsonl` held (abridged):

```text
{"event":"switched","path":"a.qmd","reason":"step 3: attach-silently"}      (startup, no record)
{"event":"held","path":"a.qmd","reason":"disk differed from the synced content when git started"}
{"event":"detached","path":"a.qmd","reason":"step 5: detach-and-ask"}
```

and `.quarto-hub/git-epochs.json` held `"head": "<side tip>"`, both blobs, and `"detached": ["a.qmd"]`. The toast read "1 files are detached:
your checked-out version differs from the shared one. Your edits to them no longer reach the project." with the actions "Adopt shared
version" and "Stay detached"; the status bar read `Quarto Hub: clone · 1 detached`.

**Decisions taken from the plan\'s "(detail chosen by the plan author; confirm)" marks.** All taken as written: the shallow-clone
fallback, the interim command names, "Use my checkout instead" never creating a second doc, the status bar and quick pick, and the owner
hook interface. Specific choices inside them:

- *Shallow overlap rule.* A shallow clone passes when at least half of the registry\'s distinct paths hold the same blob in the clone\'s HEAD
  (`SHALLOW_OVERLAP_MIN` in `attach.ts`).
- *Prefix mapping.* The project root is the folder when its `git rev-parse --show-prefix` equals the share\'s prefix, and `<folder>/<prefix>`
  when the folder is the repository root; anything else is refused with the two prefixes in the message.
- *Notices never block the gate.* A VS Code notification promise can stay pending, so the choice is handled when it arrives. The joined-live
  notice uses `warn`, not `info`, so its action can be answered (and tested).
- *"Held" files.* The controller reads "disk differs from synced" once, when the gate goes pending, but git may rewrite a file after that
  moment. A decision therefore also compares every attached file\'s disk with the doc content snapshotted at the trigger.
- *Untracked files in a clone.* A file the user creates is not shared when it appears on disk (a clone has `.gitignore`d output and
  `node_modules`); it is shared when git adds it (arrival step 1, reported to the owner hook as an `addition`). Untracked files already in
  the index follow the pointer only where the path exists. `onDiskCreate` therefore does nothing in clone mode.
- *Folders.* Git owns a clone\'s folders: the explicit-folder handling in `syncIndex` is skipped in clone mode.
- *A pointer removal of an untracked file* untracks it and leaves the disk alone (a mirror deletes it).
- *Startup order.* A freshly attached clone has no local store, so clone mode goes online before it reads the index; the startup rule
  never merges disk into a doc without the registry\'s answer.
- *Registry reads retry.* A sidecar another peer created a moment ago can be rejected as `unavailable` at once; the client asks again for up
  to 30 s (`SIDECAR_WAIT_MS`).

**Surprises and defects found and fixed during the plan (each has a test).**

- Two peers creating the sidecar at once did not always converge: `follow()` was registered after the sidecar was created, so the other
  peer\'s index change could arrive in between and nothing re-checked the key. It now subscribes first and refreshes immediately.
- The gate reported "closed" before the controller had its baseline (any sync event during startup called `check()` with the initial
  signature), which deferred re-evaluation forever. The gate is now open until `init()` finishes.
- Concurrent `writeRecord` calls shared one temporary filename and one rename failed with `ENOENT`; the name is now unique per write.
- A detached file skipped by `maybeReattach` while the gate was closed is retried after each decision.
- `ProjectSync.start()` overwrote the status label at its end; it now goes through `updateStatus()`.
- GE-9\'s follower test read the disk right after the buffer converged and flaked under load; it now waits for the disk write.
- `ts-packages/quarto-share-url/src/share-url.ts` needed one `!` (`idMatch[1]!`) to typecheck under the extension\'s
  `noUncheckedIndexedAccess`.

**Spec 15 (jj) as built.** GE-5\'s `arrive()` overrides only steps 3, 4, and 5 under colocated jj, as the contract table says. A file with no
registered doc for its new blob still takes step 1 (create from disk, register). The test therefore registers shared docs for the
side blobs, so each file reaches step 3 without jj, and asserts that with jj each is detached and none is switched.

**Not covered, and why.**

- The fork path (`fork-then-attach`) is tested at the core level; the no-history fallback (`create-from-content-no-history`) is implemented
  but not tested, because GE-1 Gate 0 item 8 found no need for it.
- The status bar\'s quick pick (`quartoHub.detachedFiles`) and the two interim commands other than Attach are exercised by hand-written
  code paths only; the Playwright flows use the toast actions and Attach. Rebind is covered at the core level, not through the command.
- ADOPT_CONFIRM is a toast with a "Replace" action, not a modal, so the flow is scriptable (the harness sets `window.dialogStyle: custom`
  for future modal confirms).
- Windows was not run. The new code uses no platform-specific API; `writeRecord` retries `EPERM`/`EBUSY` renames and `rebind` removes the
  store with `maxRetries`. GE-16 `b4` is the Windows check for both.
- The offline `git reset --hard` with an unchanged HEAD is the documented gap; a test pins that the reset content is merged as an edit.
- A late file event for git\'s own rewrite, delivered after the gate reopened, merges when its disk differs from the doc. The
  normal case (disk equals the doc after arrival) is a no-op. Not tested further.
