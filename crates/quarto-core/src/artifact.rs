/*
 * artifact.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Unified artifact storage for the render pipeline.
 */

//! Artifact store for pipeline intermediates and dependencies.
//!
//! The `ArtifactStore` is a unified key-value storage system for:
//! - Intermediate documents (e.g., markdown for book PDF compilation)
//! - Supporting files (images, data files from code execution)
//! - CSS/JS dependency files to be copied to output
//! - Source maps for error reporting
//!
//! All values are stored as byte buffers with content type hints,
//! enabling both text and binary artifacts.

use std::collections::HashMap;
use std::path::PathBuf;

/// Scope of an artifact: where it lives in the rendered output.
///
/// - [`ArtifactScope::Page`]: artifact belongs to a single page
///   (e.g. an engine-generated figure). Lands under that page's
///   per-page resource directory (`{stem}_files/...`).
/// - [`ArtifactScope::Project`]: artifact is shared across the
///   project (e.g. theme CSS, extension dependencies). In a
///   website project this lands under
///   `{output_dir}/{lib_dir}/...` once, deduplicated across
///   pages. In a default (single-doc) project this resolves to
///   the same per-page resource directory as `Page` — there's no
///   separate shared location.
///
/// See `claude-notes/plans/2026-04-24-websites-phase-5.md`
/// Decision 1 for the design rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtifactScope {
    /// Per-page artifact (default). Lands under `{stem}_files/`.
    #[default]
    Page,
    /// Project-shared artifact. Lands under `{lib_dir}/` in
    /// projects that have one (websites: `site_libs/`); falls
    /// back to per-page resource dir for default projects.
    Project,
}

/// An artifact stored during rendering.
///
/// Can represent text, binary data, or structured data.
/// The content_type provides a hint for downstream consumers.
#[derive(Debug, Clone)]
pub struct Artifact {
    /// Raw content as bytes
    pub content: Vec<u8>,

    /// Content type hint (MIME type or custom identifier)
    ///
    /// Examples:
    /// - `"text/markdown"` for intermediate markdown
    /// - `"text/css"` for stylesheets
    /// - `"text/javascript"` for scripts
    /// - `"image/png"` for images
    /// - `"application/json"` for structured data
    pub content_type: String,

    /// Optional file path if this artifact corresponds to a file on disk
    pub path: Option<PathBuf>,

    /// Arbitrary metadata for downstream consumers
    pub metadata: HashMap<String, serde_json::Value>,

    /// Scope: per-page or project-shared. See [`ArtifactScope`].
    ///
    /// Defaults to [`ArtifactScope::Page`] so the field is opt-in
    /// for producers — flipping a producer to `Project` is the
    /// explicit signal that its output is shareable across pages.
    pub scope: ArtifactScope,

    /// Extra HTML attributes for the `<link>` / `<script>` tag this
    /// artifact is emitted with by `ApplyTemplateStage` (e.g. the
    /// light/dark theme sheets' `class` / `id` / `data-mode` —
    /// bd-0pic6 A3). Empty (the default) emits today's plain tag.
    /// Order-preserving `Vec` rather than a map: attribute order is
    /// part of the deterministic output.
    pub link_attribs: Vec<(String, String)>,

    /// Sort priority for `<link>` / `<script>` emission:
    /// `ApplyTemplateStage` orders entries by `(link_order, key)`.
    /// `0` (the default) preserves the pre-existing pure
    /// lexicographic-key order; the theme sheets use positive values
    /// to pin the FOUC-safe light → dark → default-copy sequence.
    pub link_order: i32,
}

