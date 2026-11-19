---
title: 'GE-3: Hub integration: gate, registry, arrival, flips, unlist and relist'
date: 2026-11-18
description: 'Wires the quarto-git-epochs crate into the hub so that sync pauses behind the git gate, the hub registers and attaches docs through the shared sidecar, flips the pointer on checkout, and unlists and relists files in one index change per switch.'
status: approved
depends-on: [ge-2]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-2](ge-02-rust-logic-probe-controller.md)
(and through it GE-1) · **Unblocks:** [GE-4](ge-04-hub-surface.md) ·
**Branch:** `ge/03-hub-integration` off `feature/git-epochs`, merged by rebase and fast-forward, no push.

## Goal

Make the hub a git-aware disk peer. When git tracking is on, the hub keeps the gate (no sync in either direction while
git is mid-operation), registers every participating file\'s doc under its blob in the shared sidecar, and, when its own
checkout changes a file, runs the arrival rule: it attaches to the canonical doc for the new blob or creates one, writes
the sync checkpoint, then flips the pointer. A file git removes is unlisted and a returning file is relisted, all in
one index change per switch. The hub is the project owner, so it writes the pointer and no `owner` key.

This replaces the earlier plan\'s private in-memory registry and fork-and-overwrite (D4, D6, D7). With tracking off,
nothing changes. The flag, the status route, and the startup warning are GE-4; here the behavior is switched by a
config option that defaults to off.

## Decisions implemented

- **G5:** gate and hold inside `HubContext::sync_all` and `sync_file`, each entered through the controller\'s
  synchronous `check()` (markers and reflog tail, no subprocess); the shutdown flush and the initial sync skip while a
  marker is set, and `reconcile_files_with_index` is gated the same way; markers are re-checked after the read and
  before the write, under the `sync_state` mutex. A skipped flush logs how many documents held unwritten edits.
- **G17:** the hub adopts the registry and arrival rule; it creates the sidecar, writes `repo` when tracking is on,
  registers every participating file at startup (put-if-absent, only when disk equals the HEAD blob), writes the
  checkpoint before it flips, marks captures stale, fires the post-sync callback, and keeps its last 20 decisions in
  memory for the status route.
- **G13:** unlist and relist tracked files; one index change per switch.
- **G6, G9:** binary docs participate with byte hashes; ReadOnly and WriteBack arrival variants; the `reset --hard`
  retire step; the hub\'s step 4 (join, warn, record) and step 5 (new doc from disk, flip, unregistered) behavior,
  which colocated jj also takes for every changed file.
- **G1:** every file whose disk differs from its last synced content is held from the trigger to the decision (a
  `reset --hard` changes no blob); the quiet window ignores the hub\'s own writes.
- **G7, G11, G12:** generation on creation, the 5 s unavailable rule, the rename-invariant fork.
- **D14 (kept):** a failed switch falls back to ordinary sync for that file.
- **G7:** a sidecar change re-evaluates every attached file; a doc that is no longer canonical goes through the
  arrival steps.
- **G36:** `contained_join` also rejects `.git`, `.quarto-hub`, and `.vscode` segments, every backslash, segments that
  contain `:` or end in a dot or a space, and any path whose existing parent is a symlink or junction.

## Context

- `DiskWritePolicy` (`WriteBack` or `ReadOnly`) is at `crates/quarto-hub/src/sync.rs:35-51`; the load-bearing invariant
  "no checkpoint means never on disk" is stated there and used when an index entry has no file.
- The sync entry points are `sync_file_by_path` (`sync.rs:500`) and `sync_all_documents` (`sync.rs:614`); checkpoints
  are `SyncState::set_checkpoint(doc_id, heads, content_hash)` (`sync_state.rs:114`) and `has_checkpoint` (`:128`).
- `HubContext::sync_all` is at `context.rs:539` and `sync_file` at `:569`, both locking the `sync_state` mutex. The
  initial sync inside `HubContext::new` calls `sync_all_documents` directly (`context.rs:420`).
- The callers: the shutdown flush `ctx_for_shutdown.sync_all()` (`server.rs:2247`), the periodic loop (`:2281`), and the
  watcher path `ctx.sync_file(&path)` (`:2322`), which then fires the post-sync callback `OnFileChangedCallback`
  (`:2006`, `:2314`, `:2334`).
