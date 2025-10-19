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
        ASTContext {
            filenames: Vec::new(),
            example_list_counter: Cell::new(1),
            source_context: SourceContext::new(),
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
        ASTContext {
            filenames: Vec::new(),
            example_list_counter: Cell::new(1),
            source_context: SourceContext::new(),
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
}

impl Default for ASTContext {
    fn default() -> Self {
        Self::new()
    }
}
