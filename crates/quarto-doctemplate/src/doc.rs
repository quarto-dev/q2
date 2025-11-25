/*
 * doc.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Document type for structured template output.
//!
//! This module provides a `Doc` type that represents structured document content,
//! similar to the `Doc` type in Haskell's `doclayout` library. It enables proper
//! handling of nesting (indentation) and breakable spaces.
//!
//! # Why Doc instead of String?
//!
//! Nesting is structural, not post-processing. With String, we'd need to track
//! column position and post-process newlines. With Doc, we build `Prefixed` nodes
//! that the renderer handles correctly.
//!
//! # Minimal Implementation
//!
//! This is a minimal subset of doclayout's 16-variant Doc type. We include only:
//! - `Empty`: nothing
//! - `Text`: literal text
//! - `Concat`: concatenation
//! - `Prefixed`: prefix each line (for nesting)
//! - `BreakingSpace`: space that can break at line wrap
//! - `NewLine`: hard newline

/// A structured document representation.
///
/// `Doc` allows us to represent template output in a way that preserves
/// structural information needed for proper nesting and line breaking.
#[derive(Debug, Clone, PartialEq)]
pub enum Doc {
    /// Empty document (produces no output).
    Empty,

    /// Literal text.
    Text(String),

    /// Concatenation of two documents.
    Concat(Box<Doc>, Box<Doc>),

    /// Prefix each line of the inner document with the given string.
    /// Used for implementing nesting/indentation.
    Prefixed(String, Box<Doc>),

    /// A space that can break at line wrap boundaries.
    /// Without line wrapping, renders as a single space.
    BreakingSpace,

    /// A hard newline.
    NewLine,
}

impl Doc {
    /// Create a text document from a string.
    pub fn text(s: impl Into<String>) -> Self {
        let s = s.into();
        if s.is_empty() {
            Doc::Empty
        } else {
            Doc::Text(s)
        }
    }

    /// Concatenate two documents.
    ///
    /// This is smart about Empty documents - concatenating with Empty
    /// returns the other document unchanged.
    pub fn concat(self, other: Doc) -> Self {
        match (&self, &other) {
            (Doc::Empty, _) => other,
            (_, Doc::Empty) => self,
            _ => Doc::Concat(Box::new(self), Box::new(other)),
        }
    }

    /// Check if this document is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Doc::Empty => true,
            Doc::Text(s) => s.is_empty(),
            Doc::Concat(a, b) => a.is_empty() && b.is_empty(),
            Doc::Prefixed(_, inner) => inner.is_empty(),
            Doc::BreakingSpace => false,
            Doc::NewLine => false,
        }
    }

    /// Apply a prefix to each line of this document (for nesting).
    pub fn prefixed(prefix: impl Into<String>, inner: Doc) -> Self {
        let prefix = prefix.into();
        if inner.is_empty() {
            Doc::Empty
        } else {
            Doc::Prefixed(prefix, Box::new(inner))
        }
    }

    /// Create a document from a newline.
    pub fn newline() -> Self {
        Doc::NewLine
    }

    /// Create a breaking space.
    pub fn breaking_space() -> Self {
        Doc::BreakingSpace
    }

    /// Render this document to a string.
    ///
    /// # Arguments
    /// * `line_width` - Optional maximum line width for reflowing.
    ///                  If None, no reflowing is performed.
    ///
    /// # Note
    /// The current implementation ignores `line_width` and does not
    /// perform reflowing. This may be added in a future version.
    pub fn render(&self, _line_width: Option<usize>) -> String {
        self.render_simple()
    }

    /// Render without any line width constraints.
    fn render_simple(&self) -> String {
        match self {
            Doc::Empty => String::new(),
            Doc::Text(s) => s.clone(),
            Doc::Concat(a, b) => {
                let mut result = a.render_simple();
                result.push_str(&b.render_simple());
                result
            }
            Doc::Prefixed(prefix, inner) => {
                let inner_str = inner.render_simple();
                apply_prefix(&inner_str, prefix)
            }
            Doc::BreakingSpace => " ".to_string(),
            Doc::NewLine => "\n".to_string(),
        }
    }
}

/// Apply a prefix to each line after the first.
///
/// The first line is not prefixed (it continues from the current position).
/// All subsequent lines get the prefix prepended.
fn apply_prefix(s: &str, prefix: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    if lines.len() <= 1 {
        return s.to_string();
    }

    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
            result.push_str(prefix);
        }
        result.push_str(line);
    }
    result
}

impl Default for Doc {
    fn default() -> Self {
        Doc::Empty
    }
}

/// Concatenate multiple documents.
pub fn concat_docs(docs: impl IntoIterator<Item = Doc>) -> Doc {
    docs.into_iter()
        .fold(Doc::Empty, |acc, doc| acc.concat(doc))
}

/// Intersperse documents with a separator.
pub fn intersperse_docs(docs: Vec<Doc>, sep: Doc) -> Doc {
    let mut result = Doc::Empty;
    let mut first = true;

    for doc in docs {
        if doc.is_empty() {
            continue;
        }
        if first {
            first = false;
        } else {
            result = result.concat(sep.clone());
        }
        result = result.concat(doc);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(Doc::Empty.render(None), "");
        assert!(Doc::Empty.is_empty());
    }

    #[test]
    fn test_text() {
        assert_eq!(Doc::text("hello").render(None), "hello");
        assert!(!Doc::text("hello").is_empty());

        // Empty string becomes Empty
        assert!(Doc::text("").is_empty());
    }

    #[test]
    fn test_concat() {
        let doc = Doc::text("hello").concat(Doc::text(" world"));
        assert_eq!(doc.render(None), "hello world");

        // Concat with Empty is identity
        assert_eq!(Doc::text("hello").concat(Doc::Empty).render(None), "hello");
        assert_eq!(Doc::Empty.concat(Doc::text("hello")).render(None), "hello");
    }

    #[test]
    fn test_newline() {
        let doc = Doc::text("line1")
            .concat(Doc::newline())
            .concat(Doc::text("line2"));
        assert_eq!(doc.render(None), "line1\nline2");
    }

    #[test]
    fn test_breaking_space() {
        let doc = Doc::text("hello")
            .concat(Doc::breaking_space())
            .concat(Doc::text("world"));
        // Without reflow, breaking space is just a space
        assert_eq!(doc.render(None), "hello world");
    }

    #[test]
    fn test_prefixed_single_line() {
        // Single line - no prefix applied
        let doc = Doc::prefixed("  ", Doc::text("hello"));
        assert_eq!(doc.render(None), "hello");
    }

    #[test]
    fn test_prefixed_multiline() {
        // Multiline - prefix applied to lines after first
        let inner = Doc::text("line1")
            .concat(Doc::newline())
            .concat(Doc::text("line2"))
            .concat(Doc::newline())
            .concat(Doc::text("line3"));
        let doc = Doc::prefixed("  ", inner);
        assert_eq!(doc.render(None), "line1\n  line2\n  line3");
    }

    #[test]
    fn test_prefixed_empty() {
        // Prefixed empty is empty
        let doc = Doc::prefixed("  ", Doc::Empty);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_concat_docs() {
        let docs = vec![Doc::text("a"), Doc::text("b"), Doc::text("c")];
        assert_eq!(concat_docs(docs).render(None), "abc");
    }

    #[test]
    fn test_intersperse_docs() {
        let docs = vec![Doc::text("a"), Doc::text("b"), Doc::text("c")];
        let sep = Doc::text(", ");
        assert_eq!(intersperse_docs(docs, sep).render(None), "a, b, c");
    }

    #[test]
    fn test_intersperse_with_empty() {
        // Empty docs are skipped
        let docs = vec![Doc::text("a"), Doc::Empty, Doc::text("c")];
        let sep = Doc::text(", ");
        assert_eq!(intersperse_docs(docs, sep).render(None), "a, c");
    }

    #[test]
    fn test_nested_prefixed() {
        // Nested prefixes should accumulate
        let inner = Doc::text("line1")
            .concat(Doc::newline())
            .concat(Doc::text("line2"));
        let middle = Doc::prefixed("  ", inner);
        let outer = Doc::prefixed("> ", middle);

        // First line has no prefix, second line gets both prefixes
        assert_eq!(outer.render(None), "line1\n>   line2");
    }
}
