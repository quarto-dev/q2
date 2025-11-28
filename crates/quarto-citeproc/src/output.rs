//! Output building utilities for formatted citations.
//!
//! This module provides two output representations:
//!
//! 1. `Output` - A tagged AST that preserves semantic information for
//!    post-processing (disambiguation, hyperlinking, collapsing, etc.)
//!
//! 2. `OutputBuilder` - Legacy flat builder (to be phased out)
//!
//! The `Output` type is rendered to final formats via the `render()` method.

use crate::reference::Name;
use quarto_csl::Formatting;

// ============================================================================
// Output AST - Tagged intermediate representation
// ============================================================================

/// Citation item type for tagging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CitationItemType {
    /// Normal citation
    NormalCite,
    /// Author only (no date/title)
    AuthorOnly,
    /// Suppress author (date/title only)
    SuppressAuthor,
}

/// Semantic tags for output nodes.
///
/// Tags preserve semantic information that enables post-processing:
/// - Disambiguation (finding names/dates to modify)
/// - Suppress-author/author-only (locating author portions)
/// - Year suffixes (tagging dates for "2020a, 2020b")
/// - Hyperlinking (identifying titles)
/// - Collapsing (identifying years for grouping)
#[derive(Debug, Clone)]
pub enum Tag {
    /// A locale term reference.
    Term(String),
    /// Citation number for numeric styles.
    CitationNumber(i32),
    /// Marks a title for potential hyperlinking.
    Title,
    /// Marks a citation item with its type and ID.
    Item {
        item_type: CitationItemType,
        item_id: String,
    },
    /// A single formatted name.
    Name(Name),
    /// A group of names from a variable.
    Names {
        variable: String,
        names: Vec<Name>,
    },
    /// A formatted date (variable name like "issued", "accessed").
    Date(String),
    /// Year suffix for disambiguation (a, b, c, ...).
    YearSuffix(i32),
    /// Marks a locator.
    Locator,
    /// Marks a prefix.
    Prefix,
    /// Marks a suffix.
    Suffix,
}

/// Intermediate output representation with semantic tagging.
///
/// This AST preserves structure and semantic information for post-processing,
/// then renders to final formats (string, Pandoc Inlines, etc.) as a separate step.
#[derive(Debug, Clone)]
pub enum Output {
    /// A formatted group of children.
    Formatted {
        formatting: Formatting,
        children: Vec<Output>,
    },
    /// A hyperlink wrapping children.
    Linked {
        url: String,
        children: Vec<Output>,
    },
    /// Content that should appear in a footnote.
    InNote(Box<Output>),
    /// Literal text content.
    Literal(String),
    /// Semantically tagged content for post-processing.
    Tagged {
        tag: Tag,
        child: Box<Output>,
    },
    /// Empty/null output.
    Null,
}

impl Output {
    /// Create a literal text node.
    pub fn literal(s: impl Into<String>) -> Self {
        let s = s.into();
        if s.is_empty() {
            Output::Null
        } else {
            Output::Literal(s)
        }
    }

    /// Create a formatted node with children.
    pub fn formatted(formatting: Formatting, children: Vec<Output>) -> Self {
        // Filter out null children
        let children: Vec<_> = children.into_iter().filter(|c| !c.is_null()).collect();
        if children.is_empty() {
            Output::Null
        } else {
            Output::Formatted {
                formatting,
                children,
            }
        }
    }

    /// Create a tagged node.
    pub fn tagged(tag: Tag, child: Output) -> Self {
        if child.is_null() {
            Output::Null
        } else {
            Output::Tagged {
                tag,
                child: Box::new(child),
            }
        }
    }

    /// Create a linked node.
    pub fn linked(url: impl Into<String>, children: Vec<Output>) -> Self {
        let children: Vec<_> = children.into_iter().filter(|c| !c.is_null()).collect();
        if children.is_empty() {
            Output::Null
        } else {
            Output::Linked {
                url: url.into(),
                children,
            }
        }
    }

