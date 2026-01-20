//! Document abstraction for language analysis.
//!
//! This module provides a simple document representation that can be used
//! for language analysis. The design anticipates future workspace-wide features
//! where documents may come from either the editor (in-memory) or the filesystem.

use quarto_source_map::{SourceContext, SourceInfo};

/// A document for language analysis.
///
/// Documents hold the content and metadata needed for parsing and analysis.
/// They are designed to work with both in-memory content (from an editor)
/// and filesystem content (for workspace features).
#[derive(Debug, Clone)]
pub struct Document {
    /// The document's URI or path.
    uri: String,
    /// The document content.
    content: String,
    /// Version number for tracking changes (optional, used by LSP).
    version: Option<i32>,
}

impl Document {
    /// Create a new document with the given URI and content.
    pub fn new(uri: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            content: content.into(),
            version: None,
        }
    }

    /// Create a new document with a version number.
    pub fn with_version(uri: impl Into<String>, content: impl Into<String>, version: i32) -> Self {
        Self {
            uri: uri.into(),
            content: content.into(),
            version: Some(version),
        }
    }

    /// Get the document's URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Get the document's content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get the document's content as bytes.
    pub fn content_bytes(&self) -> &[u8] {
        self.content.as_bytes()
    }

    /// Get the document's version, if set.
    pub fn version(&self) -> Option<i32> {
        self.version
    }

    /// Get the filename from the URI (for display purposes).
    pub fn filename(&self) -> &str {
        self.uri.rsplit(['/', '\\']).next().unwrap_or(&self.uri)
    }

    /// Update the document content.
    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
    }

    /// Update the document content with a new version.
    pub fn set_content_with_version(&mut self, content: impl Into<String>, version: i32) {
        self.content = content.into();
        self.version = Some(version);
    }

    /// Create a SourceContext for this document.
    ///
    /// This is used to track source locations during parsing.
    /// Returns the SourceContext and the FileId for this document.
    pub fn create_source_context(&self) -> SourceContext {
        let mut ctx = SourceContext::default();
        ctx.add_file(self.filename().to_string(), Some(self.content.clone()));
        ctx
    }

    /// Create a SourceContext and get the FileId for this document.
    ///
    /// This is useful when you need both the context and the file ID.
    pub fn create_source_context_with_id(&self) -> (SourceContext, quarto_source_map::FileId) {
        let mut ctx = SourceContext::default();
        let file_id = ctx.add_file(self.filename().to_string(), Some(self.content.clone()));
        (ctx, file_id)
    }

    /// Create a SourceInfo pointing to this document.
    ///
    /// This creates a SourceInfo for the entire document content.
    pub fn create_source_info(&self, file_id: quarto_source_map::FileId) -> SourceInfo {
        SourceInfo::original(file_id, 0, self.content.len())
    }
}

/// A document store for managing multiple documents.
///
/// This is a simple in-memory store for documents. Future versions may
/// support workspace-wide features with on-demand loading from the filesystem.
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: std::collections::HashMap<String, Document>,
}

impl DocumentStore {
    /// Create a new empty document store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open or update a document in the store.
    pub fn open(&mut self, uri: impl Into<String>, content: impl Into<String>, version: i32) {
        let uri = uri.into();
        self.documents
            .insert(uri.clone(), Document::with_version(uri, content, version));
    }

    /// Update a document's content.
    pub fn change(&mut self, uri: &str, content: impl Into<String>, version: i32) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.set_content_with_version(content, version);
        }
    }

    /// Close a document (remove from store).
    pub fn close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    /// Get a document by URI.
    pub fn get(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Get all document URIs.
    pub fn uris(&self) -> impl Iterator<Item = &str> {
        self.documents.keys().map(|s| s.as_str())
    }

    /// Check if a document is in the store.
    pub fn contains(&self, uri: &str) -> bool {
        self.documents.contains_key(uri)
    }

    /// Get the number of documents in the store.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_creation() {
        let doc = Document::new("file:///test.qmd", "# Hello\n\nWorld");
        assert_eq!(doc.uri(), "file:///test.qmd");
        assert_eq!(doc.content(), "# Hello\n\nWorld");
        assert_eq!(doc.filename(), "test.qmd");
        assert_eq!(doc.version(), None);
    }

    #[test]
    fn document_with_version() {
        let doc = Document::with_version("test.qmd", "content", 1);
        assert_eq!(doc.version(), Some(1));
    }

    #[test]
    fn document_update() {
        let mut doc = Document::with_version("test.qmd", "old", 1);
        doc.set_content_with_version("new", 2);
        assert_eq!(doc.content(), "new");
        assert_eq!(doc.version(), Some(2));
    }

    #[test]
    fn document_store_lifecycle() {
        let mut store = DocumentStore::new();

        // Open
        store.open("file:///a.qmd", "content a", 1);
        store.open("file:///b.qmd", "content b", 1);
        assert_eq!(store.len(), 2);

        // Get
        assert_eq!(store.get("file:///a.qmd").unwrap().content(), "content a");

        // Change
        store.change("file:///a.qmd", "updated a", 2);
        assert_eq!(store.get("file:///a.qmd").unwrap().content(), "updated a");
        assert_eq!(store.get("file:///a.qmd").unwrap().version(), Some(2));

        // Close
        store.close("file:///a.qmd");
        assert_eq!(store.len(), 1);
        assert!(!store.contains("file:///a.qmd"));
        assert!(store.contains("file:///b.qmd"));
    }

    #[test]
    fn source_context_creation() {
        let doc = Document::new("test.qmd", "# Hello\n\nWorld");
        let (ctx, file_id) = doc.create_source_context_with_id();
        let source_info = doc.create_source_info(file_id);
        // Verify the source info has the correct length
        assert_eq!(source_info.length(), doc.content().len());
        // Verify the file is in the context
        assert!(ctx.get_file(file_id).is_some());
    }
}
