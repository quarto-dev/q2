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

    /// Convert the output to Pandoc Inlines.
    ///
    /// This produces a `Vec<Inline>` that can be consumed by quarto-markdown-pandoc's
    /// HTML writer. The conversion handles:
    /// - Text formatting (italic, bold, small-caps, superscript, subscript)
    /// - Text case transformations
    /// - Quotes (using ASCII quote characters for now)
    /// - Strip periods
    /// - Prefix/suffix
    /// - Links
    /// - Notes
    ///
    /// Tags are transparent and don't affect the output structure.
    pub fn to_inlines(&self) -> quarto_pandoc_types::Inlines {
        to_inlines_inner(self)
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

// ============================================================================
// Pandoc Inline conversion
// ============================================================================

/// Helper to create an empty SourceInfo for generated content.
fn empty_source_info() -> quarto_source_map::SourceInfo {
    quarto_source_map::SourceInfo::default()
}

/// Helper to create an empty AttrSourceInfo.
fn empty_attr_source() -> quarto_pandoc_types::AttrSourceInfo {
    quarto_pandoc_types::AttrSourceInfo::empty()
}

/// Helper to create an empty TargetSourceInfo.
fn empty_target_source() -> quarto_pandoc_types::TargetSourceInfo {
    quarto_pandoc_types::TargetSourceInfo::empty()
}

/// Convert Output to Pandoc Inlines.
fn to_inlines_inner(output: &Output) -> quarto_pandoc_types::Inlines {
    use quarto_csl::{FontStyle, FontVariant, FontWeight, VerticalAlign};
    use quarto_pandoc_types::{
        empty_attr, Block, Emph, Inline, Link, Note, Paragraph, Quoted, QuoteType, SmallCaps,
        Str, Strong, Subscript, Superscript,
    };

    match output {
        Output::Null => vec![],
        Output::Literal(s) => {
            if s.is_empty() {
                vec![]
            } else {
                vec![Inline::Str(Str {
                    text: s.clone(),
                    source_info: empty_source_info(),
                })]
            }
        }
        Output::Tagged { child, .. } => {
            // Tags are transparent for rendering
            to_inlines_inner(child)
        }
        Output::Linked { url, children } => {
            let content: Vec<Inline> = children.iter().flat_map(to_inlines_inner).collect();
            if content.is_empty() {
                vec![]
            } else {
                vec![Inline::Link(Link {
                    attr: empty_attr(),
                    content,
                    target: (url.clone(), String::new()),
                    source_info: empty_source_info(),
                    attr_source: empty_attr_source(),
                    target_source: empty_target_source(),
                })]
            }
        }
        Output::InNote(child) => {
            let content = to_inlines_inner(child);
            if content.is_empty() {
                vec![]
            } else {
                // Wrap the inlines in a Paragraph block
                vec![Inline::Note(Note {
                    content: vec![Block::Paragraph(Paragraph {
                        content,
                        source_info: empty_source_info(),
                    })],
                    source_info: empty_source_info(),
                })]
            }
        }
        Output::Formatted {
            formatting,
            children,
        } => {
            // First, recursively convert children
            let mut inner: Vec<Inline> = children.iter().flat_map(to_inlines_inner).collect();

            if inner.is_empty() {
                return vec![];
            }

            // Apply text case transformation to all Str nodes in the inner content
            if let Some(text_case) = &formatting.text_case {
                apply_text_case_to_inlines(&mut inner, text_case);
            }

            // Apply strip periods to all Str nodes in the inner content
            if formatting.strip_periods {
                apply_strip_periods_to_inlines(&mut inner);
            }

            // Now wrap with formatting wrappers
            // Order matters: innermost first
            // font_style (italic) → font_weight (bold) → font_variant (small-caps)
            // → vertical_align (sup/sub) → quotes

            // Apply font_style (italic)
            if let Some(FontStyle::Italic) = formatting.font_style {
                inner = vec![Inline::Emph(Emph {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // Apply font_weight (bold)
            if let Some(FontWeight::Bold) = formatting.font_weight {
                inner = vec![Inline::Strong(Strong {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // Apply font_variant (small-caps)
            if let Some(FontVariant::SmallCaps) = formatting.font_variant {
                inner = vec![Inline::SmallCaps(SmallCaps {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // Apply vertical_align (superscript/subscript)
            match formatting.vertical_align {
                Some(VerticalAlign::Sup) => {
                    inner = vec![Inline::Superscript(Superscript {
                        content: inner,
                        source_info: empty_source_info(),
                    })];
                }
                Some(VerticalAlign::Sub) => {
                    inner = vec![Inline::Subscript(Subscript {
                        content: inner,
                        source_info: empty_source_info(),
                    })];
                }
                _ => {}
            }

            // Apply quotes
            if formatting.quotes {
                inner = vec![Inline::Quoted(Quoted {
                    quote_type: QuoteType::DoubleQuote,
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // Apply prefix
            if let Some(ref prefix) = formatting.prefix {
                if !prefix.is_empty() {
                    let mut result = vec![Inline::Str(Str {
                        text: prefix.clone(),
                        source_info: empty_source_info(),
                    })];
                    result.extend(inner);
                    inner = result;
                }
            }

            // Apply suffix
            if let Some(ref suffix) = formatting.suffix {
                if !suffix.is_empty() {
                    inner.push(Inline::Str(Str {
                        text: suffix.clone(),
                        source_info: empty_source_info(),
                    }));
                }
            }

            inner
        }
    }
}

/// Apply text case transformation to all Str nodes in an Inlines vector.
fn apply_text_case_to_inlines(inlines: &mut quarto_pandoc_types::Inlines, text_case: &quarto_csl::TextCase) {
    use quarto_csl::TextCase;
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter_mut() {
        match inline {
            Inline::Str(s) => {
                s.text = match text_case {
                    TextCase::Lowercase => s.text.to_lowercase(),
                    TextCase::Uppercase => s.text.to_uppercase(),
                    TextCase::CapitalizeFirst => capitalize_first(&s.text),
                    TextCase::CapitalizeAll => capitalize_all(&s.text),
                    TextCase::Title => title_case(&s.text),
                    TextCase::Sentence => sentence_case(&s.text),
                };
            }
            Inline::Emph(e) => apply_text_case_to_inlines(&mut e.content, text_case),
            Inline::Strong(s) => apply_text_case_to_inlines(&mut s.content, text_case),
            Inline::SmallCaps(s) => apply_text_case_to_inlines(&mut s.content, text_case),
            Inline::Superscript(s) => apply_text_case_to_inlines(&mut s.content, text_case),
            Inline::Subscript(s) => apply_text_case_to_inlines(&mut s.content, text_case),
            Inline::Quoted(q) => apply_text_case_to_inlines(&mut q.content, text_case),
            Inline::Link(l) => apply_text_case_to_inlines(&mut l.content, text_case),
            Inline::Span(s) => apply_text_case_to_inlines(&mut s.content, text_case),
            // Other inline types don't contain text we should transform
            _ => {}
        }
    }
}

/// Apply strip-periods transformation to all Str nodes in an Inlines vector.
fn apply_strip_periods_to_inlines(inlines: &mut quarto_pandoc_types::Inlines) {
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter_mut() {
        match inline {
            Inline::Str(s) => {
                s.text = s.text.replace('.', "");
            }
            Inline::Emph(e) => apply_strip_periods_to_inlines(&mut e.content),
            Inline::Strong(s) => apply_strip_periods_to_inlines(&mut s.content),
            Inline::SmallCaps(s) => apply_strip_periods_to_inlines(&mut s.content),
            Inline::Superscript(s) => apply_strip_periods_to_inlines(&mut s.content),
            Inline::Subscript(s) => apply_strip_periods_to_inlines(&mut s.content),
            Inline::Quoted(q) => apply_strip_periods_to_inlines(&mut q.content),
            Inline::Link(l) => apply_strip_periods_to_inlines(&mut l.content),
            Inline::Span(s) => apply_strip_periods_to_inlines(&mut s.content),
            _ => {}
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

// ============================================================================
// CSL HTML writer for tests
// ============================================================================

/// Render Pandoc Inlines to HTML using CSL conventions.
///
/// CSL test fixtures use `<i>` instead of `<em>`, `<b>` instead of `<strong>`,
/// and `<sc>` instead of CSS-based small-caps. This writer produces output
/// that matches the CSL test expectations.
///
/// This is intended for testing only - production code should use
/// quarto-markdown-pandoc's HTML writer.
pub fn render_inlines_to_csl_html(inlines: &quarto_pandoc_types::Inlines) -> String {
    let mut result = String::new();
    for inline in inlines {
        render_inline_to_csl_html(inline, &mut result);
    }
    result
}

fn render_inline_to_csl_html(inline: &quarto_pandoc_types::Inline, output: &mut String) {
    use quarto_pandoc_types::{Block, Inline};

    match inline {
        Inline::Str(s) => {
            // Escape HTML special characters
            output.push_str(&html_escape(&s.text));
        }
        Inline::Space(_) => {
            output.push(' ');
        }
        Inline::SoftBreak(_) => {
            output.push(' ');
        }
        Inline::LineBreak(_) => {
            output.push_str("<br/>");
        }
        Inline::Emph(e) => {
            output.push_str("<i>");
            for child in &e.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</i>");
        }
        Inline::Strong(s) => {
            output.push_str("<b>");
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</b>");
        }
        Inline::SmallCaps(s) => {
            output.push_str("<span style=\"font-variant:small-caps;\">");
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</span>");
        }
        Inline::Superscript(s) => {
            output.push_str("<sup>");
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</sup>");
        }
        Inline::Subscript(s) => {
            output.push_str("<sub>");
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</sub>");
        }
        Inline::Quoted(q) => {
            // Use ASCII quotes for CSL test compatibility
            match q.quote_type {
                quarto_pandoc_types::QuoteType::DoubleQuote => {
                    output.push('"');
                    for child in &q.content {
                        render_inline_to_csl_html(child, output);
                    }
                    output.push('"');
                }
                quarto_pandoc_types::QuoteType::SingleQuote => {
                    output.push('\'');
                    for child in &q.content {
                        render_inline_to_csl_html(child, output);
                    }
                    output.push('\'');
                }
            }
        }
        Inline::Link(l) => {
            output.push_str("<a href=\"");
            output.push_str(&html_escape(&l.target.0));
            output.push_str("\">");
            for child in &l.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</a>");
        }
        Inline::Note(n) => {
            // For notes, render the content inline (CSL doesn't use footnote markup)
            for block in &n.content {
                match block {
                    Block::Paragraph(p) => {
                        for child in &p.content {
                            render_inline_to_csl_html(child, output);
                        }
                    }
                    Block::Plain(p) => {
                        for child in &p.content {
                            render_inline_to_csl_html(child, output);
                        }
                    }
                    // Other block types are not expected in CSL output
                    _ => {}
                }
            }
        }
        Inline::Span(s) => {
            // Spans are transparent - just render children
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
        }
        Inline::Strikeout(s) => {
            output.push_str("<del>");
            for child in &s.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</del>");
        }
        Inline::Underline(u) => {
            output.push_str("<u>");
            for child in &u.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("</u>");
        }
        // Code, Math, RawInline, etc. are not expected in CSL output
        // but handle them gracefully
        Inline::Code(c) => {
            output.push_str("<code>");
            output.push_str(&html_escape(&c.text));
            output.push_str("</code>");
        }
        Inline::Math(m) => {
            output.push_str(&html_escape(&m.text));
        }
        Inline::RawInline(r) => {
            if r.format == "html" {
                output.push_str(&r.text);
            }
        }
        Inline::Image(i) => {
            output.push_str("<img src=\"");
            output.push_str(&html_escape(&i.target.0));
            output.push_str("\" alt=\"");
            // Render alt text from content
            for child in &i.content {
                render_inline_to_csl_html(child, output);
            }
            output.push_str("\"/>");
        }
        Inline::Cite(c) => {
            // Render citations inline
            for child in &c.content {
                render_inline_to_csl_html(child, output);
            }
        }
        // Quarto-specific types
        Inline::Shortcode(_)
        | Inline::NoteReference(_)
        | Inline::Attr(_, _)
        | Inline::Insert(_)
        | Inline::Delete(_)
        | Inline::Highlight(_)
        | Inline::EditComment(_) => {
            // Not expected in CSL output
        }
    }
}

/// Escape HTML special characters.
fn html_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            _ => result.push(c),
        }
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

    // ========================================================================
    // Tests for to_inlines() and render_inlines_to_csl_html()
    // ========================================================================

    #[test]
    fn test_to_inlines_literal() {
        let output = Output::literal("Hello World");
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "Hello World");
    }

    #[test]
    fn test_to_inlines_italic() {
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);

        let output = Output::formatted(formatting, vec![Output::literal("Title")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<i>Title</i>");
    }

    #[test]
    fn test_to_inlines_bold() {
        let mut formatting = Formatting::default();
        formatting.font_weight = Some(quarto_csl::FontWeight::Bold);

        let output = Output::formatted(formatting, vec![Output::literal("Author")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<b>Author</b>");
    }

    #[test]
    fn test_to_inlines_superscript() {
        let mut formatting = Formatting::default();
        formatting.vertical_align = Some(quarto_csl::VerticalAlign::Sup);

        let output = Output::formatted(formatting, vec![Output::literal("2")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<sup>2</sup>");
    }

    #[test]
    fn test_to_inlines_subscript() {
        let mut formatting = Formatting::default();
        formatting.vertical_align = Some(quarto_csl::VerticalAlign::Sub);

        let output = Output::formatted(formatting, vec![Output::literal("x")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<sub>x</sub>");
    }

    #[test]
    fn test_to_inlines_small_caps() {
        let mut formatting = Formatting::default();
        formatting.font_variant = Some(quarto_csl::FontVariant::SmallCaps);

        let output = Output::formatted(formatting, vec![Output::literal("Author")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<span style=\"font-variant:small-caps;\">Author</span>");
    }

    #[test]
    fn test_to_inlines_quotes() {
        let mut formatting = Formatting::default();
        formatting.quotes = true;

        let output = Output::formatted(formatting, vec![Output::literal("Title")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "\"Title\"");
    }

    #[test]
    fn test_to_inlines_prefix_suffix() {
        let mut formatting = Formatting::default();
        formatting.prefix = Some("(".to_string());
        formatting.suffix = Some(")".to_string());

        let output = Output::formatted(formatting, vec![Output::literal("2020")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "(2020)");
    }

    #[test]
    fn test_to_inlines_nested_formatting() {
        let mut inner_fmt = Formatting::default();
        inner_fmt.font_style = Some(quarto_csl::FontStyle::Italic);

        let mut outer_fmt = Formatting::default();
        outer_fmt.prefix = Some("(".to_string());
        outer_fmt.suffix = Some(")".to_string());

        let inner = Output::formatted(inner_fmt, vec![Output::literal("Title")]);
        let outer = Output::formatted(outer_fmt, vec![inner, Output::literal(", 2020")]);

        let inlines = outer.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "(<i>Title</i>, 2020)");
    }

    #[test]
    fn test_to_inlines_text_case_uppercase() {
        let mut formatting = Formatting::default();
        formatting.text_case = Some(quarto_csl::TextCase::Uppercase);

        let output = Output::formatted(formatting, vec![Output::literal("hello")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "HELLO");
    }

    #[test]
    fn test_to_inlines_strip_periods() {
        let mut formatting = Formatting::default();
        formatting.strip_periods = true;

        let output = Output::formatted(formatting, vec![Output::literal("Ph.D.")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "PhD");
    }

    #[test]
    fn test_to_inlines_link() {
        let output = Output::linked("https://example.com", vec![Output::literal("Example")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<a href=\"https://example.com\">Example</a>");
    }

    #[test]
    fn test_to_inlines_html_escape() {
        let output = Output::literal("A < B & C > D");
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "A &lt; B &amp; C &gt; D");
    }
}
