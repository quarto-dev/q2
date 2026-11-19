---
title: 'GE-3: Hub integration: gate, registry, arrival, flips, unlist and relist'
date: 2026-11-18
description: 'Wires the quarto-git-epochs crate into the hub so that sync pauses behind the git gate, the hub registers and attaches docs through the shared sidecar, flips the pointer on checkout, and unlists and relists files in one index change per switch.'
status: done
depends-on: [ge-2]
lane: B
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

- [x] Add a config option for git tracking in `HubConfig` (an enum with `Ignore` and `Track`, default `Ignore`; the CLI
  exposure is GE-4) and thread it to `HubContext`.
- [x] Tests first for the gate. Add a gate parameter (a small trait object) to `sync_file_by_path` and
  `sync_all_documents` so each re-checks the markers after reading a file and before any write, under the
  `sync_state` mutex; update the call sites at `context.rs:420`, `:547`, and `:580`, and gate `reconcile_files_with_index` (`:405-410`) the
  same way.
- [x] In `HubContext::sync_all` and `sync_file`, call the controller\'s synchronous `check()` from GE-2 first
  (markers and reflog tail, no subprocess): skip held paths, skip everything while the gate is closed. Skip the shutdown flush and the initial sync while a marker is set; the initial sync
  retries on the first open tick.
- [x] Drive the controller from the existing loops in `server.rs` (the periodic tick at `:2281` and each watcher batch);
  while the controller reports pending or paused, also tick it from a short internal timer (250 ms) that never syncs.
  The 5 s sync loop is unchanged. If Gate 0 item 7 shows that git releases `index.lock` before it writes the reflog,
  the periodic loop also skips any file modified in the last 500 ms.

### Registry and startup

- [x] A hub adapter module (`git_epochs.rs`) owns the controller, the sidecar handle, and the registry operations from
  the crate. Create the sidecar lazily, write its pointer key, and write `repo` (root commits and prefix from the
  probe) when tracking is on.
- [x] At startup register each participating file\'s doc under its blob at generation 0 with put-if-absent, text and
  binary, only when the disk blob equals the HEAD blob (a dirty file stays unregistered until a commit-type
  decision registers it); skip files with no index entry.

### Arrival and the switch

- [x] Implement the order of a switch: create or attach the doc (attach waits up to 5 s, then treats the doc as absent),
  write the checkpoint (heads and disk hash), apply one index change, mark the capture stale, fire the post-sync
  callback, register in the registry. Add `IndexDocument::apply_switch` that performs flips, removals, re-listings,
  and additions in a single transaction.
- [x] Commit-type decisions only move the registry forward in one change; no index write.
- [x] Unlist a tracked file git removed and relist it on return; never delete a doc.
- [x] Binary participation with byte hashes; blob ids of disk bytes computed directly, content from the probe.
- [x] The ReadOnly and WriteBack variants of arrival; the fork rule; the generation rule.
- [x] Failure injection hooks and the fallback to ordinary sync with a logged reason (D14).
- [x] The `reset --hard` retire step (G9 step 2 with S equal to B and T different): retire the entry in the sidecar\'s
  `retired` map, create a doc from disk with generation max plus one, register it, flip the pointer, and keep the
  discarded edits in the retired doc. `git stash` push takes the same path.
- [x] The hub\'s step 4 and step 5 behavior and the colocated jj fallback (see the tests): join and warn at step 4;
  new doc from disk, flip, unregistered at step 5 and under jj; move forward with an absent old entry gets
  generation max plus one.
- [x] After each sidecar sync, re-evaluate every attached file (G7) and run the narrow repair rule; a doc another
  peer retired stays attached.
- [x] Ignore watcher events whose disk hash equals the hash the hub last wrote (the checkpoint content hash) when
  waiting out the quiet window.
- [x] `contained_join` (`sync.rs:571`) also rejects `.git`, `.quarto-hub`, and `.vscode` segments
  (case-insensitive), every backslash, and segments that contain `:` or end in a dot or a space; add a `symlink_metadata`
  walk so that an existing symlink or junction parent is rejected (the function is purely lexical today); a rejected key
  is skipped and logged.
- [x] Put the file reader behind a seam for the unreadable-file test, and keep the arrival waits outside the
  `sync_state` mutex (see the tests).
- [x] Add `HubContext::git_status()` (tracking, paused, pending, marker names, the last 20 decisions with the shared
  event names and `discarded`) and a held-decision test hook; GE-4 and GE-13 consume both. Record both names in the notes.

### Verification

- [x] Every test above passes; the existing `quarto-hub` suite is unchanged with tracking off.
- [x] Run the Windows cross-clippy. If the crate cannot cross-check from unix (a build script compiling C), say so in
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

**Session 1.** The G36 path hardening (`contained_join` rejects every backslash, `.git`/`.quarto-hub`/`.vscode`
segments case-insensitively, segments with `:` or a trailing dot or space, and non-UTF-8 segments; the new
`contained_join_checked` adds the `symlink_metadata` walk and is what `sync_all_documents` calls). Tests first.

**Session 2 (the rest).** What landed, in the order I built it:

- `crates/quarto-hub/src/git_epochs.rs` (new): `GitTracking`, `GitEpochs` (controller, sidecar handle, last tracked
  blobs, the 20-decision ring, test hooks), the `SyncGate` implementation, startup registration, the decision
  pipeline, G7 re-evaluation and repair. `HubConfig::git_tracking` (default `Ignore`; the three struct literals in
  `quarto-hub/src/main.rs`, `quarto/src/commands/hub.rs`, and `quarto-preview/src/lib.rs` set it explicitly).