impl Artifact {
    /// Create a new artifact from bytes
    pub fn from_bytes(content: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            content,
            content_type: content_type.into(),
            path: None,
            metadata: HashMap::new(),
            scope: ArtifactScope::default(),
            link_attribs: Vec::new(),
            link_order: 0,
        }
    }

    /// Create a new artifact from a string
    pub fn from_string(content: impl Into<String>, content_type: impl Into<String>) -> Self {
        Self {
            content: content.into().into_bytes(),
            content_type: content_type.into(),
            path: None,
            metadata: HashMap::new(),
            scope: ArtifactScope::default(),
            link_attribs: Vec::new(),
            link_order: 0,
        }
    }

    /// Create an artifact representing a file path (without loading content).
    ///
    /// The content is empty; this is used to record that a file at a given
    /// path is needed as a resource.
    ///
    /// `path` must be a **relative** destination — the resolver
    /// joins it under a scope-specific root (`{stem}_files/` or
    /// `{lib_dir}/`) when computing the on-disk write location. An
    /// absolute path silently bypasses that scope root (Rust's
    /// `Path::join` returns absolute paths unchanged) and was the
    /// producer-side half of bd-cfl67. Caught in dev/CI by the
    /// debug-assert; release builds rely on
    /// [`crate::output_sink::OutputSink`]'s `allowed_roots` check.
    pub fn from_path(path: impl Into<PathBuf>, content_type: impl Into<String>) -> Self {
        let path = path.into();
        // `has_root()` rather than `is_absolute()`: the latter is
        // false on `wasm32-unknown-unknown` for paths like `/foo`,
        // but those paths are still "rooted" enough to bypass
        // `scope_root.join` in the resolver. See bd-cfl67.
        debug_assert!(
            !path.has_root(),
            "Artifact::from_path requires a relative destination path (got {}); root-prefixed paths bypass the resolver's scope root and risk overwriting source files (bd-cfl67)",
            path.display(),
        );
        Self {
            content: Vec::new(),
            content_type: content_type.into(),
            path: Some(path),
            metadata: HashMap::new(),
            scope: ArtifactScope::default(),
            link_attribs: Vec::new(),
            link_order: 0,
        }
    }

    /// Set the file path for this artifact.
    ///
    /// Same relative-path contract as [`Artifact::from_path`] — see
    /// its docs.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        // See `from_path` for why this uses `has_root()` not
        // `is_absolute()`.
        debug_assert!(
            !path.has_root(),
            "Artifact::with_path requires a relative destination path (got {}); root-prefixed paths bypass the resolver's scope root and risk overwriting source files (bd-cfl67)",
            path.display(),
        );
        self.path = Some(path);
        self
    }

    /// Add metadata to this artifact
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Set the scope (per-page or project-shared) for this artifact.
    pub fn with_scope(mut self, scope: ArtifactScope) -> Self {
        self.scope = scope;
        self
    }

    /// Set the extra HTML attributes for the emitted `<link>` /
    /// `<script>` tag. See [`Artifact::link_attribs`].
    pub fn with_link_attribs(
        mut self,
        attribs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.link_attribs = attribs
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// Set the emission sort priority. See [`Artifact::link_order`].
    pub fn with_link_order(mut self, order: i32) -> Self {
        self.link_order = order;
        self
    }

    /// Get content as UTF-8 string (lossy conversion for non-UTF8 data)
    pub fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.content).into_owned()
    }

    /// Get content as UTF-8 string if valid
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.content).ok()
    }

    /// Check if this is a text-based artifact
    pub fn is_text(&self) -> bool {
        self.content_type.starts_with("text/") || self.content_type == "application/json"
    }
}

/// Unified artifact storage, shared between dependency system and intermediates.
///
/// Keys are string identifiers that follow a namespace convention:
/// - `"css:<name>"` for CSS dependencies
/// - `"js:<name>"` for JavaScript dependencies
/// - `"intermediate:<format>:<id>"` for intermediate documents
/// - `"execution:<type>:<id>"` for execution outputs
/// - `"resource:<path>"` for resource files
#[derive(Debug, Default)]
pub struct ArtifactStore {
    /// Artifacts keyed by string identifier
    artifacts: HashMap<String, Artifact>,
}

impl ArtifactStore {
    /// Create a new empty artifact store
    pub fn new() -> Self {
        Self {
            artifacts: HashMap::new(),
        }
    }

    /// Store an artifact by key
    pub fn store(&mut self, key: impl Into<String>, artifact: Artifact) {
        self.artifacts.insert(key.into(), artifact);
    }

    /// Store text content with a content type
    pub fn store_text(
        &mut self,
        key: impl Into<String>,
        content: impl Into<String>,
        content_type: impl Into<String>,
    ) {
        self.store(key, Artifact::from_string(content, content_type));
    }

    /// Store bytes with a content type
    pub fn store_bytes(
        &mut self,
        key: impl Into<String>,
        content: Vec<u8>,
        content_type: impl Into<String>,
    ) {
        self.store(key, Artifact::from_bytes(content, content_type));
    }

    /// Retrieve artifact by key
    pub fn get(&self, key: &str) -> Option<&Artifact> {
        self.artifacts.get(key)
    }

    /// Retrieve mutable artifact by key
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Artifact> {
        self.artifacts.get_mut(key)
    }

    /// Remove an artifact by key
    pub fn remove(&mut self, key: &str) -> Option<Artifact> {
        self.artifacts.remove(key)
    }

