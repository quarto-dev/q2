---
title: 'Git-aware live share: epoch switching on checkout (`--git track`)'
date: 2026-11-17
description: 'Makes q2 preview live share resilient to git checkouts, rebases, and commits by switching a file to a fresh Automerge doc when git rewrites it, keeping the old doc as a safety net, and telling guests what happened.'
status: draft  # design settled in conversation 2026-11-17; implementation not started
---

**Branch:** `feature/git-epochs` (from `origin/main` at `1d0042cb2`), worktree `workspace-6`.
No braid strand: this is plan work. Follow-ups that fall outside the plan are listed under "Out of scope".

## Overview

`q2 preview` live share syncs project files between the host\'s disk and
Automerge docs (`crates/quarto-hub/src/sync.rs`). Nothing in the hub or the
preview knows about git. When the host runs `git checkout`, `git switch`,
`git pull`, or `git rebase`, the watcher sees ordinary file changes and
`sync_document` merges them into the live docs as if the host had typed them.
Two things go wrong:

- **Noise.** The checkout is attributed to an author and appears in history.
- **Danger.** Guests\' concurrent edits are three-way merged into text that
  git just rewrote wholesale. The merge places their edits in arbitrary spots.

The fix is a new experimental mode, `q2 preview --git track`. When git
rewrites a tracked file, the hub switches that file to a fresh Automerge doc
seeded from disk, keeps the old doc, and tells connected clients. A commit
never disrupts guests: the same doc is re-keyed under the new blob id.

**Scope.** Hub-synced `q2 preview` live share inside a git repository or a
colocated jj repository, as a demo and experiment. `quarto hub` (the
long-lived sync server) is out of scope for now.

**Vocabulary.** An *epoch* is the span during which one doc stands for one
file. A *switch* replaces the doc a path points to. A *re-key* registers the
same doc under a new blob id. *Stranded edits* are edits that land in a doc
that is no longer the live one for its path.

## Decisions

All of these were settled with Gordon in the design session of 2026-11-17.