    /// Create a sequence of outputs (flattened into Formatted with no formatting).
    pub fn sequence(children: Vec<Output>) -> Self {
        let children: Vec<_> = children.into_iter().filter(|c| !c.is_null()).collect();
        match children.len() {
            0 => Output::Null,
            1 => children.into_iter().next().unwrap(),
            _ => Output::Formatted {
                formatting: Formatting::default(),
                children,
            },
        }
    }

    /// Check if this output is null/empty.
    pub fn is_null(&self) -> bool {
        match self {
            Output::Null => true,
            Output::Literal(s) => s.is_empty(),
            Output::Formatted { children, .. } => children.iter().all(|c| c.is_null()),
            Output::Linked { children, .. } => children.iter().all(|c| c.is_null()),
            Output::InNote(child) => child.is_null(),
            Output::Tagged { child, .. } => child.is_null(),
        }
    }

    /// Render the output to a plain string.
    pub fn render(&self) -> String {
        match self {
            Output::Null => String::new(),
            Output::Literal(s) => s.clone(),
            Output::Formatted {
                formatting,
                children,
            } => {
                let inner: String = children.iter().map(|c| c.render()).collect();
                render_with_formatting(&inner, formatting)
            }
            Output::Linked { children, .. } => {
                // For plain text rendering, just render children (no link markup)
                children.iter().map(|c| c.render()).collect()
            }
            Output::InNote(child) => {
                // For plain text, just render the content
                child.render()
            }
            Output::Tagged { child, .. } => {
                // Tags are transparent for rendering
                child.render()
            }
        }
    }

