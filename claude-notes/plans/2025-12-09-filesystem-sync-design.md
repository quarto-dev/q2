# Filesystem Synchronization Design for quarto-hub

**Issue:** k-ke2m
**Parent:** k-4wex
**Related:** k-r2t1 (filesystem serialization), k-yvfo (conflict resolution)
**Status:** Design Discussion
**Created:** 2025-12-09

## Problem Statement

quarto-hub creates automerge documents for each `.qmd` file but currently leaves them empty. We need to:

1. **Initial population**: When creating a new automerge document for a file, populate it with the filesystem contents
2. **Ongoing synchronization**: Keep filesystem and automerge state in sync
3. **Divergence handling**: Handle cases where filesystem and automerge have both changed

This is fundamentally about "interfacing between the automerge world and the non-automerge world."

---

## Document Schema (Simple MVP)

For MVP, we use the simplest possible schema:

```rust
// In automerge document at ROOT:
{
    text: automerge::Text  // The file contents as collaborative text
}
```

This allows:
- Full CRDT merging of text edits
- Character-level collaborative editing
- History tracking via automerge

**Future consideration**: Rich text schema for structured `.qmd` documents (headers, code blocks, etc.). This is tracked separately in k-libc.

---

## Key API: `update_text`

Automerge 0.7.x provides a built-in `update_text` method specifically designed for our use case. From the automerge source:

```rust
// From automerge::transaction::Transactable
/// This will calculate a diff between the current value and the new value and
/// then convert that diff into calls to splice. This will produce results
/// which don't merge as well as directly capturing the user input actions, but
/// sometimes it's not possible to capture user input and this is the best you
/// can do.
fn update_text<S: AsRef<str>>(&mut self, obj: &ExId, new_text: S)
    -> Result<(), AutomergeError>;
```

**Internal implementation** (`text_diff.rs`):
- Uses **grapheme-aware Myers diff** (via `unicode_segmentation`)
- Converts diff operations directly to `splice_text` calls
- Handles Unicode correctly (emoji, combining characters, etc.)

**Usage**:
```rust
use automerge::transaction::Transactable;

doc.transact(|tx| {
    let text_obj = tx.get(ROOT, "text")?.unwrap().1;
    tx.update_text(&text_obj, &new_content)?;
    Ok(())
})?;
```

**Why this is ideal**: We don't need external diff crates. Automerge's `update_text` is purpose-built for exactly this scenario - applying external text changes to a collaborative document.

---

## Document Initialization (IMPORTANT)

**Decision**: When creating a new automerge document for a file, **always initialize it with a Text object containing the file contents**.

Currently, `reconcile_files_with_index` in `context.rs` creates empty documents:
```rust
let doc = Automerge::new();  // Empty!
```

**This must change to**:
```rust
let mut doc = Automerge::new();
let file_content = std::fs::read_to_string(&file_path)?;
doc.transact::<_, _, automerge::AutomergeError>(|tx| {
    let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
    tx.update_text(&text_obj, &file_content)?;
    Ok(())
})?;
```

**Why this matters**: The sync algorithm assumes `ROOT.text` exists. Without this initialization:
- `doc.get(ROOT, "text")?.unwrap()` will panic on new documents
- Case 5 (first run) won't work correctly

This change should be made **before** implementing the sync algorithm.

---

## Unified Sync Algorithm

The key insight is that we can use **one algorithm for all sync scenarios**. The fork-and-merge pattern naturally handles all cases because it always represents "merge filesystem's changes since last sync with automerge's changes since last sync."

### Sync Checkpoint Storage

Store sync state in a **local-only file** (not in automerge, since this is per-machine state):

```
.quarto/hub/sync-state.json
{
    "documents": {
        "<doc-id-1>": {
            "last_sync_heads": ["abc123", "def456"],
            "last_sync_content_hash": "sha256:..."
        },
        "<doc-id-2>": { ... }
    }
}
```

**Why local-only**: This metadata represents "when did THIS hub instance last sync to THIS machine's filesystem." Different peers have different filesystems, so syncing this data would be meaningless and confusing.

**Atomic writes**: To prevent corruption if the process crashes mid-write, use the write-to-temp-then-rename pattern:

```rust
fn save_sync_state(hub_dir: &Path, state: &SyncState) -> Result<()> {
    let target = hub_dir.join("sync-state.json");
    let temp = hub_dir.join("sync-state.json.tmp");

    // Write to temp file
    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(&temp, &content)?;

    // Atomic rename (on most filesystems)
    std::fs::rename(&temp, &target)?;

    Ok(())
}
```

**Content hashing**: Use the `sha2` crate for SHA-256 hashing:
```rust
use sha2::{Sha256, Digest};

fn sha256(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}
```

### The Algorithm

This algorithm is called at every sync point: hub startup, hub shutdown, periodic sync, or user-requested sync.

