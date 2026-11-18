---
title: 'Epic: git-aware sync (git epochs) for live share and the VS Code extension'
date: 2026-11-18
description: 'Makes disk-synced Automerge sessions resilient to git checkouts, rebases, and commits, for the preview hub and the VS Code extension together, including the extension joining and originating projects; 18 plans with a dependency graph, one integration branch, no pushes until the whole epic is done.'
status: approved  # decisions closed 2026-11-18 in the review session; plans written; implementation not started
---

**Supersedes:** [the 2026-11-17 plan](2026-11-17-git-aware-live-share-epochs.md). Its decisions D1 to D36 are mapped
to this epic\'s decisions [below](#what-became-of-d1-to-d36); that file stays as history and is marked superseded.
**Branch:** the integration line is `feature/git-epochs` (worktree `workspace-6`). Every plan lands there by rebase and
fast-forward. Nothing is pushed until the whole epic is done and Gordon says so, except the temporary Windows tooling
branch (see [Windows runs](#windows-runs)).
**Plans:** the directory [git-epochs/](git-epochs/) holds one file per plan, GE-1 to GE-18, listed
[below](#plans-and-dependencies).
**Origin:** a review of the 2026-11-17 plan with two independent review agents, then a long design conversation with
Gordon on 2026-11-18 that settled every question listed here. Nothing in this file is an open question; items that
need a measurement are Gate 0 checks in GE-1, and the details the plan authors chose are listed under
[Details chosen while writing the plans](#details-chosen-while-writing-the-plans).

**From the old plan:** its Part A (the hub) became GE-2 to GE-4 and GE-13, its Part B (the extension) GE-8 to GE-12, its
demos GE-13 and GE-14, and its two Gate 0 sections GE-1.

## Goal

When someone runs `git switch`, `git pull`, `git rebase`, or `git reset` in a directory that is synced through
Automerge, the sync layers must not treat git\'s rewrite as an author edit, and must not three-way merge collaborators\'
concurrent edits into text that git just rewrote. Two programs sync a directory with Automerge docs: the preview hub
(`crates/quarto-hub`, Rust) and the VS Code extension (`vscode-sync-experiment/`, TypeScript). This epic makes both
git-aware, makes them work together on one session, and lets the extension join a project and start one.

The demos that prove it:

- **Hub demo.** A `q2 preview --share --git track --ui editor --allow-edit` hub, two browser guests, a VS Code window
  in each extension mode, and a branch switch, a commit, a rebase, and a switch back.
- **Hub-less demo.** A VS Code window originates a project to a sync server. `q2 preview --hub-url <server>
  --project <id> --ui editor --allow-edit` shows it in a browser, and a second VS Code window joins.

## How the system works

**Disk peers** have a working tree and git: the preview hub, and the extension in clone mode. Each keeps its docs in
step with its own disk. **Followers** have no repo of their own: browsers, the MCP server, and extension folders in
mirror or editor-only mode. They show whatever version the project owner has checked out.

- **The pointer.** In the index doc, `files[path]` names one doc per file. Followers read it. The **owner** is the one
  peer that writes it: when the owner\'s checkout changes the version of a file, the owner points the path at the doc
  for the new version. We call that change a **flip**.
- **The registry.** A separate doc, the **sidecar**, referenced from the index by one key. It maps
  `path → blob id → a set of (docId, generation)`. The blob id is git\'s content hash of the file at HEAD, so the same
  content has the same key across clones and branches. It answers "which doc stands for this file at this version?"
- **Clone peers attach by blob.** When a clone peer\'s checkout changes a file, it looks up the new blob in the
  registry and attaches to the canonical doc (the lowest `(generation, docId)`), or creates one. Peers on the same
  version collaborate live; peers on different branches are in different docs and never merge.
- **A commit does not disrupt anyone.** Git did not rewrite the file, so the doc stays and its registry entry moves
  forward to the new blob. Only git rewriting a file (checkout, pull, rebase, reset) makes a peer re-attach.
- **The classifier** decides which of the two happened: the file on disk equals what was last synced (commit) or not
  (git rewrote it), with the reflog subject as a tiebreaker.
- **The gate** pauses sync in both directions while git is mid-operation (lock files and state markers), so a
  half-written tree is never merged.
- **Servers** are only rendezvous: the preview hub\'s `/ws` (directly on loopback, or through a local
  `q2 preview --join` proxy over an iroh tunnel), the `--hub-url` mirror, a standalone hub, or the public sync server.
  Nothing about the sync endpoint is q2 specific; the q2 content lives in the docs.

### Extension modes

| Mode | Where | Follows | Who writes the disk | Git logic |
|---|---|---|---|---|
| **Mirror** | A new folder the extension creates from a project | The pointer | The extension | None |
| **Editor only** | The directory a hub is serving | The pointer | The hub | The gate only |
| **Clone** | The user\'s own git clone, attached to a project | Its own blob, through the registry | The extension | Full |

A folder\'s mode is written in `.quarto-hub/project.json` when the user joins or attaches, and stays fixed. Folders
without a mode are mirrors, which is what they have always been.

## Vocabulary

| Term | Meaning |
|---|---|
| Owner | The peer allowed to write the pointer for a project: the hub for a hub-created project, the originating folder for an extension-originated one. A convention each client follows, not a permission |
| Follower | A client with no repo of its own that follows the pointer: browser, MCP, mirror, editor-only |
| Clone peer | A disk peer with its own git clone, attached by blob |
| Pointer, flip | `files[path]` in the index, and the owner changing it |
| Registry, sidecar | The `path → blob → docs` map and the doc that holds it |
| Arrival | A clone peer\'s or the hub\'s decision, per file, of which doc to attach to after its checkout changed |
| Move forward | Registering the same doc under the new blob after a commit, and removing the old entry |
| Participating file | A tracked text or binary file under the project prefix |
| Gate | The check for in-progress git operations that pauses sync |

"Host" and "guest" are retired, except in the help text of `--git track`, which says the preview owns the project\'s
pointer. The earlier plan\'s "epoch" survives in package and crate names only.

## Decisions

All settled with Gordon on 2026-11-18 unless a row says otherwise. **Replaces** names the 2026-11-17 decision a row
changes.

### Detection and classification

| Id | Decision | Replaces |
|---|---|---|
| G1 | The trigger is a change in the HEAD reflog tail, not only a HEAD move, so `git reset --hard` (HEAD and blob unchanged, dirty edits discarded) is seen. Decide on the first quiet tick: gate open, no watcher events for 1 s, state stable; cap the wait at 30 s. Files whose blob changed are held from the trigger until the decision | D2, D11 |
| G2 | The classifier is the content rule first: disk equals the last synced content means commit-type (move forward), otherwise checkout-type (arrival). The reflog subject is the tiebreaker: a plain `commit` with a differing disk is the author\'s own edit and moves forward. Colocated jj and repos without a reflog use the content rule alone | D10 |
| G3 | The gate markers: `index.lock`, `rebase-merge`, `rebase-apply`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `sequencer/`, `BISECT_LOG` (the last two are Gate 0 checks), and jj\'s `.jj/working_copy/working_copy.lock`. Resolve the paths once with `git rev-parse --git-path`. Lock files that outlast 60 s are stale (resume with a warning); state markers pause until removed, because a rebase stopped for conflicts is legitimately long | D12 |
| G4 | Subprocess hygiene: `GIT_OPTIONAL_LOCKS=0`; plumbing only (`rev-parse`, `ls-tree -r -z`, `reflog`, `cat-file --batch`); never `hash-object` (it runs clean filters) and never `--path` filters; each call has a timeout; if git is missing or refuses the repo, the feature turns off with one log line. Blob content is read with `cat-file` without filters; the blob id of disk bytes is computed directly. Colocated jj is supported, non-colocated is off, an unresolvable HEAD means no change. The repo identity queries are `git rev-list --max-parents=0 HEAD` (root commits), `git rev-parse --is-shallow-repository`, and `git rev-parse --show-prefix` | D13, D14, D27 |
| G5 | In the hub the gate and the hold live inside `HubContext::sync_all` and `sync_file`, so the periodic loop, the watcher path, the shutdown flush, and the initial sync all pass through them. While a marker is set the shutdown flush and the initial sync are skipped (startup retries on the first open tick), and markers are re-checked after the file read and before the write | new |

### Registry and arrival

| Id | Decision | Replaces |
|---|---|---|
| G6 | The shared registry lives in a sidecar doc referenced from the index by one key. Entries are `path → blob → set of (docId, generation)`. It covers participating files only: tracked text and binary files under the project prefix; binary docs are compared by byte hash. One doc, no sharding, no pruning, no rolling. The sidecar carries its own `version` field. Whichever git-aware peer needs it first creates it; if two peers create it at once, the index key resolves to one winner, and the loser adopts it and re-registers its own attached docs | D4 |
| G7 | The canonical doc for a `(path, blob)` is the lowest `(generation, docId)`. A creation race resolves by set union, with no timing involved. A doc created by a peer that has seen the set gets generation max-seen plus one, so an established doc beats a latecomer; concurrent creators tie on generation and break by docId. The loser re-attaches to the canonical doc; edits it made in its own doc stay there, counted and announced | new |
| G8 | A commit moves the doc forward: in one change, add `(path, newBlob)` carrying the doc\'s existing generation and remove `(path, oldBlob)`. An amend does the same. Two peers committing different subsets of one doc leave it under two blobs; that is accepted | D5 |
| G9 | Arrival, per changed file, with disk text S (CRLF normalized), the canonical doc\'s text T, and the blob content B: (1) no registered doc: create from S; (2) already attached to the canonical doc: do not merge disk into it, re-baseline; (3) S equals T: attach silently; (4) S differs from T and S equals B (clean): join the doc, which holds live edits not in git, with a notice and a "use my checkout instead" action; (5) S differs from T and S differs from B (dirty): do not overwrite, detach the file (inert) and ask: adopt or stay detached; a detached file that is later committed goes through arrival for its new blob and becomes a doc then. Binary docs follow the same steps with byte hashes; adopting means taking the other peer\'s bytes when disk is clean. A hub under `ReadOnly` attaches only when S equals T and otherwise creates a doc from disk, because it must not write its disk | D6, D7, D24 |
| G10 | The startup rule for the extension, with a persisted record of `{head, blobs}` and the detached set. HEAD unchanged since the record: reconcile as today (offline edits merge). HEAD changed: run arrival for each file whose blob differs and reconcile the rest. No record: run arrival for every participating file and never merge disk into a doc. An offline `git reset --hard` with an unchanged HEAD is the known gap | D26 |
| G11 | If the canonical doc is unavailable, wait 5 s, treat it as absent for this arrival, and create one. The generation rule makes the peers converge if the original returns | new |
| G12 | A doc is live under at most one path: if an arrival would attach a doc that the index already lists under another path, fork it instead (new docId, shared history). A case-insensitive collision of two registry keys falls back to ordinary sync for that file. A repo with no commits is refused | new |
| G13 | When git removes a tracked file at the owner\'s checkout, the owner removes its index entry; when the blob returns, it lists the file again, pointing at the registry\'s doc. Clone peers hold or ignore the removal for tracked files and never propagate a delete. All index changes for one switch (flips, removals, re-listings, new files) go in a single change | D9 |
| G14 | Per-file mode: a tracked file uses blob-space (clone peers) and the pointer (followers); an untracked or ignored file follows the pointer as today. In clone mode only untracked paths that already exist on disk sync; the pointer never creates an untracked file | D8 |

### Roles and ownership

| Id | Decision | Replaces |
|---|---|---|
| G15 | The vocabulary above. The `quartoHub.git` setting is removed | D23, D29 |
| G16 | Pointer ownership is static: whoever created the project owns it. A hub-created project is owned by the hub (the `--git track` hub). An extension-originated project has an `owner` key in the index holding a random per-folder `ownerId`, also stored in the originating folder\'s `project.json`; only the folder whose id matches writes the pointer and publishes git-added files. A hub-owned project has no `owner` key. Ownership transfer is out of scope | D23 |
| G17 | The hub adopts the registry and the arrival rule: no private registry, no fork-and-overwrite. It creates the sidecar, writes the `repo` entry when `--git track` is set, registers each participating file\'s doc under its blob at startup, writes the sync checkpoint before it flips the pointer, marks captures stale, and fires the post-sync callback | D4, D6, D7, D19 |
| G18 | The index schema goes to version 4 with no structural change. New optional keys: the sidecar pointer, `owner`, and `repo` (root commits and the project path prefix, written only by a git-aware owner). Unknown keys are preserved by every reader | new |

### The extension

| Id | Decision | Replaces |
|---|---|---|
| G19 | The mode is explicit in `project.json`: `mirror`, `editor-only`, or `clone`. Legacy folders default to `mirror`. Changing a mode means joining again. Rebinding a bound folder to a new session (every `q2 preview` run is a new project) resets `.quarto-hub/` and the startup record after a confirm; clone mode re-verifies the repo identity at startup | D29 |
| G20 | A mirror follows the pointer and overwrites its disk on a flip; edits typed in the lag window stay in the old doc, counted, with the notice. Editor-only follows the pointer and writes nothing; it runs the gate (its own marker probe, the quiet window, outbound edit suspension), and on resume waits up to 10 s for the hub\'s flip before rebinding, warning if none comes. A clone is a full peer | D24 |
| G21 | Attach binds an open folder that is already a git clone. It needs a share that carries a `repo` entry, a folder inside a git repo, and a folder not already bound. The identity check compares root commits (any intersection passes); for a shallow clone it falls back to blob overlap with the registry (about half the project\'s tracked text files). A project in a subdirectory of the repo is mapped by its path prefix. A failed check refuses and offers a mirror instead. `.quarto-hub/` goes in `.git/info/exclude`. Attach never creates files the clone lacks | new |
| G22 | One "Join session" command. Input is a `q2preview…` ticket (opaque: kept in memory only, never logged or stored; the extension runs `q2 preview --join <ticket> --port N --no-browser` on a free port it picks and polls until ready, up to 15 s; it finds `q2` through a `quartoHub.q2Path` setting that defaults to `q2` on `PATH`), a `ws://` or `wss://` URL, an `http(s)://` address (the `/ws` URL is derived), or a share URL (id and server). It reads `/health` for the index id and `project_root`, compares `project_root` with the open workspace folder for every kind of join, and offers editor only when it matches (the default for a loopback URL, not the default for a ticket, always with a confirm). A git-enabled share also offers attach when the open folder passes the check; any share offers a mirror. The binding is written to `project.json` at each join. "Leave session" stops the sync and kills any spawned `q2`. The status bar shows the mode and the detached count | new |
| G23 | Mirrors are created where they are today: inside the current workspace folder, or home if none. Ignore any path segment named `.git` or `.quarto-hub`, and skip a subfolder that has its own `.quarto-hub/project.json` | D35 |
| G24 | "Share this folder" originates a project. A git clone makes a git-enabled project; a plain folder makes a plain project. It publishes tracked files minus the extension\'s ignore set, text and binary, skipping files over 10 MB with a notice. Uncommitted edits are published as on disk. The server is any ws URL, defaulting to the public one. A modal confirm shows the file count, size, skipped files, dirty files, and, for the public server, that the share cannot be unpublished. Docs are created offline first; the index, the sidecar, `repo`, `owner`, and the registry entries are written before the link is shown; the link appears only after the server confirms the heads; an interrupted publish resumes from local storage. The panel offers three copyable forms: a share URL (its `name` is the `_quarto.yml` title or the folder name, and its `file` is `index.qmd` when present), the `q2 preview --hub-url … --project <id> --ui editor --allow-edit` command, and the server and id. It writes `setIdentity` with the extension\'s id, name, and color. It refuses a bound folder and an unborn repo, and prompts when several workspace folders are open | new |
| G25 | A peer that leaves a doc because of a switch sends a short ephemeral "moved" message on the old doc, and the others show "this person switched versions" briefly. The message type is defined once in the shared package | new |
| G26 | A shared swap module in TypeScript serves every follower (browsers through `quarto-sync-client`, mirrors and editor-only through the extension): phases `pending`, `done`, `unavailable`; per-path serialization with the latest index state winning; a 5 s timeout; retired handles kept, at most 8 per path, never deleted; docId-bound writes with a stranded-edit counter; and one aggregate notice per batch ("The project owner\'s checked-out version changed.") announced through an `aria-live` region on editing surfaces | D15 |
| G27 | A detached file has an inert Binding: it neither pushes edits nor applies remote changes; only the handle stays, for the re-attach comparison. The Binding keeps a mirror of the doc text and accepts an edit only when the pre-edit buffer equals it, so a late reload event cannot corrupt a doc | D25, D30 |
| G28 | At startup `q2 preview` checks its project root for `.quarto-hub/project.json` and warns that running a hub on a directory an extension has bound syncs the same files twice. Warn only; do not refuse | new |

### Infrastructure and testing

| Id | Decision | Replaces |
|---|---|---|
| G29 | Two implementations of the same pure logic with the same module shape: a new small Rust crate (`quarto-git-epochs`) and the TypeScript package `@quarto/quarto-git-epochs`. They share fixtures in the TypeScript package: decision tables, scenario step lists (JSON, so Windows can run them), and golden binary docs for the registry schema. A normative registry schema spec lives under `claude-notes/designs/`. The pure core takes and returns plain data | D33 |
| G30 | The extension is split into a vscode-free core (the sync engine with injected watcher, buffer host, and notifier) and a thin VS Code layer. A headless peer built on the core drives automated cross-client tests. Playwright drives VS Code; the permanent subset runs against a local hub with no network, on Linux CI with a display. The `q2-preview-spa` e2e harness gains git helpers | D34, D36 |
| G31 | Windows is tested three times from a temporary tooling branch that never lands, modeled on RFU-6 (workspace-9, branch `rfu/6-windows-tooling`), with the `windows/` prefix on the remote branch. Each run covers only the new tests | new |
| G32 | The numbers: preview periodic sync 5 s, quiet window 1 s, decision cap 30 s, stale lock 60 s, client freeze and unavailable timeout 5 s, unavailable canonical doc 5 s, editor-only wait for a flip 10 s, proxy readiness 15 s, publish cap 10 MB per file, retired handles 8 per path. The hub\'s own default sync interval of 30 s is unchanged | D20 |
| G33 | `q2 preview --git <track\|ignore>`: experimental, default `ignore`, a value is required (a bare `--git` is a parse error), conflicts with `--join`, `--static`, and `--hub-url`. `track` means this preview owns the project\'s pointer. A route `/api/preview/git-status` reports tracking, paused, and pending; web clients show a "paused for git" indicator only (no read-only mode). Version skew with older web clients is accepted and documented | D18, D32 |
| G34 | Branch service (hub-client, experimental, default off): additive; reset the active branch to main on a swap; branch edits are docId-bound; no orphan UI. Attribution: a switched or seeded doc is lump-attributed and unlabeled, and the reactji "remove my contribution" feature does not recognize contributions older than a switch | D16, D17 |
| G35 | Accepted gaps, stated in the docs: the hub restart gap (the registry survives in the sidecar, the hub\'s in-memory state does not); a stale pointer while the owner is offline; extension builds older than this epic follow a flip unconditionally and can overwrite a working tree; two tips after `git add -p`; `git stash`, `git restore`, `git checkout -- file`, and `jj restore` still look like author edits; a git operation inside a nested repo or submodule never moves the superproject HEAD; two full sync agents on one directory are unsupported (G28 warns); how long the public server keeps docs is unknown; a client that joins during a pause or after a swap sees no notice; every doc version a peer has been on is retained, binaries included | D32 |

## What became of D1 to D36

| Old | Fate |
|---|---|
| D1, D3, D13, D14, D21, D22, D27 | Kept (D22\'s automated e2e is GE-13; D21 means a failed Gate 0 check stops the epic) |
| D2, D10, D11, D12 | Revised: G1, G2, G3 |
| D4, D6, D7 | Superseded by the shared registry and arrival: G6, G9, G17 |
| D5 | Revised: move forward, G8 |
| D8 | Revised: binaries participate; untracked follow the pointer: G6, G14 |
| D9 | Added files kept; removed files reversed: G13 |
| D15, D16, D17 | Kept: G26, G34 |
| D18 | Kept, with the mapping note changed: G33 |
| D19 | Revised: the hub also creates the sidecar and `repo` entry: G17 |
| D20 | Replaced by the numbers table: G32 |
| D23, D24, D25 | Superseded: G15, G16, G9, G20, G27 |
| D26 | Revised, no reflog at startup, registry shared: G10 |
| D28 | Revised: server binding per folder is in scope, single-file mode stays out: G19, G22 |
| D29 | Removed: G15 |
| D30 | Kept, with the mirror check: G27 |
| D31 | Revised: G13, G16 |
| D32 | Kept; the warning moves from a setting to the join prompt and the docs: G33, G35 |
| D33 | Revised: two implementations with shared fixtures: G29 |
| D34, D35 | Kept (D35 extended to `.quarto-hub`): G23, G30 |
| D36 | Superseded by the two demos: G30 |

## Plans and dependencies

A plan starts only when every plan it waits for is complete. There is no partial dependency: a plan never waits for a
phase of another plan. The files are in [git-epochs/](git-epochs/); the plans listing does not show them (it lists
top-level files only), so this table is the index.

| Plan | What | Waits for |
|---|---|---|
| [GE-1](git-epochs/ge-01-contract-and-gate-0.md) | Contract and Gate 0: the registry spec, the V4 index and sidecar schema, the Gate 0 spikes, the shared fixtures | nothing |
| [GE-2](git-epochs/ge-02-rust-logic-probe-controller.md) | The Rust crate: pure logic, git probe, gate and quiescence controller | GE-1 |
| [GE-3](git-epochs/ge-03-hub-integration.md) | Hub integration: gate in `sync_all`, registry through the sidecar, arrival, flips, unlist and relist | GE-2 |
| [GE-4](git-epochs/ge-04-hub-surface.md) | Hub surface: the `--git` flag, the status route, the startup warning, a test through the binary | GE-3 |
| [GE-5](git-epochs/ge-05-ts-logic-probe-controller.md) | The TypeScript package: pure logic, probe, controller | GE-1 |
| [GE-6](git-epochs/ge-06-swap-module.md) | The swap module, the notice aggregator, the moved message type | GE-1 |
| [GE-7](git-epochs/ge-07-client-integration.md) | Browser clients: `quarto-sync-client`, consumers, indicator, MCP, changelog | GE-6 |
| [GE-8](git-epochs/ge-08-extension-foundation.md) | Extension foundation: workspace, ignore fixes, vscode-free core, headless peer, Playwright harness | GE-1 |
| [GE-9](git-epochs/ge-09-extension-follower-modes.md) | Mirror and editor-only modes | GE-5, GE-6, GE-8 |
| [GE-10](git-epochs/ge-10-extension-clone-mode.md) | Clone mode: attach, controller, arrival, detach, startup, rebind | GE-9 |
| [GE-11](git-epochs/ge-11-join-command.md) | The Join session command | GE-10 |
| [GE-12](git-epochs/ge-12-origination.md) | Share this folder | GE-10 |
| [GE-13](git-epochs/ge-13-hub-e2e.md) | Hub e2e with browser guests | GE-4, GE-7 |
| [GE-14](git-epochs/ge-14-cross-client-e2e.md) | Cross-client e2e: both demos | GE-11, GE-12, GE-13 |
| [GE-15](git-epochs/ge-15-windows-run-a.md) | Windows run A: the Rust and hub items | GE-4 |
| [GE-16](git-epochs/ge-16-windows-run-b.md) | Windows run B: the TypeScript and extension items | GE-11, GE-12, GE-15 |
| [GE-17](git-epochs/ge-17-windows-run-c.md) | Windows run C: the scripted scenario and the Playwright subset | GE-14, GE-15, GE-16 |
| [GE-18](git-epochs/ge-18-docs-and-closeout.md) | Docs and closeout | GE-17 |

GE-16 waits for GE-15 because it extends the tooling branch GE-15 creates. If a Windows run finds a defect, a fix
plan is inserted after that run; the fix plans cannot be listed in advance.

**Waves** (plans in a wave can run in parallel):

| Wave | Plans |
|---|---|
| 0 | GE-1 |
| 1 | GE-2, GE-5, GE-6, GE-8 |
| 2 | GE-3, GE-7, GE-9 |
| 3 | GE-4, GE-10 |
| 4 | GE-11, GE-12, GE-13, GE-15 |
| 5 | GE-14, GE-16 |
| 6 | GE-17 |
| 7 | GE-18 |

The critical path is GE-1, GE-5, GE-9, GE-10, GE-11, GE-14, GE-17, GE-18. Four lanes open after GE-1: Rust (GE-2 to
GE-4), TypeScript logic (GE-5), the swap and browser clients (GE-6, GE-7), and the extension foundation (GE-8).

The edges, for tools:

```text
GE-1  -> GE-2, GE-5, GE-6, GE-8
GE-2  -> GE-3
GE-3  -> GE-4
GE-4  -> GE-13, GE-15
GE-5  -> GE-9
GE-6  -> GE-7, GE-9
GE-7  -> GE-13
GE-8  -> GE-9
GE-9  -> GE-10
GE-10 -> GE-11, GE-12
GE-11 -> GE-14, GE-16
GE-12 -> GE-14, GE-16
GE-13 -> GE-14
GE-14 -> GE-17
GE-15 -> GE-16, GE-17
GE-16 -> GE-17
GE-17 -> GE-18
```

## Running the epic

- **Branches.** Work in a worktree. For a plan, `git switch -c ge/NN-slug feature/git-epochs`, where `NN` is the
  two-digit plan number. There is no braid strand for plan work (see Held items). When a plan is done, rebase onto
  `feature/git-epochs` and fast-forward it in. History on the integration line is linear: no merge commits.
- **No pushes.** Nothing goes to the remote until the whole epic is done and Gordon agrees, except the temporary
  `windows/` branch the Windows plans push after Gordon says go. Never use a silent push; check the remote tip after.
- **Test first.** Every plan starts with its test specifications and fixtures, runs them to see them fail for the
  right reason, then implements. Rust plans gate each task on `cargo clippy -p <crate> --all-targets -- -D warnings`
  and `cargo nextest run -p <crate>`; TypeScript plans on the package\'s vitest suite and `npm run typecheck`.
  `cargo xtask lint` runs at each plan\'s end. `.claude/rules/` applies: integration tests live in
  `tests/integration/` behind one `main.rs`; platform gates follow `cross-platform.md`; hub-client tests follow
  `hub-client-tests.md`.
- **End to end.** A plan that changes anything a user can run is not done until it has been exercised through the
  real binary, the output inspected, and the invocation and a snippet recorded in the plan\'s "Notes from the run"
  (`AGENTS.md`). A plan that cannot do so says so.
- **Rebuild chains.** Changes to `quarto-sync-client`, `quarto-hub-mcp`, or the schema package need
  `cargo xtask build-hub-mcp-bundle`; changes that reach the WASM need the preview-SPA chain
  (`cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`); docs changes
  need `cargo xtask build-agents-docs` (`AGENTS.md`).
- **Snapshots.** A plan that adds or changes snapshot files reports the count and the reason in its commit message.
- **Plan files.** Each plan keeps its checklist current (`- [x]`) and fills "Notes from the run" with what ran, what
  did not, and surprises. Render-check any plan or note you change: `q2 render --strict <file.md>`.
- **Gate 0 failures.** If a Gate 0 check in GE-1 fails, stop and bring it to Gordon. Do not degrade silently.
- **Spelling and prose.** American spelling. Notes are qmd: follow `claude-notes/instructions/writing-notes.md`.
- **Same-wave plans share files.** Plans in one wave touch the same files (GE-5 and GE-6 both add to the TypeScript
  package; GE-7 and GE-8 both touch `quarto-sync-client` and `quarto-hub-mcp`). They resolve conflicts when they rebase;
  shared files are not dependencies. Rebuild every embedded artifact (the MCP bundle, the preview SPA and WASM, the
  agents docs) at GE-18, because a plan that finished early may have left one stale.
- **Base.** `feature/git-epochs` was cut from `origin/main` at `1d0042cb2`, and `main` has moved since (17 commits on
  2026-11-18). Rebase onto `main` before GE-1 starts (linear history), and re-find the file and line numbers cited in
  these plans: they are as of the base commit.
- **Auditing the plans.** After editing plans, check that no plan relies on another that is not in its dependency
  closure: compute each plan\'s transitive `depends-on` and read every mention of a plan outside it (forward references
  in "Unblocks" and "Out of scope" are fine). The review that produced these plans found one hidden coupling this way,
  a cross-implementation check that said "whichever plan finishes second"; it now lives in GE-14.

## Gate 0

GE-1 owns these checks. Each is a scratch experiment whose result goes in GE-1\'s notes; none ships.

1. A Node client using the extension\'s own adapter connects to a preview hub\'s `/ws`, directly and through a local
   `q2 preview --join` proxy, and exchanges docs in both directions under the announce-off share policy: a doc the
   extension creates while connected reaches the hub and the hub can attach it; a hub-created doc is fetchable by id;
   the sidecar travels both ways.
2. The negative case: a doc the extension creates while disconnected is unavailable to the hub; the 5 s timeout rule
   handles it. The adapter reconnects after a proxy restart.
3. `/health` returns the index doc id and needs no auth under preview.
4. `q2 preview --hub-url wss://sync.automerge.org --project <id>` loads the index and files (this covers TLS, since
   earlier tests used `ws://127.0.0.1`), and a pointer flip to a doc created later arrives in the browser through the
   relay.
5. The Rust `IndexDocument` preserves unknown keys on every access path. (The TypeScript `migrateIndexDocument` leaves
   a newer-version doc untouched; verified.)
6. Concurrent put and delete of a registry key in Automerge: a deterministic winner, and a delete does not remove a
   concurrent put.
7. Reflog subjects for `cherry-pick` and `revert` commits, the `sequencer/` and `BISECT_LOG` markers, and the
   ordering of lock, files, and ref for `reset --hard`, `pull`, and `rebase` (the sub-agent measured these; they are
   not yet reproduced).
8. JS doc forking in automerge-repo `2.6.0-alpha.5` (there is no `repo.clone` by a text search of its types), and that
   a fork keeps history and syncs.
9. A registry built from a large repo (thousands of tracked files): load time and size.
10. The extension host: `git` is on `PATH` where the extension runs, including under remote development.
11. Whether `q2 preview` without `--allow-edit` still renders a clone peer\'s doc edits in the browser (the preview
    may render from docs or from disk).

## Windows runs

Windows is not in CI and the aws-lc crates block a cross-check from macOS, so Gordon runs these on a Windows machine.
The model is workspace-9\'s branch `rfu/6-windows-tooling`: a temporary tooling branch (never lands; the final commit
deletes the tooling), a PowerShell run script of independent items, an instructions page, a failure extractor, and
results committed back to the branch as JSON and a summary. The remote branch is `windows/git-epochs` (pushed with an
explicit refspec after Gordon says go). Each run covers only the new tests, and the items are specific:

- **Git probe on Git for Windows:** `rev-parse`, `--git-path` (absolute `C:/…` paths, linked worktrees), `ls-tree -z`
  with a prefix, symlink filtering, `cat-file --batch`, reflog parsing.
- **The gate during a real `git switch`** of a generated tree of a few thousand files: no sync while a marker exists;
  a sharing-violation read error is a skip, never a delete.
- **`core.autocrlf=true`** with `\r\n` fixtures through the content rule and arrival.
- **Paths and keys:** registry keys use `/`; case-insensitive collisions; drive letters and UNC prefixes; nested
  `.quarto-hub` and `.git` segments; long paths with `core.longpaths`.
- **Hub gate and checkpoint integration** with a real temp repo in `%TEMP%`, including the directory-rename rule in
  `cross-platform.md`.
- **The TypeScript core against real repos** with `git.exe`, jj skipped when absent.
- **The Join command\'s `q2` spawn:** process-tree kill on Leave, the stderr path, the readiness poll, against a stub
  `q2.cmd`; a real `--share` and `--join` pair if the network allows.
- **The permanent Playwright VS Code subset.**
- **A scripted end-to-end scenario:** a `--git track` hub, a headless peer, a `git switch`.
- **Native clippy and nextest** for the epic\'s crates.

Run A is GE-15 (Rust and hub, after GE-4), run B is GE-16 (TypeScript and extension, after the extension plans), and
run C is GE-17 (the scenario and the Playwright subset, after the e2e plans).

## Facts checked

These were read in the code during the review, with the locations as of 2026-11-18.

- `IndexDocument::add_file` overwrites an existing key with `tx.put` (`crates/quarto-hub/src/index.rs:169-185`).
- "No checkpoint means never on disk" is the stated invariant for `WriteBack` (`crates/quarto-hub/src/sync.rs:649-670`).
- The shutdown flush, the periodic loop, and the initial sync all call `sync_all`
  (`crates/quarto-hub/src/server.rs:2244-2250`, `:2278-2281`; `crates/quarto-hub/src/context.rs:420`).
- `syncWithFiles` compares path sets and ignores a changed docId for an existing path
  (`ts-packages/quarto-sync-client/src/client.ts:1393-1440`); `recoverFileDocument` is the precedent for swapping a
  handle in place (`:1086`); `createNewProject` builds the index inline and then one doc per file (`:2443`).
- The extension\'s `syncIndex` untracks, retracks, and writes a changed docId to disk with no gating
  (`vscode-sync-experiment/src/project.ts:322-330`); `isIgnored` checks the first path segment only (`:67-70`);
  `relOf` and `listDiskFiles` already normalize to `/` (`:214`, `:267`); `createFromDisk` creates file docs
  (`:383-400`); `createProjectFolder` writes a mirror (`:109-155`); the folder is placed inside the workspace folder
  (`vscode-sync-experiment/src/extension.ts:96-112`); the sync server is a constant
  (`extension.ts:26`, `project.ts:40`) though `ProjectSync` dials `meta.syncServer` (`project.ts:183`).
- Every buffer edit becomes a splice with no check of the pre-edit text, and `applyRemote` rewrites the buffer from
  the doc (`vscode-sync-experiment/src/binding.ts:54-96`).
- `/ws` takes no credential and does no Origin check when auth is not configured, and the preview sets no auth
  (`crates/quarto-hub/src/server.rs:1646`, `crates/quarto-preview/src/lib.rs:735`). `/health` returns
  `index_document_id` and an optional `project_root` (`server.rs:47-53`).
- The hub\'s share policy is read-write with announce off, which also stops the hub from requesting a doc it lacks;
  an inbound sync for an unknown doc is still applied (`crates/quarto-hub/src/context.rs:216`,
  `claude-notes/plans/2026-10-26-samod-0-16-share-policy.md`).
- `upstream.rs` makes no HTTP calls: the `--hub-url` mirror uses only the Automerge protocol after the dial
  (`crates/quarto-hub/src/upstream.rs`); `peer.rs` already dials external sync servers.
- `migrateIndexDocument` leaves a document with a newer version untouched
  (`ts-packages/quarto-automerge-schema/src/index.ts:107-133`); the Rust hub reads and writes the index by named key.
- `git hash-object --path` ran a configured clean filter, and `commit (merge)` is a reflog subject (both reproduced
  in the review). The `q2preview…` ticket carries the host\'s endpoint address and a 32-byte session token
  (`crates/quarto-p2p/src/ticket.rs`).
- The extension has three commands (`quartoHub.connect`, `quartoHub.disconnect`, `quartoHub.createFolder`) and one
  setting (`quartoHub.userName`); its README says "pick a new folder" while the code does not prompt.
- `q2 preview --hub-url <ws> --project <id or share URL>` mirrors a project from an existing sync server. It was merged
  as a spike (strand bd-yq3awhmw, `claude-notes/plans/2026-11-13-preview-against-remote-hub.md`); it implies standalone
  mode, conflicts with a path, `--no-project`, `--join`, `--static`, `--share`, `--to`, `-M`, and `--profile`, and gives
  its browser clients read-only access unless `--allow-edit` is set. It was tested only against `q2 hub --project <dir>`.
- `q2 preview --join` runs a local proxy that serves matching UI assets from its own binary and tunnels `/ws`,
  `/api/*`, `/auth/*`, and `/health` (`crates/quarto-preview/src/join_frontend.rs`); `--port`, `--host`, and
  `--no-browser` apply (`crates/quarto/src/main.rs`).
- When the Rust hub creates an index it writes only `files` (`crates/quarto-hub/src/index.rs:467`); TypeScript clients
  migrate it later. GE-1 reads how the version gets set and makes creation write the full shape.
- "Standalone mode" in the hub means no project directory at all, a pure sync server
  (`crates/quarto-hub/src/context.rs:228`). The 2026-11-17 plan called it "a long-lived sync server on a project
  directory", which is wrong; do not repeat it.
- Preview storage is ephemeral by default ("per-session data dir by default",
  `crates/quarto-preview/src/lib.rs:661`), which is why a bound folder goes stale between runs (G19).

**Not checked** (each is a Gate 0 item above): hub and extension doc exchange under announce-off; preview `/ws` with
the extension\'s adapter; `--hub-url` against the public server and a flip through the relay; Rust unknown-key
preservation on every path; registry put and delete semantics; cherry-pick and revert reflog subjects; the
`sequencer/` and `BISECT_LOG` markers; the lock and ref ordering of `reset`, `pull`, and `rebase`; JS forking; the
retention of docs on the public server, and whether a new `q2 preview` run gets a new index doc id (inferred from the
ephemeral data directory).

## Why these decisions

The decision record says what was decided; this says why, and what was rejected, so a reader does not reopen it by
accident.

- **Blob id, not commit hash, is the version key.** Commit hashes change under amend, rebase, and jj (`jj new` makes a
  commit with an identical tree), and a commit does not rewrite the working tree, so rotating on commit would disrupt
  every guest for nothing.
- **The content rule leads the classifier.** The reflog misses `commit (merge)` (reproduced), cherry-pick commits, and
  every colocated jj operation, and says nothing about `git reset --hard HEAD`, which moves neither HEAD nor blob.
  "Does disk equal what was last synced" is true in every case; the reflog only tells a plain commit from a rewrite
  when disk differs.
- **A commit moves the doc forward and removes the old entry,** instead of adding an alias. With an alias,
  `git checkout <old commit>` would join the doc whose text has moved on and overwrite the working tree.
- **Canonical is the lowest `(generation, docId)`,** not the smallest docId. With docId alone, a fresh doc could
  displace a doc people had edited for an hour, decided by random bytes.
- **The registry is a sidecar, not part of the index.** The index docId is the share identity and cannot be replaced,
  and every commit adds and deletes entries in a doc whose history never shrinks.
- **Peers attach by blob.** People on different branches must work at once, and people on one version must collaborate
  live. Keeping a detached peer bound to the owner\'s doc would push its edits and overwrite its own buffer.
- **Binary files participate.** Otherwise one peer\'s checkout replaces an image doc and every clone on another branch
  gets that image written over a tracked file.
- **Followers follow the pointer; clone peers ignore it for tracked files.** Browsers see the owner\'s version, and a
  clone\'s working tree is never written from another branch.
- **Unlist and relist (reversing D9).** Otherwise followers\' previews and file trees keep files the owner\'s checkout
  lacks.
- **The gate lives inside `sync_all` and `sync_file`.** The shutdown flush and the initial sync call `sync_all` outside
  the server loops and would merge a half-written tree.
- **Editor-only needs the gate.** A checkout makes VS Code reload buffers, and the Binding would send the reload to the
  old doc as an edit. The mirror check cannot catch it, because the buffer equals the doc until the reload.
- **The mode is explicit in `project.json`.** A mirror nested in a git repo that someone `git add`s would otherwise turn
  into a clone of the wrong repo and silently split off. Mirrors are always folders the extension created, so mirror
  mode can never overwrite a user\'s own clone.
- **Attach needs the repo identity.** Without root commits, any repo containing an `index.qmd` would pass and silently
  split off.
- **Ownership is static.** No election and no heartbeats, and a hub-created project cannot gain a second writer.
- **The hub joins the shared registry.** Extension peers and the hub collaborate on a version, and there is one arrival
  rule instead of two.
- **Two implementations, not WASM.** The extension loads no q2 WASM today (the render module is about 36 MB) and the
  logic is small; fixtures and golden docs hold the two together.
- **The hub is not special as a sync server.** The sync endpoint is samod; only the docs are q2 content. A VS Code
  originator can publish to any Automerge sync server, and `q2 preview --hub-url` can show it with no hub-client change.
- **No `quartoHub.git` setting.** A kill switch would reintroduce the original bug in one click.
- **No "share mine".** A second doc under the current blob can never be canonical, so nobody could join it.
- **Rejected:** pausing guests read-only during a checkout (an indicator only, G33); a hub-written marker file for
  co-location discovery (deferred); the extension electing a pointer owner; a synced `epochs` map inside the index;
  hub-client `server=` for the hub-less demo (it uses `--hub-url`); a read-only mirror in a git repo.

## Background a new agent will not have

Learned in the review session; none of it is in `AGENTS.md` or the earlier plan.

- **The extension is a prototype.** Elliot wrote it (three one-line commits in October 2026, moved to the repo root in
  `251de0df0`, dropped from the workspaces in `8ae461f1b`). It has no tests, no CI, a stale README, and `tsc` fails
  without the vscode types. It syncs every file on disk except an ignore list, while the hub publishes a curated set
  (`.qmd`, `.md`, config, binary resources, `_extensions/**`, `.tsx`, `.scss`); a project originated from VS Code
  can therefore differ from a hub-created one. Elliot also wrote the experimental branch service; shikokuchuo maintains
  it and the attribution code. Gordon loops them in out of band.
- **Mirrors already exist.** "Create Folder from Project" makes a new folder inside the open workspace folder (home if
  none), from an index doc id. It is not a clone and there is no git in it. This epic keeps it as mirror mode.
- **The hub cannot pull.** With announce off it never requests a doc from a peer, so a doc a peer creates reaches the
  hub only if the peer pushes it while connected. Attach to an unreachable doc is the 5 s timeout rule, not an error.
- **`/health` reveals the host\'s `project_root`,** a path on the host\'s machine, even through a `--join` proxy. The
  Join command compares it with the open folder for every kind of join and always confirms, because the same path can
  exist on two machines.
- **The `q2 preview --join` ticket is a capability** (the host\'s endpoint address and a 32-byte session token); the
  extension passes it in argv, which other local users can see. Accepted at demo quality.
- **Gordon\'s scope moved several times** (mirrors are fine for git shares, origination is in scope, the hub and the
  extension work together). Plans must not block what he might add next; the three decisions above are why origination
  is a plan and not a deferral.
- **How he wants to work:** he decides pushes and the timing of strands; he wants a recommendation with each decision
  and patient explanations of anything unfamiliar; he reviews the roughly 80 "confirm" marks later; the Windows machine
  is his, and he runs it from a branch with the `windows/` prefix; held items are reviewed at the end.
- **Review lessons.** Two independent reviewers found real defects (the inert Binding contradiction, the late reload
  race, `off` peers overwritten, the shutdown flush) and some overstated claims (a "blocking" handle-caching finding
  was important at most, and several timings were not reproduced). Verify a sub-agent claim against the code before
  acting on it. Forks inherit the conversation but not the reasoning, so a record like this file is what keeps them
  consistent; tell a fork explicitly not to spawn sub-agents.
- **Tool quirks in this environment:** a single-file `q2 render --strict` prints about 56 `Q-12-13` listing errors
  about other plans and exits 1 (filter them with `grep -v Q-12-13` and read the rest); the shell is zsh, so quote a
  glob passed to `grep --include`; `git show REV:path` mangled a path in the tool\'s output once, so use
  `git grep PATTERN REV -- path`; a local `git clone --depth` is ignored unless the source is a `file://` URL.
- **Plans are not listed on the notes site.** `claude-notes/plans/index.md` lists top-level files only, so this
  epic\'s plans are reached through the plan table above.

## Details chosen while writing the plans

The plans were written from this record. Where a detail was not decided they chose the simplest option that fits and
marked it `(detail chosen by the plan author; confirm)`; there are about 80 such marks, found with
`grep -rn "confirm)" claude-notes/plans/git-epochs/`. Gordon reviews them before the plans run. The ones that change
what a user sees or what the system does:

- **Editor-only and the gate.** While the gate is closed an editor-only folder drops edits typed into bound buffers
  (they are not sent to the old doc). After the quiet window it waits for the hub\'s flip and rebinds, so characters
  typed during a git operation are not kept (GE-9).
- **`--git track` with `--no-project`** logs one line and runs without git logic (GE-4).
- **Join input.** An `http(s)` address becomes the `ws(s)` URL with path `/ws`; a loopback `project_root` match hides
  attach; a non-loopback URL is treated like a ticket (GE-11).
- **Origination.** The share link\'s `name` and `file`, and a 30 s wait for the server to confirm the heads (GE-12).
- **The Playwright launch method** is `@vscode/test-electron` plus `_electron.launch`, and a test-only
  `QUARTO_HUB_TEST_SYNC_SERVER` seam lets the Create Folder smoke test avoid the public server until GE-11 replaces it
  (GE-8).
- **The permanent e2e list** is the scenarios H1 to H3, H6, H7, L1, L2, and L4 that GE-14 names; the permanent hub
  tests run without `--share`.
- **The sidecar** pointer key is proposed as `epochs`, and its schema is path to blob to docId to generation (an
  integer) as a set union (GE-1).
- **The extension\'s event log** (`.quarto-hub/events.jsonl`) gains `flipped` (GE-9) and `switched`, `detached`, `reattached`, and `held` (GE-10); the earlier plan listed the last four.
- **The Windows tooling branch** is refreshed by merging `feature/git-epochs` into it, never by rebasing a pushed
  branch (GE-15).

## Held items

Reviewed when the epic is complete, not before. Under Gordon\'s rules a braid strand is for work outside every
active plan, so none is opened now.

1. Ownership transfer: an explicit command to make a folder the owner when the owner is gone.
2. Auth for the extension and authenticated hubs (a Bearer sign-in like hub-mcp\'s).
3. A long-lived hub on a project directory: registry persistence, startup against a persisted record, auth.
4. Sidecar rolling and pruning of retired docs, including Sedimentree compaction.
5. A viewer for retired versions and their attribution.
6. Blame-seeded attribution.
7. Attribution parity for the extension: the author on every change.
8. Files rewritten without a HEAD move: `git stash`, `git restore`, `git checkout -- file`, `jj restore`.
9. Non-colocated jj, by querying jj with `--ignore-working-copy`.
10. The orphaned-branch UI of the branch service.
11. A project-wide presence roster showing who is on which version of which file.

Settled and out of scope: single-file mode following the index, extension distribution and publishing, a real VS Code
extension-host e2e beyond the Playwright subset, and the strands the `--hub-url` plan already tracks (hiding the
local-disk routes, the editor banner wording, the `/health` file count).

## Risks

- **GE-2 and GE-5 are large.** Each merges logic, probe, and controller. A plan can split without changing the shape:
  a split adds one edge to its own chain.
- **Registry growth.** One sidecar doc with permanent history; the Gate 0 measurement on a large repo bounds it.
- **A flip through the `--hub-url` relay is untested.** If it fails, the hub-less demo changes (Gate 0 item 4).
- **Two implementations can drift.** The shared fixtures, the golden docs, and the schema spec are the mitigation.
- **Windows behavior is unmeasured** until the runs; some findings will add fix plans.
- **The public server has no auth.** Origination warns, and the demos use throwaway content.
- **CI will not gate the browser e2e at first.** The `q2-preview-spa` e2e workflow is a trial whose legs are
  `continue-on-error`, and GE-14\'s new workflow for the VS Code tests starts the same way. Gordon decides when they
  gate; until then `cargo xtask verify` and the plans\' own runs are the check.
- **The permanent hub tests cannot use `--share`,** which needs the network. The ticket path is covered by the manual
  transcript and the Windows runs.
- **Retention grows.** Every doc version a peer has been on is kept, including each binary version, and a fork for the
  rename invariant copies full history. There is no cap at demo scale; the sidecar rolling strand is held.
- **Plans in a wave conflict on shared files** (the TypeScript package index, `package.json`); they resolve it at rebase.

## Closeout

GE-18 runs the final composition review (two agents on the finished work, as in the review that started this epic),
reviews the held items, and prepares the pull request. Gordon decides when to push.