| Id | Decision |
|---|---|
| D1 | Goal is resilience to branch switches and rewrites, not history truncation. Commits must not disrupt guests. |
| D2 | Trigger is a HEAD change, detected globally with git subprocesses. Disk is always the authority. |
| D3 | The epoch key is the git **blob id** of the file at HEAD, not the commit hash. Commit hashes churn under amend, rebase, and jj; blob ids are content-addressed. |
| D4 | The registry is in memory and hub-local: `path` to `blob id` to a list of docIds, newest last. No index schema change. Clients learn of a switch only from `files[path]` changing. |
| D5 | Commit-type HEAD change (git did not touch the file): **re-key** the same doc under the new blob. No swap. |
| D6 | Checkout-type HEAD change (git rewrote the file): **switch**. Seed a fresh doc from disk, or **fork on return** if the new blob is already in the registry. |
| D7 | Fork on return: copy the newest registered doc for that blob (new docId, shared history), then overwrite its text with disk as one change on current heads. Never reuse the old docId. |
| D8 | Participation: tracked text files only. Untracked and ignored files, files outside the repo, and binary docs behave as today. Captures follow their file (mark stale on a switch). |
| D9 | Added at the new HEAD: create a fresh seeded doc. Deleted at the new HEAD: keep the index entry as today; the registry keeps the doc. |
| D10 | Classifier for git is the HEAD reflog subject (`commit`, `commit (amend)`, `commit (initial)` mean commit-type; anything else is checkout-type). Fallback for jj and repos without a reflog: disk equals the last synced content means commit-type, otherwise checkout-type. |
| D11 | Decide on the first quiet tick after the gate lifts: gate open, no watcher events for 1 s, HEAD stable. Cap the wait at 30 s. Files whose blob changed are held out of ordinary sync from the HEAD change until the decision. |
| D12 | Pause sync in both directions while an operation is in progress: `index.lock`, `rebase-merge`, `rebase-apply`, `MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, and jj\'s `.jj/working_copy/working_copy.lock`. Resume after 60 s with a warning if a marker is stale. Show a visible "paused for git" indicator. |
| D13 | Colocated jj is supported. Non-colocated jj stays off. An unresolvable HEAD means no change; keep re-checking each tick. |
| D14 | On any failure of a switch, log it and fall back to ordinary sync for that file. |
| D15 | Client swap: freeze edits while the new doc loads, route stranded edits to the old doc, announce with `onFileReplaced`. Edits carry the docId they were made against. |
| D16 | Branch service (experimental, default off): reset the active branch to main on a swap; branch edits are docId-bound; orphaned branches are noted in the announcement. No rebase or orphan UI. |
| D17 | Attribution: the seed is lump-attributed and unlabeled. Unchanged text keeps authorship only through forks. |
| D18 | Opt-in flag `--git track` or `--git ignore`, a value flag that requires a value, default `ignore`. The default can flip to `track` later by changing one constant. |
| D19 | Startup: record HEAD and register each tracked file\'s doc under its blob; assume the tree matches HEAD. |
| D20 | Timing: existing 5 s tick, 1 s quiet window, 30 s decision cap, 60 s pause timeout, 5 s client freeze timeout. |
| D21 | If a Gate 0 check fails, stop and bring it to Gordon. Do not degrade silently. |
| D22 | Demo deliverable: an automated end-to-end test with real git and two clients, plus the recorded transcript in this plan. |

### Rejected alternatives

- **Commit hash as the key, or a doc per file per commit.** Commit hashes are unstable under amend, rebase, and jj (every `jj new` made a new commit with an identical tree). A commit also does not rewrite the working tree, so rotating on commit disrupts guests for no benefit.
- **A per-commit index document.** The index docId is the share identity (`/health`, join ticket), so every guest would have to follow a moving pointer.
- **Reusing the old docId on return.** A late sync from a guest\'s stranded edit lands in the live text and is written to disk (tested, see Evidence).
- **A sidecar `epochs` map in the index.** Clients do not need it, preview data is ephemeral, and it would grow with every checkout. Promote it to the index only when a client needs it.
- **Re-keying branches onto a new doc.** Merging a branch clone into an unrelated-history doc conflicts on the text key and picks a winner arbitrarily (tested, see Evidence).

## Evidence

These were checked during the design session. Scratch scripts were not kept;
the tests in Phase 1 to 3 re-establish each fact in the repository.

- **Ordering of HEAD, locks, and file writes** (4,000 files, one run each, git 2.55.0 and jj 0.47.1):

  | | `git switch` | colocated `jj edit` |
  |---|---|---|
  | Lock held | `.git/index.lock`, 17 to 690 ms | `.jj/working_copy/working_copy.lock`, 40 to 884 ms; `.git/index.lock` only 1.5 ms |
  | Files rewritten | 165 to 589 ms | 42 to 736 ms |
  | `.git/HEAD` changes | 691 ms (last) | 21 ms (first) |

  Git moves HEAD last; jj moves it first. Hence D11: do not classify when HEAD moves. Untested: `reset --hard`, `pull`, `rebase`.
- **Reflog subjects.** Git writes `commit: ...`, `commit (amend): ...`, `checkout: moving from A to B`, `reset: moving to ...`, `rebase (start)`, `rebase (pick)`, `rebase (finish)`. Colocated jj writes `export from jj` for everything.
- **jj HEAD behavior.** In a colocated repo `.git/HEAD` is always detached and is the parent of the working-copy commit. `jj new` and `jj commit` move HEAD with an identical tree. `jj edit` of a commit whose parent is the root left `.git/HEAD` as `ref: refs/jj/root` and `git rev-parse HEAD` failed. `jj restore` rewrites files without moving HEAD, as does `git stash`.
- **Merge of unrelated Automerge histories** (`automerge` 3.5.0). Merging a clone of an old doc into an independently seeded doc reports a conflict on the `text` key. Over 40 runs the branch text won 24 times and the new doc won 16, so the result is arbitrary.
- **Late sync into a resurrected doc.** After overwriting a doc with disk content, a guest\'s earlier stranded edit synced in and appeared in the live text (`GUEST-STRAY` inserted). Forking the doc first kept the copy clean; the late edit landed only in the old doc.
- **Git plumbing.** `git rev-parse --git-path index.lock` and `--git-path rebase-merge` give correct per-worktree paths. `git rev-parse --show-prefix` returns `sub/deep/` in a subdirectory. `git rev-parse --verify -q HEAD` exits 1 on an unborn repo. `git ls-tree -r -z HEAD` took 80 ms for 15,406 entries and includes symlinks (mode 120000), which must be filtered to modes 100644 and 100755.
- **Code facts.** `IndexDocument::add_file` uses `tx.put` and overwrites an existing key (`index.rs:170-183`). `doc.fork()` exists and the hub already uses `fork_at`. The index schema is already V3 (`folders`). `syncWithFiles` (`client.ts:1393`) compares path sets only and ignores a changed docId for an existing path. `recoverFileDocument` (`client.ts:1070`) is a precedent for swapping a handle in place and re-firing `onFileAdded`. `applyBranchEdits` silently returns when it cannot resolve the branch doc. Sync checkpoints are keyed by docId (`sync_state.rs:27`), and under `WriteBack` "no checkpoint" means "never on disk" (`sync.rs:659-670`).
- **Branch feature.** `branchService`, `BranchBar`, and `useDocBranches` are behind the default-off `documentBranches` preference, exist only in hub-client, and have 34 passing unit tests (`branchService.test.ts`, `BranchBar.test.tsx`, `useDocBranches.test.tsx`).

## Design

### Model

For each tracked text file the hub keeps an in-memory registry entry:

```text
registry[path][blob_id] = [doc_id, ...]   // newest last
```

`files[path]` in the index stays the single live pointer. A HEAD change
produces one action per tracked path, from the old and new blob maps plus the
classifier verdict:

| Situation | Action |
|---|---|
| Blob unchanged between HEADs | Nothing, even if the file is dirty. Git carries uncommitted edits across. |
| Blob changed, commit-type | Re-key: append the current doc under the new blob. No index write, no client effect. |
| Blob changed, checkout-type, blob not in registry | Fresh seed from disk, register it, flip `files[path]`. |
| Blob changed, checkout-type, blob in registry | Fork the newest registered doc, overwrite its text with disk, register, flip `files[path]`. |
| Path newly tracked (first commit of an untracked file) | Register the current doc under the new blob. |
| Path added at the new HEAD, no index entry | Create a fresh seeded doc and add the entry. |
| Path deleted at the new HEAD | Leave the entry. The doc stays in the registry. |

The overwrite on return is `update_text` on the fork at its current heads, as
one change. It must not use the fork-at-checkpoint merge in `sync_document`,
which would three-way merge older edits into the rewritten text.

### Detector

1. **Gate.** Each tick, and before each watcher batch, check the in-progress
   markers from D12 using `git rev-parse --git-path` so linked worktrees
   resolve. While any marker exists, skip sync in both directions.
2. **HEAD.** `git rev-parse --verify -q HEAD`. Unresolvable means no change.
3. **Pending.** When HEAD differs from the recorded one, mark the decision
   pending and hold every file whose blob changed out of ordinary sync.
4. **Quiescence.** Decide when the gate is open, no watcher events arrived for
   1 s, and HEAD is stable across two checks. Cap at 30 s.
5. **Blob map.** One `git ls-tree -r -z HEAD` (limited to the project prefix
   from `git rev-parse --show-prefix`), modes 100644 and 100755 only.
6. **Classify.** Read the HEAD reflog entries since the recorded HEAD. If every
   entry is commit-type, the batch is commit-type; otherwise checkout-type. For
   jj and repos without a reflog, use the content rule.
7. **Apply.** Run the actions, then resume ordinary sync for the held files.

Subprocess hygiene: set `GIT_OPTIONAL_LOCKS=0`; use only plumbing that does
not touch the index or run repo-configured programs (`rev-parse`, `ls-tree`,
`reflog`); time out each call; if `git` is missing or refuses the repository,
turn the feature off with one log line. Convert paths with
`quarto_util::to_forward_slashes` on Windows.

### Order of a switch (hub)

1. Create the new doc (fresh seed, or fork then overwrite).
2. Write its sync checkpoint (heads and disk hash). This must precede the
   index flip, because "no checkpoint" means "never on disk" under `WriteBack`
   and would let a deleted file be re-created.
3. Replace `files[path]` in the index (`add_file` overwrites).
4. Mark the capture for that path stale (existing `staleness` field).
5. Fire the same post-sync callback the watcher path uses (`on_file_changed`),
   so the preview re-renders and capture staleness recomputes.
6. Register the doc in the registry.

Any failure falls back to ordinary sync for that file (D14).

### Controller structure

Implement the controller as a state machine driven by explicit ticks and a
`GitProbe` trait (HEAD, markers, reflog, blob map). Production uses a
subprocess probe. Most tests use a fake probe with no sleeps. Proposed
modules in `crates/quarto-hub/src/`: `epoch.rs` (registry, pure decision
function, controller) and `git_probe.rs` (trait and subprocess
implementation). The server loops in `server.rs` (watcher and periodic sync)
consult the controller before syncing a path.

### Client swap (`ts-packages/quarto-sync-client`)

- **Detect.** In `syncWithFiles`, an existing path whose index docId differs
  from `state.fileHandles.get(path).documentId` starts a swap. Treat the repair
  of an unavailable entry separately, as today.
- **Serialize.** One swap at a time per path, latest index state wins. If the
  index flips twice before the first swap completes, skip to the final state.
- **Freeze.** Fire `onFileReplaced(path, {phase: 'pending', oldDocId, newDocId})`.
  Editing surfaces make the file read-only. Path-based writes during the freeze
  still hit the old handle, which is the correct stranded destination.
- **Swap.** When the new doc is loaded, repoint `fileHandles[path]`, detach the
  old change handler, move the old handle into `retiredHandles` (kept until
  disconnect, capped at 8 per path, never `repo.delete`), re-fire `onFileAdded`
  with the new content and docId, then fire `onFileReplaced` with phase `done`.
- **Timeout.** If the new doc is not ready after 5 s, fire phase `unavailable`
  and route the path into the existing unavailable-file machinery
  (`markFileUnavailable` and the retry poll). The path stays read-only.
- **docId-bound writes.** `FilePayload` gains an optional `docId`.
  `applyEditorOperations`, `updateFileContent`, and `applyBranchEdits` gain an
  optional `baseDocId`. A write whose `baseDocId` is stale goes to the retired
  handle for that docId; if none is known it is dropped with a console warning.
  A write with no `baseDocId` behaves as today. Debounced writers (the block
  editor, replay) should pass it.
- **Counter and notice.** Count writes routed to retired docs per path. On the
  editing surfaces only, show one aggregate notice per batch, naming the open
  file when one is open:
  - "This file was replaced because the host\'s checked-out version changed."
  - "\{N\} files were replaced because the host\'s checked-out version changed."
  - Append "\{N\} of your recent edits stayed with the previous version." when
    the counter is nonzero.
  The default read-only viewer shows no notice; `onFileAdded` already
  re-renders it.
- **Presence and replay.** Clear remote cursors for the path when the swap
  completes. Close any replay session on the path. The local Monaco cursor and
  undo history reset, the same accepted trade-off as the recovery path.
- **Consumers.** `ts-packages/preview-runtime/src/automergeSync.ts` (wraps the
  client), `q2-preview-spa/src/PreviewApp.tsx` (block editor under
  `--allow-edit`), hub-client `App.tsx` and `useReplayMode`, `branchService`.
  The MCP `edit` tool is path-based and relies on `expected_hash`; confirm in a
  test that a swap makes it refuse a stale write.
- **MCP bundle.** Changing `quarto-sync-client` requires
  `cargo xtask build-hub-mcp-bundle` before verifying (see `AGENTS.md`).

### CLI (`crates/quarto/src/main.rs`)

```text
--git <track|ignore>    experimental; default ignore
```

Mirror `--ui`: a `PreviewGitArg` value enum in the CLI crate mapping to a
library enum, declared with `value_enum` and `default_value_t`, no
`default_missing_value`, so a bare `--git` is a parse error. Add `git` to the
`conflicts_with_all` lists next to `ui` for `--join`, `--static`, and the
`--hub-url` mirror. Help text says what `track` does, that `ignore` is today\'s
behavior, "(experimental)", and that colocated jj repos work. Parse tests in
the existing `cli_parse_tests` module, following the `preview_join_conflicts_with_*`
pattern.

### Attribution and branches (hand-off notes)

- A switched doc is lump-attributed to the hub actor with no author footer.
  Text that survives a fork keeps its original authors. Returning to the latest
  blob a doc was registered under keeps nearly all authorship; returning to an
  earlier blob mostly yields lump-attributed text.
- The reactji "remove my contribution" behavior (see
  `claude-notes/plans/2026-05-25-reactji-authorship-q2-preview.md`) will not
  recognize contributions that predate a switch. Record this as a known
  limitation.
- The branch service changes are additive and keep the public API. The owners
  (elliot wrote the feature; shikokuchuo maintains it and the attribution code)
  are looped in by Gordon out of band.

## Checklist

Gate each task on `cargo clippy -p <crate> --all-targets -- -D warnings` and
`cargo nextest run -p <crate>` for Rust, and the relevant vitest suites for
TypeScript. Cross-check `quarto-hub` with
`cargo clippy -p quarto-hub --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
once the git probe lands. Do not push without Gordon\'s permission.

