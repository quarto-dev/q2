---
title: 'GE-12: Origination: Share this folder'
date: 2026-11-18
description: A VS Code folder can start a project on a sync server, becoming its owner and the writer of the pointer, with a confirm that says what is published and a share panel shown only after the server has the data.
status: approved
depends-on: [ge-10]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-10](ge-10-extension-clone-mode.md) ·
**Unblocks:** [GE-14](ge-14-cross-client-e2e.md), [GE-16](ge-16-windows-run-b.md) ·
**Branch:** `ge/12-origination` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Prove the system from the editor\'s side: a VS Code user runs **Quarto Hub: Share this folder** and gets a project that
browsers, mirrors, and other clones can join. The originating folder is the project\'s owner. When its checkout
changes the version of a file it re-points the project\'s pointer for the followers, and it publishes files git adds.
Nothing in the epic may block this, and this plan makes it real.

## Decisions implemented

- **G24**, in full: the command, the refusals, the file set, the confirm, the server, the publish order, the link
  panel, the identity.
- **G16:** the per-folder `ownerId`, stored in the index `owner` key and the folder\'s `project.json`; only a folder
  whose id matches writes the pointer and publishes git-added files.
- **G18:** the index is created at version 4 with the sidecar pointer, `owner`, and `repo` keys.
- **G13, extension side:** the owner removes the entry of a tracked file git removed, re-lists it on return, and does
  all index changes for one switch in a single change.
- **G6, G7:** the registry bootstrap and the sidecar creation race.
- **G32:** the 10 MB publish cap.

## Context

- Project creation today lives in `quarto-sync-client`: `createNewProject` builds the index inline as
  `{ files: {}, version: CURRENT_SCHEMA_VERSION, identities: {}, folders: {} }`, records the author with
  `setIdentity`, then creates one doc per file (text: `text`; binary: `content`, `mimeType`, `hash`)
  (`ts-packages/quarto-sync-client/src/client.ts:2443`; `setIdentity` at
  `ts-packages/quarto-automerge-schema/src/index.ts:141`). GE-1 added `createIndexDocumentContent()` to the schema
  package so a fourth caller does not copy that literal.
- The extension already creates file docs from disk: `createFromDisk` picks binary by `isBinaryExtension` and text
  otherwise, then adds the index entry (`vscode-sync-experiment/src/project.ts:383-400`). Its ignore set is
  `IGNORED_TOP_LEVEL` and `isIgnored` (`:42`, `:67-70`, extended in GE-8). Its identity is `getIdentity` (userId from
  global state, a name, a color) (`src/extension.ts:73-83`). The project name logic is `projectFolderName` (`:93`).
- `project.json` is `ProjectMeta` (`project.ts:44-48`), extended by GE-9 and GE-10.
- Share links need all four parts: `buildShareUrl({ server, indexDocId, name, file })` in
  `ts-packages/quarto-hub-mcp/src/share-url.ts` (lines 49 to 66), moved to a shared package by GE-8.
- Waiting for the server to hold the data: `drainOutbound(drainMs)` inside `createSyncClient` waits for the hub to
  confirm local heads, and the test hub offers `hubHasHeadsOf` (`quarto-sync-client` `client.ts:1663-1685`,
  `test-hub.ts:61`; `.claude/rules/hub-client-tests.md`, "presence is not delivery"). The extension uses a raw `Repo`
  (`vscode-sync-experiment/src/project.ts:63`), not a `SyncClient`, so it cannot call `drainOutbound`: this plan builds
  `waitForServerHeads(repo, handles, ms)` in the core on the same primitives (`getSyncInfo` and the `remote-heads`
  event). Gate 0 items 1 and 4 record whether the preview hub and the public server report remote heads to a raw
  `Repo`; if one does not, the fallback is a second raw `Repo` that fetches each doc by id from the server and
  compares heads.
- From earlier plans: the TypeScript logic and the registry document operations (GE-5), the controller and the owner
  hook it leaves empty (GE-10), the probe\'s `lsTree`, `showPrefix`, and root-commit queries (GE-5 and GE-10; if one is
  missing, add it to the probe here with a fixture test).

## Behavior