    /// Check if an artifact exists
    pub fn contains(&self, key: &str) -> bool {
        self.artifacts.contains_key(key)
    }

    /// Get all artifacts matching a prefix
    ///
    /// Useful for collecting related artifacts, e.g.:
    /// - `"intermediate:markdown:"` for all chapter markdowns
    /// - `"css:"` for all CSS dependencies
    pub fn get_by_prefix(&self, prefix: &str) -> Vec<(&str, &Artifact)> {
        self.artifacts
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Get all artifact keys
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.artifacts.keys().map(|s| s.as_str())
    }

    /// Get all artifacts
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Artifact)> {
        self.artifacts.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Get the number of stored artifacts
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    /// Clear all artifacts
    pub fn clear(&mut self) {
        self.artifacts.clear();
    }

    // === Phase 5: scope-aware drain / merge ===

    /// Iterate over keys whose artifact is [`ArtifactScope::Project`].
    pub fn project_scoped_keys(&self) -> impl Iterator<Item = &str> {
        self.artifacts
            .iter()
            .filter(|(_, a)| a.scope == ArtifactScope::Project)
            .map(|(k, _)| k.as_str())
    }

    /// Iterate over keys whose artifact is [`ArtifactScope::Page`].
    pub fn page_scoped_keys(&self) -> impl Iterator<Item = &str> {
        self.artifacts
            .iter()
            .filter(|(_, a)| a.scope == ArtifactScope::Page)
            .map(|(k, _)| k.as_str())
    }

    /// Move every [`ArtifactScope::Project`] artifact out of `self`
    /// into a fresh [`ArtifactStore`].
    ///
    /// `self` retains all [`ArtifactScope::Page`] entries unchanged.
    /// The returned store inherits each artifact's scope tag (so it
    /// stays `Project` for downstream merging).
    ///
    /// See `claude-notes/plans/2026-04-24-websites-phase-5.md`
    /// Decision 2 for the parallelism contract this enables.
    pub fn drain_project_scoped(&mut self) -> ArtifactStore {
        // Snapshot keys first to avoid aliasing the borrow on iter.
        let mut keys: Vec<String> = self.project_scoped_keys().map(String::from).collect();
        keys.sort(); // determinism for downstream iteration
        let mut out = ArtifactStore::new();
        for k in keys {
            if let Some(a) = self.artifacts.remove(&k) {
                out.artifacts.insert(k, a);
            }
        }
        out
    }

    /// Merge `other` into `self`, treating identical-key entries as
    /// dedup candidates.
    ///
    /// Behavior per key in `other`:
    /// - If the key is **absent** in `self`: insert.
    /// - If the key is **present** with byte-equal content: count as
    ///   deduped; do not insert again.
    /// - If the key is **present** with different content: return
    ///   `Err(ArtifactMergeConflict)` naming the key. `self` is
    ///   left in a partially-merged state — the caller is expected
    ///   to abort the build, so this is acceptable.
    ///
    /// Iteration over `other` is in sorted-key order so error
    /// reporting is deterministic across runs / threads.
    ///
    /// Consumes `other` because drained artifacts should not be
    /// reused after a merge attempt.
    pub fn merge_into_project(
        &mut self,
        other: ArtifactStore,
    ) -> std::result::Result<MergeStats, ArtifactMergeConflict> {
        let mut entries: Vec<(String, Artifact)> = other.artifacts.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut stats = MergeStats::default();
        for (key, incoming) in entries {
            match self.artifacts.get(&key) {
                Some(existing) => {
                    if existing.content == incoming.content {
                        stats.deduped += 1;
                    } else {
                        return Err(ArtifactMergeConflict {
                            key,
                            existing_len: existing.content.len(),
                            incoming_len: incoming.content.len(),
                        });
                    }
                }
                None => {
                    self.artifacts.insert(key, incoming);
                    stats.inserted += 1;
                }
            }
        }
        Ok(stats)
    }
}

/// Outcome of a successful [`ArtifactStore::merge_into_project`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeStats {
    /// Number of new entries inserted into the destination.
    pub inserted: usize,
    /// Number of entries skipped because the same key was already
    /// present with byte-equal content.
    pub deduped: usize,
}

