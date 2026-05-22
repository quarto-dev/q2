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
///
/// **bd-ky14a**: FileIds are derived via
/// [`quarto_yaml::file_id_for_filename`] (hashing the filename
/// string), matching the scheme `quarto_yaml::parse_file` uses.
/// This makes pampa's `SourceInfo`s globally interchangeable with
/// `SourceInfo`s produced by any other parser that uses the same
/// hash recipe — no out-of-band binding required when bridging
/// diagnostics across contexts.
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
    /// FileId of the primary parsed file, cached so
    /// [`current_file_id`](Self::current_file_id) doesn't have to
    /// re-hash the filename on every call.
    ///
    /// Always equal to `quarto_yaml::file_id_for_filename(filenames[0])`
    /// — see the constructors. Stored as a field, not derived, so
    /// the invariant holds even if `filenames[0]` is later mutated
    /// (constructors don't expose that, but `add_filename` does for
    /// secondary files).
    primary_file_id: FileId,
}

impl ASTContext {
    pub fn new() -> Self {
        Self::with_filename("<unknown>".to_string())
    }

    pub fn with_filename(filename: impl Into<String>) -> Self {
        let filename_str = filename.into();
        // bd-ky14a: derive the FileId from the filename string the
        // same way quarto_yaml::parse_file does.
        let file_id = quarto_yaml::file_id_for_filename(&filename_str);
        let mut source_context = SourceContext::new();
        source_context.add_file_with_id(file_id, filename_str.clone(), None);

        ASTContext {
            filenames: vec![filename_str],
            example_list_counter: Cell::new(1),
            source_context,
            parent_source_info: None,
            primary_file_id: file_id,
        }
    }

    pub fn anonymous() -> Self {
        Self::with_filename("<anonymous>".to_string())
    }

    /// Construct an [`ASTContext`] from pre-built parts.
    ///
    /// Intended for deserialization paths (the JSON reader) where
    /// the source-context and filenames are reconstructed from
    /// serialized data rather than from parsing. The caller is
    /// responsible for computing `primary_file_id` consistently
    /// with the source_context (typically
    /// `quarto_yaml::file_id_for_filename(&filenames[0])`).
    pub fn from_parts(
        filenames: Vec<String>,
        source_context: SourceContext,
        primary_file_id: FileId,
    ) -> Self {
        ASTContext {
            filenames,
            example_list_counter: Cell::new(1),
            source_context,
            parent_source_info: None,
            primary_file_id,
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

    /// Get the primary file ID, if any file exists in the source
    /// context.
    pub fn primary_file_id(&self) -> Option<FileId> {
        if self.source_context.get_file(self.primary_file_id).is_some() {
            Some(self.primary_file_id)
        } else {
            None
        }
    }

    /// Get the FileId to use for new SourceInfo instances.
    ///
    /// bd-ky14a: this is the hash-based FileId of the primary
    /// filename. See the struct docs.
    pub fn current_file_id(&self) -> FileId {
        self.primary_file_id
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
        // bd-ky14a: primary_file_id is the hash of the filename
        // (via quarto_yaml::file_id_for_filename), not FileId(0).
        let ctx = ASTContext::new();
        assert_eq!(
            ctx.primary_file_id(),
            Some(quarto_yaml::file_id_for_filename("<unknown>")),
        );

        let ctx = ASTContext::with_filename("test.qmd");
        assert_eq!(
            ctx.primary_file_id(),
            Some(quarto_yaml::file_id_for_filename("test.qmd")),
        );
    }

    #[test]
    fn test_current_file_id() {
        let ctx = ASTContext::new();
        assert_eq!(
            ctx.current_file_id(),
            quarto_yaml::file_id_for_filename("<unknown>"),
        );
    }

    #[test]
    fn test_example_list_counter() {
        let ctx = ASTContext::new();
        assert_eq!(ctx.example_list_counter.get(), 1);
        ctx.example_list_counter.set(5);
        assert_eq!(ctx.example_list_counter.get(), 5);
    }

    // === bd-ky14a contract tests ===
    //
    // These pin down the new "FileIds are hash(filename)" contract.
    // They are deliberately RED on main and turn GREEN once
    // ASTContext::with_filename starts computing FileIds via
    // `quarto_yaml::file_id_for_filename`.

    /// Contract #1: single-parser invariant — pampa and quarto_yaml
    /// must agree on the FileId for a given filename. Without this,
    /// a `SourceInfo` produced by pampa cannot be looked up in a
    /// `SourceContext` populated by `quarto_yaml` (or vice versa).
    #[test]
    fn bd_ky14a_with_filename_uses_quarto_yaml_file_id() {
        let ctx = ASTContext::with_filename("foo.qmd");
        let yaml_fid = quarto_yaml::file_id_for_filename("foo.qmd");
        assert_eq!(
            ctx.current_file_id(),
            yaml_fid,
            "pampa's current_file_id must match quarto_yaml::file_id_for_filename for the same path",
        );
    }

    /// Contract #1b: the SourceContext registered by `with_filename`
    /// uses the hash FileId — i.e. the file entry is reachable via
    /// `get_file(file_id_for_filename(path))`, not just
    /// `get_file(FileId(0))`.
    #[test]
    fn bd_ky14a_source_context_indexed_by_hash_file_id() {
        let ctx = ASTContext::with_filename("bar.qmd");
        let yaml_fid = quarto_yaml::file_id_for_filename("bar.qmd");
        assert!(
            ctx.source_context.get_file(yaml_fid).is_some(),
            "ASTContext::with_filename must register the file under the hash FileId",
        );
    }

    #[test]
    fn test_clone() {
        let ctx1 = ASTContext::with_filename("test.qmd");
        let ctx2 = ctx1.clone();
        assert_eq!(ctx2.filenames[0], "test.qmd");
    }
}