- `IndexDocument::add_file` overwrites a key with `tx.put` (`index.rs:169-185`); `remove_file` deletes it (`:189-200`);
  each is its own transaction, so a switch needs a new batch method. Capture staleness is the optional
  `CaptureEntry.staleness` (`index.rs:71`, key `:80`, set at `:287`).
- GE-1 added the schema keys and accessors for the sidecar pointer, `owner`, and `repo`; GE-2 provides the crate.
- The hub share policy (`context.rs:216`) is read-write with announce off: the hub cannot pull a doc from a client, so
  attaching a doc it has not received can time out; that is the G11 rule.
- Precedent for hub-level tests: `crates/quarto-hub/tests/integration/upstream_mirror.rs` and `support.rs`.
- `reconcile_files_with_index` runs before the initial sync (`context.rs:405-410`) and reads disk; `contained_join` is
  `sync.rs:571`. The participating-file set comes from the discovery code (`crates/quarto-hub/src/discovery.rs` at
  the base commit; on `origin/main` it moved to the `quarto-project-files` crate, so use the path that exists after
  the rebase). The hub\'s periodic loop ticks every 5 s and its watcher debounce is 500 ms.

## Test specifications

Hub-level tests live in `crates/quarto-hub/tests/integration/git_epochs.rs` (registered alphabetically in `main.rs`),
with real temp git repos, a real `HubContext`, and real Automerge docs, in both policies unless noted. All written first.

- **Gate:** with a marker present the periodic sync, a watcher event, the shutdown flush, and the initial sync all do
  nothing and write nothing; after the marker is removed they resume. A test hook makes a marker appear between the
  file read and the write: the write is skipped and nothing merges. The initial sync\'s `reconcile_files_with_index`
  is skipped the same way. A change to the reflog file alone, with no marker, makes `check()` report pending, and the
  entry holds its files before any tick runs.
- **Commit:** a plain commit moves the registry entry forward (new blob added with the same generation, old removed),
  changes no index entry, and creates no doc; clients\' view is unchanged.
- **Checkout, new blob:** the hub creates a doc, writes the checkpoint before the index change (asserted through an
  ordering hook), points `files[path]` at the new doc, marks the capture stale, and fires the callback once.
- **Checkout back:** switching back attaches the existing canonical doc (no fork), disk equals the doc, no stray text.
- **Late stranded edit:** after a switch, a late edit from another peer synced into the old doc does not change the
  new doc and does not reach disk.
- **Unlist and relist:** a file removed by a checkout loses its index entry while its doc stays in the registry; when the
  blob returns the entry is relisted pointing at the registry\'s doc. A switch with flips, removals, re-listings, and
  new files produces exactly one index change.
- **Binary:** a binary changed by a checkout gets a new binary doc, compared by byte hash; switching back attaches the
  old one.
- **ReadOnly arrival:** when disk differs from the canonical doc the hub creates a doc from disk and never writes the
  disk. **WriteBack clean-but-drifted:** the hub joins the doc, writes its text to disk, then writes the checkpoint.
- **Held files:** between the trigger and the decision ordinary sync skips every file whose disk differs from its
  last synced content, including a file a `reset --hard` restored to its blob.
- **Failure injection:** if attaching the canonical doc fails or times out (5 s, with a short timeout in the test), the
  hub falls back to ordinary sync for that file and logs it; other files in the switch still apply.
- **Startup registration:** with tracking on, the registry holds the doc of every participating file whose disk blob equals the HEAD blob, under that blob (a
  dirty file stays unregistered, and registration is put-if-absent), the
  sidecar pointer and `repo` entry exist, and there is no `owner` key; restarting with the same data directory adds no
  duplicates. With tracking off none of these keys are written.
- **Generation and fork:** creating after another peer\'s entry gives generation max (over the path, any blob) plus
  one; a doc already listed under another path is forked, not reused.