    /// Extract the rendered text of names (Tag::Names) from this output.
    /// Returns None if no names are found.
    pub fn extract_names_text(&self) -> Option<String> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(names) = child.extract_names_text() {
                        return Some(names);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_names_text(),
            Output::Tagged { tag, child } => match tag {
                Tag::Names { .. } => Some(child.render()),
                _ => child.extract_names_text(),
            },
        }
    }

    /// Extract the rendered text of dates (Tag::Date) from this output.
    /// Returns None if no dates are found.
    pub fn extract_date_text(&self) -> Option<String> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(date) = child.extract_date_text() {
                        return Some(date);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_date_text(),
            Output::Tagged { tag, child } => match tag {
                Tag::Date(_) => Some(child.render()),
                _ => child.extract_date_text(),
            },
        }
    }

    /// Extract the citation number (Tag::CitationNumber) from this output.
    /// Returns None if no citation number is found.
    pub fn extract_citation_number(&self) -> Option<i32> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(num) = child.extract_citation_number() {
                        return Some(num);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_citation_number(),
            Output::Tagged { tag, child } => match tag {
                Tag::CitationNumber(n) => Some(*n),
                _ => child.extract_citation_number(),
            },
        }
    }

    /// Return a copy of this output with names (Tag::Names) suppressed (made null).
    /// Also strips any leading whitespace that was used as a delimiter after names.
    pub fn suppress_names(&self) -> Output {
        let result = self.suppress_names_inner();
        result.strip_leading_whitespace()
    }

    /// Inner implementation of suppress_names without whitespace stripping.
    fn suppress_names_inner(&self) -> Output {
        match self {
            Output::Null => Output::Null,
            Output::Literal(s) => Output::Literal(s.clone()),
            Output::Formatted {
                formatting,
                children,
            } => {
                let new_children: Vec<_> = children
                    .iter()
                    .map(|c| c.suppress_names_inner())
                    .filter(|c| !c.is_null())
                    .collect();
                if new_children.is_empty() {
                    Output::Null
                } else {
                    Output::Formatted {
                        formatting: formatting.clone(),
                        children: new_children,
                    }
                }
            }
            Output::Linked { url, children } => {
                let new_children: Vec<_> = children
                    .iter()
                    .map(|c| c.suppress_names_inner())
                    .filter(|c| !c.is_null())
                    .collect();
                if new_children.is_empty() {
                    Output::Null
                } else {
                    Output::Linked {
                        url: url.clone(),
                        children: new_children,
                    }
                }
            }
            Output::InNote(child) => {
                let new_child = child.suppress_names_inner();
                if new_child.is_null() {
                    Output::Null
                } else {
                    Output::InNote(Box::new(new_child))
                }
            }
            Output::Tagged { tag, child } => match tag {
                Tag::Names { .. } => Output::Null, // Suppress names
                _ => {
                    let new_child = child.suppress_names_inner();
                    if new_child.is_null() {
                        Output::Null
                    } else {
                        Output::Tagged {
                            tag: tag.clone(),
                            child: Box::new(new_child),
                        }
                    }
                }
            },
        }
    }

    /// Strip leading whitespace from this output.
    /// This handles the case where a group delimiter remains after suppressing names.
    fn strip_leading_whitespace(&self) -> Output {
        match self {
            Output::Null => Output::Null,
            Output::Literal(s) => {
                let trimmed = s.trim_start();
                if trimmed.is_empty() {
                    Output::Null
                } else {
                    Output::Literal(trimmed.to_string())
                }
            }
            Output::Formatted {
                formatting,
                children,
            } => {
                if children.is_empty() {
                    return Output::Null;
                }

                // Strip leading whitespace from the first child
                let mut new_children = Vec::with_capacity(children.len());
                let mut first = true;
                for child in children {
                    if first {
                        let stripped = child.strip_leading_whitespace();
                        if !stripped.is_null() {
                            new_children.push(stripped);
                            first = false;
                        }
                        // If stripped is null, skip it and try the next child
                    } else {
                        new_children.push(child.clone());
                    }
                }

                if new_children.is_empty() {
                    Output::Null
                } else {
                    Output::Formatted {
                        formatting: formatting.clone(),
                        children: new_children,
                    }
                }
            }
            Output::Linked { url, children } => {
                if children.is_empty() {
                    return Output::Null;
                }

                // Strip leading whitespace from the first child
                let mut new_children = Vec::with_capacity(children.len());
                let mut first = true;
                for child in children {
                    if first {
                        let stripped = child.strip_leading_whitespace();
                        if !stripped.is_null() {
                            new_children.push(stripped);
                            first = false;
                        }
                    } else {
                        new_children.push(child.clone());
                    }
                }

                if new_children.is_empty() {
                    Output::Null
                } else {
                    Output::Linked {
                        url: url.clone(),
                        children: new_children,
                    }
                }
            }
            Output::InNote(child) => {
                let new_child = child.strip_leading_whitespace();
                if new_child.is_null() {
                    Output::Null
                } else {
                    Output::InNote(Box::new(new_child))
                }
            }
            Output::Tagged { tag, child } => {
                let new_child = child.strip_leading_whitespace();
                if new_child.is_null() {
                    Output::Null
                } else {
                    Output::Tagged {
                        tag: tag.clone(),
                        child: Box::new(new_child),
                    }
                }
            }
        }
    }
}

/// Render text with formatting applied.
fn render_with_formatting(text: &str, formatting: &Formatting) -> String {
    use quarto_csl::{FontStyle, FontWeight, TextCase, VerticalAlign};

    if text.is_empty() {
        return String::new();
    }

    let mut result = String::new();

    // Apply prefix
    if let Some(ref prefix) = formatting.prefix {
        result.push_str(prefix);
    }

    // Apply text case
    let cased = match formatting.text_case {
        Some(TextCase::Lowercase) => text.to_lowercase(),
        Some(TextCase::Uppercase) => text.to_uppercase(),
        Some(TextCase::CapitalizeFirst) => capitalize_first(text),
        Some(TextCase::CapitalizeAll) => capitalize_all(text),
        Some(TextCase::Title) => title_case(text),
        Some(TextCase::Sentence) => sentence_case(text),
        None => text.to_string(),
    };

    // Apply strip periods
    let stripped = if formatting.strip_periods {
        cased.replace('.', "")
    } else {
        cased
    };

    // Apply font style
    let styled = match formatting.font_style {
        Some(FontStyle::Italic) => format!("*{}*", stripped),
        _ => stripped,
    };

    // Apply font weight
    let weighted = match formatting.font_weight {
        Some(FontWeight::Bold) => format!("**{}**", styled),
        _ => styled,
    };

    // Apply vertical align
    let aligned = match formatting.vertical_align {
        Some(VerticalAlign::Sup) => format!("^{}^", weighted),
        Some(VerticalAlign::Sub) => format!("~{}~", weighted),
        _ => weighted,
    };

    // Apply quotes
    let quoted = if formatting.quotes {
        format!("\"{}\"", aligned)
    } else {
        aligned
    };

    result.push_str(&quoted);

    // Apply suffix
    if let Some(ref suffix) = formatting.suffix {
        result.push_str(suffix);
    }

    result
}

