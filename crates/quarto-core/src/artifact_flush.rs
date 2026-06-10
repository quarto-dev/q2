/*
 * artifact_flush.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Flush rendered artifacts into an in-memory VFS (bd-q3bxnq2e).
 */

//! Shared artifact → VFS flush used by the WASM render entry points.
//!
//! The hub-client's render tail populates the session VFS with every
//! artifact the pipeline produced (theme CSS, shared JS, fonts, plot
//! images, …) so the iframe post-processor can read them back at the
//! resolver's matching path (`/.quarto/project-artifacts/...` — the
//! Phase 9 "VFS is load-bearing across renders" contract; see
//! `crates/wasm-quarto-hub-client/CLAUDE.md`).
//!
//! Before bd-q3bxnq2e this loop lived inline (twice) in
//! `wasm-quarto-hub-client/src/lib.rs` and unconditionally
//! `content.clone()`d every artifact on every render. Centralizing it
//! here gives one code path for both WASM flush sites, the native
//! perf-harness proxy (`perf-harness/src/bin/vfs_flush.rs`), and unit
//! tests — and one place for the change-detection that skips
//! byte-identical re-writes.
//!
//! Contract notes:
//! - **bd-3gtn**: artifacts with empty content are manifest entries
//!   (`Artifact::from_path`) whose write target can alias the user's
//!   upload location — they must never be written. The skip lives
//!   here, ahead of the VFS, and is independent of change-detection.
//! - Artifacts without a `path` have no on-disk destination; skipped.
//! - Read-back contract: after a flush, every written artifact is
//!   readable at `resolver.on_disk_path_for(scope, path)` — whether
//!   the write was performed or skipped as byte-identical.

use quarto_system_runtime::VirtualFileSystem;

use crate::artifact::ArtifactStore;
use crate::resource_resolver::ResourceResolverContext;

