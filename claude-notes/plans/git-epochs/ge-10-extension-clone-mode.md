---
title: 'GE-10: Extension clone mode: attach, controller, arrival, detach, startup, rebind'
date: 2026-11-18
description: Implements the extension as a git-aware clone peer that attaches an existing git clone to a project, verifies it is the same repository, and then follows its own checkout through the registry, with detach, startup, and rebind behavior.
status: approved
depends-on: [ge-9]
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
   registered with the right generation); step 2 (already attached: disk not merged into the doc); step 3 (equal text,
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
    re-register their attached docs.
12. **Late reload:** while the gate is closed a whole-file rewrite of an open buffer is not spliced into the doc.

## Checklist

- [ ] `git switch -c ge/10-extension-clone-mode feature/git-epochs`; confirm GE-9\'s gates pass on the integration
  line.
- [ ] Write test specs 1 to 4 and see them fail. Implement `src/core/clone/attach.ts` (preconditions, identity check,
  prefix mapping) and the exclude-file writer. Extend the GE-5 probe with the missing helpers if needed.
- [ ] Add `mode: 'clone'` and the `repo` identity (roots, prefix) to `ProjectMeta`; write them at attach; re-verify
  roots (or blob overlap when shallow) at every startup and refuse to start sync on a mismatch, with a message.
- [ ] Write test specs 5 and 6. Implement `src/core/clone/registry-client.ts`: find the sidecar from the index key
  (create lazily; write the key; on a lost race adopt the winner and re-register), read and write entries with the
  GE-5 registry operations, and write each move forward as one change.
- [ ] Implement `src/core/clone/controller.ts`: build the GE-5 controller on the subprocess probe for the project\'s
  repository (project prefix applied); on its `hold` and gate events suspend outbound edits from every bound Binding
  (G27) and skip disk-to-doc and doc-to-disk sync for held paths; on its decision run `classify` (G2) and then, per
  file, a move forward or `arrive`; resume sync for the held files. The controller runs only in clone mode.
- [ ] Arrival side effects: a created doc is registered with generation max-seen plus one (zero when the set is empty);
  attach rebinds any open buffer to the chosen doc; step 4 writes the doc text to disk and queues the aggregate
  join notice; step 5 detaches. A failure on one file falls back to ordinary sync for that file and is logged (D14).
- [ ] Write test spec 7. In clone mode change `syncIndex`: tracked files ignore the pointer and removals; untracked
  files follow the pointer only where the path exists on disk. Gate `onDocChange` so it does not recreate a file whose
  path is absent at HEAD or is held. Leave `onDiskDelete` for a user\'s own delete of a tracked file as it is today
  (a hand delete with no HEAD change behaves as today).
- [ ] Add `Binding.setInert(bool)` in the core: inert drops local edits and skips `applyRemote`, keeping the handle for
  the re-attach comparison. Write test spec 8 and implement the detached set, the aggregated notices, and the actions
  (texts below).
- [ ] **Notices** (texts chosen by the plan author; confirm): "N files are detached: your checked-out version differs
  from the shared one. Your edits to them no longer reach the project." with actions "Adopt shared version" and
  "Stay detached"; "N files joined live edits from collaborators that are not committed to git." with the action "Use
  my checkout instead"; the adopt confirm: "This file has uncommitted changes. Replace it with the shared version?"
  Notices aggregate per batch, one message each.
- [ ] **No "share mine".** A dirty file offers two actions, adopt and stay detached (decided 2026-11-18: a second doc
  under the current blob would lose to the canonical doc under G7, so it could never be joined). A detached file that
  is later committed goes through arrival again for its new blob; no doc is registered there, so one is created from
  the disk text and the file is shareable from then on. Add that as a test case: detach a dirty file, commit it, and
  assert a doc exists under the new blob and the file is attached.
- [ ] **"Use my checkout instead"** (the action on the joined-live-edits notice, G9 step 4). It writes the blob's
  content (read with `git cat-file`, no filters) back to the file and detaches the file (inert); it never creates a
  second doc (detail chosen by the plan author; confirm). Test it: the file text equals the blob and the Binding is
  inert.
- [ ] Status bar: the mode plus the detached count; clicking opens a quick pick listing the detached files with
  "Adopt" and "Stay detached" per file (detail chosen by the plan author; confirm). Automatic re-attach: on a watcher
  event or a doc change, compare normalized text and re-attach when equal.
- [ ] Write test spec 9. Implement the persisted record `.quarto-hub/git-epochs.json` (`version`, `head`, `blobs` as a
  path-to-blob map for the participating files, `detached` as a list of paths) written atomically after each decision
  and on shutdown, and the startup rule over it. Reconcile as today for files whose blob is unchanged.
- [ ] Write test specs 10 and 11. Implement `rebind(root, newMeta)` in the core (confirm, remove the Automerge store
  and the record, rewrite `project.json`, re-run the first-run rule) and the sidecar race handling.
- [ ] Write test spec 12 if it is not already covered by GE-9\'s spec 2 and the gate: assert the whole-file reload
  during a closed gate is not spliced.
- [ ] **Interim commands** so every flow is driveable end to end: "Quarto Hub: Attach Folder to Project" (asks for a
  server URL and a project id or share URL, parsed with the shared package) and "Quarto Hub: Rebind Folder to Project"
  (detail chosen by the plan author; confirm; GE-11\'s Join command calls the same core functions and then these two
  are removed or kept as shortcuts).
- [ ] An owner hook: define a `PointerWriter` interface that the clone controller calls after each decision, with a
  no-op default; GE-12 supplies the owner implementation. Do not write the pointer here.
- [ ] **Playwright tests** (GE-8 harness; a project published by the headless peer with a repo entry, a temp git repo
  as the workspace; later steps use `git switch` run from the test): attach succeeds and the status bar says clone;
  attach to an unrelated repo is refused and a mirror is offered; a `git switch` to a branch another peer already
  created attaches to that doc and shows the join notice; a dirty file detaches, "Adopt" asks first, and then takes the
  doc; "Stay detached" survives closing and reopening the window; an offline `git switch` while the window is closed
  runs arrival at startup and nothing from disk reaches a doc.
- [ ] List the tests that are candidates for the permanent subset in a comment at the top of each file.
- [ ] Reconcile this checklist with what landed, update "Notes from the run", and commit.

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

Not started.