**Note on samod's `with_document`**: The `DocHandle::with_document(|doc| ...)` method provides `&mut Automerge`. The `fork_at()` and `merge()` methods are on `Automerge` directly (not on transactions), while `update_text()` is on the `Transactable` trait and must be called within a transaction.

```rust
use automerge::{transaction::Transactable, ReadDoc, ROOT};

fn sync_document(
    doc_handle: &DocHandle,
    file_path: &Path,
    sync_state: &mut SyncState,
) -> Result<()> {
    let doc_id = doc_handle.document_id().to_string();

    doc_handle.with_document(|doc| {
        // 1. Get sync checkpoint (use current heads if none exists or invalid)
        let checkpoint_heads = sync_state.get_heads(&doc_id);
        let last_sync_heads = checkpoint_heads
            .filter(|heads| heads.iter().all(|h| doc.get_change_by_hash(h).is_some()))
            .unwrap_or_else(|| doc.get_heads());

        // 2. Read filesystem content
        let fs_content = std::fs::read_to_string(file_path)?;

        // 3. Fork at sync checkpoint (with fallback if fork_at fails)
        let mut forked = doc.fork_at(&last_sync_heads)
            .unwrap_or_else(|_| doc.fork());  // Fallback to current state if checkpoint invalid

        // 4. Apply filesystem content to fork (update_text handles diff internally)
        //    Note: fork_at/merge are on Automerge; update_text requires transaction
        let text_obj = forked.get(ROOT, "text")?.unwrap().1;
        forked.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.update_text(&text_obj, &fs_content)?;
            Ok(())
        })?;

        // 5. Merge fork back into main document
        doc.merge(&mut forked)?;

        // 6. Write merged state back to filesystem
        let text_obj = doc.get(ROOT, "text")?.unwrap().1;
        let merged_content = doc.text(&text_obj)?;
        std::fs::write(file_path, &merged_content)?;

        // 7. Update sync checkpoint
        let content_hash = sha256(&merged_content);
        sync_state.set(&doc_id, doc.get_heads(), content_hash);

        Ok(())
    })
}
```

### Why This Works for All Cases

The algorithm naturally handles every scenario:

#### Case 1: No changes (heads match, content matches)
- Fork at last_sync_heads → identical to current doc
- `update_text(current_text, fs_content)` → strings identical, Myers diff = empty, no operations
- Merge → nothing to merge
- Write-back → writes same content
- **Result**: Correct no-op ✓

#### Case 2: Automerge changed, filesystem unchanged
- Fork at last_sync_heads → doc at OLD state (before automerge changes)
- `update_text(old_text, fs_content)` → fs_content == old_text, no operations
- Merge forked into current → forked has nothing new (current already has all changes since checkpoint)
- Write-back → writes current automerge content to filesystem
- **Result**: Filesystem gets updated with automerge changes ✓

