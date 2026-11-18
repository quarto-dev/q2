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

- **G5:** gate and hold inside `HubContext::sync_all` and `sync_file`; the shutdown flush and the initial sync skip
  while a marker is set; markers are re-checked after the read and before the write, under the `sync_state` mutex.
- **G17:** the hub adopts the registry and arrival rule; it creates the sidecar, writes `repo` when tracking is on,
  registers every participating file at startup, writes the checkpoint before it flips, marks captures stale, and fires
  the post-sync callback.
- **G13:** unlist and relist tracked files; one index change per switch.
- **G6, G9:** binary docs participate with byte hashes; ReadOnly and WriteBack arrival variants.
- **G1:** files whose blob changed are held from the trigger to the decision.
- **G7, G11, G12:** generation on creation, the 5 s unavailable rule, the rename-invariant fork.
- **D14 (kept):** a failed switch falls back to ordinary sync for that file.

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

## Test specifications

Hub-level tests live in `crates/quarto-hub/tests/integration/git_epochs.rs` (registered alphabetically in `main.rs`),
with real temp git repos, a real `HubContext`, and real Automerge docs, in both policies unless noted. All written first.

- **Gate:** with a marker present the periodic sync, a watcher event, the shutdown flush, and the initial sync all do
  nothing and write nothing; after the marker is removed they resume. A test hook makes a marker appear between the
  file read and the write: the write is skipped and nothing merges.
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
- **Held files:** between the trigger and the decision ordinary sync skips a changed file.
- **Failure injection:** if attaching the canonical doc fails or times out (5 s, with a short timeout in the test), the
  hub falls back to ordinary sync for that file and logs it; other files in the switch still apply.
- **Startup registration:** with tracking on, the registry holds every participating file\'s doc under its blob, the
  sidecar pointer and `repo` entry exist, and there is no `owner` key; restarting with the same data directory adds no
  duplicates. With tracking off none of these keys are written.
- **Generation and fork:** creating after another peer\'s entry gives generation max plus one; a doc already listed
  under another path is forked, not reused.

## Checklist

### Configuration and the gate

- [ ] Add a config option for git tracking in `HubConfig` (an enum with `Ignore` and `Track`, default `Ignore`; the CLI
  exposure is GE-4) and thread it to `HubContext`.
- [ ] Tests first for the gate. Add a gate parameter (a small trait object) to `sync_file_by_path` and
  `sync_all_documents` so each re-checks the markers after reading a file and before any write, under the
  `sync_state` mutex; update the call sites at `context.rs:420`, `:547`, and `:580`.
- [ ] In `HubContext::sync_all` and `sync_file`, consult the controller from GE-2: skip held paths, skip everything
  while the gate is closed. Skip the shutdown flush and the initial sync while a marker is set; the initial sync
  retries on the first open tick.
- [ ] Drive the controller from the existing loops in `server.rs` (the periodic tick at `:2281` and each watcher batch)
  without adding a new timer.

### Registry and startup

- [ ] A hub adapter module (`git_epochs.rs`) owns the controller, the sidecar handle, and the registry operations from
  the crate. Create the sidecar lazily, write its pointer key, and write `repo` (root commits and prefix from the
  probe) when tracking is on.
- [ ] At startup register each participating file\'s doc under its blob at generation 0, text and binary, even when
  disk is dirty (the dirty-seed case is accepted); skip files with no index entry.

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

### Verification

- [ ] Every test above passes; the existing `quarto-hub` suite is unchanged with tracking off.
- [ ] Run the Windows cross-clippy. If the crate cannot cross-check from unix (a build script compiling C), say so in
  Notes from the run so a Windows run can cover it (GE-15).

## Gates

- `cargo clippy -p quarto-hub --all-targets -- -D warnings`
- `cargo nextest run -p quarto-hub`
- `cargo clippy -p quarto-hub --all-targets --target x86_64-pc-windows-msvc -- -D warnings` (note any limitation)
- `cargo xtask lint`; `q2 render --strict` on this file.
- Recommend, but do not run unprompted, `cargo nextest run -p quarto-preview -p quarto` in the pre-push summary,
  since `quarto-hub` has dependents.

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
