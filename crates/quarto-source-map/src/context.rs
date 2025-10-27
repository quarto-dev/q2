//! Source context for managing files

use crate::file_info::FileInformation;
use crate::types::FileId;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

/// Context for managing source files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceContext {
    files: Vec<SourceFile>,
    /// Sparse mapping for non-sequential file IDs (e.g., from hash-based IDs)
    /// Only populated when add_file_with_id is used
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    file_id_map: HashMap<usize, usize>, // Maps FileId.0 -> index in files vec
}

/// A source file with content and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    /// File path or identifier
    pub path: String,
    /// File content (for ephemeral/in-memory files)
    /// When Some, content is stored in memory (e.g., for <anonymous> or test files)
    /// When None, content should be read from disk using the path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// File information for efficient location lookups (optional for serialization)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_info: Option<FileInformation>,
    /// File metadata
    pub metadata: FileMetadata,
}

/// Metadata about a source file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// File type (qmd, yaml, md, etc.)
    pub file_type: Option<String>,
}

impl SourceContext {
    /// Create a new empty source context
    pub fn new() -> Self {
        SourceContext {
            files: Vec::new(),
            file_id_map: HashMap::new(),
        }
    }

    /// Add a file to the context and return its ID
    ///
    /// - If content is Some: Creates an ephemeral (in-memory) file. Content is stored and used for ariadne rendering.
    /// - If content is None: Creates a disk-backed file. Content will be read from disk when needed (path must exist).
    ///
    /// For ephemeral files, FileInformation is created immediately from the provided content.
    /// For disk-backed files, FileInformation is created by reading from disk if the path exists.
    pub fn add_file(&mut self, path: String, content: Option<String>) -> FileId {
        let id = FileId(self.files.len());

        // For ephemeral files (content provided), store it and create FileInformation
        // For disk-backed files (no content), try to read from disk for FileInformation only
        let (stored_content, content_for_info) = match content {
            Some(c) => {
                // Ephemeral file: store content and use it for FileInformation
                (Some(c.clone()), Some(c))
            }
            None => {
                // Disk-backed file: don't store content, but try to read for FileInformation
                (None, std::fs::read_to_string(&path).ok())
            }
        };

        let file_info = content_for_info.as_ref().map(|c| FileInformation::new(c));
        self.files.push(SourceFile {
            path,
            content: stored_content,
            file_info,
            metadata: FileMetadata { file_type: None },
        });
        id
    }

    /// Add a file with pre-computed FileInformation
    ///
    /// This is useful when deserializing from formats (like JSON) that include
    /// serialized FileInformation, avoiding the need to recompute line breaks
    /// or read from disk.
    ///
    /// The file is created without content (content=None), so ariadne rendering
    /// won't work, but map_offset() will work using the provided FileInformation.
    pub fn add_file_with_info(&mut self, path: String, file_info: FileInformation) -> FileId {
        let id = FileId(self.files.len());
        self.files.push(SourceFile {
            path,
            content: None,
            file_info: Some(file_info),
            metadata: FileMetadata { file_type: None },
        });
        id
    }

    /// Add a file with a specific FileId
    ///
    /// This is useful when interfacing with systems that use hash-based or non-sequential
    /// FileIds (like quarto-yaml). The FileId must not already exist in the context.
    ///
    /// # Panics
    ///
    /// Panics if the FileId already exists in the context.
    pub fn add_file_with_id(
        &mut self,
        id: FileId,
        path: String,
        content: Option<String>,
    ) -> FileId {
        // Check if ID already exists
        if self.get_file(id).is_some() {
            panic!("FileId {:?} already exists in SourceContext", id);
        }

        // Process content same as add_file
        let (stored_content, content_for_info) = match content {
            Some(c) => (Some(c.clone()), Some(c)),
            None => (None, std::fs::read_to_string(&path).ok()),
        };

        let file_info = content_for_info.as_ref().map(|c| FileInformation::new(c));

        // Add to files vec and create mapping
        let index = self.files.len();
        self.files.push(SourceFile {
            path,
            content: stored_content,
            file_info,
            metadata: FileMetadata { file_type: None },
        });

        // Store mapping from FileId to index
        self.file_id_map.insert(id.0, index);

        id
    }