#### Case 3: Filesystem changed, automerge unchanged
- Fork at last_sync_heads → doc at current state (heads haven't changed)
- `update_text(current_text, new_fs_content)` → generates diff operations
- Merge forked into current → applies filesystem edits
- Write-back → writes merged content (equals fs since that's all that changed)
- **Result**: Automerge gets filesystem changes ✓

#### Case 4: Both changed (true divergence)
- Fork at last_sync_heads → doc at common ancestor
- `update_text(ancestor_text, fs_content)` → generates "filesystem's edit path"
- Merge forked into current → CRDT merge of both edit paths
- Write-back → writes merged result
- **Result**: Three-way merge, both changes preserved ✓

#### Case 5: First run (no sync-state.json)
- `last_sync_heads = doc.get_heads()` (fallback: current state)
- Fork at current_heads → clone of current (empty doc)
- `update_text("", fs_content)` → generates insertions for all content
- Merge → applies insertions
- Write-back → writes content (same as filesystem)
- **Result**: Bootstraps correctly ✓

### Convergence Property

After each sync, the algorithm **settles to agreement**:
1. Automerge has merged content (both automerge and filesystem changes)
2. Filesystem has identical merged content (just written)
3. Checkpoint records current heads and content hash

On the next sync with no changes, the algorithm detects identical content and performs no-ops throughout.

### Edge Case: Invalid Checkpoint Heads

If `sync-state.json` points to heads that don't exist (e.g., automerge repo was recreated), `fork_at` would fail. The algorithm handles this by validating heads before use and falling back to current heads, effectively treating it as a fresh start.

---

## When to Sync

The unified algorithm is called at these sync points:

### MVP Sync Points
1. **Hub startup** - After connecting to sync peers and waiting for sync to settle
2. **Hub shutdown** - Before exiting (graceful shutdown)

### Future Sync Points
3. **Periodic timer** - Every N seconds (e.g., 30s) for robustness against crashes
4. **User request** - Explicit "sync to disk" command
5. **Filesystem watch** - When external changes detected (via fsnotify)

---

## Sync Peer Timing Consideration

User's concern: "What if the sync server has newer changes we haven't received yet?"

**Problem**: If we detect filesystem divergence BEFORE connecting to sync peer, we might make wrong decisions.

**Solution: Wait for Sync Stabilization**

```
On startup:
1. Connect to configured sync peers
2. Wait for sync to "settle" (no new messages for N seconds, or explicit "caught up" signal)
3. THEN check for filesystem divergence
4. Apply merge logic
```

**How to detect "caught up"**:
- samod/automerge-repo doesn't have explicit "sync complete" signal
- Heuristic: no new changes for 5-10 seconds
- Or: track `last_sync_heads` sent by each peer, compare with local heads

**Alternative**: Don't wait, just merge anyway
- If sync peer sends changes later, automerge handles merge automatically
- Filesystem is written on next write-back opportunity
- Simpler, but filesystem might briefly be "behind"

---

## Implementation Phases

### Phase 0: Prerequisites (Before Sync Implementation)

**Critical**: These changes must be made before the sync algorithm will work correctly.

1. **Modify `reconcile_files_with_index`** in `context.rs` to initialize documents with file contents:
   ```rust
   // Replace: let doc = Automerge::new();
   // With:
   let mut doc = Automerge::new();
   let file_content = std::fs::read_to_string(&file_path)?;
   doc.transact::<_, _, automerge::AutomergeError>(|tx| {
       let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
       tx.update_text(&text_obj, &file_content)?;
       Ok(())
   })?;
   ```

2. **Write API verification test** to confirm automerge behavior:
   - Test `get_change_by_hash()` returns Some for valid hashes
   - Test `fork_at()` works with valid heads
   - Test `fork_at()` fails gracefully with invalid heads
   - Test `merge()` correctly combines divergent changes
   - Test `update_text()` produces correct CRDT operations

### Phase 1: Core Sync Infrastructure
- Add `sha2` crate dependency for content hashing
- Implement `SyncState` struct for reading/writing `sync-state.json` (with atomic writes)
- Implement `sync_document()` function (the unified algorithm)
- Add `sync_all_documents()` to iterate over index and sync each file
- Write comprehensive tests for all 5 cases (no change, automerge-only, fs-only, divergence, first-run)

### Phase 2: Startup/Shutdown Integration
- Call `sync_all_documents()` on hub startup (after peer sync settles)
- Call `sync_all_documents()` on graceful shutdown
- Handle shutdown signals (SIGTERM, SIGINT)

### Phase 3: Periodic Sync (Robustness)
- Add configurable periodic timer (default: 30s)
- Sync all documents on timer tick
- Protects against crashes losing work

### Phase 4: Continuous Sync (Future)
- Watch filesystem for changes (fsnotify)
- Sync individual documents on external modification
- Real-time bidirectional sync

---

## Resolved Questions

1. **What if automerge repo is corrupted?**
   - **Decision**: Hard fail and report to user clearly
   - Recovery typically involves creating a new automerge repo (new root document ID, etc.)
   - This is acceptable because we expect every important Quarto project to be backed by version control (git, jj, etc.)
   - While live collaboration is happening, automerge is source of truth
   - A good version of the project will exist in the VCS repo as fallback

2. **How to handle file renames/moves?**
   - **MVP**: Old path removed from index, new path added (loses history connection)
   - **Future**: Use file content hashes to detect moves, then update the index key while preserving the same document ID (maintains history)
   - **File deletion**: Simply remove the file from the index document (automerge doesn't really support deletion, but removing from index is sufficient)

3. **Integration with `quarto render`?**
   - Not implemented yet, so not an immediate concern
   - **Key insight**: The automerge repo will be the source of document truth for projects with hub running
   - **Important future consideration**: When `quarto hub` and `quarto lsp` features interact, the in-memory version of documents being edited by LSP clients will be managed by the automerge repo
   - This ensures collaborative edits appear to editor tools as they are made in real-time
   - Implementation deferred, but this architectural direction is noted

## Open Questions

1. **What about binary files or non-text content?**
   - MVP: only sync `.qmd` files (text)
   - Future: could store binary files as blobs, or exclude from sync

2. **Should we track file metadata (mtime, permissions)?**
   - Probably not for MVP
   - Could be useful for smarter change detection

---

## References

- [Automerge Documentation](https://automerge.org/docs/hello/)
- [Automerge Rust API](https://docs.rs/automerge/latest/automerge/)
- [Automerge Rust - Transactable trait](https://docs.rs/automerge/latest/automerge/transaction/trait.Transactable.html) - includes `update_text`
- [Local, first, forever (Tonsky)](https://tonsky.me/blog/crdt-filesync/) - CRDT + filesystem sync patterns
- [Automerge Repo 2.0](https://automerge.org/blog/2025/05/13/automerge-repo-2/) - Latest patterns
- [samod source](external-sources/samod/) - Rust implementation details
- [similar crate](https://lib.rs/crates/similar) - Alternative diff library if needed
- [imara-diff crate](https://lib.rs/crates/imara-diff) - High-performance diff library