- **`reset --hard` and `git stash`:** a browser edit lands in doc D and reaches disk; `git reset --hard` (and a
  `git stash` push) restores the blob. The hub retires `(path, B, D)`, creates a doc from disk with generation max
  plus one, registers it, and flips the pointer; guests end on the HEAD text, the dirty text stays in the retired
  doc, and the decision appears in the status route. `stash pop` afterward arrives as an author edit in the new doc.
- **Step 4 and step 5 at the hub:** a clean file whose canonical doc holds live uncommitted edits is joined, the hub
  logs a warning, and the decision is recorded; a dirty file that differs from both the doc and the blob gets a new
  doc from disk, a flip, and no registration, and a later commit registers it with generation max plus one.
- **Colocated jj:** every changed file takes the step 5 behavior; nothing is registered by arrival.
- **Retired and superseded:** canonical selection skips a retired doc; a peer\'s sidecar change that makes an attached
  doc non-canonical runs the arrival steps for it, and a doc another peer retired is left attached.
- **Quiet window and own writes:** a guest typing continuously (the hub writing disk every tick) does not keep the
  decision waiting to the 30 s cap; the hub\'s own writes are recognized by the disk hash equal to the checkpoint
  hash.
- **Paths:** an index key naming `.git/hooks/h`, `.quarto-hub/x`, `.vscode/tasks.json`, `a/../../x`, `C:\x`, `a\b`,
  or `.git.` is rejected and logged, like `../x`; on POSIX the backslash forms need the product check, because a
  lexical check sees one ordinary component. A key whose parent directory is a symlink (a junction on Windows) pointing
  outside the root is rejected, and nothing is written through it.
- **Unreadable file:** the file reader sits behind a small seam, and a test injects `PermissionDenied` (the Windows
  sharing violation) for one file: the sync skips that file, logs it, never deletes the doc or the file, and syncs the
  others. GE-15 `a5` runs the same test on Windows.
- **Lock scope:** the arrival waits (5 s per unreachable doc) run outside the `sync_state` mutex, concurrently per file
  with one overall cap of 5 s per switch, so a switch with many unreachable docs does not stall every sync for N times
  5 s; a test with three unreachable docs finishes within the cap.
- **Status accessor:** `HubContext::git_status()` returns tracking, paused, pending, the marker names, and at most 20
  decisions (kind, time, files, G9 step, `discarded`), using the shared event names (including `moved-forward`,
  `joined`, and `seeded`); a `reset --hard` decision carries the number of edits set aside as `discarded`; a
  held-decision test hook exists for GE-4 and GE-13.

## Checklist

### Configuration and the gate

- [ ] Add a config option for git tracking in `HubConfig` (an enum with `Ignore` and `Track`, default `Ignore`; the CLI
  exposure is GE-4) and thread it to `HubContext`.
- [ ] Tests first for the gate. Add a gate parameter (a small trait object) to `sync_file_by_path` and
  `sync_all_documents` so each re-checks the markers after reading a file and before any write, under the
  `sync_state` mutex; update the call sites at `context.rs:420`, `:547`, and `:580`, and gate `reconcile_files_with_index` (`:405-410`) the
  same way.
- [ ] In `HubContext::sync_all` and `sync_file`, call the controller\'s synchronous `check()` from GE-2 first
  (markers and reflog tail, no subprocess): skip held paths, skip everything while the gate is closed. Skip the shutdown flush and the initial sync while a marker is set; the initial sync
  retries on the first open tick.
- [ ] Drive the controller from the existing loops in `server.rs` (the periodic tick at `:2281` and each watcher batch);
  while the controller reports pending or paused, also tick it from a short internal timer (250 ms) that never syncs.
  The 5 s sync loop is unchanged. If Gate 0 item 7 shows that git releases `index.lock` before it writes the reflog,
  the periodic loop also skips any file modified in the last 500 ms.

### Registry and startup

- [ ] A hub adapter module (`git_epochs.rs`) owns the controller, the sidecar handle, and the registry operations from
  the crate. Create the sidecar lazily, write its pointer key, and write `repo` (root commits and prefix from the
  probe) when tracking is on.
- [ ] At startup register each participating file\'s doc under its blob at generation 0 with put-if-absent, text and
  binary, only when the disk blob equals the HEAD blob (a dirty file stays unregistered until a commit-type
  decision registers it); skip files with no index entry.