/// Join multiple outputs with a delimiter.
pub fn join_outputs(outputs: Vec<Output>, delimiter: &str) -> Output {
    let non_null: Vec<_> = outputs.into_iter().filter(|o| !o.is_null()).collect();

    if non_null.is_empty() {
        return Output::Null;
    }

    if non_null.len() == 1 {
        return non_null.into_iter().next().unwrap();
    }

    // Interleave with delimiters
    let mut children = Vec::new();
    for (i, output) in non_null.into_iter().enumerate() {
        if i > 0 && !delimiter.is_empty() {
            children.push(Output::Literal(delimiter.to_string()));
        }
        children.push(output);
    }

    Output::Formatted {
        formatting: Formatting::default(),
        children,
    }
}

// ============================================================================
// Legacy OutputBuilder (for migration compatibility)
// ============================================================================

/// A piece of formatted output.
#[derive(Debug, Clone)]
pub struct OutputPiece {
    /// The text content.
    pub text: String,
    /// Applied formatting.
    pub formatting: Formatting,
}

impl OutputPiece {
    /// Create a new output piece with default formatting.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            formatting: Formatting::default(),
        }
    }

    /// Create an output piece with specific formatting.
    pub fn with_formatting(text: impl Into<String>, formatting: Formatting) -> Self {
        Self {
            text: text.into(),
            formatting,
        }
    }
}

/// Builder for accumulating formatted output.
#[derive(Debug, Default)]
pub struct OutputBuilder {
    pieces: Vec<OutputPiece>,
}

impl OutputBuilder {
    /// Create a new output builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add plain text.
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            self.pieces.push(OutputPiece::new(text));
        }
    }

    /// Add formatted text.
    pub fn push_formatted(&mut self, text: impl Into<String>, formatting: &Formatting) {
        let text = text.into();
        if !text.is_empty() {
            self.pieces
                .push(OutputPiece::with_formatting(text, formatting.clone()));
        }
    }

    /// Add a prefix if present.
    pub fn push_prefix(&mut self, formatting: &Formatting) {
        if let Some(ref prefix) = formatting.prefix {
            self.push(prefix.clone());
        }
    }

    /// Add a suffix if present.
    pub fn push_suffix(&mut self, formatting: &Formatting) {
        if let Some(ref suffix) = formatting.suffix {
            self.push(suffix.clone());
        }
    }

    /// Check if the builder is empty.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Get the number of pieces.
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    /// Merge another builder into this one.
    pub fn append(&mut self, other: OutputBuilder) {
        self.pieces.extend(other.pieces);
    }

    /// Convert to a string, applying formatting markers.
    pub fn to_string(&self) -> String {
        let mut result = String::new();

        for piece in &self.pieces {
            let text = apply_text_case(&piece.text, &piece.formatting);
            let formatted = apply_formatting(&text, &piece.formatting);
            result.push_str(&formatted);
        }

        result
    }

    /// Convert to a string with quotes applied.
    pub fn to_string_with_quotes(&self, quotes: bool) -> String {
        let inner = self.to_string();
        if quotes {
            format!("\"{}\"", inner)
        } else {
            inner
        }
    }
}

