/*
 * ast_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

/// Context passed through the parsing pipeline to provide information
/// about the current parse operation and manage string ownership.
/// The filenames vector will eventually be used to deduplicate strings
/// in the AST by storing indices instead of cloning strings.
#[derive(Debug, Clone)]
pub struct ASTContext {
    pub filenames: Vec<String>,
}

impl ASTContext {
    pub fn new() -> Self {
        ASTContext {
            filenames: Vec::new(),
        }
    }

    pub fn with_filename(filename: impl Into<String>) -> Self {
        ASTContext {
            filenames: vec![filename.into()],
        }
    }

    pub fn anonymous() -> Self {
        ASTContext {
            filenames: Vec::new(),
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
}

impl Default for ASTContext {
    fn default() -> Self {
        Self::new()
    }
}