### Arrival and the switch

- [ ] Implement the order of a switch: create or attach the doc (attach waits up to 5 s, then treats the doc as absent),
  write the checkpoint (heads and disk hash), apply one index change, mark the capture stale, fire the post-sync
  callback, register in the registry. Add `IndexDocument::apply_switch` that performs flips, removals, re-listings,
  and additions in a single transaction.
- [ ] Commit-type decisions only move the registry forward in one change; no index write.
- [ ] Unlist a tracked file git removed and relist it on return; never delete a doc.
- [ ] Binary participation with byte hashes; blob ids of disk bytes computed directly, content from the probe.
- [ ] The ReadOnly and WriteBack variants of arrival; the fork rule; the generation rule.
- [ ] Failure injection hooks and the fallback to ordinary sync with a logged reason (D14).
- [ ] The `reset --hard` retire step (G9 step 2 with S equal to B and T different): retire the entry in the sidecar\'s
  `retired` map, create a doc from disk with generation max plus one, register it, flip the pointer, and keep the
  discarded edits in the retired doc. `git stash` push takes the same path.
- [ ] The hub\'s step 4 and step 5 behavior and the colocated jj fallback (see the tests): join and warn at step 4;
  new doc from disk, flip, unregistered at step 5 and under jj; move forward with an absent old entry gets
  generation max plus one.
- [ ] After each sidecar sync, re-evaluate every attached file (G7) and run the narrow repair rule; a doc another
  peer retired stays attached.
- [ ] Ignore watcher events whose disk hash equals the hash the hub last wrote (the checkpoint content hash) when
  waiting out the quiet window.
- [ ] `contained_join` (`sync.rs:571`) also rejects `.git`, `.quarto-hub`, and `.vscode` segments
  (case-insensitive), every backslash, and segments that contain `:` or end in a dot or a space; add a `symlink_metadata`
  walk so that an existing symlink or junction parent is rejected (the function is purely lexical today); a rejected key
  is skipped and logged.
- [ ] Put the file reader behind a seam for the unreadable-file test, and keep the arrival waits outside the
  `sync_state` mutex (see the tests).
- [ ] Add `HubContext::git_status()` (tracking, paused, pending, marker names, the last 20 decisions with the shared
  event names and `discarded`) and a held-decision test hook; GE-4 and GE-13 consume both. Record both names in the notes.

### Verification

- [ ] Every test above passes; the existing `quarto-hub` suite is unchanged with tracking off.
- [ ] Run the Windows cross-clippy. If the crate cannot cross-check from unix (a build script compiling C), say so in
  Notes from the run so a Windows run can cover it (GE-15).

## Gates

- `cargo clippy -p quarto-hub -p quarto-preview -p quarto --all-targets -- -D warnings` (the new `HubConfig` field
  touches struct literals in `quarto-preview` and `quarto/src/commands/hub.rs`; recount with
  `grep -rn "HubConfig {" crates --include='*.rs'`)
- `cargo nextest run -p quarto-hub`
- `cargo clippy -p quarto-hub --all-targets --target x86_64-pc-windows-msvc -- -D warnings` (note any limitation)
- `cargo xtask lint`; `q2 render --strict` on this file.
- Recommend, but do not run unprompted, `cargo nextest run -p quarto-preview -p quarto -p quarto-hub-lsp -p
  quarto-hub-provider` in the pre-push summary, since `quarto-hub` has those dependents (`cargo tree -i quarto-hub`).

## End-to-end verification

This plan changes behavior only behind a config option with no CLI surface, so there is no real-binary check here:
GE-4 adds the flag and runs the scenarios through `q2 preview --git track`. The hub-level tests with real repos and real
docs are the verification in this plan.

## Exit criteria

With tracking on, the hub pauses behind the gate in both policies, registers at startup, switches and unlists by the
rules above with one index change per switch, never writes disk under ReadOnly, and falls back safely on a failed
switch; with tracking off the suite is unchanged. All gates are green.

## Out of scope

The `--git` flag, the `/api/preview/git-status` route, the `.quarto-hub` startup warning (all GE-4); client-side swap
behavior (GE-6, GE-7); anything in the extension.

## Notes from the run

Not started.