/// Apply text case transformation.
fn apply_text_case(text: &str, formatting: &Formatting) -> String {
    use quarto_csl::TextCase;

    match formatting.text_case {
        Some(TextCase::Lowercase) => text.to_lowercase(),
        Some(TextCase::Uppercase) => text.to_uppercase(),
        Some(TextCase::CapitalizeFirst) => capitalize_first(text),
        Some(TextCase::CapitalizeAll) => capitalize_all(text),
        Some(TextCase::Title) => title_case(text),
        Some(TextCase::Sentence) => sentence_case(text),
        None => text.to_string(),
    }
}

/// Apply formatting markers (for now, simple text markers).
fn apply_formatting(text: &str, formatting: &Formatting) -> String {
    use quarto_csl::{FontStyle, FontWeight, VerticalAlign};

    let mut result = text.to_string();

    // Apply font style
    if let Some(FontStyle::Italic) = formatting.font_style {
        result = format!("*{}*", result);
    }

    // Apply font weight
    if let Some(FontWeight::Bold) = formatting.font_weight {
        result = format!("**{}**", result);
    }

    // Apply vertical align
    match formatting.vertical_align {
        Some(VerticalAlign::Sup) => result = format!("^{}^", result),
        Some(VerticalAlign::Sub) => result = format!("~{}~", result),
        _ => {}
    }

    // Strip periods if requested
    if formatting.strip_periods {
        result = result.replace('.', "");
    }

    result
}

/// Capitalize the first character.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Capitalize the first character of each word.
fn capitalize_all(s: &str) -> String {
    s.split_whitespace()
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Apply title case (capitalize major words).
fn title_case(s: &str) -> String {
    // Simplified title case - capitalize all words
    // A proper implementation would use language-specific rules
    capitalize_all(s)
}

/// Apply sentence case (lowercase, capitalize first word).
fn sentence_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let lower = s.to_lowercase();
    capitalize_first(&lower)
}