**Refusals.** A folder that is already bound ("Already shared; showing its share information", then the share panel);
a repo with no commits ("Make an initial commit first."); `git` missing while the folder is inside a repo ("git is not
available on the extension host.", detail chosen by the plan author; confirm). With several workspace folders open, a
QuickPick asks which one.

**What is published.** A git clone makes a git-enabled project; a plain folder makes a plain project (no `repo`
entry). Files are the tracked files under the project prefix (modes 100644 and 100755; symlinks and submodule links
dropped) minus the extension\'s ignore set, text and binary alike. A plain folder uses the extension\'s current walk.
A file over 10 MB is skipped and listed. Uncommitted edits are published as they are on disk.

**The confirm** is a modal. The body line about the public server shows only when the server is the public one.

```text
Share this folder on wss://sync.automerge.org?

This publishes a copy of your project to a public server. Anyone who gets the link can read and edit it, and it can't be unpublished.

  Project:        docs-site  (git repo, branch main)
  Files:          142 tracked files, 18.4 MB
  Skipped:        3 files over 10 MB (listed in the Quarto Hub output)
  Uncommitted:    4 files will be shared as they are on disk:
                    index.qmd
                    guides/intro.qmd
                    guides/setup.qmd
                    and 1 more

[ Share ]   [ Cancel ]
```

A custom server shows "Anyone with access to that server and the project link can read and edit it." instead of the
irrevocable line, and "Uncommitted: none" when clean. A plain folder shows "(plain folder)" and no Uncommitted line
(detail chosen by the plan author; confirm). When a published file name looks like a secret (`.env`, `.Renviron`,
`*.pem`, `*secret*`, `*token*`), the confirm lists up to three of them under "Secret-looking files:" (added by the
second reviewer; confirm). The server is any `ws` or `wss` URL, defaulting to
`wss://sync.automerge.org`, remembered per folder in workspace state; the confirm appears every time.

**Publish order.** (1) Bind first: write `.quarto-hub/project.json` and the `ownerId`. (2) Create everything offline in
the folder\'s own Automerge storage: the index (via `createIndexDocumentContent`), `setIdentity` with the extension\'s
id, name, and color, one doc per file, the sidecar, the sidecar pointer, `owner`, and (git) `repo` with the root
commits and path prefix, and the registry entries, each doc under its blob at HEAD with generation 0, in one change.
(3) Connect and sync. (4) Wait until the server confirms the heads of every doc (`waitForServerHeads`; 30 s, detail
chosen by the plan author; confirm). (5) Show the share panel. An interrupted publish resumes from local storage:
reopening the folder starts the normal sync, which finishes the upload, and **Quarto Hub: Show share information**
shows the panel.

**The share panel** is a QuickPick of three copyable forms, each copied to the clipboard on selection: the share URL
(`name` from the `_quarto.yml` title as `projectFolderName` does, else the folder name; `file` is `index.qmd` if
published, else the first `.qmd`, else the first file; detail chosen by the plan author; confirm); the command
`q2 preview --hub-url <server> --project <id> --ui editor --allow-edit`; and the server and id for the Join command
(which also accepts the share URL).

**Owner behavior.** The folder runs in clone mode (a plain folder in mirror mode) with the owner hook filled: on each
arrival that changes docs, in one index change, point the changed paths at their docs, remove the entries of tracked
files git removed, re-list returned files at the registry\'s doc, and publish files git added (fresh doc, entry,
registry entry); a `reset --hard` is the new doc from disk and the retired entry of G9 step 2. Before it writes the
index change it waits (up to 10 s) until the server holds the heads of the new docs, so followers never meet a
pointer to a doc the server lacks (added by the second reviewer; confirm). A folder whose `ownerId` does not match the index writes none of this, even with the same user id. A
hub-owned project has no `owner` key.

**The sidecar race.** If two peers create the sidecar at once, the index key resolves to one winner. The loser adopts
it and re-registers its own attached docs there.

## Test specifications

In the vscode-free core, written and seen failing first.

1. File selection: a table of tracked and untracked files, the ignore set including a nested `.quarto-hub`, symlinks,
   submodule links, an LFS pointer, text against binary, the 10 MB cap, a plain folder.