### Phase 0: Gate 0 verification (stop and ask Gordon on any failure)

- [ ] Samod accepts a forked Automerge doc in `repo.create` and the fork keeps its history.
- [ ] A guest\'s change to a doc that is no longer in `files` still syncs to the host under the hub share policy (`HubSharePolicy`, `announce` off).
- [ ] An existing preview end-to-end harness can drive two clients and a real git checkout (or list what is missing).
- [ ] Confirm `IndexDocument::add_file` replaces an existing entry (read: yes; add a unit test).

### Phase 1: decision function (tests first)

- [ ] Table-driven tests for the pure decision function: blob unchanged (clean and dirty), commit-type re-key, checkout-type fresh seed, checkout-type fork on return, newly tracked, added, deleted, untracked, partial staging (`git add -p`), read-only and write-back policy.
- [ ] Tests for the classifier: reflog subjects, a batch containing a non-commit entry, the content fallback for jj, a save-then-commit window.
- [ ] Implement the decision function and classifier in `epoch.rs`.

### Phase 2: git probe

- [ ] Fake-probe tests for the controller state machine: gate, pending, hold, quiescence, 30 s cap, 60 s pause timeout, unresolvable HEAD, resume.
- [ ] Real-repo tests with temp git repos: HEAD, `ls-tree` with prefix, linked worktree marker paths, unborn HEAD, symlink and submodule filtering, rebase marker.
- [ ] Real-repo tests with colocated jj, skipped when `jj` is not installed.
- [ ] Implement `git_probe.rs` and the controller.

