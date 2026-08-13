# Write VFS-created files to disk under `--allow-edit` (WriteBack)

**Date:** 2026-08-13
**Status:** Active — reviewed 2026-08-13, ready for implementation
**Context:** In a `q2 preview --share --ui editor --allow-edit` session, a
guest who creates a new file (New File dialog / asset upload) gets a
VFS-only document: `createFile` (`ts-packages/quarto-sync-client/src/client.ts:1255`)
creates the automerge doc and adds `files[path] = docId` to the index doc,
both of which sync to the hub — but the hub never creates the file on disk.
Every periodic sync logs `File not found on disk, skipping sync`
(`crates/quarto-hub/src/sync.rs:644`). This contradicts the `--allow-edit`
contract printed in the share banner ("…and EDIT the project's files on
disk", `crates/quarto-preview/src/share.rs:109`).

## Overview

`sync_all_documents` (`crates/quarto-hub/src/sync.rs:608`) skips any index
entry whose disk file is missing, before `DiskWritePolicy` is consulted.
That guard exists so files **deleted on disk** are not resurrected from
their automerge docs every sync cycle — but a **VFS-created** file lands in
the same state (index entry, no disk file) and inherits the skip.

The two cases are distinguished by state the hub already persists:
`SyncState` (`.quarto/hub/sync-state.json`, keyed by doc_id) records a
checkpoint only after a successful `sync_document` / `sync_binary_document`
run. So:

| Index entry + missing disk file | Checkpoint for doc_id? | Meaning | Action |
|---|---|---|---|
| Guest created file in VFS | No (sync never ran for it) | VFS-created | **Create on disk** (WriteBack only) |
| File deleted on disk | Yes (synced before) | Deleted on disk | Skip (today's behavior) |
| Client-side `deleteFile` | — (index entry removed; never reaches sync) | — | — |

Scope: **WriteBack only.** Under ReadOnly (no `--allow-edit`) nothing
persists by design; behavior there is unchanged.

## Design decisions (recorded up front)

1. **The discriminator is `SyncState::has_checkpoint(doc_id)`
   (`sync_state.rs:128`).** No schema, client, or protocol changes. The
   invariant "no checkpoint ⇔ this doc was never synced to disk" must be
   pinned with a comment at the branch site and on
   `DiskWritePolicy::WriteBack` — it is load-bearing and not obvious.

2. **The change lives entirely in `sync_all_documents`.** The watcher path
   (`sync_file_by_path`) needs no change: it only ever syncs existing
   files (read-modify-write) and never creates them. (The watcher itself
   — `watch.rs:157` — maps every debounced event to `WatchEvent::Modified`
   with no event-kind or existence filtering, so deletions fire events
   too; those reach `sync_file_by_path` and error harmlessly at the fs
   read. Pre-existing behavior, unchanged.) Startup sync
   (`context.rs:359`) and periodic sync (`context.rs:479`) both call
   `sync_all_documents`, so both pick up the behavior. A VFS-created file
   appears on disk at the next sync tick.

3. **Branch in place; don't restructure the loop.** The `exists()`
   early-out (sync.rs:643) stays where it is, and the missing-file
   branch makes the skip-or-create decision in place:
   `policy == WriteBack && !sync_state.has_checkpoint(doc_id_str)` →
   parse doc id, `repo.find`, create; otherwise → existing skip.
   `has_checkpoint` takes the doc-id *string*, so the skip decision
   needs neither parse nor find — deleted files keep skipping cheaply
   instead of paying a `repo.find` every sync tick (deleted entries
   stay in the index permanently). The existing-file path (doc-id parse
   → `repo.find` → canonicalize check at sync.rs:703 →
   `sync_document_auto`) is byte-for-byte untouched. The
   `contained_join` traversal guard (sync.rs:636) stays first and
   untouched — index poisoning (`../victim.txt`) must remain rejected
   before any of this runs.

4. **Creation path mirrors the existing write paths.**
   - Dispatch text vs. binary via `detect_document_type`
     (`resource.rs:270`), falling back to `is_binary_extension` on the
     path for `Invalid`, exactly as `sync_document_auto` does.
   - Text: read `ROOT.text`, write with `std::fs::write`, checkpoint with
     `sha256_hash(content)`.
   - Binary: `read_binary_content(doc)` (`resource.rs:292`), write bytes,
     checkpoint with the doc's stored hash or `compute_hash(&content)`
     (same as `sync_binary_document`, sync.rs:342).
   - Containment matches the existing write path's posture: walk up
     from `file_path` to the nearest *existing* ancestor, canonicalize
     it, and require `starts_with(real_root)`; then
     `std::fs::create_dir_all(parent)` and `std::fs::write`. A
     pre-existing in-project symlink pointing outside the root is
     rejected deterministically (its canonical target fails the prefix
     check) and maps to `result.rejected`. The residual check/use
     window is the same class the pre-existing write-back path already
     accepts (sync.rs:703→201) and is reachable only by a local process
     already inside the trust boundary — the VFS has no symlink
     concept, so a share guest can neither plant a symlink nor reach
     the race. One containment mechanism, no new dependency.
     `contained_join` stays as the lexical first pass.
   - On success: `sync_state.set_checkpoint(doc_id, doc.get_heads(),
     hash)` so the next sync is a `NoChanges` no-op — including the
     watcher event our own write triggers (no write → watch → sync →
     write loop).
   - Count creations as `automerge_changed` in `SyncAllResult` (automerge
     → disk direction); no new counter. Log at `info!`: `Created new file
     on disk from VFS document`.

5. **The existing missing-file test changes meaning.**
   `test_sync_all_documents_with_missing_file` (sync.rs:1246) adds a
   no-checkpoint missing file and asserts a skip under WriteBack — that
   scenario now *creates* the file. Rework it into the deleted-on-disk
   test it always intended to model: sync an existing file once (creating
   the checkpoint), delete it from disk, sync again, assert skip and no
   resurrection.

6. **Known limitation (not a regression): rename.** Client `renameFile`
   (`client.ts:1400`) reuses the same doc_id under the new path, and
   checkpoints are keyed by doc_id — so a renamed file *has* a checkpoint
   and is classified "deleted on disk": the new path is not created.
   Identical to today's behavior (rename already doesn't persist); fixing
   it wants a per-path "seen on disk" record and is out of scope. File a
   follow-up bead.

7. **`cap-std` was considered and rejected.** Review found the threat
   model doesn't justify it: the capability walk defends against a
   symlink swap between check and use, but the VFS has no symlink
   concept (verified: no symlink support anywhere in
   `quarto-sync-client`), so a share guest can neither plant a symlink
   on the host nor reach the race window — the only remaining attacker
   is a local process that already has write access to the project
   tree. `cap-std` is also absent from `Cargo.lock` entirely (the
   cap-primitives subtree would be genuinely new, not marginal over
   transitive rustix), and it would have introduced a second
   containment mechanism alongside `contained_join` + canonicalize
   whose semantics must be kept in sync. The nearest-existing-ancestor
   canonicalize (decision 4) keeps one mechanism; if the write-path
   TOCTOU follow-up bead ever moves the codebase to capabilities, the
   create path moves with it.

## Work items

### Phase 0 — Failing tests (TDD)

- [x] New tests in `crates/quarto-hub/src/sync.rs` (helpers
  `create_test_repo`, `create_doc_with_text`, `create_doc_with_binary`
  already exist):
  - `sync_all_creates_vfs_only_text_file_under_writeback` — index entry,
    no disk file, no checkpoint, WriteBack → file exists on disk with the
    doc's text; `automerge_changed == 1`; second `sync_all_documents` →
    `NoChanges` (checkpoint was set).
  - `sync_all_creates_vfs_only_binary_file_under_writeback` — same with a
    binary doc; bytes round-trip.
  - `sync_all_creates_nested_parent_dirs` — path `chapters/new/intro.qmd`
    → parents created.
  - `sync_all_skips_deleted_file_under_writeback` — the reworked
    missing-file test (decision 5): sync once, delete on disk, sync →
    `skipped == 1`, file stays absent.
  - `sync_all_skips_vfs_only_file_under_readonly` — no checkpoint,
    ReadOnly → skipped, nothing on disk (ReadOnly contract unchanged).
  - `sync_all_create_rejects_symlink_escape` — index path whose parent is
    a symlink pointing outside the root → rejected, nothing written
    outside (the nearest-existing-ancestor canonicalize fails the
    `starts_with(real_root)` check; maps to `result.rejected`). Asserts
    `rejected == 1`, so pre-implementation this is a **fourth** red test —
    but it fails for a different reason than the create tests (the entry
    is `skipped`, not `rejected`), which is itself the useful signal.
- [x] Confirm the deliberate-red tests fail for the right reason: the
  three create tests because the file is not created; the symlink-escape
  test because the entry is `skipped` rather than `rejected`. The
  reworked deleted-file-skip test and the ReadOnly test are pinning
  tests — green both before and after. The existing traversal-poison
  test (`context.rs:1178`) must stay green unmodified (`contained_join`
  rejects before the create branch).

### Phase 1 — Implementation

- [x] Extend the missing-file early-out in `sync_all_documents` per
  decisions 3–4 (`contained_join` and the existing-file path stay
  untouched):
  - missing + `policy == WriteBack` +
    `!sync_state.has_checkpoint(doc_id_str)` → parse doc id,
    `repo.find`, create path (new helper `create_file_from_document`
    in sync.rs);
  - missing otherwise → existing skip (`result.skipped += 1`). Keep the
    `warn!` for the has-checkpoint case (genuine deletion signal);
    use `debug!` for the ReadOnly no-checkpoint case (expected
    VFS-only file in an ephemeral session) — small log-hygiene call, same
    commit is fine.
- [x] `create_file_from_document`: nearest-existing-ancestor
  canonicalize + `starts_with(real_root)` check, then
  `create_dir_all(parent)` + `std::fs::write`; checkpoint on success.
  Do **not** reuse `sync_document` / `sync_binary_document` for
  creation — they read the disk file up front, so a
  placeholder-then-delegate approach backfires (for binary,
  filesystem-wins would clobber the doc with the empty placeholder).
  Recorded here so the helper's existence isn't second-guessed later.
- [x] Comments pinning the checkpoint invariant (decision 1) and the
  rename limitation (decision 6).

### Phase 2 — Verification + e2e

- [x] `cargo nextest run -p quarto-hub` green; full
  `cargo xtask verify --skip-hub-build` green (Rust-only change; the
  hub-build leg is not needed — no embed inputs change).
- [ ] Real e2e per CLAUDE.md: `q2 preview --share --ui editor
  --allow-edit` on a fixture project, join from a second profile/browser,
  create a new text file and upload a binary asset as the guest; record
  in this file: file appears on disk at the next sync tick, server logs
  the `info!` creation line (no more repeating `warn!`), host edits to
  the new file sync back to the guest, and deleting the file on disk does
  **not** resurrect it.
- [ ] File follow-up beads: (a) rename persistence (decision 6);
  (b) ReadOnly warning spam for expected VFS-only files if the Phase 1
  log tweak was deferred.

## Risks / edge cases

- **sync-state.json loss resurrects deletions.** If the state file is
  wiped, all checkpoints vanish and previously-deleted-but-still-indexed
  files are recreated once. Moot for `q2 preview` (ephemeral hub dir);
  possible for a long-lived standalone hub. Chosen direction fails
  toward data *appearing* rather than disappearing; noted in the
  invariant comment.
- **TOCTOU on create (accepted, narrow).** The create path uses the
  same canonicalize-then-write posture as the pre-existing write-back
  path (`std::fs::write` at sync.rs:201/386 after canonicalize), so
  both keep a narrow check/use window. Reachable only by a local
  process already able to write inside the project tree — not by a
  share guest (the VFS has no symlink concept). One follow-up bead
  covers moving both paths to capability-based I/O (`cap-std`) if the
  posture is ever raised.
- **Empty / Invalid docs.** A just-created empty text doc writes an empty
  file (fine). `Invalid` type falls back to extension inference, matching
  `sync_document_auto`.
- **Watcher echo.** Our own write fires a `Modified` event; the resulting
  `sync_file_by_path` is a `NoChanges` no-op because the checkpoint was
  set at creation. Pinned by the "second sync → NoChanges" assertion.
- **Guest path collides with a directory on disk.** `exists()` is true
  for directories, so the entry takes the existing-file path and the fs
  read errors every sync cycle. Pre-existing behavior, unchanged by
  this work; noted so it isn't mistaken for a regression.

## Communication record

- 2026-08-13: Plan drafted from the share-session investigation.
  Behavior confirmed in code: client `createFile` adds the index entry;
  `sync_all_documents` skips missing disk files before consulting
  `DiskWritePolicy`; no create-on-disk path exists anywhere. Checkpoint
  disambiguation chosen over watcher tombstones (bigger semantic change:
  disk deletes would propagate to all clients) as the minimal correct
  fix. Design decisions 1–6 recorded before implementation.
- 2026-08-13: TOCTOU review: create path switched from
  canonicalize-the-parent + `std::fs::write` to capability-based
  creation via `cap-std` (new decision 7). The race is eliminated for
  creation rather than narrowed; the remaining window on the
  pre-existing write-back path is recorded as a follow-up bead, not
  folded into this change.
- 2026-08-13: Plan review, two simplifications folded in:
  (a) **cap-std dropped** — the VFS has no symlink concept (verified in
  `quarto-sync-client`), so the race it closed is unreachable by a
  share guest, and the crate is absent from `Cargo.lock` entirely; the
  create path now uses nearest-existing-ancestor canonicalize,
  matching the existing write path's posture (decisions 4/7
  rewritten). (b) **Loop restructure dropped** — the missing-file
  branch handles skip-or-create in place, keeping the existing-file
  path untouched and avoiding a `repo.find` per deleted doc per sync
  tick (decision 3 rewritten). Also: recorded why
  `create_file_from_document` doesn't reuse `sync_document`, fixed the
  deliberate-red test count (three, not two), and noted the
  directory-collision edge case.
- 2026-08-13: Pre-implementation verification against the code, two
  corrections folded in: (a) **symlink-test classification pinned** —
  asserting `rejected == 1` makes it a fourth deliberate-red test (the
  "three, not two" count above was itself wrong), failing on
  `skipped` vs `rejected` rather than on file creation; the reworked
  deleted-file-skip and ReadOnly tests are pinning tests, green before
  and after. (b) **Watcher rationale corrected** (decision 2) — the
  watcher fires on deletions too (`watch.rs` has no event-kind or
  existence filtering); the conclusion (no watcher change) stands
  because the watcher path never creates files. Also fixed the
  poison-test line ref (context.rs:1178) and activated the plan via
  CURRENT.md.
