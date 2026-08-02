/*
 * artifact_flush.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The artifact-write family: one loop, four destinations
 * (bd-q3bxnq2e, bd-v8gx, bd-gdhk).
 */

//! Where rendered artifacts go, and who owns the write.
//!
//! Every render produces an [`ArtifactStore`]; this module owns the
//! *one* loop that turns those artifacts into writes, plus the small
//! set of entry points that differ only in **who owns the sink**:
//!
//! | Function | Destination | Sink lifecycle |
//! | --- | --- | --- |
//! | [`enqueue_artifacts`] | an [`OutputSink`] | **caller** constructs + flushes |
//! | [`flush_project_artifacts`] | an [`OutputSink`] | **owns** one, flushes it |
//! | [`route_drained_project_artifacts`] | accumulator *or* sink | caller's sink |
//! | [`flush_artifacts_to_vfs`] | a [`VirtualFileSystem`] | n/a (VFS is the sink) |
//!
//! The `enqueue_` / `flush_` verb prefix is the whole distinction:
//! `enqueue_*` adds to a sink somebody else will flush, `flush_*`
//! performs the write before returning.
//!
//! Before bd-v8gx / bd-gdhk these lived in three places under three
//! unrelated names — `render_to_file::enqueue_artifacts`,
//! `project::website_post_render::flush_site_libs` (misleading: two of
//! its three callers were not website renders), and the VFS loop below.
//! `render_to_file` is `#[cfg(not(target_arch = "wasm32"))]` as a
//! *module*, so the primitive was unreachable from WASM and
//! `flush_site_libs` had to duplicate it rather than call it. Hosting
//! the family here — an ungated module — is what lets them share one
//! loop.
//!
//! ## Scope contract
//!
//! [`enqueue_artifacts`] selects by `artifact.scope`; the resolver then
//! decides the destination root for that scope. Note that
//! [`ArtifactStore::merge_into_project`] inserts entries **verbatim**
//! and does not re-stamp scope, so "everything in the project
//! accumulator is Project-scoped" holds only by construction of its
//! callers (all of which feed it from
//! [`ArtifactStore::drain_project_scoped`]). Nothing enforces it, which
//! is why [`flush_project_artifacts`] `debug_assert!`s the invariant
//! instead of silently mis-routing or silently dropping a violation.
//!
//! ## VFS flush
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

use std::path::Path;

use quarto_system_runtime::{SystemRuntime, VirtualFileSystem};

use crate::Result;
use crate::artifact::{ArtifactScope, ArtifactStore};
use crate::error::QuartoError;
use crate::output_sink::OutputSink;
use crate::resource_resolver::ResourceResolverContext;

/// Enqueue every artifact in `store` whose scope matches `scope_filter`
/// into `sink` at its resolver-determined on-disk location. Skips
/// artifacts without a `path`.
///
/// The caller owns the sink lifecycle (construct, enqueue producers,
/// flush) — use this when artifacts share a sink with other writes, so
/// one validated flush covers the whole render. When there is nothing
/// to share a sink with, [`flush_project_artifacts`] wraps this.
///
/// Iteration is sorted-key so the resulting flush order is
/// deterministic across runs / platforms.
pub fn enqueue_artifacts(
    store: &ArtifactStore,
    resolver: &ResourceResolverContext,
    scope_filter: ArtifactScope,
    sink: &mut OutputSink,
) -> Result<()> {
    let mut entries: Vec<(&str, &crate::artifact::Artifact)> = store
        .iter()
        .filter(|(_, a)| a.scope == scope_filter)
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    for (_, artifact) in entries {
        let Some(path) = &artifact.path else { continue };
        let on_disk = resolver.on_disk_path_for(artifact.scope, path);
        sink.write(on_disk, artifact.content.clone())
            .map_err(QuartoError::from)?;
    }
    Ok(())
}