### Phase 3: hub integration

- [ ] Tests with real Automerge docs: fresh seed, fork on return with overwrite, checkpoint written before the index flip, `files[path]` replaced, capture marked stale, callback fired, ordinary sync skips held files, sync pauses behind the gate, in both `ReadOnly` and `WriteBack`.
- [ ] Regression test: after a fork, a late stranded edit synced into the old doc does not change the new doc.
- [ ] Failure injection: a failed switch falls back to ordinary sync.
- [ ] Startup registration of every tracked file under its blob.
- [ ] Wire the controller into the watcher and periodic loops in `server.rs`, behind the new config option.

### Phase 4: CLI flag

- [ ] Parse tests: default is `ignore`; `--git track` and `--git ignore` parse; bare `--git` errors; an invalid value errors; conflicts with `--join`, `--static`, and the `--hub-url` mirror.
- [ ] Add `PreviewGitArg`, the library enum, the flag, and the help text; thread the config into `quarto-preview`.

### Phase 5: client swap

- [ ] Vitest for the swap with a fake repo: phases pending then done, per-path serialization and coalescing, retired routing by `baseDocId`, the stranded counter, the 5 s unavailable timeout, the retired-handle cap, an index flip back to a known docId.
- [ ] `FilePayload.docId`, the `baseDocId` parameters, and `onFileReplaced`.
- [ ] Branch service: reset active branch to main on a swap, `applyBranchEdits` takes `baseDocId` and no longer drops silently; keep the existing 34 tests green and add flag-on tests.
- [ ] Presence clear and replay close on swap.