2. Dirty detection, with CRLF checkouts: text normalized, binary by bytes.
3. The confirm text builder: the exact strings for public and custom servers, clean and dirty, three names then "and
   N more", a plain folder, and the secret-looking line.
4. Publish order, with a recording fake network: no network call before the docs exist; the registry entries are in
   the sidecar before the link; the link function is not called until the heads are confirmed; `waitForServerHeads`
   returns only when the server\'s heads equal the local heads for every doc (against the test hub), and times out
   after the cap otherwise.
5. Bootstrap content: the index at version 4 with the three keys, one registry entry per published doc under its HEAD
   blob with generation 0, a dirty file registered under the HEAD blob.
6. The sidecar race: two peers create it offline, merge through the test hub, both converge on one pointer, and the
   loser\'s entries are present in the winner. No entry is lost.
7. Owner behavior against real temp repos and the test hub, with headless peers: a checkout changes docs (one new
   change on the index, counted), removals and returns, a git-added file published, a binary flip, a `reset --hard` (a new doc from disk, the old entry
   retired, one index change), and the index change written only after the new docs\' heads reached the server. A second clone with
   the same user id but its own folder, attached to the same project, writes nothing to `files`.
8. Resume: stop after the offline step and before connect, reopen, and the server ends up with the index and every doc;
   a connection drop mid-sync resumes the same way.
9. Refusals: bound folder, unborn repo, git unavailable inside a repo, several workspace folders.
10. Playwright VS Code, against a standalone local hub on loopback (started as `test-hub.ts` does) with no network:
    run **Share this folder**, read the modal, confirm, wait for the panel, copy the id, and have a headless peer
    join it as a clone and as a mirror.

## Checklist

- [ ] Write test specifications 1 to 3 and 9 and watch them fail; implement the selection, dirty detection, confirm
  builder, and refusals in the core.
- [ ] Write test specifications 4 to 6 and 8; implement the offline build, the registry bootstrap, the sidecar creation
  and adoption, the heads wait, and resume.
- [ ] Write test specification 7; implement GE-10\'s `PointerWriter.applyDecision(batch)` (one index change per call:
  the batch\'s flips, removals, relists, additions, and resets) and the `ownerId` match. Spec 7 uses GE-10\'s
  `joinAsClone` and `joinAsMirror` helpers and GE-9\'s headless owner.
- [ ] Add `quartoHub.shareFolder` and `quartoHub.showShare` to `package.json`; implement the VS Code layer (folder
  pick, server prompt, modal, panel, clipboard).
- [ ] Store `name`, `file`, and `ownerId` in `project.json` (additive fields) and remember the server in workspace
  state.
- [ ] Write the Playwright spec in test specification 10 on the GE-8 harness; mark it permanent.
- [ ] Update the extension README: the command, the three forms, the owner, the public-server warning.
- [ ] `cargo xtask lint`; `q2 render --strict` on this file after editing its checkboxes.

## Gates

- `npm run typecheck -w quarto-hub-vscode`, `npm run test -w quarto-hub-vscode`, `npm run build -w quarto-hub-vscode`.
- `npm run test -w @quarto/quarto-git-epochs` and `-w @quarto/quarto-automerge-schema` if either changed.
- The Playwright VS Code run script from GE-8, after `cargo build --bin q2`.
- `cargo xtask lint`.

## End-to-end verification

In a real VS Code with a temporary git repo open, run **Share this folder** against a local standalone hub, confirm,
and copy the command from the panel. Run `q2 preview --hub-url <server> --project <id> --ui editor --allow-edit` and
look at the result in a browser; edit in the browser and read the originator\'s disk. Then run `git switch` in the
originating folder and watch the pointer flip. Record the invocations and what was seen. The public server is a manual
run with throwaway content only.

## Exit criteria

- A project originated from a git clone and from a plain folder can be joined by a clone, a mirror, and the
  `--hub-url` preview.
- The owner flips, removes, re-lists, and publishes in one change; a non-owner never does.
- No link is shown before the server confirmed the heads.
- Refusals work; resume works.

## Out of scope

Ownership transfer, auth for hubs with sign-in, unpublishing, attribution beyond the identity entry, a per-file
selection of what to publish, and single-file mode.

## Notes from the run

Not started.