/// Join multiple outputs with a delimiter.
pub fn join_with_delimiter(outputs: Vec<OutputBuilder>, delimiter: &str) -> OutputBuilder {
    let mut result = OutputBuilder::new();
    let non_empty: Vec<_> = outputs.into_iter().filter(|o| !o.is_empty()).collect();

    for (i, output) in non_empty.into_iter().enumerate() {
        if i > 0 {
            result.push(delimiter);
        }
        result.append(output);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_builder_basic() {
        let mut builder = OutputBuilder::new();
        builder.push("Hello");
        builder.push(" ");
        builder.push("World");

        assert_eq!(builder.to_string(), "Hello World");
    }

    #[test]
    fn test_output_builder_formatting() {
        let mut builder = OutputBuilder::new();
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);

        builder.push_formatted("Title", &formatting);

        assert_eq!(builder.to_string(), "*Title*");
    }

    #[test]
    fn test_text_case() {
        let formatting = Formatting {
            text_case: Some(quarto_csl::TextCase::Uppercase),
            ..Default::default()
        };
        assert_eq!(apply_text_case("hello", &formatting), "HELLO");

        let formatting = Formatting {
            text_case: Some(quarto_csl::TextCase::Lowercase),
            ..Default::default()
        };
        assert_eq!(apply_text_case("HELLO", &formatting), "hello");

        let formatting = Formatting {
            text_case: Some(quarto_csl::TextCase::CapitalizeFirst),
            ..Default::default()
        };
        assert_eq!(apply_text_case("hello world", &formatting), "Hello world");
    }

    #[test]
    fn test_join_with_delimiter() {
        let mut a = OutputBuilder::new();
        a.push("A");

        let mut b = OutputBuilder::new();
        b.push("B");

        let mut c = OutputBuilder::new();
        c.push("C");

        let result = join_with_delimiter(vec![a, b, c], ", ");
        assert_eq!(result.to_string(), "A, B, C");
    }

    #[test]
    fn test_join_skips_empty() {
        let mut a = OutputBuilder::new();
        a.push("A");

        let empty = OutputBuilder::new();

        let mut c = OutputBuilder::new();
        c.push("C");

        let result = join_with_delimiter(vec![a, empty, c], ", ");
        assert_eq!(result.to_string(), "A, C");
    }

    // ========================================================================
    // Tests for new Output AST
    // ========================================================================

    #[test]
    fn test_output_literal() {
        let output = Output::literal("Hello");
        assert_eq!(output.render(), "Hello");
        assert!(!output.is_null());
    }

    #[test]
    fn test_output_literal_empty_is_null() {
        let output = Output::literal("");
        assert!(output.is_null());
        assert_eq!(output.render(), "");
    }

    #[test]
    fn test_output_formatted_basic() {
        let output = Output::formatted(
            Formatting::default(),
            vec![Output::literal("Hello"), Output::literal(" World")],
        );
        assert_eq!(output.render(), "Hello World");
    }

    #[test]
    fn test_output_formatted_with_italic() {
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);

        let output = Output::formatted(formatting, vec![Output::literal("Title")]);
        assert_eq!(output.render(), "*Title*");
    }

    #[test]
    fn test_output_formatted_with_prefix_suffix() {
        let mut formatting = Formatting::default();
        formatting.prefix = Some("(".to_string());
        formatting.suffix = Some(")".to_string());

        let output = Output::formatted(formatting, vec![Output::literal("2020")]);
        assert_eq!(output.render(), "(2020)");
    }

    #[test]
    fn test_output_formatted_filters_null() {
        let output = Output::formatted(
            Formatting::default(),
            vec![Output::literal("A"), Output::Null, Output::literal("B")],
        );
        assert_eq!(output.render(), "AB");
    }

    #[test]
    fn test_output_formatted_all_null_is_null() {
        let output = Output::formatted(Formatting::default(), vec![Output::Null, Output::Null]);
        assert!(output.is_null());
    }

    #[test]
    fn test_output_sequence() {
        let output = Output::sequence(vec![
            Output::literal("A"),
            Output::literal("B"),
            Output::literal("C"),
        ]);
        assert_eq!(output.render(), "ABC");
    }

    #[test]
    fn test_output_sequence_single_unwraps() {
        let output = Output::sequence(vec![Output::literal("Only")]);
        assert!(matches!(output, Output::Literal(_)));
        assert_eq!(output.render(), "Only");
    }

    #[test]
    fn test_output_tagged_transparent() {
        let output = Output::tagged(Tag::Title, Output::literal("My Book"));
        assert_eq!(output.render(), "My Book");
    }

    #[test]
    fn test_output_tagged_null_is_null() {
        let output = Output::tagged(Tag::Title, Output::Null);
        assert!(output.is_null());
    }

    #[test]
    fn test_join_outputs() {
        let outputs = vec![
            Output::literal("A"),
            Output::literal("B"),
            Output::literal("C"),
        ];
        let joined = join_outputs(outputs, ", ");
        assert_eq!(joined.render(), "A, B, C");
    }

    #[test]
    fn test_join_outputs_skips_null() {
        let outputs = vec![
            Output::literal("A"),
            Output::Null,
            Output::literal("C"),
        ];
        let joined = join_outputs(outputs, ", ");
        assert_eq!(joined.render(), "A, C");
    }

    #[test]
    fn test_join_outputs_single_unwraps() {
        let outputs = vec![Output::literal("Only")];
        let joined = join_outputs(outputs, ", ");
        assert!(matches!(joined, Output::Literal(_)));
    }

    #[test]
    fn test_output_nested_formatting() {
        let mut inner_fmt = Formatting::default();
        inner_fmt.font_style = Some(quarto_csl::FontStyle::Italic);

        let mut outer_fmt = Formatting::default();
        outer_fmt.prefix = Some("(".to_string());
        outer_fmt.suffix = Some(")".to_string());

        let inner = Output::formatted(inner_fmt, vec![Output::literal("Title")]);
        let outer = Output::formatted(outer_fmt, vec![inner, Output::literal(", 2020")]);

        assert_eq!(outer.render(), "(*Title*, 2020)");
    }
}