### Phase 6: consumers

- [ ] `preview-runtime` passes through `docId`, `baseDocId`, and `onFileReplaced`.
- [ ] Preview SPA (block editor under `--allow-edit`) and hub-client (`App.tsx`, `useReplayMode`) pass `baseDocId`, freeze on `pending`, and show the aggregate notice.
- [ ] A test that the MCP `edit` tool refuses a stale write after a swap.
- [ ] `cargo xtask build-hub-mcp-bundle`.

### Phase 7: end-to-end demo

- [ ] Rebuild chain from `AGENTS.md`: `cd hub-client && npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`.
- [ ] Automated e2e with `q2 preview --share --git track --ui editor --allow-edit` against a temp git repo and two guests:
  - a host commit while guests edit: no swap, no notice, edits intact;
  - a `git switch` to another branch: guests see the replacement notice, the file matches the branch, an edit typed just before the switch is counted as stranded;
  - a switch back to the first branch: fork on return, the file matches disk;
  - a rebase: sync pauses, then one decision after it finishes;
  - colocated jj `jj edit`: same result as the git switch;
  - save-then-commit inside the debounce window: handled as a commit.
- [ ] Record the exact invocations and inspected output in this plan, per `AGENTS.md`.

### Phase 8: docs and wrap-up

- [ ] User-facing note in `docs/` about live share and `--git track` (usage, the gaps below, jj support), then `cargo xtask build-agents-docs` per `AGENTS.md`.
- [ ] `hub-client/changelog.md` entry via the two-commit workflow if hub-client changed.
- [ ] `cargo xtask lint`, `q2 render --strict` on this file, and `cargo xtask verify --skip-hub-build` (full verify if the WASM leg is affected).
- [ ] Re-read this checklist against what landed and correct the checkmarks before handing off.

