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
}

impl Artifact {
    /// Create a new artifact from bytes
    pub fn from_bytes(content: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            content,
            content_type: content_type.into(),
            path: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a new artifact from a string
    pub fn from_string(content: impl Into<String>, content_type: impl Into<String>) -> Self {
        Self {
            content: content.into().into_bytes(),
            content_type: content_type.into(),
            path: None,
            metadata: HashMap::new(),
        }
    }

    /// Create an artifact representing a file path (without loading content).
    ///
    /// The content is empty; this is used to record that a file at a given
    /// path is needed as a resource.
    pub fn from_path(path: impl Into<PathBuf>, content_type: impl Into<String>) -> Self {
        Self {
            content: Vec::new(),
            content_type: content_type.into(),
            path: Some(path.into()),
            metadata: HashMap::new(),
        }
    }

    /// Set the file path for this artifact
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Add metadata to this artifact
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
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
}

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
        let artifact = Artifact::from_string("content", "text/plain")
            .with_path("/path/to/file.txt")
            .with_metadata("version", serde_json::json!("1.0"));

        assert_eq!(artifact.path, Some(PathBuf::from("/path/to/file.txt")));
        assert_eq!(artifact.metadata.get("version"), Some(&serde_json::json!("1.0")));
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
}