    /// Get a file by ID
    pub fn get_file(&self, id: FileId) -> Option<&SourceFile> {
        // First check if this is a mapped ID
        if let Some(&index) = self.file_id_map.get(&id.0) {
            return self.files.get(index);
        }

        // Otherwise use direct indexing (for sequential IDs from add_file)
        self.files.get(id.0)
    }

    /// Create a copy without FileInformation (for serialization)
    ///
    /// Note: This preserves the content field for ephemeral files, as they need
    /// content to be serialized for proper deserialization. Only FileInformation
    /// is removed since it can be reconstructed from content.
    pub fn without_content(&self) -> Self {
        SourceContext {
            files: self
                .files
                .iter()
                .map(|f| SourceFile {
                    path: f.path.clone(),
                    content: f.content.clone(), // Preserve content for ephemeral files
                    file_info: None,
                    metadata: f.metadata.clone(),
                })
                .collect(),
            file_id_map: self.file_id_map.clone(), // Preserve mapping
        }
    }
}

impl Default for SourceContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = SourceContext::new();
        assert!(ctx.get_file(FileId(0)).is_none());
    }

    #[test]
    fn test_add_and_get_file() {
        let mut ctx = SourceContext::new();
        let id = ctx.add_file("test.qmd".to_string(), Some("# Hello".to_string()));

        assert_eq!(id, FileId(0));
        let file = ctx.get_file(id).unwrap();
        assert_eq!(file.path, "test.qmd");
        assert!(file.file_info.is_some());

        // Verify the file info was built correctly
        let info = file.file_info.as_ref().unwrap();
        assert_eq!(info.total_length(), 7);
    }

    #[test]
    fn test_multiple_files() {
        let mut ctx = SourceContext::new();
        let id1 = ctx.add_file("first.qmd".to_string(), Some("First".to_string()));
        let id2 = ctx.add_file("second.qmd".to_string(), Some("Second".to_string()));

        assert_eq!(id1, FileId(0));
        assert_eq!(id2, FileId(1));

        let file1 = ctx.get_file(id1).unwrap();
        let file2 = ctx.get_file(id2).unwrap();

        assert_eq!(file1.path, "first.qmd");
        assert_eq!(file2.path, "second.qmd");
        assert!(file1.file_info.is_some());
        assert!(file2.file_info.is_some());
        assert_eq!(file1.file_info.as_ref().unwrap().total_length(), 5);
        assert_eq!(file2.file_info.as_ref().unwrap().total_length(), 6);
    }

    #[test]
    fn test_file_without_content() {
        let mut ctx = SourceContext::new();
        let id = ctx.add_file("no-content.qmd".to_string(), None);

        let file = ctx.get_file(id).unwrap();
        assert_eq!(file.path, "no-content.qmd");
        assert!(file.file_info.is_none());
    }

    #[test]
    fn test_without_content() {
        let mut ctx = SourceContext::new();
        ctx.add_file("test1.qmd".to_string(), Some("Content 1".to_string()));
        ctx.add_file("test2.qmd".to_string(), Some("Content 2".to_string()));

        let ctx_no_content = ctx.without_content();

        let file1 = ctx_no_content.get_file(FileId(0)).unwrap();
        let file2 = ctx_no_content.get_file(FileId(1)).unwrap();

        assert_eq!(file1.path, "test1.qmd");
        assert_eq!(file2.path, "test2.qmd");
        assert!(file1.file_info.is_none());
        assert!(file2.file_info.is_none());
    }

    #[test]
    fn test_serialization() {
        let mut ctx = SourceContext::new();
        ctx.add_file("test.qmd".to_string(), Some("# Test".to_string()));

        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: SourceContext = serde_json::from_str(&json).unwrap();

        let file = deserialized.get_file(FileId(0)).unwrap();
        assert_eq!(file.path, "test.qmd");
        assert!(file.file_info.is_some());
        assert_eq!(file.file_info.as_ref().unwrap().total_length(), 6);
    }

    #[test]
    fn test_serialization_without_content() {
        let mut ctx = SourceContext::new();
        ctx.add_file("test.qmd".to_string(), Some("# Test".to_string()));

        let ctx_no_content = ctx.without_content();
        let json = serde_json::to_string(&ctx_no_content).unwrap();

        // Verify that None file_info is skipped in serialization
        assert!(!json.contains("\"file_info\""));
    }
}