/// Reason a merge attempt aborted: two artifacts share a key but
/// carry different content. The orchestrator is expected to format
/// a user-facing diagnostic that names the source documents
/// (which the store itself doesn't track).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMergeConflict {
    /// The artifact key whose content differed.
    pub key: String,
    /// Length in bytes of the entry already in the destination
    /// store. Useful for diagnostics ("you already have N bytes,
    /// new doc is producing M bytes").
    pub existing_len: usize,
    /// Length in bytes of the new entry that triggered the
    /// conflict.
    pub incoming_len: usize,
}

impl std::fmt::Display for ArtifactMergeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "artifact key '{}' has conflicting content (existing: {} bytes, incoming: {} bytes)",
            self.key, self.existing_len, self.incoming_len
        )
    }
}

impl std::error::Error for ArtifactMergeConflict {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_from_string() {
        let artifact = Artifact::from_string("body { color: red; }", "text/css");
        assert_eq!(artifact.as_string(), "body { color: red; }");
        assert_eq!(artifact.content_type, "text/css");
        assert!(artifact.is_text());
    }

    #[test]
    fn test_artifact_from_bytes() {
        let png_header = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        let artifact = Artifact::from_bytes(png_header.clone(), "image/png");
        assert_eq!(artifact.content, png_header);
        assert!(!artifact.is_text());
    }

    #[test]
    fn test_artifact_with_path_and_metadata() {
        // Path must be relative (see Artifact::with_path docs / bd-cfl67).
        let artifact = Artifact::from_string("content", "text/plain")
            .with_path("path/to/file.txt")
            .with_metadata("version", serde_json::json!("1.0"));

        assert_eq!(artifact.path, Some(PathBuf::from("path/to/file.txt")));
        assert_eq!(
            artifact.metadata.get("version"),
            Some(&serde_json::json!("1.0"))
        );
    }

    #[test]
    fn test_artifact_store_basic() {
        let mut store = ArtifactStore::new();

        store.store_text("css:bootstrap", ".btn { }", "text/css");
        store.store_bytes("image:logo", vec![0x89, 0x50], "image/png");

        assert!(store.contains("css:bootstrap"));
        assert!(store.contains("image:logo"));
        assert!(!store.contains("nonexistent"));

        let css = store.get("css:bootstrap").unwrap();
        assert_eq!(css.as_string(), ".btn { }");
    }

    #[test]
    fn test_artifact_store_prefix_query() {
        let mut store = ArtifactStore::new();

        store.store_text("css:bootstrap", "bootstrap", "text/css");
        store.store_text("css:quarto", "quarto", "text/css");
        store.store_text("js:highlight", "highlight", "text/javascript");

        let css_artifacts = store.get_by_prefix("css:");
        assert_eq!(css_artifacts.len(), 2);

        let js_artifacts = store.get_by_prefix("js:");
        assert_eq!(js_artifacts.len(), 1);
    }

    #[test]
    fn test_artifact_store_remove() {
        let mut store = ArtifactStore::new();
        store.store_text("key", "value", "text/plain");

        assert!(store.contains("key"));
        let removed = store.remove("key");
        assert!(removed.is_some());
        assert!(!store.contains("key"));
    }

    // === Phase 5 Decision 1: ArtifactScope ===

    /// Plan test 1: every constructor produces an artifact whose
    /// scope defaults to Page. Producers must explicitly opt into
    /// Project scope.
    #[test]
    fn artifact_default_scope_is_page() {
        let a = Artifact::from_string("body { color: red; }", "text/css");
        assert_eq!(a.scope, ArtifactScope::Page);

        let b = Artifact::from_bytes(vec![0u8, 1, 2], "image/png");
        assert_eq!(b.scope, ArtifactScope::Page);

        let c = Artifact::from_path("relative/file.css", "text/css");
        assert_eq!(c.scope, ArtifactScope::Page);
    }

    /// Plan test 2: `.with_scope()` sets the scope without
    /// touching content / content_type / path / metadata.
    #[test]
    fn artifact_with_scope_builder_only_touches_scope() {
        let original = Artifact::from_string("body {}", "text/css")
            .with_path("styles.css")
            .with_metadata("k", serde_json::json!("v"));

        let scoped = original.clone().with_scope(ArtifactScope::Project);

        assert_eq!(scoped.scope, ArtifactScope::Project);
        assert_eq!(scoped.content, original.content);
        assert_eq!(scoped.content_type, original.content_type);
        assert_eq!(scoped.path, original.path);
        assert_eq!(scoped.metadata.len(), original.metadata.len());
        assert_eq!(scoped.metadata.get("k"), original.metadata.get("k"));
    }

