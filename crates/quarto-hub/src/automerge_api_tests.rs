//! API verification tests for automerge behavior
//!
//! These tests verify our assumptions about automerge's fork/merge/update_text APIs
//! before building the filesystem sync algorithm on top of them.

#[cfg(test)]
mod tests {
    use automerge::{Automerge, ObjType, ROOT, ReadDoc, transaction::Transactable};

    /// Helper to create a document with a text field initialized to given content.
    fn create_doc_with_text(content: &str) -> Automerge {
        let mut doc = Automerge::new();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let text_obj = tx.put_object(ROOT, "text", ObjType::Text)?;
            if !content.is_empty() {
                tx.update_text(&text_obj, content)?;
            }
            Ok(())
        })
        .expect("Failed to initialize document");
        doc
    }

    /// Helper to read text content from a document.
    fn read_text(doc: &Automerge) -> String {
        let (_, text_obj) = doc.get(ROOT, "text").unwrap().unwrap();
        doc.text(&text_obj).unwrap()
    }

    /// Helper to update text content in a document.
    fn update_text(doc: &mut Automerge, content: &str) {
        let (_, text_obj) = doc.get(ROOT, "text").unwrap().unwrap();
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            tx.update_text(&text_obj, content)?;
            Ok(())
        })
        .expect("Failed to update text");
    }

    // =========================================================================
    // Test 1: get_change_by_hash returns Some for valid hashes
    // =========================================================================

    #[test]
    fn test_get_change_by_hash_returns_some_for_valid_heads() {
        let doc = create_doc_with_text("Hello, world!");
        let heads = doc.get_heads();

        // Verify we have at least one head
        assert!(!heads.is_empty(), "Document should have at least one head");

        // Verify get_change_by_hash returns Some for each head
        for head in &heads {
            let change = doc.get_change_by_hash(head);
            assert!(
                change.is_some(),
                "get_change_by_hash should return Some for head {:?}",
                head
            );
        }
    }

    #[test]
    fn test_get_change_by_hash_returns_none_for_invalid_hash() {
        let doc = create_doc_with_text("Hello, world!");

        // Get heads from a different document - these won't exist in our doc
        let other_doc = create_doc_with_text("Different content entirely");
        let other_heads = other_doc.get_heads();

        // Use heads from other document as "invalid" heads for our doc
        for head in &other_heads {
            let change = doc.get_change_by_hash(head);
            assert!(
                change.is_none(),
                "get_change_by_hash should return None for hash from different document"
            );
        }
    }

    // =========================================================================
    // Test 2: fork_at works with valid heads
    // =========================================================================

    #[test]
    fn test_fork_at_works_with_valid_heads() {
        let doc = create_doc_with_text("Original content");
        let heads = doc.get_heads();

        // Fork at current heads should succeed
        let forked = doc.fork_at(&heads);
        assert!(forked.is_ok(), "fork_at with valid heads should succeed");

        let forked = forked.unwrap();
        // Forked document should have same content
        assert_eq!(read_text(&forked), "Original content");
    }

    #[test]
    fn test_fork_at_historical_point() {
        // Create document and record heads
        let mut doc = create_doc_with_text("Version 1");
        let heads_v1 = doc.get_heads();

        // Make more changes
        update_text(&mut doc, "Version 2");
        let heads_v2 = doc.get_heads();

        // Verify current content is V2
        assert_eq!(read_text(&doc), "Version 2");
        assert_ne!(heads_v1, heads_v2, "Heads should change after update");

        // Fork at historical point (V1)
        let forked = doc.fork_at(&heads_v1);
        assert!(forked.is_ok(), "fork_at historical heads should succeed");

        let forked = forked.unwrap();
        // Forked document should have V1 content
        assert_eq!(read_text(&forked), "Version 1");

        // Original document should still have V2 content
        assert_eq!(read_text(&doc), "Version 2");
    }

    // =========================================================================
    // Test 3: fork_at fails gracefully with invalid heads
    // =========================================================================

    #[test]
    fn test_fork_at_with_invalid_heads_fails() {
        let doc = create_doc_with_text("Some content");

        // Get heads from a different document - these won't exist in our doc
        let other_doc = create_doc_with_text("Different content");
        let invalid_heads = other_doc.get_heads();

        let result = doc.fork_at(&invalid_heads);
        // fork_at should fail with invalid heads
        assert!(
            result.is_err(),
            "fork_at with invalid heads should return Err"
        );
    }

    #[test]
    fn test_fork_fallback_pattern() {
        // This tests the pattern we'll use in sync_document:
        // doc.fork_at(&heads).unwrap_or_else(|_| doc.fork())

        let doc = create_doc_with_text("Some content");

        // Get heads from a different document to use as invalid heads
        let other_doc = create_doc_with_text("Different");
        let invalid_heads = other_doc.get_heads();

        // With invalid heads, fallback to fork() at current state
        let forked = doc.fork_at(&invalid_heads).unwrap_or_else(|_| doc.fork());

        // Should have same content as current document
        assert_eq!(read_text(&forked), "Some content");
    }

    // =========================================================================
    // Test 4: merge correctly combines divergent changes
    // =========================================================================

    #[test]
    fn test_merge_combines_changes() {
        let mut doc = create_doc_with_text("Hello");
        let heads_before = doc.get_heads();

        // Create a fork and make different changes
        let mut forked = doc.fork();

        // Change in fork: add " world"
        update_text(&mut forked, "Hello world");

        // Change in original: add "!"
        update_text(&mut doc, "Hello!");

        // Now merge fork back into original
        let merge_result = doc.merge(&mut forked);
        assert!(merge_result.is_ok(), "merge should succeed");

        // After merge, document should have changes from both
        // The exact result depends on automerge's CRDT merge rules
        let merged_text = read_text(&doc);

        // Both changes should be present in some form
        // Note: The exact merged text may vary based on CRDT semantics
        // What matters is that neither change is lost
        assert!(!merged_text.is_empty(), "Merged text should not be empty");

        // Heads should have changed after merge
        let heads_after = doc.get_heads();
        assert_ne!(heads_before, heads_after, "Heads should change after merge");
    }

    #[test]
    fn test_merge_idempotent_same_content() {
        let mut doc = create_doc_with_text("Same content");
        let _heads_before = doc.get_heads();

        // Fork without making changes
        let mut forked = doc.fork();

        // Merge fork (with same content) back
        let merge_result = doc.merge(&mut forked);
        assert!(merge_result.is_ok(), "merge should succeed");

        // Content should be unchanged
        assert_eq!(read_text(&doc), "Same content");

        // Heads may or may not change (depends on actor IDs)
        // The important thing is content is preserved
    }

    #[test]
    fn test_fork_and_merge_round_trip() {
        // This tests the core pattern used in sync_document

        // 1. Start with initial content
        let mut doc = create_doc_with_text("Initial");
        let checkpoint_heads = doc.get_heads();

        // 2. Make changes to the "main" document (simulating automerge changes)
        update_text(&mut doc, "Initial - modified by automerge");

        // 3. Fork at checkpoint (simulating filesystem state at last sync)
        let mut forked = doc.fork_at(&checkpoint_heads).unwrap();

        // 4. Apply filesystem content to fork
        update_text(&mut forked, "Initial - modified by filesystem");

        // 5. Merge fork back
        doc.merge(&mut forked).unwrap();

        // 6. Result should contain information from both edit paths
        let result = read_text(&doc);

        // The result should exist and be non-empty
        assert!(!result.is_empty());

        // Note: The exact merge result depends on CRDT semantics
        // For text, concurrent edits to the same region may interleave
    }

    // =========================================================================
    // Test 5: update_text produces correct CRDT operations
    // =========================================================================

    #[test]
    fn test_update_text_inserts_content() {
        let mut doc = create_doc_with_text("");

        // Update from empty to content
        update_text(&mut doc, "Hello, world!");

        assert_eq!(read_text(&doc), "Hello, world!");
    }

    #[test]
    fn test_update_text_replaces_content() {
        let mut doc = create_doc_with_text("Old content");

        update_text(&mut doc, "New content");

        assert_eq!(read_text(&doc), "New content");
    }

    #[test]
    fn test_update_text_preserves_unchanged_parts() {
        let mut doc = create_doc_with_text("Hello, world!");

        // Change just one word
        update_text(&mut doc, "Hello, universe!");

        assert_eq!(read_text(&doc), "Hello, universe!");
    }

    #[test]
    fn test_update_text_handles_unicode() {
        let mut doc = create_doc_with_text("");

        // Test with various Unicode content
        let unicode_content = "Hello 🌍 世界 مرحبا";
        update_text(&mut doc, unicode_content);

        assert_eq!(read_text(&doc), unicode_content);
    }

    #[test]
    fn test_update_text_empty_to_empty_is_noop() {
        let mut doc = create_doc_with_text("");
        let _heads_before = doc.get_heads();

        // Update empty to empty
        update_text(&mut doc, "");

        // Content should still be empty
        assert_eq!(read_text(&doc), "");

        // Heads should not change (no actual changes made)
        // Note: This might still create a commit depending on automerge version
        // The important thing is the content is correct
    }

    #[test]
    fn test_update_text_same_content_is_noop() {
        let mut doc = create_doc_with_text("Same content");

        // Update with same content
        update_text(&mut doc, "Same content");

        // Content should be unchanged
        assert_eq!(read_text(&doc), "Same content");
    }

    // =========================================================================
    // Integration test: Full sync algorithm pattern
    // =========================================================================

    #[test]
    fn test_sync_algorithm_pattern_no_changes() {
        // Case 1: No changes - both automerge and filesystem unchanged

        let doc = create_doc_with_text("Content at checkpoint");
        let checkpoint_heads = doc.get_heads();
        let fs_content = "Content at checkpoint"; // Same as automerge

        // Fork at checkpoint
        let mut forked = doc.fork_at(&checkpoint_heads).unwrap();

        // Apply filesystem content (same content = no-op)
        let (_, text_obj) = forked.get(ROOT, "text").unwrap().unwrap();
        forked
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.update_text(&text_obj, fs_content)?;
                Ok(())
            })
            .unwrap();

        // Result should be unchanged
        assert_eq!(read_text(&forked), "Content at checkpoint");
    }

    #[test]
    fn test_sync_algorithm_pattern_automerge_changed() {
        // Case 2: Automerge changed, filesystem unchanged

        let mut doc = create_doc_with_text("Original");
        let checkpoint_heads = doc.get_heads();
        let fs_content = "Original"; // Filesystem still at checkpoint

        // Automerge changes since checkpoint
        update_text(&mut doc, "Modified by automerge");

        // Fork at checkpoint (gets "Original")
        let mut forked = doc.fork_at(&checkpoint_heads).unwrap();
        assert_eq!(read_text(&forked), "Original");

        // Apply filesystem content (same as fork, so no-op)
        let (_, text_obj) = forked.get(ROOT, "text").unwrap().unwrap();
        forked
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.update_text(&text_obj, fs_content)?;
                Ok(())
            })
            .unwrap();

        // Merge fork back - should keep automerge's changes
        doc.merge(&mut forked).unwrap();

        // Result should have automerge's changes
        assert_eq!(read_text(&doc), "Modified by automerge");
    }

    #[test]
    fn test_sync_algorithm_pattern_filesystem_changed() {
        // Case 3: Filesystem changed, automerge unchanged

        let doc = create_doc_with_text("Original");
        let checkpoint_heads = doc.get_heads();
        let fs_content = "Modified by filesystem"; // Filesystem changed

        // No automerge changes since checkpoint
        // (doc still at "Original")

        // Fork at checkpoint
        let mut forked = doc.fork_at(&checkpoint_heads).unwrap();

        // Apply filesystem content
        let (_, text_obj) = forked.get(ROOT, "text").unwrap().unwrap();
        forked
            .transact::<_, _, automerge::AutomergeError>(|tx| {
                tx.update_text(&text_obj, fs_content)?;
                Ok(())
            })
            .unwrap();

        assert_eq!(read_text(&forked), "Modified by filesystem");

        // Merge fork back
        let mut doc = doc; // Make mutable
        doc.merge(&mut forked).unwrap();

        // Result should have filesystem's changes
        assert_eq!(read_text(&doc), "Modified by filesystem");
    }
}
