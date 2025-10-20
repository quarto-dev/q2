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