/// Flush every Project-scoped artifact in `store` to its
/// resolver-determined on-disk (or VFS) location, through a sink this
/// function owns.
///
/// Used by a project type's `post_render` hook, which has accumulated
/// artifacts across every Pass-2 render and has no other writes to
/// batch with. Callers that already hold an [`OutputSink`] should use
/// [`enqueue_artifacts`] directly instead of paying for a second one.
///
/// The resolver decides the destination: native website renders pass a
/// [`ResourceResolverContext::project_root`] resolver (artifacts land
/// under `{output_dir}/{lib_dir}/{path}`); the WASM hub-client passes a
/// [`ResourceResolverContext::vfs_root`] resolver (artifacts land under
/// `/{vfs_root}/{path}`).
///
/// Decoupling the destination from the function's logic enforces the
/// construction-level invariant from the Phase 9 plan §Decision 4: the
/// URL embedded in HTML by `html_url_for(Project, p)` and the on-disk
/// write path returned by `on_disk_path_for(Project, p)` must
/// round-trip through the same resolver.
///
/// An empty `store` is a no-op that touches no directories —
/// [`OutputSink::flush`] short-circuits before materializing allowed
/// roots.
///
/// # Panics
///
/// In debug builds, panics if `store` holds a non-Project-scoped entry.
/// See the module's scope contract: such an entry means a caller broke
/// the accumulator invariant, and both silent outcomes (writing it at
/// the Project root, or filtering it away) hide a real bug.
pub(crate) fn flush_project_artifacts(
    store: &ArtifactStore,
    resolver: &ResourceResolverContext,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    debug_assert!(
        store.iter().all(|(_, a)| a.scope == ArtifactScope::Project),
        "flush_project_artifacts requires every entry to be Project-scoped; \
         a non-Project entry here means a caller bypassed \
         ArtifactStore::drain_project_scoped"
    );

    // bd-cfl67: all destructive output flows through the validated
    // sink so we can never silently truncate a file outside the
    // resolver's declared output roots.
    let mut sink = OutputSink::new(resolver.allowed_output_roots());
    enqueue_artifacts(store, resolver, ArtifactScope::Project, &mut sink)?;
    sink.flush(runtime).map_err(QuartoError::from)?;
    Ok(())
}

