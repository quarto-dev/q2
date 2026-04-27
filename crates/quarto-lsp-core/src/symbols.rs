//! Thin wrappers around [`crate::analysis::analyze_document`] for callers
//! that only need a single slice of the analysis result.
//!
//! Prefer [`crate::analyze_document`] when you need more than one — the
//! unified entry point does a single parse + pipeline run for all three.

use crate::analysis::analyze_document;
use crate::document::Document;
use crate::types::{FoldingRange, Symbol};

/// Get document symbols (outline) for a document.
pub fn get_symbols(doc: &Document) -> Vec<Symbol> {
    analyze_document(doc).symbols
}

/// Get folding ranges for a document.
pub fn get_folding_ranges(doc: &Document) -> Vec<FoldingRange> {
    analyze_document(doc).folding_ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_headers() {
        let doc = Document::new(
            "test.qmd",
            "# Section 1\n\nSome content.\n\n## Subsection 1.1\n\n# Section 2\n",
        );
        let symbols = get_symbols(&doc);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Section 1");
        assert_eq!(symbols[1].name, "Section 2");
    }

    #[test]
    fn empty_document_has_no_symbols() {
        let doc = Document::new("test.qmd", "");
        assert!(get_symbols(&doc).is_empty());
    }
}