/// Flush every path-bearing, non-empty artifact in `artifacts` into
/// `vfs` at its resolver-computed location, skipping writes whose
/// byte-identical content is already present
/// ([`VirtualFileSystem::add_file_if_changed`]).
pub fn flush_artifacts_to_vfs(
    artifacts: &ArtifactStore,
    resolver: &ResourceResolverContext,
    vfs: &mut VirtualFileSystem,
) {
    for (_key, artifact) in artifacts.iter() {
        let Some(artifact_path) = &artifact.path else {
            continue;
        };
        // bd-3gtn: empty content means "manifest entry", not "write
        // empty bytes" — see module docs.
        if artifact.content.is_empty() {
            continue;
        }
        let vfs_path = resolver.on_disk_path_for(artifact.scope, artifact_path);
        vfs.add_file_if_changed(&vfs_path, &artifact.content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{Artifact, ArtifactScope};
    use std::path::Path;

    /// Read an artifact back from the VFS at the path the flush would
    /// have used — the read-back contract helper.
    fn read_back(
        vfs: &VirtualFileSystem,
        resolver: &ResourceResolverContext,
        scope: ArtifactScope,
        path: &Path,
    ) -> Option<Vec<u8>> {
        vfs.read_file(&resolver.on_disk_path_for(scope, path)).ok()
    }

    fn resolver() -> ResourceResolverContext {
        ResourceResolverContext::vfs_root("/.quarto/project-artifacts")
    }

    fn themed_store() -> ArtifactStore {
        let mut store = ArtifactStore::new();
        store.store(
            "css:theme:abc123",
            Artifact::from_string("body { color: red; }", "text/css")
                .with_path("quarto/quarto-theme-abc123.css")
                .with_scope(ArtifactScope::Project),
        );
        store.store(
            "js:bootstrap",
            Artifact::from_bytes(vec![0x42; 1000], "text/javascript")
                .with_path("quarto/bootstrap.bundle.min.js")
                .with_scope(ArtifactScope::Project),
        );
        store.store(
            "image:fig-1",
            Artifact::from_bytes(vec![0x89, 0x50, 0x4E, 0x47], "image/png")
                .with_path("input_files/fig-1.png"), // Page scope (default)
        );
        store
    }

    /// Every path-bearing, non-empty artifact lands at the resolver's
    /// path and reads back byte-identical (Phase 9 read-back contract).
    #[test]
    fn flush_writes_artifacts_at_resolver_paths() {
        let store = themed_store();
        let r = resolver();
        let mut vfs = VirtualFileSystem::new();

        flush_artifacts_to_vfs(&store, &r, &mut vfs);

        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Project,
                Path::new("quarto/quarto-theme-abc123.css")
            )
            .as_deref(),
            Some(b"body { color: red; }".as_slice())
        );
        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Project,
                Path::new("quarto/bootstrap.bundle.min.js")
            ),
            Some(vec![0x42; 1000])
        );
        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Page,
                Path::new("input_files/fig-1.png")
            ),
            Some(vec![0x89, 0x50, 0x4E, 0x47])
        );
        let s = vfs.write_stats();
        assert_eq!(s.writes, 3);
        assert_eq!(s.bytes_written, 20 + 1000 + 4);
    }

    /// bd-3gtn: empty-content artifacts are manifest entries and must
    /// not be written, even though they carry a path.
    #[test]
    fn flush_skips_empty_content_artifacts() {
        let mut store = ArtifactStore::new();
        store.store(
            "resource:upload",
            Artifact::from_path("images/upload.png", "image/png"),
        );
        let r = resolver();
        let mut vfs = VirtualFileSystem::new();

        flush_artifacts_to_vfs(&store, &r, &mut vfs);

        assert_eq!(vfs.write_stats().writes, 0);
        assert!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Page,
                Path::new("images/upload.png")
            )
            .is_none()
        );
    }

    /// Artifacts without a path have no destination; skipped.
    #[test]
    fn flush_skips_pathless_artifacts() {
        let mut store = ArtifactStore::new();
        store.store(
            "intermediate:markdown:ch1",
            Artifact::from_string("# intermediate", "text/markdown"),
        );
        let mut vfs = VirtualFileSystem::new();

        flush_artifacts_to_vfs(&store, &resolver(), &mut vfs);

        assert_eq!(vfs.write_stats().writes, 0);
    }

    /// The keystroke steady state: a second flush of a byte-identical
    /// store performs zero writes — everything is skipped — and the
    /// read-back contract still holds.
    #[test]
    fn second_flush_of_unchanged_store_skips_all_writes() {
        let r = resolver();
        let mut vfs = VirtualFileSystem::new();

        // Render 1.
        flush_artifacts_to_vfs(&themed_store(), &r, &mut vfs);
        let after_first = vfs.write_stats();
        assert_eq!(after_first.writes, 3);

        // Render 2: rebuilt store (as every render does), same bytes.
        flush_artifacts_to_vfs(&themed_store(), &r, &mut vfs);
        let after_second = vfs.write_stats();

        assert_eq!(
            after_second.writes, after_first.writes,
            "unchanged artifacts must not be re-written"
        );
        assert_eq!(after_second.skipped_writes, 3);
        assert_eq!(after_second.bytes_skipped, 20 + 1000 + 4);

        // Read-back contract survives the skipped flush.
        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Project,
                Path::new("quarto/quarto-theme-abc123.css")
            )
            .as_deref(),
            Some(b"body { color: red; }".as_slice())
        );
    }

    /// A changed artifact is re-written; unchanged siblings are skipped.
    #[test]
    fn changed_artifact_rewritten_unchanged_skipped() {
        let r = resolver();
        let mut vfs = VirtualFileSystem::new();

        flush_artifacts_to_vfs(&themed_store(), &r, &mut vfs);

        // Theme edit: new fingerprint → new key AND new path, the way
        // CompileThemeCssStage actually behaves. Old theme entry stays
        // in VFS (stale but harmless); new one is written.
        let mut store2 = themed_store();
        store2.remove("css:theme:abc123");
        store2.store(
            "css:theme:def456",
            Artifact::from_string("body { color: blue; }", "text/css")
                .with_path("quarto/quarto-theme-def456.css")
                .with_scope(ArtifactScope::Project),
        );
        flush_artifacts_to_vfs(&store2, &r, &mut vfs);

        let s = vfs.write_stats();
        assert_eq!(s.writes, 4, "3 initial + 1 new theme");
        assert_eq!(s.skipped_writes, 2, "bootstrap.js + fig-1 skipped");
        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Project,
                Path::new("quarto/quarto-theme-def456.css")
            )
            .as_deref(),
            Some(b"body { color: blue; }".as_slice())
        );

        // Same-path content change (e.g. styles.css default CSS after
        // a doc-vars edit) must also re-write.
        let mut store3 = ArtifactStore::new();
        store3.store(
            "image:fig-1",
            Artifact::from_bytes(vec![0xFF, 0xD8], "image/jpeg").with_path("input_files/fig-1.png"),
        );
        flush_artifacts_to_vfs(&store3, &r, &mut vfs);
        assert_eq!(
            read_back(
                &vfs,
                &r,
                ArtifactScope::Page,
                Path::new("input_files/fig-1.png")
            ),
            Some(vec![0xFF, 0xD8])
        );
        assert_eq!(vfs.write_stats().writes, 5);
    }
}