/// Route a render's freshly-[drained] Project-scope artifacts to
/// whichever destination the project layout calls for.
///
/// Two outcomes, and the choice is *artifact-flow*, not payload-flow —
/// which is why all three Pass-2 / native render tails share it:
///
/// - **Shared lib dir + an accumulator** (e.g. websites, `lib_dir ==
///   "site_libs"`): merge into `accumulator` so the project type's
///   `post_render` can [`flush_project_artifacts`] them **once** across
///   the whole project, deduping identical bytes between pages.
/// - **Otherwise** (default projects with `lib_dir == ""`, or a
///   standalone render with no orchestrator): enqueue into `sink` for
///   in-place writing via the per-page resolver.
///
/// The `else` branch is load-bearing, not a fallback:
/// `DefaultProjectType::post_render` is a no-op, so anything left in
/// the accumulator would silently disappear and the hub-client iframe
/// would VFS-miss on the theme `<link>` URL its own HTML embeds
/// (bd-87fu).
///
/// `input` names the document being rendered, for the merge-conflict
/// diagnostic.
///
/// [drained]: ArtifactStore::drain_project_scoped
pub(crate) fn route_drained_project_artifacts(
    drained: ArtifactStore,
    accumulator: Option<&mut ArtifactStore>,
    has_shared_lib: bool,
    resolver: &ResourceResolverContext,
    sink: &mut OutputSink,
    input: &Path,
) -> Result<()> {
    match (accumulator, has_shared_lib) {
        (Some(dest), true) => {
            dest.merge_into_project(drained).map_err(|e| {
                QuartoError::other(format!(
                    "Project-scoped artifact merge failed for {}: {}",
                    input.display(),
                    e
                ))
            })?;
        }
        _ => {
            enqueue_artifacts(&drained, resolver, ArtifactScope::Project, sink)?;
        }
    }
    Ok(())
}

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

    // ═══════════════════════════════════════════════════════════════
    // bd-v8gx — flush_project_artifacts
    // ═══════════════════════════════════════════════════════════════

    /// A `vfs_root` resolver writes Project-scoped artifacts to
    /// `<vfs_root>/<artifact_path>` — the hub-client convention,
    /// where `vfs_root == "/.quarto/project-artifacts"`.
    #[test]
    fn flush_project_artifacts_vfs_root_writes_under_vfs_root() {
        use quarto_system_runtime::NativeRuntime;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let vfs_root = temp.path().join(".quarto/project-artifacts");
        let resolver = ResourceResolverContext::vfs_root(vfs_root.clone());

        let mut store = ArtifactStore::new();
        store.store(
            "theme",
            Artifact::from_bytes(b"body { color: red; }".to_vec(), "text/css")
                .with_path("quarto/theme.css")
                .with_scope(ArtifactScope::Project),
        );

        flush_project_artifacts(&store, &resolver, &NativeRuntime::new()).unwrap();

        let written = vfs_root.join("quarto/theme.css");
        let bytes = std::fs::read(&written)
            .unwrap_or_else(|e| panic!("expected artifact at {}: {}", written.display(), e));
        assert_eq!(bytes, b"body { color: red; }");
    }

    /// An empty store creates no directories. This is the guard for
    /// dropping the old explicit `is_empty()` early return:
    /// `OutputSink::flush` short-circuits on empty `ops` *before*
    /// materializing allowed roots, so no directory is created.
    #[test]
    fn flush_project_artifacts_empty_store_is_noop() {
        use quarto_system_runtime::NativeRuntime;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let vfs_root = temp.path().join("vfs");
        let resolver = ResourceResolverContext::vfs_root(vfs_root.clone());

        flush_project_artifacts(&ArtifactStore::new(), &resolver, &NativeRuntime::new()).unwrap();

        assert!(!vfs_root.exists(), "no-op flush must not touch the FS");
    }

    /// The native website resolver routes Project-scope artifacts
    /// under `{site_root}/{lib_dir}/`. Same function as the VFS case,
    /// different destination, decided entirely by the resolver
    /// (Phase 9 §Decision 4).
    #[test]
    fn flush_project_artifacts_native_website_routes_under_lib_dir() {
        use quarto_system_runtime::NativeRuntime;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let site_root = temp.path().join("_site");
        let resolver = ResourceResolverContext::project_root(site_root.clone(), "site_libs");

        let mut store = ArtifactStore::new();
        store.store(
            "kbd",
            Artifact::from_bytes(b"kbd { font-family: monospace; }".to_vec(), "text/css")
                .with_path("libs/kbd/kbd.css")
                .with_scope(ArtifactScope::Project),
        );

        flush_project_artifacts(&store, &resolver, &NativeRuntime::new()).unwrap();

        let written = site_root.join("site_libs/libs/kbd/kbd.css");
        assert_eq!(
            std::fs::read(&written).ok().as_deref(),
            Some(b"kbd { font-family: monospace; }".as_slice()),
            "expected artifact at {}",
            written.display()
        );
    }

    /// **The one deliberate behavior change on this branch.**
    ///
    /// This function's predecessor called
    /// `on_disk_path_for(Project, p)` unconditionally, so a
    /// non-Project entry that reached it was
    /// written *at the Project root* — silently misplaced.
    /// `flush_project_artifacts` delegates to [`enqueue_artifacts`],
    /// which filters on `artifact.scope`, so such an entry would instead
    /// be silently dropped. Neither silent outcome is good: nothing
    /// enforces the invariant, because `merge_into_project`
    /// (`artifact.rs:344`) inserts entries verbatim without re-stamping
    /// scope. We make the violation loud in dev instead.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Project-scoped")]
    fn flush_project_artifacts_debug_asserts_on_non_project_entry() {
        use quarto_system_runtime::NativeRuntime;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let resolver = ResourceResolverContext::vfs_root(temp.path().join("vfs"));

        let mut store = ArtifactStore::new();
        store.store(
            "page-scoped-intruder",
            Artifact::from_bytes(b"x".to_vec(), "text/css").with_path("oops.css"),
        );

        let _ = flush_project_artifacts(&store, &resolver, &NativeRuntime::new());
    }

    /// Release-mode counterpart: with `debug_assert` compiled out, the
    /// scope filter drops the entry rather than misplacing it.
    #[cfg(not(debug_assertions))]
    #[test]
    fn flush_project_artifacts_filters_non_project_entry_in_release() {
        use quarto_system_runtime::NativeRuntime;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let vfs_root = temp.path().join("vfs");
        let resolver = ResourceResolverContext::vfs_root(vfs_root.clone());

        let mut store = ArtifactStore::new();
        store.store(
            "page-scoped-intruder",
            Artifact::from_bytes(b"x".to_vec(), "text/css").with_path("oops.css"),
        );

        flush_project_artifacts(&store, &resolver, &NativeRuntime::new()).unwrap();

        assert!(
            !vfs_root.join("oops.css").exists(),
            "a Page-scoped entry must not be written at the Project root"
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // bd-gdhk — route_drained_project_artifacts
    // ═══════════════════════════════════════════════════════════════

    fn drained_project_store() -> ArtifactStore {
        let mut store = ArtifactStore::new();
        store.store(
            "css:theme:abc123",
            Artifact::from_string("body { color: red; }", "text/css")
                .with_path("quarto/quarto-theme-abc123.css")
                .with_scope(ArtifactScope::Project),
        );
        store
    }

    /// Shared lib dir (websites) **and** an accumulator present: the
    /// drained artifacts merge into the accumulator so the project
    /// type's `post_render` can flush them once for the whole project.
    /// Nothing is enqueued for writing here.
    #[test]
    fn route_shared_lib_with_accumulator_merges_and_writes_nothing() {
        let resolver = ResourceResolverContext::project_root("/tmp/_site", "site_libs");
        let mut accumulator = ArtifactStore::new();
        let mut sink = OutputSink::new(resolver.allowed_output_roots());

        route_drained_project_artifacts(
            drained_project_store(),
            Some(&mut accumulator),
            true,
            &resolver,
            &mut sink,
            std::path::Path::new("index.qmd"),
        )
        .unwrap();

        assert_eq!(sink.pending(), 0, "merge path must not enqueue writes");
        assert_eq!(
            accumulator
                .get("css:theme:abc123")
                .map(|a| a.content.as_slice()),
            Some(b"body { color: red; }".as_slice()),
            "artifact must land in the accumulator verbatim"
        );
    }

    /// No shared lib dir (default projects, `lib_dir == ""`): the
    /// drained artifacts are enqueued for in-place writing via the
    /// per-page resolver. This is the bd-87fu path — leaving them in an
    /// accumulator that `DefaultProjectType::post_render` ignores made
    /// themes silently disappear.
    #[test]
    fn route_no_shared_lib_enqueues_for_in_place_write() {
        let resolver = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        let mut accumulator = ArtifactStore::new();
        let mut sink = OutputSink::new(resolver.allowed_output_roots());

        route_drained_project_artifacts(
            drained_project_store(),
            Some(&mut accumulator),
            false,
            &resolver,
            &mut sink,
            std::path::Path::new("index.qmd"),
        )
        .unwrap();

        assert_eq!(sink.pending(), 1, "expected one enqueued write");
        assert_eq!(
            accumulator.len(),
            0,
            "in-place path must not touch the accumulator"
        );
    }

    /// Standalone render (no orchestrator, so no accumulator) even with
    /// a shared lib dir: enqueue in place. Mirrors the `_ =>` arm of
    /// `render_document_to_file`'s `match (project_artifacts, has_shared_lib)`.
    #[test]
    fn route_without_accumulator_enqueues_even_with_shared_lib() {
        let resolver = ResourceResolverContext::project_root("/tmp/_site", "site_libs");
        let mut sink = OutputSink::new(resolver.allowed_output_roots());

        route_drained_project_artifacts(
            drained_project_store(),
            None,
            true,
            &resolver,
            &mut sink,
            std::path::Path::new("index.qmd"),
        )
        .unwrap();

        assert_eq!(sink.pending(), 1, "expected one enqueued write");
    }

    /// A merge conflict (same key, different bytes) surfaces as an error
    /// naming the input document — the diagnostic the three render sites
    /// each hand-rolled before this helper existed.
    #[test]
    fn route_merge_conflict_error_names_the_input_document() {
        let resolver = ResourceResolverContext::project_root("/tmp/_site", "site_libs");
        let mut accumulator = ArtifactStore::new();
        accumulator.store(
            "css:theme:abc123",
            Artifact::from_string("body { color: blue; }", "text/css")
                .with_path("quarto/quarto-theme-abc123.css")
                .with_scope(ArtifactScope::Project),
        );
        let mut sink = OutputSink::new(resolver.allowed_output_roots());

        let err = route_drained_project_artifacts(
            drained_project_store(),
            Some(&mut accumulator),
            true,
            &resolver,
            &mut sink,
            std::path::Path::new("chapters/intro.qmd"),
        )
        .expect_err("conflicting bytes under one key must error");

        let msg = err.to_string();
        assert!(
            msg.contains("chapters/intro.qmd"),
            "error must name the input document, got: {msg}"
        );
    }
}