## Out of scope and known gaps

- **`quarto hub` standalone mode** (long-lived sync server on a project directory). It needs startup reconciliation against persisted state and persistence of the registry. File a follow-up when this work is done.
- **`git stash`, `git restore`, `git checkout -- <file>`, `jj restore`.** They rewrite files without moving HEAD, so they still look like author edits.
- **Rebase of branches onto a new doc**, and any UI for orphaned branches.
- **A viewer for retired epochs and their attribution.** The data is retained; nothing opens it yet.
- **Blame-seeded attribution** (reuse the `--attribution=git` blame source to attribute a seeded doc).
- **Persisting the registry**, or promoting it to an index sidecar.
- **Non-colocated jj.** The feature is off there; querying jj directly with `--ignore-working-copy` is a possible follow-up.
- **Sedimentree and Subduction compaction** as an answer to unbounded history (Ink and Switch, early preview, not available with samod).

## Risks

- **Save-then-commit misclassification for jj**, which has no reflog hint. The cost is guest disruption, not a wrong file.
- **Retained docs grow memory** in a long session. No cap at demo scale.
- **Fork copies full history**, so storage duplicates per fork.
- **Late stranded edits** reach the old doc only if Gate 0 confirms sync works for unlisted docs.
- **Untested git operations** for the lock-versus-HEAD ordering: `reset --hard`, `pull`, `rebase`.
