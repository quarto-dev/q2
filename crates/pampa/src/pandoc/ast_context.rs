/*
 * ast_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_source_map::{FileId, SourceContext};
use std::cell::Cell;

/// Context passed through the parsing pipeline to provide information
/// about the current parse operation and manage string ownership.
/// The filenames vector will eventually be used to deduplicate strings
/// in the AST by storing indices instead of cloning strings.
#[derive(Debug, Clone)]
pub struct ASTContext {
    pub filenames: Vec<String>,
    /// Counter for example list numbering across the document
    /// Example lists continue numbering even when interrupted by other content
    pub example_list_counter: Cell<usize>,
    /// Source context for tracking files and their content
    pub source_context: SourceContext,
    /// Parent source info for recursive parses (e.g., metadata values)
    /// When set, all SourceInfo instances created during parsing are wrapped
    /// as Substrings of this parent, enabling correct location tracking through
    /// nested parse operations.
    pub parent_source_info: Option<quarto_source_map::SourceInfo>,
}

impl ASTContext {
    pub fn new() -> Self {
        let mut source_context = SourceContext::new();
        // Always add an anonymous file so FileId(0) is valid
        source_context.add_file("<unknown>".to_string(), None);

        ASTContext {
            filenames: vec!["<unknown>".to_string()],
            example_list_counter: Cell::new(1),
            source_context,
            parent_source_info: None,
        }
    }

    pub fn with_filename(filename: impl Into<String>) -> Self {
        let filename_str = filename.into();
        let mut source_context = SourceContext::new();
        // Add the file without content for now (content can be added later if needed)
        source_context.add_file(filename_str.clone(), None);

        ASTContext {
            filenames: vec![filename_str],
            example_list_counter: Cell::new(1),
            source_context,
            parent_source_info: None,
        }
    }

    pub fn anonymous() -> Self {
        let mut source_context = SourceContext::new();
        // Always add an anonymous file so FileId(0) is valid
        source_context.add_file("<anonymous>".to_string(), None);

        ASTContext {
            filenames: vec!["<anonymous>".to_string()],
            example_list_counter: Cell::new(1),
            source_context,
            parent_source_info: None,
        }
    }

    /// Add a filename to the context and return its index
    pub fn add_filename(&mut self, filename: String) -> usize {
        self.filenames.push(filename);
        self.filenames.len() - 1
    }

    /// Get the primary filename (first in the vector), if any
    pub fn primary_filename(&self) -> Option<&String> {
        self.filenames.first()
    }

    /// Get the primary file ID (FileId(0)), if any file exists in the source context
    pub fn primary_file_id(&self) -> Option<FileId> {
        if self.source_context.get_file(FileId(0)).is_some() {
            Some(FileId(0))
        } else {
            None
        }
    }

    /// Get the FileId to use for new SourceInfo instances.
    /// Since ASTContext constructors now ensure FileId(0) always exists,
    /// this always returns FileId(0).
    ///
    /// This method exists for:
    /// 1. Code clarity - makes it obvious we're getting a file ID from context
    /// 2. Future flexibility - if we need to track current file differently
    pub fn current_file_id(&self) -> FileId {
        FileId(0)
    }
}

impl Default for ASTContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let ctx = ASTContext::new();
        assert_eq!(ctx.filenames.len(), 1);
        assert_eq!(ctx.filenames[0], "<unknown>");
        assert_eq!(ctx.example_list_counter.get(), 1);
        assert!(ctx.parent_source_info.is_none());
    }

    #[test]
    fn test_with_filename() {
        let ctx = ASTContext::with_filename("test.qmd");
        assert_eq!(ctx.filenames.len(), 1);
        assert_eq!(ctx.filenames[0], "test.qmd");
    }

    #[test]
    fn test_anonymous() {
        let ctx = ASTContext::anonymous();
        assert_eq!(ctx.filenames.len(), 1);
        assert_eq!(ctx.filenames[0], "<anonymous>");
    }

    #[test]
    fn test_default() {
        let ctx = ASTContext::default();
        assert_eq!(ctx.filenames[0], "<unknown>");
    }

    #[test]
    fn test_add_filename() {
        let mut ctx = ASTContext::new();
        assert_eq!(ctx.filenames.len(), 1);

        let idx = ctx.add_filename("second.qmd".to_string());
        assert_eq!(idx, 1);
        assert_eq!(ctx.filenames.len(), 2);
        assert_eq!(ctx.filenames[1], "second.qmd");

        let idx = ctx.add_filename("third.qmd".to_string());
        assert_eq!(idx, 2);
        assert_eq!(ctx.filenames.len(), 3);
    }

    #[test]
    fn test_primary_filename() {
        let ctx = ASTContext::with_filename("primary.qmd");
        assert_eq!(ctx.primary_filename(), Some(&"primary.qmd".to_string()));

        let mut ctx = ASTContext::with_filename("first.qmd");
        ctx.add_filename("second.qmd".to_string());
        // Primary is still the first one
        assert_eq!(ctx.primary_filename(), Some(&"first.qmd".to_string()));
    }

    #[test]
    fn test_primary_file_id() {
        let ctx = ASTContext::new();
        // Since constructor adds a file, FileId(0) should exist
        assert_eq!(ctx.primary_file_id(), Some(FileId(0)));

        let ctx = ASTContext::with_filename("test.qmd");
        assert_eq!(ctx.primary_file_id(), Some(FileId(0)));
    }

    #[test]
    fn test_current_file_id() {
        let ctx = ASTContext::new();
        assert_eq!(ctx.current_file_id(), FileId(0));
    }

    #[test]
    fn test_example_list_counter() {
        let ctx = ASTContext::new();
        assert_eq!(ctx.example_list_counter.get(), 1);
        ctx.example_list_counter.set(5);
        assert_eq!(ctx.example_list_counter.get(), 5);
    }

    #[test]
    fn test_clone() {
        let ctx1 = ASTContext::with_filename("test.qmd");
        let ctx2 = ctx1.clone();
        assert_eq!(ctx2.filenames[0], "test.qmd");
    }
}