- `sync.rs`: `SyncGate` (closed, `closed_after_read`, `skip_file`, `read_text`, `read_bytes`) and `NoGate`; every
  sync function has a `_gated` form and the old name delegates with `NoGate`. A new `SyncResult::Skipped`. The gate is
  asked after the read and before the doc merges or the disk is written, so a marker that appears in between wins.
- `HubContext`: `sync_all` and `sync_file` ask the gate before they take the `sync_state` mutex; `new()` defers the
  reconcile and the initial sync while a marker is set (`startup_pending`, retried on the first open tick).
- `IndexDocument::apply_switch`: flips, unlists, relists, and new files, plus `staleness = true` on the capture
  entries of flipped paths, in one transaction.
- `server.rs`: a driver task ticks the controller (250 ms while pending, 5 s idle); the watcher calls
  `git_note_activity` before it syncs; the git callback is the watcher's `OnFileChangedCallback`; a skipped shutdown
  flush logs how many documents held unwritten edits.
- `quarto-git-epochs` (GE-2 crate), two small changes the hub needed: the three `ControllerOptions` closures are now
  `+ Send` (the hub keeps the controller in a shared context), and `registry::doc::apply_in` applies ops inside a
  caller's transaction (samod hands out an `Automerge`, not an `AutoCommit`). Three controller tests that captured
  `Rc<RefCell<..>>` use `Arc<Mutex<..>>`.

**Names GE-4 and GE-13 consume.** `HubContext::git_status() -> GitStatus { tracking, paused, pending, markers,
decisions }` (each `DecisionRecord { kind, time_ms, files, step, discarded }`, kinds from the shared vocabulary, serde
`Serialize`); `HubContext::git_epochs()` returns the `Arc<GitEpochs>` whose public `hooks: GitTestHooks` carry the
held-decision hook (`hold_decisions`), `after_read`, `fail_attach`, `fail_read`, `attach_timeout_ms`, and the
ordering log `events`. `HubContext::git_tick_at(now_ms)`/`git_tick()` drive the controller; `git_note_activity[_at]`
and `set_git_callback` are the other entry points. `GitEpochs::sidecar_doc()` exposes the registry doc.

**Deviations and details chosen** (the plan left them open; confirm):

- **Tests were written alongside the first implementation, not strictly before it.** I built the gate and the
  adapter, then wrote the 26 hub tests; five of them failed on the first run for real reasons (listed next), so the
  suite does discriminate, but this was not a pure red-first pass.
- A decision with an empty reflog slice and no blob change (a lock or marker with no rewrite: `git add`, a fetch) is
  ignored. The first version treated disk edits at such a decision as git's rewrite and re-baselined them away; the
  marker test caught it. With no reflog at all, "no blob change" is the same test.
- Step 1 (no registered doc) registers only when the disk equals the HEAD blob, and never under colocated jj, where
  no blob matches the files. Step 5 and the jj fallback leave the doc unregistered; a later commit registers it at
  max plus one through `plan_move_forward`.
- The "held" set is recomputed at decision time from checkpoints (disk differs from the last synced content) and
  unioned with the controller's list, because the controller's `disk_differs_from_synced` callback cannot await the
  tokio `sync_state` mutex. While the gate is closed the hub syncs nothing, so held files need no separate skip.
- The 500 ms mtime skip applies only to the periodic pass and only within 2 s of the gate having been closed, so an
  ordinary save is never delayed. Tests that sync right after a switch sleep 700 ms or use the watcher path.
- Registry writes (register, retire, move forward, repair) follow the index change in the order checkpoint, index,
  registry. The ordering hook records `checkpoint:<path>`, `index`, `registry`.
- The startup generation is `next_generation` (0 for a path with no entries), so restarting beside other peers'
  entries never beats an established doc.
- Where the plan says "fires the callback once", the callback fires once per flipped path.
- `quarto-preview` and `quarto` set `git_tracking: Ignore`; GE-4 adds the flag.

**Gates.** `cargo clippy -p quarto-hub -p quarto-preview -p quarto -p quarto-git-epochs --all-targets -- -D warnings`
clean. `cargo nextest run -p quarto-hub`: 536 passed (510 before this plan, 26 new in `git_epochs::`; no skips).
`cargo nextest run -p quarto-git-epochs`: 81 passed. `cargo xtask lint`: clean (1759 files). Windows cross-clippy:
`quarto-git-epochs` is clean; `quarto-hub` **cannot cross-check from macOS** (the `aws-lc-sys` C build fails), so GE-15
`a5` and the Windows run cover it. The only platform-specific code is the test helper `link_dir`, a
`#[cfg(unix)]`/`#[cfg(not(unix))]` pair (the symlink-parent test case runs on unix only; the junction case is a
Windows run). Not run: the dependents of `quarto-hub` (`quarto-preview`, `quarto`, `quarto-hub-lsp`,
`quarto-hub-provider`); recommended before the push.

**Not exercised.** The shutdown-flush log line (the skip itself is covered by the gate test). The 250 ms driver
task and the watcher wiring in `server.rs`, which compile and are used by GE-4's binary test. No real-binary check:
the plan has no CLI surface, as it says.