    /// Plan test 3: scope survives store/get round-trip.
    #[test]
    fn artifact_scope_round_trips_through_store() {
        let mut store = ArtifactStore::new();
        store.store(
            "css:project",
            Artifact::from_string("project", "text/css").with_scope(ArtifactScope::Project),
        );
        store.store(
            "css:page",
            Artifact::from_string("page", "text/css"), // default Page
        );

        assert_eq!(
            store.get("css:project").unwrap().scope,
            ArtifactScope::Project
        );
        assert_eq!(store.get("css:page").unwrap().scope, ArtifactScope::Page);
    }

    // === Phase 5 Decision 2 / 3: drain + merge ===

    /// Plan test 11: drain returns Project entries; Page entries
    /// stay in the source store.
    #[test]
    fn drain_returns_only_project_scoped() {
        let mut per_doc = ArtifactStore::new();
        per_doc.store(
            "css:theme:abc",
            Artifact::from_string("theme", "text/css").with_scope(ArtifactScope::Project),
        );
        per_doc.store(
            "image:fig-1",
            Artifact::from_bytes(vec![0xff, 0xd8], "image/jpeg"), // Page (default)
        );

        let drained = per_doc.drain_project_scoped();

        assert_eq!(drained.len(), 1);
        assert!(drained.contains("css:theme:abc"));
        assert_eq!(per_doc.len(), 1);
        assert!(per_doc.contains("image:fig-1"));
        assert!(!per_doc.contains("css:theme:abc"));
    }

    /// Plan test 12: two drains with same key + same bytes
    /// dedup; the project store has exactly one entry.
    #[test]
    fn merge_dedupes_byte_equal_keys() {
        let mut project = ArtifactStore::new();

        let make = || {
            let mut s = ArtifactStore::new();
            s.store(
                "css:theme:abc",
                Artifact::from_string("body{}", "text/css").with_scope(ArtifactScope::Project),
            );
            s
        };

        let stats1 = project.merge_into_project(make()).unwrap();
        assert_eq!(stats1.inserted, 1);
        assert_eq!(stats1.deduped, 0);

        let stats2 = project.merge_into_project(make()).unwrap();
        assert_eq!(stats2.inserted, 0);
        assert_eq!(stats2.deduped, 1);

        assert_eq!(project.len(), 1);
    }

    /// Plan test 13: merging two stores with the same key but
    /// different bytes is a hard error naming the key.
    #[test]
    fn merge_errors_on_byte_mismatch() {
        let mut project = ArtifactStore::new();

        let mut a = ArtifactStore::new();
        a.store(
            "css:libs:kbd:kbd.css",
            Artifact::from_string("v1", "text/css").with_scope(ArtifactScope::Project),
        );
        let mut b = ArtifactStore::new();
        b.store(
            "css:libs:kbd:kbd.css",
            Artifact::from_string("v2-different", "text/css").with_scope(ArtifactScope::Project),
        );

        project.merge_into_project(a).expect("first merge");
        let err = project
            .merge_into_project(b)
            .expect_err("second merge must conflict");
        assert_eq!(err.key, "css:libs:kbd:kbd.css");
        assert_eq!(err.existing_len, "v1".len());
        assert_eq!(err.incoming_len, "v2-different".len());

        // Display includes the key for orchestrator-side
        // diagnostic composition.
        let msg = format!("{}", err);
        assert!(msg.contains("css:libs:kbd:kbd.css"), "msg: {msg}");
    }

    /// Plan test 14: drain + merge preserves all artifact fields
    /// (path, content_type, metadata, scope).
    #[test]
    fn drain_preserves_artifact_metadata_path() {
        let mut per_doc = ArtifactStore::new();
        per_doc.store(
            "css:theme:abc",
            Artifact::from_string("body{}", "text/css")
                .with_path("quarto/quarto-theme-abc.css")
                .with_metadata("source", serde_json::json!("compile_theme_css"))
                .with_scope(ArtifactScope::Project),
        );

        let drained = per_doc.drain_project_scoped();
        let mut project = ArtifactStore::new();
        project.merge_into_project(drained).unwrap();

        let got = project.get("css:theme:abc").expect("present after merge");
        assert_eq!(got.scope, ArtifactScope::Project);
        assert_eq!(
            got.path,
            Some(std::path::PathBuf::from("quarto/quarto-theme-abc.css"))
        );
        assert_eq!(got.content_type, "text/css");
        assert_eq!(
            got.metadata.get("source"),
            Some(&serde_json::json!("compile_theme_css"))
        );
        assert_eq!(got.as_str(), Some("body{}"));
    }
}
