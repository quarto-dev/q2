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
    Names { variable: String, names: Vec<Name> },
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
    /// Marks content that should not have case transformations applied.
    NoCase,
    /// Marks content with no decoration (reset all formatting to normal).
    NoDecoration,
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
    Linked { url: String, children: Vec<Output> },
    /// Content that should appear in a footnote.
    InNote(Box<Output>),
    /// Literal text content.
    Literal(String),
    /// Semantically tagged content for post-processing.
    Tagged { tag: Tag, child: Box<Output> },
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

    /// Create a formatted node with children and a delimiter.
    ///
    /// The delimiter is stored in the formatting and applied at render time,
    /// enabling smart punctuation handling.
    pub fn formatted_with_delimiter(
        mut formatting: Formatting,
        children: Vec<Output>,
        delimiter: &str,
    ) -> Self {
        // Filter out null children
        let children: Vec<_> = children.into_iter().filter(|c| !c.is_null()).collect();
        if children.is_empty() {
            Output::Null
        } else {
            if !delimiter.is_empty() {
                formatting.delimiter = Some(delimiter.to_string());
            }
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

    /// Capitalize the first term in the output content.
    ///
    /// This is used for sentence-initial capitalization in citations.
    /// Only capitalizes content tagged as Term (locale terms like "ibid", "et al").
    /// Literal values from `<text value="..."/>` are NOT capitalized.
    pub fn capitalize_first(self) -> Self {
        self.capitalize_first_inner().0
    }

    /// Internal helper that returns (modified_output, did_find_content).
    /// Only capitalizes content inside Tag::Term nodes at the very start.
    /// Returns true in the second element if we found any non-empty content
    /// (whether we capitalized it or not), signaling to stop searching.
    fn capitalize_first_inner(self) -> (Self, bool) {
        match self {
            Output::Null => (Output::Null, false),
            // Don't capitalize bare literals - they come from <text value="..."/>
            // But DO signal that we found content, so we stop looking
            Output::Literal(s) => {
                let found_content = !s.is_empty();
                (Output::Literal(s), found_content)
            }
            Output::Formatted {
                formatting,
                children,
            } => {
                let mut new_children = Vec::with_capacity(children.len());
                let mut capitalized = false;
                for child in children {
                    if capitalized {
                        new_children.push(child);
                    } else {
                        let (new_child, did_cap) = child.capitalize_first_inner();
                        new_children.push(new_child);
                        capitalized = did_cap;
                    }
                }
                (
                    Output::Formatted {
                        formatting,
                        children: new_children,
                    },
                    capitalized,
                )
            }
            Output::Linked { url, children } => {
                let mut new_children = Vec::with_capacity(children.len());
                let mut capitalized = false;
                for child in children {
                    if capitalized {
                        new_children.push(child);
                    } else {
                        let (new_child, did_cap) = child.capitalize_first_inner();
                        new_children.push(new_child);
                        capitalized = did_cap;
                    }
                }
                (
                    Output::Linked {
                        url,
                        children: new_children,
                    },
                    capitalized,
                )
            }
            Output::InNote(child) => {
                let (new_child, did_cap) = child.capitalize_first_inner();
                (Output::InNote(Box::new(new_child)), did_cap)
            }
            Output::Tagged { tag, child } => {
                // Only capitalize specific citation terms at sentence start
                // (ibid, idem, infra, supra, op. cit., loc. cit., etc.)
                // NOT labels like "pages", "chapters", etc.
                let should_capitalize = match &tag {
                    Tag::Term(name) => matches!(
                        name.as_str(),
                        "ibid" | "ibidem" | "idem" | "infra" | "supra" | "op. cit." | "loc. cit."
                    ),
                    _ => false,
                };

                if should_capitalize {
                    let capitalized_child = capitalize_literal(*child);
                    (
                        Output::Tagged {
                            tag,
                            child: Box::new(capitalized_child),
                        },
                        true,
                    )
                } else {
                    // For other tags, recurse into child
                    let (new_child, did_cap) = child.capitalize_first_inner();
                    (
                        Output::Tagged {
                            tag,
                            child: Box::new(new_child),
                        },
                        did_cap,
                    )
                }
            }
        }
    }
}

/// Capitalize the first letter of a literal Output.
/// Recursively finds the first literal and capitalizes it.
fn capitalize_literal(output: Output) -> Output {
    match output {
        Output::Null => Output::Null,
        Output::Literal(s) => {
            if s.is_empty() {
                Output::Literal(s)
            } else {
                let mut chars = s.chars();
                let first = chars.next().unwrap();
                let rest: String = chars.collect();
                Output::Literal(format!("{}{}", first.to_uppercase(), rest))
            }
        }
        Output::Formatted {
            formatting,
            mut children,
        } => {
            // Capitalize first non-empty child
            for child in children.iter_mut() {
                if !child.is_null() {
                    *child = capitalize_literal(std::mem::replace(child, Output::Null));
                    break;
                }
            }
            Output::Formatted {
                formatting,
                children,
            }
        }
        Output::Linked { url, mut children } => {
            for child in children.iter_mut() {
                if !child.is_null() {
                    *child = capitalize_literal(std::mem::replace(child, Output::Null));
                    break;
                }
            }
            Output::Linked { url, children }
        }
        Output::InNote(child) => Output::InNote(Box::new(capitalize_literal(*child))),
        Output::Tagged { tag, child } => Output::Tagged {
            tag,
            child: Box::new(capitalize_literal(*child)),
        },
    }
}

impl Output {
    /// Render the output to a plain string.
    pub fn render(&self) -> String {
        match self {
            Output::Null => String::new(),
            Output::Literal(s) => s.clone(),
            Output::Formatted {
                formatting,
                children,
            } => {
                // Join children with delimiter if specified, using smart punctuation handling
                let inner: String = if let Some(ref delim) = formatting.delimiter {
                    let rendered: Vec<String> = children
                        .iter()
                        .map(|c| c.render())
                        .filter(|s| !s.is_empty())
                        .collect();
                    join_with_smart_delim(rendered, delim)
                } else {
                    // No delimiter - still apply punctuation fixing
                    let rendered: Vec<String> = children.iter().map(|c| c.render()).collect();
                    fix_punct(rendered).join("")
                };
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

    /// Extract the date text WITHOUT year suffix (for year-suffix collapsing comparison).
    ///
    /// This renders the date but excludes any TagYearSuffix content, so we can
    /// compare dates by year only and collapse same-year items.
    pub fn extract_date_text_without_suffix(&self) -> Option<String> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(date) = child.extract_date_text_without_suffix() {
                        return Some(date);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_date_text_without_suffix(),
            Output::Tagged { tag, child } => match tag {
                Tag::Date(_) => Some(child.render_without_year_suffix()),
                _ => child.extract_date_text_without_suffix(),
            },
        }
    }

    /// Render this output to a string, excluding any TagYearSuffix content.
    fn render_without_year_suffix(&self) -> String {
        match self {
            Output::Null => String::new(),
            Output::Literal(s) => s.clone(),
            Output::Formatted {
                formatting,
                children,
            } => {
                let child_strings: Vec<String> = children
                    .iter()
                    .map(|c| c.render_without_year_suffix())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut result = if let Some(ref delim) = formatting.delimiter {
                    child_strings.join(delim)
                } else {
                    child_strings.concat()
                };
                if let Some(ref prefix) = formatting.prefix {
                    result = format!("{}{}", prefix, result);
                }
                if let Some(ref suffix) = formatting.suffix {
                    result = format!("{}{}", result, suffix);
                }
                result
            }
            Output::Linked { children, .. } => {
                children
                    .iter()
                    .map(|c| c.render_without_year_suffix())
                    .collect::<Vec<_>>()
                    .concat()
            }
            Output::InNote(child) => child.render_without_year_suffix(),
            Output::Tagged { tag, child } => {
                // Skip year suffix tags
                if matches!(tag, Tag::YearSuffix(_)) {
                    String::new()
                } else {
                    child.render_without_year_suffix()
                }
            }
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

    /// Extract all tagged citation items from this output.
    ///
    /// Returns a vector of (item_id, item_type, output) tuples for each
    /// Tag::Item found in the tree.
    pub fn extract_citation_items(&self) -> Vec<(String, CitationItemType, Output)> {
        let mut items = Vec::new();
        self.extract_citation_items_into(&mut items);
        items
    }

    fn extract_citation_items_into(&self, items: &mut Vec<(String, CitationItemType, Output)>) {
        match self {
            Output::Null | Output::Literal(_) => {}
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    child.extract_citation_items_into(items);
                }
            }
            Output::InNote(child) => child.extract_citation_items_into(items),
            Output::Tagged { tag, child } => {
                if let Tag::Item { item_id, item_type } = tag {
                    items.push((item_id.clone(), item_type.clone(), (**child).clone()));
                }
                child.extract_citation_items_into(items);
            }
        }
    }

    /// Extract all names (from Tag::Names tags) from this output.
    ///
    /// Returns a vector of Name objects found in the tree.
    pub fn extract_all_names(&self) -> Vec<Name> {
        let mut names = Vec::new();
        self.extract_all_names_into(&mut names);
        names
    }

    fn extract_all_names_into(&self, names: &mut Vec<Name>) {
        match self {
            Output::Null | Output::Literal(_) => {}
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    child.extract_all_names_into(names);
                }
            }
            Output::InNote(child) => child.extract_all_names_into(names),
            Output::Tagged { tag, child } => {
                if let Tag::Names {
                    names: tag_names, ..
                } = tag
                {
                    names.extend(tag_names.iter().cloned());
                }
                child.extract_all_names_into(names);
            }
        }
    }

    /// Check if this output contains a Tag::Prefix tag anywhere in its tree.
    pub fn has_prefix_tag(&self) -> bool {
        match self {
            Output::Null | Output::Literal(_) => false,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                children.iter().any(|c| c.has_prefix_tag())
            }
            Output::InNote(child) => child.has_prefix_tag(),
            Output::Tagged { tag, child } => {
                matches!(tag, Tag::Prefix) || child.has_prefix_tag()
            }
        }
    }

    /// Check if this output contains a Tag::Suffix tag anywhere in its tree.
    pub fn has_suffix_tag(&self) -> bool {
        match self {
            Output::Null | Output::Literal(_) => false,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                children.iter().any(|c| c.has_suffix_tag())
            }
            Output::InNote(child) => child.has_suffix_tag(),
            Output::Tagged { tag, child } => {
                matches!(tag, Tag::Suffix) || child.has_suffix_tag()
            }
        }
    }

    /// Extract the year-suffix value (Tag::YearSuffix) from this output.
    /// Returns None if no year suffix is found.
    pub fn extract_year_suffix(&self) -> Option<i32> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(suffix) = child.extract_year_suffix() {
                        return Some(suffix);
                    }
                }
                None
            }
            Output::InNote(child) => child.extract_year_suffix(),
            Output::Tagged { tag, child } => match tag {
                Tag::YearSuffix(n) => Some(*n),
                _ => child.extract_year_suffix(),
            },
        }
    }

    /// Return a copy of this output with the date (Tag::Date) replaced by just
    /// the year-suffix content (for year-suffix collapsing).
    ///
    /// This removes the year from the date and keeps only the suffix.
    /// If there's no year-suffix, the date becomes null.
    pub fn extract_year_suffix_only(&self) -> Output {
        match self {
            Output::Null => Output::Null,
            Output::Literal(s) => Output::Literal(s.clone()),
            Output::Formatted {
                formatting,
                children,
            } => {
                let new_children: Vec<_> = children
                    .iter()
                    .map(|c| c.extract_year_suffix_only())
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
                    .map(|c| c.extract_year_suffix_only())
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
                let new_child = child.extract_year_suffix_only();
                if new_child.is_null() {
                    Output::Null
                } else {
                    Output::InNote(Box::new(new_child))
                }
            }
            Output::Tagged { tag, child } => match tag {
                // For date tags, extract only the year suffix from the contents
                Tag::Date(_) => {
                    // Find year-suffix within this date and return just that
                    self.find_year_suffix_output().unwrap_or(Output::Null)
                }
                _ => {
                    let new_child = child.extract_year_suffix_only();
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

    /// Find the year-suffix Output node within this tree.
    fn find_year_suffix_output(&self) -> Option<Output> {
        match self {
            Output::Null | Output::Literal(_) => None,
            Output::Formatted { children, .. } | Output::Linked { children, .. } => {
                for child in children {
                    if let Some(suffix) = child.find_year_suffix_output() {
                        return Some(suffix);
                    }
                }
                None
            }
            Output::InNote(child) => child.find_year_suffix_output(),
            Output::Tagged { tag, child } => {
                if matches!(tag, Tag::YearSuffix(_)) {
                    // Return a copy of this entire Tagged node
                    Some(self.clone())
                } else {
                    child.find_year_suffix_output()
                }
            }
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
    ///
    /// Note: This method ignores the `display` attribute. For bibliography entries
    /// that use display attributes, use `to_blocks()` instead.
    pub fn to_inlines(&self) -> quarto_pandoc_types::Inlines {
        to_inlines_inner(self)
    }

    /// Convert the output to Pandoc Blocks.
    ///
    /// This produces a `Vec<Block>` that properly handles the `display` attribute
    /// used in bibliography formatting. The display attribute creates:
    /// - `display="block"` → `<div class="csl-block">`
    /// - `display="left-margin"` → `<div class="csl-left-margin">`
    /// - `display="right-inline"` → `<div class="csl-right-inline">`
    /// - `display="indent"` → `<div class="csl-indent">`
    ///
    /// Content without display attributes is wrapped in a Plain block.
    pub fn to_blocks(&self) -> Vec<quarto_pandoc_types::Block> {
        to_blocks_inner(self)
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
        Block, Emph, Inline, Link, Note, Paragraph, QuoteType, Quoted, SmallCaps, Span, Str,
        Strong, Subscript, Superscript, empty_attr,
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
        Output::Tagged { tag, child } => {
            // Most tags are transparent for rendering
            let inner = to_inlines_inner(child);
            match tag {
                Tag::NoCase => {
                    // Wrap in a Span with class "nocase" to prevent text-case transformations
                    if inner.is_empty() {
                        vec![]
                    } else {
                        // Create attr with nocase class: (id, classes, attrs)
                        let mut attr = empty_attr();
                        attr.1.push("nocase".to_string());
                        vec![Inline::Span(Span {
                            attr,
                            content: inner,
                            source_info: empty_source_info(),
                            attr_source: empty_attr_source(),
                        })]
                    }
                }
                Tag::NoDecoration => {
                    // Wrap in a Span with class "nodecoration" to reset formatting
                    if inner.is_empty() {
                        vec![]
                    } else {
                        let mut attr = empty_attr();
                        attr.1.push("nodecoration".to_string());
                        vec![Inline::Span(Span {
                            attr,
                            content: inner,
                            source_info: empty_source_info(),
                            attr_source: empty_attr_source(),
                        })]
                    }
                }
                _ => inner,
            }
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
            // Collect as Vec<Vec<Inline>> to apply fix_punct_siblings at sibling boundaries
            let child_results: Vec<Vec<Inline>> = children
                .iter()
                .map(to_inlines_inner)
                .filter(|inlines| !inlines.is_empty())
                .collect();

            if child_results.is_empty() {
                return vec![];
            }

            // Pandoc's order of operations:
            // 1. Render children (done above)
            // 2. Add delimiters as separate elements
            // 3. Apply fixPunct on the whole list (including delimiters)
            // 4. mconcat to flatten

            // Step 2: Add delimiters between elements (as separate siblings)
            let with_delimiters: Vec<Vec<Inline>> = if let Some(ref delim) = formatting.delimiter {
                let mut result = Vec::new();
                for (i, child_inlines) in child_results.into_iter().enumerate() {
                    if i > 0 && !delim.is_empty() {
                        // Smart delimiter: skip if next element starts with punctuation
                        // (This is Pandoc's addDelimiters behavior)
                        let first_char = get_leading_char(&child_inlines);
                        if !matches!(first_char, Some(',') | Some(';') | Some('.')) {
                            result.push(vec![Inline::Str(Str {
                                text: delim.clone(),
                                source_info: empty_source_info(),
                            })]);
                        }
                    }
                    result.push(child_inlines);
                }
                result
            } else {
                child_results
            };

            // Step 3: Apply fix_punct_siblings to fix collisions at sibling boundaries
            // This includes collisions between content and delimiters
            let fixed_siblings = fix_punct_siblings(with_delimiters);

            // Step 4: Flatten into a single Vec<Inline>
            let mut inner: Vec<Inline> = fixed_siblings.into_iter().flatten().collect();

            if inner.is_empty() {
                return vec![];
            }

            // CSL formatting order (from innermost to outermost):
            // 1. strip-periods
            // 2. prefix/suffix (when affixes_inside=true, for layout elements)
            // 3. font-style (italic)
            // 4. text-case
            // 5. font-variant (small-caps)
            // 6. font-weight (bold)
            // 7. vertical-align (sup/sub)
            // 8. quotes
            // 9. prefix/suffix (when affixes_inside=false, for regular elements)
            //
            // See Pandoc citeproc Types.hs:addFormatting for reference.

            // 1. Apply strip periods
            if formatting.strip_periods {
                apply_strip_periods_to_inlines(&mut inner);
            }

            // 2. Apply prefix/suffix INSIDE formatting (for layout elements)
            if formatting.affixes_inside {
                apply_affixes(&mut inner, &formatting.prefix, &formatting.suffix);
            }

            // 3. Apply font_style (italic)
            if let Some(FontStyle::Italic) = formatting.font_style {
                inner = vec![Inline::Emph(Emph {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // 4. Apply text case transformation to all Str nodes
            if let Some(text_case) = &formatting.text_case {
                apply_text_case_to_inlines(&mut inner, text_case);
            }

            // 5. Apply font_variant (small-caps)
            if let Some(FontVariant::SmallCaps) = formatting.font_variant {
                inner = vec![Inline::SmallCaps(SmallCaps {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // 6. Apply font_weight (bold)
            if let Some(FontWeight::Bold) = formatting.font_weight {
                inner = vec![Inline::Strong(Strong {
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // 7. Apply vertical_align (superscript/subscript)
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

            // 8. Apply quotes
            if formatting.quotes {
                inner = vec![Inline::Quoted(Quoted {
                    quote_type: QuoteType::DoubleQuote,
                    content: inner,
                    source_info: empty_source_info(),
                })];
            }

            // 9. Apply prefix/suffix OUTSIDE formatting (for regular elements)
            if !formatting.affixes_inside {
                apply_affixes(&mut inner, &formatting.prefix, &formatting.suffix);
            }

            inner
        }
    }
}

// ============================================================================
// Pandoc Block conversion (for display attribute support)
// ============================================================================

/// A display region extracted from the Output AST.
#[derive(Debug)]
struct DisplayRegion {
    display: Option<quarto_csl::Display>,
    content: Output,
}

/// Convert Output to Pandoc Blocks, handling the display attribute.
///
/// The display attribute creates block-level structure in bibliography entries.
/// This function extracts display regions and wraps them in Div blocks with
/// the appropriate CSS classes.
fn to_blocks_inner(output: &Output) -> Vec<quarto_pandoc_types::Block> {
    use quarto_pandoc_types::{Block, Div, Plain};

    // Extract display regions from the output
    let regions = extract_display_regions(output);

    // If there's only one region with no display attribute, just wrap in Plain
    if regions.len() == 1 && regions[0].display.is_none() {
        let inlines = to_inlines_inner(&regions[0].content);
        if inlines.is_empty() {
            return vec![];
        }
        return vec![Block::Plain(Plain {
            content: inlines,
            source_info: empty_source_info(),
        })];
    }

    // Create blocks for each display region
    let mut blocks = Vec::new();
    for region in regions {
        let inlines = to_inlines_inner(&region.content);
        if inlines.is_empty() {
            continue;
        }

        match region.display {
            Some(display) => {
                // Create a Div with the appropriate CSS class
                let class = match display {
                    quarto_csl::Display::Block => "csl-block",
                    quarto_csl::Display::LeftMargin => "csl-left-margin",
                    quarto_csl::Display::RightInline => "csl-right-inline",
                    quarto_csl::Display::Indent => "csl-indent",
                };
                let mut attr = quarto_pandoc_types::empty_attr();
                attr.1.push(class.to_string());

                blocks.push(Block::Div(Div {
                    attr,
                    content: vec![Block::Plain(Plain {
                        content: inlines,
                        source_info: empty_source_info(),
                    })],
                    source_info: empty_source_info(),
                    attr_source: empty_attr_source(),
                }));
            }
            None => {
                // No display attribute - wrap in Plain
                blocks.push(Block::Plain(Plain {
                    content: inlines,
                    source_info: empty_source_info(),
                }));
            }
        }
    }

    blocks
}

/// Extract display regions from an Output AST.
///
/// This walks the top-level children and groups content by display attribute.
/// Content with the same display attribute (or no display attribute) is grouped together.
fn extract_display_regions(output: &Output) -> Vec<DisplayRegion> {
    match output {
        Output::Null => vec![],
        Output::Literal(_) => vec![DisplayRegion {
            display: None,
            content: output.clone(),
        }],
        Output::Tagged { child, .. } => {
            // Tags like NoDecoration and NoCase must be preserved as-is.
            // They are inline-level markup that should not be stripped during
            // display region extraction. If there's a display attribute nested
            // inside (unusual), we still preserve the tag wrapper.
            //
            // Check if the child has any display regions that need extraction.
            // If all child regions have no display, preserve the whole Tagged node.
            // If there are display regions inside, we need to return those (losing
            // the tag at block level is acceptable since display creates blocks).
            let child_regions = extract_display_regions(child);
            if child_regions.iter().all(|r| r.display.is_none()) {
                // No display in child - preserve the whole Tagged node
                vec![DisplayRegion {
                    display: None,
                    content: output.clone(),
                }]
            } else {
                // There's display inside - return child regions
                // (Tag may be lost but display creates block-level structure)
                child_regions
            }
        }
        Output::Linked { .. } | Output::InNote(_) => vec![DisplayRegion {
            display: None,
            content: output.clone(),
        }],
        Output::Formatted {
            formatting,
            children,
        } => {
            // If this node has a display attribute, it's a single region
            if formatting.display.is_some() {
                return vec![DisplayRegion {
                    display: formatting.display,
                    content: output.clone(),
                }];
            }

            // No display attribute on this node - check children
            let mut regions = Vec::new();
            let mut current_no_display = Vec::new();

            for child in children {
                let child_regions = extract_display_regions(child);

                for region in child_regions {
                    if region.display.is_some() {
                        // Flush accumulated no-display content first
                        if !current_no_display.is_empty() {
                            regions.push(DisplayRegion {
                                display: None,
                                content: Output::Formatted {
                                    formatting: formatting.clone(),
                                    children: std::mem::take(&mut current_no_display),
                                },
                            });
                        }
                        regions.push(region);
                    } else {
                        // Accumulate content without display attribute
                        current_no_display.push(region.content);
                    }
                }
            }

            // Flush remaining no-display content
            if !current_no_display.is_empty() {
                regions.push(DisplayRegion {
                    display: None,
                    content: Output::Formatted {
                        formatting: formatting.clone(),
                        children: current_no_display,
                    },
                });
            }

            // If no regions were found, treat the whole thing as one region
            if regions.is_empty() {
                regions.push(DisplayRegion {
                    display: None,
                    content: output.clone(),
                });
            }

            regions
        }
    }
}

/// Apply text case transformation to all Str nodes in an Inlines vector.
fn apply_text_case_to_inlines(
    inlines: &mut quarto_pandoc_types::Inlines,
    text_case: &quarto_csl::TextCase,
) {
    use quarto_csl::TextCase;

    // For title case, we need to track state across all Str nodes
    // and detect the last word for capitalization
    if matches!(text_case, TextCase::Title) {
        let mut seen_first_word = false;
        // Find the index of the last Str that contains word content
        let last_word_index = find_last_word_str_index(inlines);
        apply_title_case_to_inlines_with_last(
            inlines,
            &mut seen_first_word,
            last_word_index,
            &mut 0,
        );
    } else {
        apply_text_case_to_inlines_simple(inlines, text_case);
    }
}

/// Count the number of Str nodes in an Inlines structure.
/// Used to keep indices in sync when skipping protected content.
fn count_str_nodes(inlines: &quarto_pandoc_types::Inlines) -> usize {
    use quarto_pandoc_types::Inline;

    fn count(inline: &Inline) -> usize {
        match inline {
            Inline::Str(_) => 1,
            Inline::Emph(e) => e.content.iter().map(count).sum(),
            Inline::Strong(s) => s.content.iter().map(count).sum(),
            Inline::Quoted(q) => q.content.iter().map(count).sum(),
            Inline::Link(l) => l.content.iter().map(count).sum(),
            Inline::Span(s) => s.content.iter().map(count).sum(),
            // SmallCaps, Superscript, Subscript don't contribute to count
            // because find_last_word_str_index also skips them
            _ => 0,
        }
    }

    inlines.iter().map(count).sum()
}

/// Find the index of the last Str node that contains word characters.
/// Returns None if no such node exists.
fn find_last_word_str_index(inlines: &quarto_pandoc_types::Inlines) -> Option<usize> {
    use quarto_pandoc_types::Inline;

    let mut last_index: Option<usize> = None;
    let mut current_index = 0;

    fn visit(
        inline: &quarto_pandoc_types::Inline,
        current_index: &mut usize,
        last_index: &mut Option<usize>,
    ) {
        match inline {
            Inline::Str(s) => {
                // Check if this Str contains any word characters
                if s.text.chars().any(|c| c.is_alphabetic()) {
                    *last_index = Some(*current_index);
                }
                *current_index += 1;
            }
            Inline::Emph(e) => {
                for child in &e.content {
                    visit(child, current_index, last_index);
                }
            }
            Inline::Strong(s) => {
                for child in &s.content {
                    visit(child, current_index, last_index);
                }
            }
            Inline::Quoted(q) => {
                for child in &q.content {
                    visit(child, current_index, last_index);
                }
            }
            Inline::Link(l) => {
                for child in &l.content {
                    visit(child, current_index, last_index);
                }
            }
            Inline::Span(s) => {
                for child in &s.content {
                    visit(child, current_index, last_index);
                }
            }
            // SmallCaps, Superscript, Subscript are "implicit nocase" - don't count them
            // for last word detection since they won't be transformed
            _ => {}
        }
    }

    for inline in inlines.iter() {
        visit(inline, &mut current_index, &mut last_index);
    }

    last_index
}

/// Apply title case transformation with last-word tracking.
/// `last_word_index` is the index of the last Str node containing word content.
/// `current_index` tracks the current Str node index during traversal.
fn apply_title_case_to_inlines_with_last(
    inlines: &mut quarto_pandoc_types::Inlines,
    seen_first_word: &mut bool,
    last_word_index: Option<usize>,
    current_index: &mut usize,
) {
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter_mut() {
        match inline {
            Inline::Str(s) => {
                let is_last_word_segment = last_word_index == Some(*current_index);
                s.text =
                    title_case_with_state_and_last(&s.text, seen_first_word, is_last_word_segment);
                *current_index += 1;
            }
            Inline::Emph(e) => {
                apply_title_case_to_inlines_with_last(
                    &mut e.content,
                    seen_first_word,
                    last_word_index,
                    current_index,
                );
            }
            Inline::Strong(s) => {
                apply_title_case_to_inlines_with_last(
                    &mut s.content,
                    seen_first_word,
                    last_word_index,
                    current_index,
                );
            }
            // SmallCaps, Superscript, Subscript are "implicit nocase" per CSL-JSON spec
            // But they still "consume" the first word position if they contain text
            Inline::SmallCaps(s) => {
                if has_text_content(&s.content) {
                    *seen_first_word = true;
                }
            }
            Inline::Superscript(s) => {
                if has_text_content(&s.content) {
                    *seen_first_word = true;
                }
            }
            Inline::Subscript(s) => {
                if has_text_content(&s.content) {
                    *seen_first_word = true;
                }
            }
            Inline::Quoted(q) => {
                apply_title_case_to_inlines_with_last(
                    &mut q.content,
                    seen_first_word,
                    last_word_index,
                    current_index,
                );
            }
            Inline::Link(l) => {
                apply_title_case_to_inlines_with_last(
                    &mut l.content,
                    seen_first_word,
                    last_word_index,
                    current_index,
                );
            }
            Inline::Span(s) => {
                let (_, classes, _) = &s.attr;
                // nocase and nodecoration spans are protected from text-case transformations
                // per CSL-JSON spec and Pandoc citeproc behavior
                if classes.iter().any(|c| c == "nocase" || c == "nodecoration") {
                    // These spans still consume the first word position
                    if has_text_content(&s.content) {
                        *seen_first_word = true;
                    }
                    // Increment current_index by the number of Str nodes inside
                    // to keep in sync with find_last_word_str_index
                    *current_index += count_str_nodes(&s.content);
                } else {
                    apply_title_case_to_inlines_with_last(
                        &mut s.content,
                        seen_first_word,
                        last_word_index,
                        current_index,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Apply non-title text case transformations (simple per-node processing).
fn apply_text_case_to_inlines_simple(
    inlines: &mut quarto_pandoc_types::Inlines,
    text_case: &quarto_csl::TextCase,
) {
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
                    TextCase::Title => unreachable!(), // Handled separately
                    TextCase::Sentence => sentence_case(&s.text),
                };
            }
            Inline::Emph(e) => apply_text_case_to_inlines_simple(&mut e.content, text_case),
            Inline::Strong(s) => apply_text_case_to_inlines_simple(&mut s.content, text_case),
            // SmallCaps, Superscript, Subscript are "implicit nocase" per CSL-JSON spec
            Inline::SmallCaps(_) => {}
            Inline::Superscript(_) => {}
            Inline::Subscript(_) => {}
            Inline::Quoted(q) => apply_text_case_to_inlines_simple(&mut q.content, text_case),
            Inline::Link(l) => apply_text_case_to_inlines_simple(&mut l.content, text_case),
            Inline::Span(s) => {
                let (_, classes, _) = &s.attr;
                // nocase and nodecoration spans are protected from text-case transformations
                // per CSL-JSON spec and Pandoc citeproc behavior
                if !classes.iter().any(|c| c == "nocase" || c == "nodecoration") {
                    apply_text_case_to_inlines_simple(&mut s.content, text_case);
                }
            }
            _ => {}
        }
    }
}

/// Check if Inlines contain any actual text content.
fn has_text_content(inlines: &quarto_pandoc_types::Inlines) -> bool {
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter() {
        match inline {
            Inline::Str(s) => {
                if s.text.chars().any(|c| c.is_alphabetic()) {
                    return true;
                }
            }
            Inline::Emph(e) => {
                if has_text_content(&e.content) {
                    return true;
                }
            }
            Inline::Strong(s) => {
                if has_text_content(&s.content) {
                    return true;
                }
            }
            Inline::SmallCaps(s) => {
                if has_text_content(&s.content) {
                    return true;
                }
            }
            Inline::Superscript(s) => {
                if has_text_content(&s.content) {
                    return true;
                }
            }
            Inline::Subscript(s) => {
                if has_text_content(&s.content) {
                    return true;
                }
            }
            Inline::Quoted(q) => {
                if has_text_content(&q.content) {
                    return true;
                }
            }
            Inline::Link(l) => {
                if has_text_content(&l.content) {
                    return true;
                }
            }
            Inline::Span(s) => {
                if has_text_content(&s.content) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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

    // Apply quotes (Unicode curly quotes as per CSL locale conventions)
    let quoted = if formatting.quotes {
        format!("\u{201C}{}\u{201D}", aligned) // "..."
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
// Smart Punctuation Handling
// ============================================================================

/// Fix punctuation collisions between adjacent strings.
///
/// This implements the CSL/citeproc punctuation collision algorithm:
/// - When two punctuation marks collide, one may be suppressed
/// - Based on Pandoc citeproc's fixPunct function
///
/// Returns a vector of strings with punctuation collisions fixed.
pub fn fix_punct(strings: Vec<String>) -> Vec<String> {
    if strings.len() < 2 {
        return strings;
    }

    let mut result = Vec::with_capacity(strings.len());
    let mut iter = strings.into_iter().peekable();

    while let Some(x) = iter.next() {
        if let Some(y) = iter.peek() {
            let x_end = x.chars().last().unwrap_or('\u{FFFD}');
            let y_start = y.chars().next().unwrap_or('\u{FFFD}');

            // Determine how to handle the punctuation collision
            let (keep_x, keep_y) = match (x_end, y_start) {
                // Based on Pandoc citeproc Types.hs fixPunct
                ('!', '.') => (true, false), // keepFirst
                ('!', '?') => (true, true),  // keepBoth
                ('!', ':') => (true, false), // keepFirst
                ('!', ',') => (true, true),  // keepBoth
                ('!', ';') => (true, true),  // keepBoth
                ('?', '!') => (true, true),  // keepBoth
                ('?', '.') => (true, false), // keepFirst
                ('?', ':') => (true, false), // keepFirst
                ('?', ',') => (true, true),  // keepBoth
                ('?', ';') => (true, true),  // keepBoth
                ('.', '!') => (true, true),  // keepBoth
                ('.', '?') => (true, true),  // keepBoth
                ('.', ':') => (true, true),  // keepBoth
                ('.', ',') => (true, true),  // keepBoth
                ('.', ';') => (true, true),  // keepBoth
                (':', '!') => (false, true), // keepSecond
                (':', '?') => (false, true), // keepSecond
                (':', '.') => (true, false), // keepFirst
                (':', ',') => (true, true),  // keepBoth
                (':', ';') => (true, true),  // keepBoth
                (',', '!') => (true, true),  // keepBoth
                (',', '?') => (true, true),  // keepBoth
                (',', ':') => (true, true),  // keepBoth
                (',', '.') => (true, true),  // keepBoth
                (',', ';') => (true, true),  // keepBoth
                (';', '!') => (false, true), // keepSecond
                (';', '?') => (false, true), // keepSecond
                (';', ':') => (true, false), // keepFirst
                (';', '.') => (true, false), // keepFirst
                (';', ',') => (true, true),  // keepBoth
                ('!', '!') => (true, false), // keepFirst
                ('?', '?') => (true, false), // keepFirst
                ('.', '.') => (true, false), // keepFirst
                (':', ':') => (true, false), // keepFirst
                (';', ';') => (true, false), // keepFirst
                (',', ',') => (true, false), // keepFirst
                (' ', ' ') => (false, true), // keepSecond
                (' ', ',') => (false, true), // keepSecond
                (' ', '.') => (false, true), // keepSecond
                _ => (true, true),           // keepBoth - default
            };

            if keep_x {
                result.push(x);
            } else {
                // Trim the end character from x
                let trimmed = x
                    .chars()
                    .take(x.chars().count().saturating_sub(1))
                    .collect::<String>();
                if !trimmed.is_empty() {
                    result.push(trimmed);
                }
            }

            if !keep_y {
                // Skip y by consuming it and using the trimmed version
                let y_owned = iter.next().unwrap();
                let trimmed: String = y_owned.chars().skip(1).collect();
                if !trimmed.is_empty() {
                    // Push back via a new iteration
                    result.push(trimmed);
                }
            }
            // If keep_y is true, y will be handled in the next iteration
        } else {
            // Last element - just add it
            result.push(x);
        }
    }

    result
}

/// Join strings with delimiter, using smart punctuation rules.
///
/// This implements Pandoc's addDelimiters function:
/// - Skips delimiter before certain punctuation marks (comma, semicolon, period)
/// - Then applies fix_punct to handle remaining collisions
pub fn join_with_smart_delim(strings: Vec<String>, delimiter: &str) -> String {
    if strings.is_empty() {
        return String::new();
    }

    let non_empty: Vec<String> = strings.into_iter().filter(|s| !s.is_empty()).collect();

    if non_empty.is_empty() {
        return String::new();
    }

    if non_empty.len() == 1 {
        return non_empty.into_iter().next().unwrap();
    }

    // Add delimiters, skipping before comma/semicolon/period
    let mut with_delims = Vec::new();
    for (i, s) in non_empty.into_iter().enumerate() {
        if i > 0 && !delimiter.is_empty() {
            // Check if this string starts with punctuation that should suppress delimiter
            let first_char = s.chars().next().unwrap_or(' ');
            if first_char != ',' && first_char != ';' && first_char != '.' {
                with_delims.push(delimiter.to_string());
            }
        }
        with_delims.push(s);
    }

    // Fix punctuation collisions and join
    let fixed = fix_punct(with_delims);
    fixed.join("")
}

/// Determine how to handle a punctuation collision between two characters.
///
/// Returns (keep_first, keep_second) based on the characters at the boundary.
fn punct_collision_rule(x_end: char, y_start: char) -> (bool, bool) {
    match (x_end, y_start) {
        // Based on Pandoc citeproc Types.hs fixPunct
        ('!', '.') => (true, false), // keepFirst
        ('!', '?') => (true, true),  // keepBoth
        ('!', ':') => (true, false), // keepFirst
        ('!', ',') => (true, true),  // keepBoth
        ('!', ';') => (true, true),  // keepBoth
        ('?', '!') => (true, true),  // keepBoth
        ('?', '.') => (true, false), // keepFirst
        ('?', ':') => (true, false), // keepFirst
        ('?', ',') => (true, true),  // keepBoth
        ('?', ';') => (true, true),  // keepBoth
        ('.', '!') => (true, true),  // keepBoth
        ('.', '?') => (true, true),  // keepBoth
        ('.', ':') => (true, true),  // keepBoth
        ('.', ',') => (true, true),  // keepBoth
        ('.', ';') => (true, true),  // keepBoth
        (':', '!') => (false, true), // keepSecond
        (':', '?') => (false, true), // keepSecond
        (':', '.') => (true, false), // keepFirst
        (':', ',') => (true, true),  // keepBoth
        (':', ';') => (true, true),  // keepBoth
        (',', '!') => (true, true),  // keepBoth
        (',', '?') => (true, true),  // keepBoth
        (',', ':') => (true, true),  // keepBoth
        (',', '.') => (true, true),  // keepBoth
        (',', ';') => (true, true),  // keepBoth
        (';', '!') => (false, true), // keepSecond
        (';', '?') => (false, true), // keepSecond
        (';', ':') => (true, false), // keepFirst
        (';', '.') => (true, false), // keepFirst
        (';', ',') => (true, true),  // keepBoth
        ('!', '!') => (true, false), // keepFirst
        ('?', '?') => (true, false), // keepFirst
        ('.', '.') => (true, false), // keepFirst
        (':', ':') => (true, false), // keepFirst
        (';', ';') => (true, false), // keepFirst
        (',', ',') => (true, false), // keepFirst
        (' ', ' ') => (false, true), // keepSecond
        (' ', ',') => (false, true), // keepSecond
        (' ', '.') => (false, true), // keepSecond
        _ => (true, true),           // keepBoth - default
    }
}

/// Fix punctuation collisions between adjacent sibling outputs.
///
/// This is the correct implementation matching Pandoc citeproc's approach.
/// Unlike the global `fix_punct_inlines`, this operates on a list of sibling
/// results (each sibling is a `Vec<Inline>`) and only fixes collisions at
/// the boundaries between siblings - not between arbitrary adjacent strings.
///
/// The key insight from Pandoc citeproc is that `fixPunct` is called on the
/// list of rendered siblings, not on the final flattened output. This preserves
/// intentional punctuation sequences (like `, , ` from separate delimiters)
/// while still fixing collisions at affix boundaries.
fn fix_punct_siblings(
    siblings: Vec<Vec<quarto_pandoc_types::Inline>>,
) -> Vec<Vec<quarto_pandoc_types::Inline>> {
    use quarto_pandoc_types::Inline;

    if siblings.len() < 2 {
        return siblings;
    }

    let mut result: Vec<Vec<Inline>> = Vec::with_capacity(siblings.len());
    let mut iter = siblings.into_iter().peekable();

    while let Some(mut current) = iter.next() {
        if let Some(next) = iter.peek_mut() {
            // Get the last char of current sibling and first char of next sibling
            let current_end = get_trailing_char(&current);
            let next_start = get_leading_char(next);

            if let (Some(x_end), Some(y_start)) = (current_end, next_start) {
                let (keep_x_end, keep_y_start) = punct_collision_rule(x_end, y_start);

                // Modify current's trailing character if needed
                if !keep_x_end {
                    trim_trailing_char(&mut current);
                }

                // Modify next's leading character if needed
                if !keep_y_start {
                    trim_leading_char(next);
                }
            }
        }

        if !current.is_empty() {
            result.push(current);
        }
    }

    result
}

/// Get the trailing character of the last Str in an Inlines vector.
fn get_trailing_char(inlines: &[quarto_pandoc_types::Inline]) -> Option<char> {
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter().rev() {
        match inline {
            Inline::Str(s) if !s.text.is_empty() => {
                return s.text.chars().last();
            }
            Inline::Space(_) => return Some(' '),
            // Skip formatting wrappers and look inside
            Inline::Emph(e) => {
                if let Some(c) = get_trailing_char(&e.content) {
                    return Some(c);
                }
            }
            Inline::Strong(s) => {
                if let Some(c) = get_trailing_char(&s.content) {
                    return Some(c);
                }
            }
            Inline::SmallCaps(s) => {
                if let Some(c) = get_trailing_char(&s.content) {
                    return Some(c);
                }
            }
            Inline::Quoted(q) => {
                if let Some(c) = get_trailing_char(&q.content) {
                    return Some(c);
                }
            }
            Inline::Span(s) => {
                if let Some(c) = get_trailing_char(&s.content) {
                    return Some(c);
                }
            }
            _ => continue,
        }
    }
    None
}

/// Get the leading character of the first Str in an Inlines vector.
fn get_leading_char(inlines: &[quarto_pandoc_types::Inline]) -> Option<char> {
    use quarto_pandoc_types::Inline;

    for inline in inlines.iter() {
        match inline {
            Inline::Str(s) if !s.text.is_empty() => {
                return s.text.chars().next();
            }
            Inline::Space(_) => return Some(' '),
            // Skip formatting wrappers and look inside
            Inline::Emph(e) => {
                if let Some(c) = get_leading_char(&e.content) {
                    return Some(c);
                }
            }
            Inline::Strong(s) => {
                if let Some(c) = get_leading_char(&s.content) {
                    return Some(c);
                }
            }
            Inline::SmallCaps(s) => {
                if let Some(c) = get_leading_char(&s.content) {
                    return Some(c);
                }
            }
            Inline::Quoted(q) => {
                if let Some(c) = get_leading_char(&q.content) {
                    return Some(c);
                }
            }
            Inline::Span(s) => {
                if let Some(c) = get_leading_char(&s.content) {
                    return Some(c);
                }
            }
            _ => continue,
        }
    }
    None
}

/// Trim the trailing character from the last Str in an Inlines vector.
fn trim_trailing_char(inlines: &mut Vec<quarto_pandoc_types::Inline>) {
    use quarto_pandoc_types::Inline;

    for i in (0..inlines.len()).rev() {
        match &mut inlines[i] {
            Inline::Str(s) if !s.text.is_empty() => {
                let new_text: String = s.text.chars().take(s.text.chars().count() - 1).collect();
                if new_text.is_empty() {
                    inlines.remove(i);
                } else {
                    s.text = new_text;
                }
                return;
            }
            Inline::Space(_) => {
                inlines.remove(i);
                return;
            }
            Inline::Emph(e) => {
                trim_trailing_char(&mut e.content);
                if e.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Strong(s) => {
                trim_trailing_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::SmallCaps(s) => {
                trim_trailing_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Quoted(q) => {
                trim_trailing_char(&mut q.content);
                if q.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Span(s) => {
                trim_trailing_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            _ => continue,
        }
    }
}

/// Trim the leading character from the first Str in an Inlines vector.
fn trim_leading_char(inlines: &mut Vec<quarto_pandoc_types::Inline>) {
    use quarto_pandoc_types::Inline;

    for i in 0..inlines.len() {
        match &mut inlines[i] {
            Inline::Str(s) if !s.text.is_empty() => {
                let new_text: String = s.text.chars().skip(1).collect();
                if new_text.is_empty() {
                    inlines.remove(i);
                } else {
                    s.text = new_text;
                }
                return;
            }
            Inline::Space(_) => {
                inlines.remove(i);
                return;
            }
            Inline::Emph(e) => {
                trim_leading_char(&mut e.content);
                if e.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Strong(s) => {
                trim_leading_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::SmallCaps(s) => {
                trim_leading_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Quoted(q) => {
                trim_leading_char(&mut q.content);
                if q.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            Inline::Span(s) => {
                trim_leading_char(&mut s.content);
                if s.content.is_empty() {
                    inlines.remove(i);
                }
                return;
            }
            _ => continue,
        }
    }
}

/// Apply prefix and suffix to inlines with smart punctuation handling.
///
/// This is used in `to_inlines` to apply affixes either inside or outside
/// formatting based on the `affixes_inside` flag.
fn apply_affixes(
    inner: &mut Vec<quarto_pandoc_types::Inline>,
    prefix: &Option<String>,
    suffix: &Option<String>,
) {
    use quarto_pandoc_types::{Inline, Str};

    // Apply prefix
    if let Some(prefix) = prefix {
        if !prefix.is_empty() {
            let mut result = vec![Inline::Str(Str {
                text: prefix.clone(),
                source_info: empty_source_info(),
            })];
            result.append(inner);
            *inner = result;
        }
    }

    // Apply suffix with punctuation collision handling
    if let Some(suffix) = suffix {
        if !suffix.is_empty() {
            let content_end = get_trailing_char(inner);
            let suffix_start = suffix.chars().next();
            if let (Some(x_end), Some(y_start)) = (content_end, suffix_start) {
                let (keep_x_end, keep_y_start) = punct_collision_rule(x_end, y_start);
                if !keep_x_end {
                    trim_trailing_char(inner);
                }
                if keep_y_start {
                    inner.push(Inline::Str(Str {
                        text: suffix.clone(),
                        source_info: empty_source_info(),
                    }));
                } else {
                    // Skip first char of suffix
                    let trimmed_suffix: String = suffix.chars().skip(1).collect();
                    if !trimmed_suffix.is_empty() {
                        inner.push(Inline::Str(Str {
                            text: trimmed_suffix,
                            source_info: empty_source_info(),
                        }));
                    }
                }
            } else {
                inner.push(Inline::Str(Str {
                    text: suffix.clone(),
                    source_info: empty_source_info(),
                }));
            }
        }
    }
}

/// Move periods and commas from after closing quotes to inside them.
///
/// This implements the CSL `punctuation-in-quote` locale option, which
/// is used for American English style where periods and commas go inside
/// closing quotation marks.
///
/// This operates on the Output AST, looking for Formatted nodes with
/// `quotes: true` followed by content starting with `.` or `,`, and
/// moves that punctuation inside the quoted content.
///
/// This matches the reference citeproc implementation which operates on
/// Pandoc Inlines rather than strings.
pub fn move_punctuation_inside_quotes(output: Output) -> Output {
    move_punct_in_children(vec![output])
        .into_iter()
        .next()
        .unwrap_or(Output::Null)
}

/// Process a list of Output nodes, moving punctuation inside quotes.
fn move_punct_in_children(children: Vec<Output>) -> Vec<Output> {
    if children.is_empty() {
        return children;
    }

    let mut result = Vec::with_capacity(children.len());
    let mut iter = children.into_iter().peekable();

    while let Some(current) = iter.next() {
        // Check if current ends with a quoted node (direct or nested)
        if let Some(quoted_path) = find_trailing_quoted(&current) {
            // Look ahead to see if next sibling starts with punctuation
            if let Some(next) = iter.peek() {
                if let Some((punct, rest)) = extract_leading_punct(next) {
                    // Check that quoted content doesn't already end with this punctuation
                    if !ends_with_punct_in_path(&current, &quoted_path, punct) {
                        // Move punctuation inside the quoted node
                        let modified = insert_punct_in_path(current, &quoted_path, punct);

                        result.push(recurse_punct_in_quotes(modified));

                        // Consume the next element and push the remainder (if any)
                        iter.next(); // consume the peeked element
                        if let Some(rest_output) = rest {
                            result.push(move_punctuation_inside_quotes(rest_output));
                        }
                        continue;
                    }
                }
            }
        }

        // Recursively process the current node
        result.push(recurse_punct_in_quotes(current));
    }

    result
}

/// Path to a quoted node within a tree (indices of children to follow).
type QuotedPath = Vec<usize>;

/// Find the path to a trailing quoted element in an Output tree.
/// Returns the path of child indices to reach the quoted Formatted node.
fn find_trailing_quoted(output: &Output) -> Option<QuotedPath> {
    match output {
        Output::Formatted {
            formatting,
            children,
        } => {
            if formatting.quotes {
                // This node itself is quoted
                return Some(vec![]);
            }
            // Check the last non-null child
            for (i, child) in children.iter().enumerate().rev() {
                if !child.is_null() {
                    if let Some(mut path) = find_trailing_quoted(child) {
                        path.insert(0, i);
                        return Some(path);
                    }
                    break;
                }
            }
            None
        }
        Output::Tagged { child, .. } => {
            // Look through tagged nodes
            if let Some(mut path) = find_trailing_quoted(child) {
                // Use usize::MAX as a sentinel for "go into Tagged child"
                path.insert(0, usize::MAX);
                return Some(path);
            }
            None
        }
        _ => None,
    }
}

/// Check if the node at the given path ends with the specified punctuation.
fn ends_with_punct_in_path(output: &Output, path: &[usize], punct: char) -> bool {
    if path.is_empty() {
        // We're at the quoted node
        if let Output::Formatted { children, .. } = output {
            return ends_with_punct(children, punct);
        }
        return false;
    }

    match output {
        Output::Formatted { children, .. } => {
            if let Some(&idx) = path.first() {
                if let Some(child) = children.get(idx) {
                    return ends_with_punct_in_path(child, &path[1..], punct);
                }
            }
            false
        }
        Output::Tagged { child, .. } => {
            if path.first() == Some(&usize::MAX) {
                return ends_with_punct_in_path(child, &path[1..], punct);
            }
            false
        }
        _ => false,
    }
}

/// Insert punctuation into the quoted node at the given path.
fn insert_punct_in_path(output: Output, path: &[usize], punct: char) -> Output {
    if path.is_empty() {
        // We're at the quoted node - insert punctuation at the end
        if let Output::Formatted {
            formatting,
            mut children,
        } = output
        {
            children.push(Output::Literal(punct.to_string()));
            return Output::Formatted {
                formatting,
                children,
            };
        }
        return output;
    }

    match output {
        Output::Formatted {
            formatting,
            children,
        } => {
            if let Some(&idx) = path.first() {
                let new_children: Vec<Output> = children
                    .into_iter()
                    .enumerate()
                    .map(|(i, child)| {
                        if i == idx {
                            insert_punct_in_path(child, &path[1..], punct)
                        } else {
                            child
                        }
                    })
                    .collect();
                return Output::Formatted {
                    formatting,
                    children: new_children,
                };
            }
            Output::Formatted {
                formatting,
                children,
            }
        }
        Output::Tagged { tag, child } => {
            if path.first() == Some(&usize::MAX) {
                return Output::Tagged {
                    tag,
                    child: Box::new(insert_punct_in_path(*child, &path[1..], punct)),
                };
            }
            Output::Tagged { tag, child }
        }
        other => other,
    }
}

/// Recursively apply punctuation-in-quote transformation to a single node.
fn recurse_punct_in_quotes(output: Output) -> Output {
    match output {
        Output::Formatted {
            formatting,
            children,
        } => {
            // First, process children
            let processed_children = move_punct_in_children(children);

            // Then, check if delimiter starts with punctuation that should move into quotes.
            // For each child that ends with a quoted element (except the last),
            // we may need to move the delimiter's leading punctuation into the quote.
            // We use a custom delimiter per-child if needed.
            let (final_children, mut final_formatting) =
                move_delimiter_punct_into_quotes(processed_children, formatting);

            // Also check if this node has quotes=true and a suffix starting with punctuation.
            // If so, move the punctuation into the content and strip it from the suffix.
            let final_children = if final_formatting.quotes {
                move_suffix_punct_into_quoted_content(final_children, &mut final_formatting)
            } else {
                final_children
            };

            Output::Formatted {
                formatting: final_formatting,
                children: final_children,
            }
        }
        Output::Linked { url, children } => Output::Linked {
            url,
            children: move_punct_in_children(children),
        },
        Output::InNote(child) => Output::InNote(Box::new(move_punctuation_inside_quotes(*child))),
        Output::Tagged { tag, child } => {
            // Don't apply punctuation-in-quote inside Prefix or Suffix tags.
            // These contain user-specified content where punctuation should stay as typed.
            let should_recurse = !matches!(tag, Tag::Prefix | Tag::Suffix);
            Output::Tagged {
                tag,
                child: Box::new(if should_recurse {
                    move_punctuation_inside_quotes(*child)
                } else {
                    *child
                }),
            }
        }
        // Literal and Null don't have children
        other => other,
    }
}

/// Move delimiter punctuation into trailing quoted elements.
///
/// When a delimiter starts with `.` or `,` and a child ends with a quoted element,
/// we move that punctuation into the quote. The delimiter is kept intact;
/// the smart punctuation handling at render time will skip redundant punctuation
/// when the content already ends with that punctuation inside a quote.
fn move_delimiter_punct_into_quotes(
    mut children: Vec<Output>,
    formatting: Formatting,
) -> (Vec<Output>, Formatting) {
    let Some(ref delim) = formatting.delimiter else {
        return (children, formatting);
    };

    let first_char = delim.chars().next();
    if !matches!(first_char, Some('.') | Some(',')) {
        return (children, formatting);
    }

    let punct = first_char.unwrap();

    // Check each child except the last - if it ends with a quoted element,
    // move the delimiter's leading punctuation into the quote
    for i in 0..children.len().saturating_sub(1) {
        if let Some(quoted_path) = find_trailing_quoted(&children[i]) {
            // Check if the quoted content doesn't already end with this punctuation
            if !ends_with_punct_in_path(&children[i], &quoted_path, punct) {
                // Move punctuation into the quote
                children[i] = insert_punct_in_path(
                    std::mem::replace(&mut children[i], Output::Null),
                    &quoted_path,
                    punct,
                );
            }
        }
    }

    // Keep the delimiter intact - the smart punctuation handling at render time
    // will handle collisions between the quoted content's trailing punctuation
    // and the delimiter's leading punctuation.

    (children, formatting)
}

/// Move suffix punctuation into quoted content.
///
/// When a Formatted node has quotes=true and a suffix starting with `.` or `,`,
/// this moves the punctuation to the end of the content (inside the quotes)
/// and strips it from the suffix. This implements the punctuation-in-quote
/// rule for suffixes when affixes are applied outside quotes.
fn move_suffix_punct_into_quoted_content(
    mut children: Vec<Output>,
    formatting: &mut Formatting,
) -> Vec<Output> {
    // Check if suffix starts with punctuation
    let Some(ref suffix) = formatting.suffix else {
        return children;
    };

    let first_char = suffix.chars().next();
    if !matches!(first_char, Some('.') | Some(',')) {
        return children;
    }

    let punct = first_char.unwrap();

    // Check if content already ends with this punctuation
    if ends_with_punct(&children, punct) {
        // Just strip the punctuation from the suffix (it's already there)
        let new_suffix: String = suffix.chars().skip(1).collect();
        formatting.suffix = if new_suffix.is_empty() {
            None
        } else {
            Some(new_suffix)
        };
        return children;
    }

    // Add the punctuation to the end of the content
    children.push(Output::Literal(punct.to_string()));

    // Strip the punctuation from the suffix
    let new_suffix: String = suffix.chars().skip(1).collect();
    formatting.suffix = if new_suffix.is_empty() {
        None
    } else {
        Some(new_suffix)
    };

    children
}

/// Extract leading punctuation (. or ,) from an Output node.
/// Returns the punctuation character and the remaining Output (if any).
/// When extracting from wrapped nodes (Tagged, Formatted), reconstructs
/// the wrapper around the remaining content.
fn extract_leading_punct(output: &Output) -> Option<(char, Option<Output>)> {
    match output {
        Output::Literal(s) => {
            let first = s.chars().next()?;
            if first == '.' || first == ',' {
                let rest = &s[first.len_utf8()..];
                let rest_output = if rest.is_empty() {
                    None
                } else {
                    Some(Output::Literal(rest.to_string()))
                };
                Some((first, rest_output))
            } else {
                None
            }
        }
        Output::Formatted {
            formatting,
            children,
        } => {
            // Check first non-null child
            for (i, child) in children.iter().enumerate() {
                if !child.is_null() {
                    if let Some((punct, rest_child)) = extract_leading_punct(child) {
                        // Reconstruct the Formatted node with the rest
                        let mut new_children: Vec<Output> = Vec::new();
                        if let Some(rest) = rest_child {
                            new_children.push(rest);
                        }
                        new_children.extend(children[i + 1..].iter().cloned());

                        let rest_output = if new_children.is_empty()
                            || new_children.iter().all(|c| c.is_null())
                        {
                            None
                        } else {
                            Some(Output::Formatted {
                                formatting: formatting.clone(),
                                children: new_children,
                            })
                        };
                        return Some((punct, rest_output));
                    }
                    return None;
                }
            }
            None
        }
        Output::Tagged { tag, child } => {
            // Recurse into the child but preserve the tag wrapper
            if let Some((punct, rest_child)) = extract_leading_punct(child) {
                let rest_output = rest_child.map(|rest| Output::Tagged {
                    tag: tag.clone(),
                    child: Box::new(rest),
                });
                Some((punct, rest_output))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if output ends with the given punctuation character.
fn ends_with_punct(children: &[Output], punct: char) -> bool {
    for child in children.iter().rev() {
        if child.is_null() {
            continue;
        }
        return output_ends_with_char(child, punct);
    }
    false
}

/// Check if a single Output node ends with the given character.
fn output_ends_with_char(output: &Output, c: char) -> bool {
    match output {
        Output::Literal(s) => s.ends_with(c),
        Output::Formatted {
            formatting,
            children,
        } => {
            // If there's a suffix, check the suffix for the ending character
            if let Some(ref suffix) = formatting.suffix {
                if !suffix.is_empty() {
                    return suffix.ends_with(c);
                }
            }
            // Otherwise check children
            ends_with_punct(children, c)
        }
        Output::Tagged { child, .. } => output_ends_with_char(child, c),
        Output::Linked { children, .. } => ends_with_punct(children, c),
        Output::InNote(child) => output_ends_with_char(child, c),
        Output::Null => false,
    }
}

/// Check if an Output ends with a character that suppresses the following space.
///
/// Per CSL spec (and Pandoc citeproc behavior), when joining name parts with spaces,
/// no space should be added after these characters:
/// - ' (apostrophe U+0027)
/// - ' (right single quotation mark U+2019)
/// - - (hyphen-minus U+002D)
/// - – (en dash U+2013)
/// - (non-breaking space U+00A0)
///
/// This is important for name particles like "d'" (as in "d'Artagnan") or
/// hyphenated names.
pub fn ends_with_no_space_char(output: &Output) -> bool {
    const NO_SPACE_CHARS: &[char] = &[
        '\'',       // apostrophe U+0027
        '\u{2019}', // right single quotation mark
        '-',        // hyphen-minus
        '\u{2013}', // en dash
        '\u{00A0}', // non-breaking space
    ];

    NO_SPACE_CHARS
        .iter()
        .any(|&c| output_ends_with_char(output, c))
}

// ============================================================================
// CSL Rich Text Parser
// ============================================================================

/// Parse CSL-JSON rich text (HTML-like markup) into Output AST.
///
/// CSL-JSON text fields can contain inline HTML markup:
/// - `<i>...</i>` - italics
/// - `<b>...</b>` - bold
/// - `<sup>...</sup>` - superscript
/// - `<sub>...</sub>` - subscript
/// - `<span style="font-variant:small-caps;">...</span>` - small caps
/// - `<span class="nocase">...</span>` - no case transformation
///
/// This parser converts that markup into Output AST nodes.
/// Malformed HTML (e.g., `</i>` without opening) is escaped.
pub fn parse_csl_rich_text(text: &str) -> Output {
    // Apply French typography rules before parsing
    let transformed = apply_french_typography(text);
    let mut parser = RichTextParser::new(&transformed);
    parser.parse()
}

/// Apply French typography rules: convert regular spaces to narrow no-break spaces (U+202F)
/// in contexts where French orthography requires them.
///
/// This follows the citeproc reference implementation which applies these transformations:
/// - `" ;"` → `"\u{202F};"` (narrow space before semicolon)
/// - `" ?"` → `"\u{202F}?"` (narrow space before question mark)
/// - `" !"` → `"\u{202F}!"` (narrow space before exclamation mark)
/// - `" »"` → `"\u{202F}»"` (narrow space before closing guillemet)
/// - `"« "` → `"«\u{202F}"` (narrow space after opening guillemet)
fn apply_french_typography(text: &str) -> String {
    text.replace(" ;", "\u{202F};")
        .replace(" ?", "\u{202F}?")
        .replace(" !", "\u{202F}!")
        .replace(" »", "\u{202F}»")
        .replace("« ", "«\u{202F}")
}

struct RichTextParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> RichTextParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn parse(&mut self) -> Output {
        let children = self.parse_children(None);
        if children.is_empty() {
            Output::Null
        } else if children.len() == 1 {
            children.into_iter().next().unwrap()
        } else {
            Output::sequence(children)
        }
    }

    fn parse_children(&mut self, end_tag: Option<&str>) -> Vec<Output> {
        let mut children = Vec::new();
        let mut text_start = self.pos;

        while self.pos < self.input.len() {
            // Check for end tag
            if let Some(tag) = end_tag {
                if self.remaining().starts_with(tag) {
                    // Flush accumulated text
                    if self.pos > text_start {
                        children.push(Output::Literal(
                            self.input[text_start..self.pos].to_string(),
                        ));
                    }
                    self.pos += tag.len();
                    return children;
                }
            }

            // Check for start of a tag
            if self.remaining().starts_with('<') {
                // Flush accumulated text
                if self.pos > text_start {
                    children.push(Output::Literal(
                        self.input[text_start..self.pos].to_string(),
                    ));
                }

                // Try to parse a tag
                if let Some(output) = self.try_parse_tag() {
                    children.push(output);
                    text_start = self.pos;
                } else {
                    // Not a valid tag, treat '<' as literal but escape it
                    children.push(Output::Literal("<".to_string()));
                    self.pos += 1;
                    text_start = self.pos;
                }
            } else if self.remaining().starts_with('\'') || self.remaining().starts_with('"') {
                // CSL uses straight quotes as generic quote markers that get localized
                // See: https://github.com/jgm/citeproc (parseCslJson, pCslQuoted)
                let quote_char = self.remaining().chars().next().unwrap();

                // Check if this could be an opening quote vs an apostrophe.
                // An opening quote must:
                // 1. Be followed by a non-space character
                // 2. NOT be preceded by an alphanumeric character (that would be an apostrophe
                //    like in "l'Égypte" or "don't")
                //
                // This matches Pandoc citeproc's behavior where apostrophes are handled as
                // part of text parsing (pCslText) and only standalone quotes trigger pCslQuoted.
                let next_char = self.remaining().chars().nth(1);
                let prev_char = if self.pos > 0 {
                    self.input[..self.pos].chars().last()
                } else {
                    None
                };
                let is_followed_by_non_space =
                    next_char.map(|c| !c.is_whitespace()).unwrap_or(false);
                let is_preceded_by_alphanumeric =
                    prev_char.map(|c| c.is_alphanumeric()).unwrap_or(false);

                if is_followed_by_non_space && !is_preceded_by_alphanumeric {
                    // Flush accumulated text
                    if self.pos > text_start {
                        children.push(Output::Literal(
                            self.input[text_start..self.pos].to_string(),
                        ));
                    }

                    // Try to parse as quoted content
                    if let Some(output) = self.try_parse_quoted(quote_char) {
                        children.push(output);
                        text_start = self.pos;
                    } else {
                        // Not a valid quote, treat as literal
                        self.pos += 1;
                        text_start = self.pos - 1; // Include the quote char
                    }
                } else {
                    // Apostrophe or quote followed by space - treat as literal
                    self.pos += 1;
                }
            } else {
                self.pos += self
                    .remaining()
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            }
        }

        // Flush remaining text
        if self.pos > text_start {
            children.push(Output::Literal(
                self.input[text_start..self.pos].to_string(),
            ));
        }

        children
    }

    fn try_parse_tag(&mut self) -> Option<Output> {
        let remaining = self.remaining();

        // Try each known tag type
        if remaining.starts_with("<i>") {
            self.pos += 3;
            let inner = self.parse_children(Some("</i>"));
            let mut fmt = Formatting::default();
            fmt.font_style = Some(quarto_csl::FontStyle::Italic);
            return Some(Output::formatted(fmt, inner));
        }

        if remaining.starts_with("<b>") {
            self.pos += 3;
            let inner = self.parse_children(Some("</b>"));
            let mut fmt = Formatting::default();
            fmt.font_weight = Some(quarto_csl::FontWeight::Bold);
            return Some(Output::formatted(fmt, inner));
        }

        if remaining.starts_with("<sup>") {
            self.pos += 5;
            let inner = self.parse_children(Some("</sup>"));
            let mut fmt = Formatting::default();
            fmt.vertical_align = Some(quarto_csl::VerticalAlign::Sup);
            return Some(Output::formatted(fmt, inner));
        }

        if remaining.starts_with("<sub>") {
            self.pos += 5;
            let inner = self.parse_children(Some("</sub>"));
            let mut fmt = Formatting::default();
            fmt.vertical_align = Some(quarto_csl::VerticalAlign::Sub);
            return Some(Output::formatted(fmt, inner));
        }

        // Small caps shorthand: <sc>...</sc> (Pandoc extension)
        if remaining.starts_with("<sc>") {
            self.pos += 4;
            let inner = self.parse_children(Some("</sc>"));
            let mut fmt = Formatting::default();
            fmt.font_variant = Some(quarto_csl::FontVariant::SmallCaps);
            return Some(Output::formatted(fmt, inner));
        }

        // Small caps: <span style="font-variant:small-caps;">
        if remaining.starts_with("<span style=\"font-variant:small-caps;\">")
            || remaining.starts_with("<span style=\"font-variant: small-caps;\">")
        {
            let tag_end = remaining.find('>').unwrap() + 1;
            self.pos += tag_end;
            let inner = self.parse_children(Some("</span>"));
            let mut fmt = Formatting::default();
            fmt.font_variant = Some(quarto_csl::FontVariant::SmallCaps);
            return Some(Output::formatted(fmt, inner));
        }

        // No case: <span class="nocase">
        if remaining.starts_with("<span class=\"nocase\">") {
            self.pos += 21;
            let inner = self.parse_children(Some("</span>"));
            // Tag this content as NoCase to prevent case transformations
            let inner_output = if inner.len() == 1 {
                inner.into_iter().next().unwrap()
            } else {
                Output::sequence(inner)
            };
            return Some(Output::tagged(Tag::NoCase, inner_output));
        }

        // No decoration: <span class="nodecor"> - resets all formatting to normal
        if remaining.starts_with("<span class=\"nodecor\">") {
            self.pos += 22;
            let inner = self.parse_children(Some("</span>"));
            // Tag this content as NoDecoration to reset formatting
            let inner_output = if inner.len() == 1 {
                inner.into_iter().next().unwrap()
            } else {
                Output::sequence(inner)
            };
            return Some(Output::tagged(Tag::NoDecoration, inner_output));
        }

        // Stray closing tag - escape it
        if remaining.starts_with("</i>")
            || remaining.starts_with("</b>")
            || remaining.starts_with("</sup>")
            || remaining.starts_with("</sub>")
            || remaining.starts_with("</sc>")
            || remaining.starts_with("</span>")
        {
            // Find the end of the tag
            if let Some(end) = remaining.find('>') {
                let tag = &remaining[..=end];
                self.pos += tag.len();
                // Return escaped version
                return Some(Output::Literal(escape_html_tag(tag)));
            }
        }

        None
    }

    /// Try to parse quoted content starting at the current position.
    /// The quote_char is the opening quote character (' or ").
    /// Returns formatted output with quotes=true if successful.
    ///
    /// This follows Pandoc's citeproc behavior:
    /// - Reject empty quoted strings ('' or "")
    /// - A closing quote must NOT be followed by alphanumeric (to avoid matching
    ///   mid-word apostrophes like "don't" or "l'ami")
    fn try_parse_quoted(&mut self, quote_char: char) -> Option<Output> {
        // Skip the opening quote
        self.pos += 1;

        // Reject empty quoted strings - if immediately followed by closing quote,
        // this is not a valid quote (e.g., '' in l''' should not be parsed as empty quoted)
        // See Pandoc citeproc pCslQuoted: "fail unexpected close quote"
        if self.remaining().starts_with(quote_char) {
            self.pos -= 1; // Reset to before opening quote
            return None;
        }

        let content_start = self.pos;

        // Find the closing quote
        while self.pos < self.input.len() {
            let c = self.remaining().chars().next()?;
            if c == quote_char {
                // Check if this could be a closing quote.
                // A closing quote must NOT be followed by alphanumeric.
                // This avoids matching mid-word apostrophes like "don't" or "l'ami".
                // See Pandoc citeproc pCslText: apostrophe handling
                let next_after_quote = self.remaining().chars().nth(1);
                let is_valid_closing_quote = next_after_quote
                    .map(|c| !c.is_alphanumeric())
                    .unwrap_or(true);

                if is_valid_closing_quote {
                    // Found closing quote
                    let content = &self.input[content_start..self.pos];
                    self.pos += 1; // Skip closing quote

                    // Parse the content recursively (may contain tags)
                    let inner = if content.is_empty() {
                        vec![]
                    } else {
                        let mut inner_parser = RichTextParser::new(content);
                        let children = inner_parser.parse_children(None);
                        children
                    };

                    // Wrap with quotes formatting
                    let mut fmt = Formatting::default();
                    fmt.quotes = true;
                    return Some(Output::formatted(fmt, inner));
                }
            }
            self.pos += c.len_utf8();
        }

        // No closing quote found - reset position and return None
        self.pos = content_start - 1;
        None
    }
}

/// Escape an HTML tag for display (convert < > to entity references).
fn escape_html_tag(tag: &str) -> String {
    tag.replace('<', "&#60;").replace('>', "&#62;")
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
            format!("\u{201C}{}\u{201D}", inner) // "..."
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
/// Preserves leading and trailing whitespace.
fn capitalize_all(s: &str) -> String {
    // Preserve leading whitespace
    let leading: String = s.chars().take_while(|c| c.is_whitespace()).collect();
    // Preserve trailing whitespace
    let trailing: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    // Process the middle part
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return s.to_string(); // Just whitespace, return as-is
    }

    let capitalized = trimmed
        .split_whitespace()
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ");

    format!("{}{}{}", leading, capitalized, trailing)
}

/// English stop words that should remain lowercase in title case (unless first/last word).
/// Based on CSL/Chicago Manual of Style conventions, matching Pandoc's citeproc.
const TITLE_CASE_STOP_WORDS: &[&str] = &[
    // Standard English stop words
    "a", "an", "and", "as", "at", "but", "by", "down", "for", "from", "in", "into", "nor", "of",
    "on", "onto", "or", "over", "so", "the", "till", "to", "up", "via", "with", "yet",
    // Name particles (should stay lowercase in middle position)
    "von", "van", "de", "d", "l", // Additional stop words from Pandoc's citeproc
    "about",
];

/// Check if a word is all uppercase (an acronym).
fn is_all_uppercase(s: &str) -> bool {
    let letters: String = s.chars().filter(|c| c.is_alphabetic()).collect();
    !letters.is_empty() && letters.chars().all(|c| c.is_uppercase())
}

/// Check if a word has internal uppercase letters (like "iPhone", "macOS").
fn has_internal_uppercase(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 && chars[i - 1].is_alphabetic() {
            return true;
        }
    }
    false
}

/// Check if a string starts with an alphabetic character.
fn starts_with_letter(s: &str) -> bool {
    s.chars().next().map_or(false, |c| c.is_alphabetic())
}

/// Title case a single word (may contain hyphens or slashes).
fn title_case_word(word: &str, force_capitalize: bool) -> String {
    // If the word doesn't start with a letter (like "07-x"), preserve it
    if !starts_with_letter(word) {
        return word.to_string();
    }

    // If the word is all uppercase (like "UK", "IS"), preserve it
    if is_all_uppercase(word) {
        return word.to_string();
    }

    // If the word has internal uppercase (like "iPhone"), preserve it
    if has_internal_uppercase(word) {
        return word.to_string();
    }

    // Handle hyphenated words: "out-of-fashion" → "Out-of-Fashion"
    // First and last parts are always capitalized, middle parts follow stop word rules
    if word.contains('-') {
        let parts: Vec<&str> = word.split('-').collect();
        let len = parts.len();
        return parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                // First part: capitalize if force_capitalize OR if it's the first part
                // Last part: always capitalize
                // Middle parts: follow stop word rules (not forced)
                let is_first = i == 0;
                let is_last = i == len - 1;
                let force = (is_first && force_capitalize) || is_last;
                title_case_word(part, force)
            })
            .collect::<Vec<_>>()
            .join("-");
    }

    // Handle slashed words: "cat/mouse" → "Cat/Mouse"
    if word.contains('/') {
        return word
            .split('/')
            .map(|part| title_case_word(part, true))
            .collect::<Vec<_>>()
            .join("/");
    }

    // Check if it's a stop word (should remain lowercase unless forced)
    let lower = word.to_lowercase();
    if !force_capitalize && TITLE_CASE_STOP_WORDS.contains(&lower.as_str()) {
        return lower;
    }

    // Regular word: capitalize first letter
    capitalize_first(word)
}

/// Apply title case (capitalize major words, keep stop words lowercase).
fn title_case(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }

    // Preserve leading whitespace
    let leading: String = s.chars().take_while(|c| c.is_whitespace()).collect();
    // Preserve trailing whitespace
    let trailing: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let trimmed = s.trim();
    if trimmed.is_empty() {
        return s.to_string();
    }

    let mut result = String::new();
    let mut force_next = true; // First word is always capitalized
    let mut skip_next_transform = false; // After apostrophe, leave next word as-is
    let mut chars = trimmed.chars().peekable();
    let mut current_word = String::new();

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            // End of word - process it
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                force_next = false;
            }
            result.push(c);
        } else if c == ':' || c == '.' || c == '?' || c == '!' {
            // Sentence-ending punctuation forces next word capitalization
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
            }
            result.push(c);
            // After these punctuation marks, force capitalize next word
            force_next = true;
        } else if c == '—' || c == '–' {
            // Em-dash and en-dash are word breaks but do NOT force capitalization
            // (unlike sentence-ending punctuation). Regular hyphen is handled as
            // part of the word by title_case_word.
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
            }
            result.push(c);
            force_next = false;
        } else if c == '\u{201C}' || c == '\u{2018}' {
            // Opening curly quotes (left double " and left single ') force capitalization
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
            }
            result.push(c);
            // After opening quotes, force capitalize next word
            force_next = true;
        } else if c == '\u{201D}' {
            // Closing curly double quote - just output, don't force capitalize
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                force_next = false;
            }
            result.push(c);
        } else if c == '\u{2019}' || c == '\'' || c == '`' {
            // Apostrophes (curly and straight) and backticks act as word separators
            // for title case purposes, matching Pandoc citeproc's behavior.
            // This ensures proper handling of:
            // - French articles: "l'Égypte" - "l" is a stop word (stays lowercase),
            //   "Égypte" keeps its original case
            // - Names with apostrophes: "Shafi'i" - both parts keep their case
            //
            // Process current word first
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
            }
            // Output the apostrophe
            result.push(c);
            // After an apostrophe, the next word is NOT transformed at all
            // (it's not a "word end" in Pandoc's state machine, so the next
            // chunk keeps its original case)
            skip_next_transform = true;
            force_next = false;
        } else if c == '"' {
            // Straight double quotes - determine if opening or closing based on context
            // If followed by alphanumeric (peeking), it's opening
            // Otherwise it's closing
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
            }
            result.push(c);
            // Check next character to determine if this is opening or closing
            if let Some(&next_c) = chars.peek() {
                if next_c.is_alphanumeric() {
                    // Opening quote - force capitalize next word
                    force_next = true;
                } else {
                    // Closing quote - don't force
                    force_next = false;
                }
            } else {
                force_next = false;
            }
        } else {
            current_word.push(c);
        }
    }

    // Handle the last word - always capitalize (CSL spec: stop words capitalized at start/end)
    // unless we're in skip mode (after apostrophe)
    if !current_word.is_empty() {
        if skip_next_transform {
            result.push_str(&current_word);
        } else {
            result.push_str(&title_case_word(&current_word, true));
        }
    }

    format!("{}{}{}", leading, result, trailing)
}

/// Apply title case with state tracking and last-segment awareness.
/// When `is_last_segment` is true, the final word in this segment will be
/// force-capitalized (CSL spec: stop words capitalized at start/end).
fn title_case_with_state_and_last(
    s: &str,
    seen_first_word: &mut bool,
    is_last_segment: bool,
) -> String {
    if s.is_empty() {
        return String::new();
    }

    // Preserve leading whitespace
    let leading: String = s.chars().take_while(|c| c.is_whitespace()).collect();
    // Preserve trailing whitespace
    let trailing: String = s
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let trimmed = s.trim();
    if trimmed.is_empty() {
        return s.to_string();
    }

    let mut result = String::new();
    // Use external state: force_next is true only if we haven't seen the first word yet
    let mut force_next = !*seen_first_word;
    let mut skip_next_transform = false; // After apostrophe, leave next word as-is
    let mut chars = trimmed.chars().peekable();
    let mut current_word = String::new();

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            // End of word - process it
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                // After processing a word, we've seen the first word
                *seen_first_word = true;
                force_next = false;
            }
            result.push(c);
        } else if c == ':' || c == '.' || c == '?' || c == '!' {
            // Sentence-ending punctuation forces next word capitalization
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
            }
            result.push(c);
            // After these punctuation marks, force capitalize next word
            force_next = true;
        } else if c == '—' || c == '–' {
            // Em-dash and en-dash are word breaks but do NOT force capitalization
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
            }
            result.push(c);
            force_next = false;
        } else if c == '\u{201C}' || c == '\u{2018}' {
            // Opening curly quotes force capitalization
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
            }
            result.push(c);
            force_next = true;
        } else if c == '\u{201D}' {
            // Closing curly double quote - just output, don't force capitalize
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
                force_next = false;
            }
            result.push(c);
        } else if c == '\u{2019}' || c == '\'' || c == '`' {
            // Apostrophes (curly and straight) and backticks act as word separators
            // for title case purposes, matching Pandoc citeproc's behavior.
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
            }
            result.push(c);
            // After an apostrophe, the next word is NOT transformed at all
            skip_next_transform = true;
            force_next = false;
        } else if c == '"' {
            // Straight double quotes - determine if opening or closing based on context
            if !current_word.is_empty() {
                if skip_next_transform {
                    result.push_str(&current_word);
                    skip_next_transform = false;
                } else {
                    result.push_str(&title_case_word(&current_word, force_next));
                }
                current_word.clear();
                *seen_first_word = true;
            }
            result.push(c);
            if let Some(&next_c) = chars.peek() {
                if next_c.is_alphanumeric() {
                    force_next = true;
                } else {
                    force_next = false;
                }
            } else {
                force_next = false;
            }
        } else {
            current_word.push(c);
        }
    }

    // Handle the last word - if this is the last segment, force capitalize
    // unless we're in skip mode (after apostrophe)
    if !current_word.is_empty() {
        if skip_next_transform {
            result.push_str(&current_word);
        } else {
            let force = force_next || is_last_segment;
            result.push_str(&title_case_word(&current_word, force));
        }
        *seen_first_word = true;
    }

    format!("{}{}{}", leading, result, trailing)
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

/// Configuration for locale-specific quote characters.
///
/// Different locales use different quotation marks:
/// - English: "..." '...'
/// - French: « ... » "..."
/// - German: „..." ‚...'
///
/// The quote terms are looked up from the locale:
/// - `open-quote` / `close-quote` for outer (primary) quotes
/// - `open-inner-quote` / `close-inner-quote` for inner (secondary) quotes
#[derive(Debug, Clone)]
pub struct QuoteConfig {
    /// Opening character(s) for outer (primary) quotes
    pub outer_open: String,
    /// Closing character(s) for outer (primary) quotes
    pub outer_close: String,
    /// Opening character(s) for inner (secondary/nested) quotes
    pub inner_open: String,
    /// Closing character(s) for inner (secondary/nested) quotes
    pub inner_close: String,
}

impl Default for QuoteConfig {
    /// Default to English curly quotes.
    fn default() -> Self {
        Self {
            outer_open: "\u{201C}".to_string(), // " left double quotation mark
            outer_close: "\u{201D}".to_string(), // " right double quotation mark
            inner_open: "\u{2018}".to_string(), // ' left single quotation mark
            inner_close: "\u{2019}".to_string(), // ' right single quotation mark
        }
    }
}

/// Convert Unicode superscript characters to their base form.
///
/// Returns Some(base_string) if the character is a known superscript character,
/// None otherwise. This matches Pandoc citeproc's `superscriptChars` map.
fn superscript_char_to_base(c: char) -> Option<&'static str> {
    match c {
        '\u{00AA}' => Some("a"),        // ª
        '\u{00B2}' => Some("2"),        // ²
        '\u{00B3}' => Some("3"),        // ³
        '\u{00B9}' => Some("1"),        // ¹
        '\u{00BA}' => Some("o"),        // º
        '\u{02B0}' => Some("h"),        // ʰ
        '\u{02B1}' => Some("\u{0266}"), // ʱ -> ɦ
        '\u{02B2}' => Some("j"),        // ʲ
        '\u{02B3}' => Some("r"),        // ʳ
        '\u{02B4}' => Some("\u{0279}"), // ʴ -> ɹ
        '\u{02B5}' => Some("\u{027B}"), // ʵ -> ɻ
        '\u{02B6}' => Some("\u{0281}"), // ʶ -> ʁ
        '\u{02B7}' => Some("w"),        // ʷ
        '\u{02B8}' => Some("y"),        // ʸ
        '\u{02E0}' => Some("\u{0263}"), // ˠ -> ɣ
        '\u{02E1}' => Some("l"),        // ˡ
        '\u{02E2}' => Some("s"),        // ˢ
        '\u{02E3}' => Some("x"),        // ˣ
        '\u{02E4}' => Some("\u{0295}"), // ˤ -> ʕ
        '\u{1D2C}' => Some("A"),        // ᴬ
        '\u{1D2D}' => Some("\u{00C6}"), // ᴭ -> Æ
        '\u{1D2E}' => Some("B"),        // ᴮ
        '\u{1D30}' => Some("D"),        // ᴰ
        '\u{1D31}' => Some("E"),        // ᴱ
        '\u{1D32}' => Some("\u{018E}"), // ᴲ -> Ǝ
        '\u{1D33}' => Some("G"),        // ᴳ
        '\u{1D34}' => Some("H"),        // ᴴ
        '\u{1D35}' => Some("I"),        // ᴵ
        '\u{1D36}' => Some("J"),        // ᴶ
        '\u{1D37}' => Some("K"),        // ᴷ
        '\u{1D38}' => Some("L"),        // ᴸ
        '\u{1D39}' => Some("M"),        // ᴹ
        '\u{1D3A}' => Some("N"),        // ᴺ
        '\u{1D3C}' => Some("O"),        // ᴼ
        '\u{1D3D}' => Some("\u{0222}"), // ᴽ -> Ȣ
        '\u{1D3E}' => Some("P"),        // ᴾ
        '\u{1D3F}' => Some("R"),        // ᴿ
        '\u{1D40}' => Some("T"),        // ᵀ
        '\u{1D41}' => Some("U"),        // ᵁ
        '\u{1D42}' => Some("W"),        // ᵂ
        '\u{1D43}' => Some("a"),        // ᵃ
        '\u{1D44}' => Some("\u{0250}"), // ᵄ -> ɐ
        '\u{1D45}' => Some("\u{0251}"), // ᵅ -> ɑ
        '\u{1D46}' => Some("\u{1D02}"), // ᵆ -> ᴂ
        '\u{1D47}' => Some("b"),        // ᵇ
        '\u{1D48}' => Some("d"),        // ᵈ
        '\u{1D49}' => Some("e"),        // ᵉ
        '\u{1D4A}' => Some("\u{0259}"), // ᵊ -> ə
        '\u{1D4B}' => Some("\u{025B}"), // ᵋ -> ɛ
        '\u{1D4C}' => Some("\u{025C}"), // ᵌ -> ɜ
        '\u{1D4D}' => Some("g"),        // ᵍ
        '\u{1D4F}' => Some("k"),        // ᵏ
        '\u{1D50}' => Some("m"),        // ᵐ
        '\u{1D51}' => Some("\u{014B}"), // ᵑ -> ŋ
        '\u{1D52}' => Some("o"),        // ᵒ
        '\u{1D53}' => Some("\u{0254}"), // ᵓ -> ɔ
        '\u{1D54}' => Some("\u{1D16}"), // ᵔ -> ᴖ
        '\u{1D55}' => Some("\u{1D17}"), // ᵕ -> ᴗ
        '\u{1D56}' => Some("p"),        // ᵖ
        '\u{1D57}' => Some("t"),        // ᵗ
        '\u{1D58}' => Some("u"),        // ᵘ
        '\u{1D59}' => Some("\u{1D1D}"), // ᵙ -> ᴝ
        '\u{1D5A}' => Some("\u{026F}"), // ᵚ -> ɯ
        '\u{1D5B}' => Some("v"),        // ᵛ
        '\u{1D5C}' => Some("\u{1D25}"), // ᵜ -> ᴥ
        '\u{1D5D}' => Some("\u{03B2}"), // ᵝ -> β
        '\u{1D5E}' => Some("\u{03B3}"), // ᵞ -> γ
        '\u{1D5F}' => Some("\u{03B4}"), // ᵟ -> δ
        '\u{1D60}' => Some("\u{03C6}"), // ᵠ -> φ
        '\u{1D61}' => Some("\u{03C7}"), // ᵡ -> χ
        '\u{2070}' => Some("0"),        // ⁰
        '\u{2071}' => Some("i"),        // ⁱ
        '\u{2074}' => Some("4"),        // ⁴
        '\u{2075}' => Some("5"),        // ⁵
        '\u{2076}' => Some("6"),        // ⁶
        '\u{2077}' => Some("7"),        // ⁷
        '\u{2078}' => Some("8"),        // ⁸
        '\u{2079}' => Some("9"),        // ⁹
        '\u{207A}' => Some("+"),        // ⁺
        '\u{207B}' => Some("\u{2212}"), // ⁻ -> −
        '\u{207C}' => Some("="),        // ⁼
        '\u{207D}' => Some("("),        // ⁽
        '\u{207E}' => Some(")"),        // ⁾
        '\u{207F}' => Some("n"),        // ⁿ
        '\u{2120}' => Some("SM"),       // ℠
        '\u{2122}' => Some("TM"),       // ™
        '\u{3192}' => Some("\u{4E00}"), // ㆒ -> 一
        '\u{3193}' => Some("\u{4E8C}"), // ㆓ -> 二
        '\u{3194}' => Some("\u{4E09}"), // ㆔ -> 三
        '\u{3195}' => Some("\u{56DB}"), // ㆕ -> 四
        '\u{3196}' => Some("\u{4E0A}"), // ㆖ -> 上
        '\u{3197}' => Some("\u{4E2D}"), // ㆗ -> 中
        '\u{3198}' => Some("\u{4E0B}"), // ㆘ -> 下
        '\u{3199}' => Some("\u{7532}"), // ㆙ -> 甲
        '\u{319A}' => Some("\u{4E59}"), // ㆚ -> 乙
        '\u{319B}' => Some("\u{4E19}"), // ㆛ -> 丙
        '\u{319C}' => Some("\u{4E01}"), // ㆜ -> 丁
        '\u{319D}' => Some("\u{5929}"), // ㆝ -> 天
        '\u{319E}' => Some("\u{5730}"), // ㆞ -> 地
        '\u{319F}' => Some("\u{4EBA}"), // ㆟ -> 人
        '\u{02C0}' => Some("\u{0294}"), // ˀ -> ʔ
        '\u{02C1}' => Some("\u{0295}"), // ˁ -> ʕ
        '\u{06E5}' => Some("\u{0648}"), // ۥ -> و
        '\u{06E6}' => Some("\u{064A}"), // ۦ -> ي
        _ => None,
    }
}

/// Render Pandoc Inlines to HTML using CSL conventions.
///
/// Rendering context for CSL HTML output with flip-flop formatting support.
///
/// CSL implements "flip-flop" formatting where nested identical styles toggle:
/// - `<i>outer <i>inner</i> outer</i>` → `<i>outer <span style="font-style:normal;">inner</span> outer</i>`
/// - Same for bold, small-caps, and quotes
///
/// The context tracks whether we're currently inside each formatting type.
/// When `use_italics` is true, we're NOT inside italics (can use `<i>`).
/// When `use_italics` is false, we're inside italics (must use normal span for nested).
///
/// For quotes, `use_outer_quotes` tracks nesting level:
/// - true: use outer quotes ("..."), then toggle to false for nested
/// - false: use inner quotes ('...'), then toggle back to true for nested
#[derive(Clone, Copy)]
struct CslRenderContext {
    use_italics: bool,
    use_bold: bool,
    use_small_caps: bool,
    use_outer_quotes: bool,
}

impl Default for CslRenderContext {
    fn default() -> Self {
        Self {
            use_italics: true,
            use_bold: true,
            use_small_caps: true,
            use_outer_quotes: true,
        }
    }
}

/// CSL test fixtures use `<i>` instead of `<em>`, `<b>` instead of `<strong>`,
/// and `<sc>` instead of CSS-based small-caps. This writer produces output
/// that matches the CSL test expectations.
///
/// This is intended for testing only - production code should use
/// quarto-markdown-pandoc's HTML writer.
///
/// Uses default English quotes. For locale-specific quotes, use
/// `render_inlines_to_csl_html_with_locale` instead.
pub fn render_inlines_to_csl_html(inlines: &quarto_pandoc_types::Inlines) -> String {
    render_inlines_to_csl_html_with_locale(inlines, &QuoteConfig::default())
}

/// Render Pandoc Inlines to CSL HTML with locale-specific quotes.
///
/// This variant uses the provided `QuoteConfig` for quote characters,
/// enabling proper rendering of locale-specific quotation marks
/// (e.g., English "..." vs French « ... »).
pub fn render_inlines_to_csl_html_with_locale(
    inlines: &quarto_pandoc_types::Inlines,
    quotes: &QuoteConfig,
) -> String {
    let mut result = String::new();
    let ctx = CslRenderContext::default();
    for inline in inlines {
        render_inline_to_csl_html_with_ctx(inline, &mut result, ctx, quotes);
    }
    result
}

/// Render Pandoc Blocks to CSL HTML.
///
/// This handles the display attribute by rendering Divs with the appropriate
/// CSS classes. For testing only.
///
/// Uses default English quotes. For locale-specific quotes, use
/// `render_blocks_to_csl_html_with_locale` instead.
pub fn render_blocks_to_csl_html(blocks: &[quarto_pandoc_types::Block]) -> String {
    render_blocks_to_csl_html_with_locale(blocks, &QuoteConfig::default())
}

/// Render Pandoc Blocks to CSL HTML with locale-specific quotes.
///
/// This variant uses the provided `QuoteConfig` for quote characters,
/// enabling proper rendering of locale-specific quotation marks.
pub fn render_blocks_to_csl_html_with_locale(
    blocks: &[quarto_pandoc_types::Block],
    quotes: &QuoteConfig,
) -> String {
    let mut result = String::new();
    let ctx = CslRenderContext::default();

    for block in blocks {
        render_block_to_csl_html(block, &mut result, ctx, quotes);
    }

    result
}

fn render_block_to_csl_html(
    block: &quarto_pandoc_types::Block,
    output: &mut String,
    ctx: CslRenderContext,
    quotes: &QuoteConfig,
) {
    use quarto_pandoc_types::Block;

    match block {
        Block::Plain(p) => {
            for inline in &p.content {
                render_inline_to_csl_html_with_ctx(inline, output, ctx, quotes);
            }
        }
        Block::Paragraph(p) => {
            for inline in &p.content {
                render_inline_to_csl_html_with_ctx(inline, output, ctx, quotes);
            }
        }
        Block::Div(d) => {
            let (_, classes, _) = &d.attr;
            // Get the first class as the div class
            if let Some(class) = classes.first() {
                output.push_str("<div class=\"");
                output.push_str(class);
                output.push_str("\">");
            }
            for inner_block in &d.content {
                render_block_to_csl_html(inner_block, output, ctx, quotes);
            }
            if classes.first().is_some() {
                output.push_str("</div>");
            }
        }
        // Other block types are not expected in CSL output
        _ => {}
    }
}

fn render_inline_to_csl_html_with_ctx(
    inline: &quarto_pandoc_types::Inline,
    output: &mut String,
    ctx: CslRenderContext,
    quotes: &QuoteConfig,
) {
    use quarto_pandoc_types::{Block, Inline};

    match inline {
        Inline::Str(s) => {
            // Check if this is a single Unicode superscript character that should
            // be converted to <sup> tags. This matches Pandoc citeproc's behavior.
            let mut chars = s.text.chars();
            if let (Some(first), None) = (chars.next(), chars.next()) {
                if let Some(base) = superscript_char_to_base(first) {
                    output.push_str("<sup>");
                    output.push_str(&html_escape(base));
                    output.push_str("</sup>");
                    return;
                }
            }

            // Convert straight apostrophes to curly (typographic) apostrophes
            // and escape HTML special characters
            let text = s.text.replace('\'', "\u{2019}"); // ' -> '
            output.push_str(&html_escape(&text));
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
            // Flip-flop: if we can use italics, output <i> and toggle off
            // If we're already in italics, output normal span and toggle on
            if ctx.use_italics {
                output.push_str("<i>");
                let new_ctx = CslRenderContext {
                    use_italics: false,
                    ..ctx
                };
                for child in &e.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</i>");
            } else {
                output.push_str("<span style=\"font-style:normal;\">");
                let new_ctx = CslRenderContext {
                    use_italics: true,
                    ..ctx
                };
                for child in &e.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</span>");
            }
        }
        Inline::Strong(s) => {
            // Flip-flop for bold
            if ctx.use_bold {
                output.push_str("<b>");
                let new_ctx = CslRenderContext {
                    use_bold: false,
                    ..ctx
                };
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</b>");
            } else {
                output.push_str("<span style=\"font-weight:normal;\">");
                let new_ctx = CslRenderContext {
                    use_bold: true,
                    ..ctx
                };
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</span>");
            }
        }
        Inline::SmallCaps(s) => {
            // Flip-flop for small-caps
            if ctx.use_small_caps {
                output.push_str("<span style=\"font-variant:small-caps;\">");
                let new_ctx = CslRenderContext {
                    use_small_caps: false,
                    ..ctx
                };
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</span>");
            } else {
                output.push_str("<span style=\"font-variant:normal;\">");
                let new_ctx = CslRenderContext {
                    use_small_caps: true,
                    ..ctx
                };
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str("</span>");
            }
        }
        Inline::Superscript(s) => {
            output.push_str("<sup>");
            for child in &s.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
            }
            output.push_str("</sup>");
        }
        Inline::Subscript(s) => {
            output.push_str("<sub>");
            for child in &s.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
            }
            output.push_str("</sub>");
        }
        Inline::Quoted(q) => {
            // Flip-flop quotes: outer quotes use locale-specific characters,
            // inner quotes use the inner quote characters from locale.
            // This matches Pandoc citeproc's renderCslJson behavior.
            // Note: We ignore q.quote_type and use context-based flip-flopping.
            if ctx.use_outer_quotes {
                // Use outer (primary) quotes from locale
                output.push_str(&quotes.outer_open);
                let new_ctx = CslRenderContext {
                    use_outer_quotes: false, // Nested quotes will use inner
                    ..ctx
                };
                for child in &q.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str(&quotes.outer_close);
            } else {
                // Use inner (secondary) quotes from locale
                output.push_str(&quotes.inner_open);
                let new_ctx = CslRenderContext {
                    use_outer_quotes: true, // Nested quotes will flip back to outer
                    ..ctx
                };
                for child in &q.content {
                    render_inline_to_csl_html_with_ctx(child, output, new_ctx, quotes);
                }
                output.push_str(&quotes.inner_close);
            }
        }
        Inline::Link(l) => {
            output.push_str("<a href=\"");
            output.push_str(&html_escape(&l.target.0));
            output.push_str("\">");
            for child in &l.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
            }
            output.push_str("</a>");
        }
        Inline::Note(n) => {
            // For notes, render the content inline (CSL doesn't use footnote markup)
            for block in &n.content {
                match block {
                    Block::Paragraph(p) => {
                        for child in &p.content {
                            render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
                        }
                    }
                    Block::Plain(p) => {
                        for child in &p.content {
                            render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
                        }
                    }
                    // Other block types are not expected in CSL output
                    _ => {}
                }
            }
        }
        Inline::Span(s) => {
            let (_, classes, _) = &s.attr;
            if classes.iter().any(|c| c == "nodecoration") {
                // NoDecoration: reset all currently active formatting to normal
                // Build a style string with normal values for all active formats
                // The context uses inverted semantics:
                //   use_italics=true means NOT inside italics (can use <i>)
                //   use_italics=false means inside italics (need to reset)
                // So we emit font-style:normal when !ctx.use_italics (we ARE inside italics)
                let mut styles = Vec::new();
                if !ctx.use_small_caps {
                    styles.push("font-variant:normal;");
                }
                if !ctx.use_bold {
                    styles.push("font-weight:normal;");
                }
                if !ctx.use_italics {
                    styles.push("font-style:normal;");
                }
                if !styles.is_empty() {
                    output.push_str("<span style=\"");
                    for style in &styles {
                        output.push_str(style);
                    }
                    output.push_str("\">");
                }
                // Reset context to all true (not inside any formatting)
                let reset_ctx = CslRenderContext::default();
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, reset_ctx, quotes);
                }
                if !styles.is_empty() {
                    output.push_str("</span>");
                }
            } else {
                // Other spans are transparent - just render children
                for child in &s.content {
                    render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
                }
            }
        }
        Inline::Strikeout(s) => {
            output.push_str("<del>");
            for child in &s.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
            }
            output.push_str("</del>");
        }
        Inline::Underline(u) => {
            output.push_str("<u>");
            for child in &u.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
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
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
            }
            output.push_str("\"/>");
        }
        Inline::Cite(c) => {
            // Render citations inline
            for child in &c.content {
                render_inline_to_csl_html_with_ctx(child, output, ctx, quotes);
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
///
/// Preserves existing numeric character references (&#NN;) and named entities (&name;)
/// to avoid double-escaping content that's already escaped.
fn html_escape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '&' => {
                // Check if this is already an entity reference
                if is_entity_reference(&chars[i..]) {
                    // Copy the entity reference as-is
                    result.push('&');
                    i += 1;
                    while i < chars.len() && chars[i] != ';' {
                        result.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        result.push(';');
                        i += 1;
                    }
                } else {
                    // Use numeric entity like Pandoc citeproc
                    result.push_str("&#38;");
                    i += 1;
                }
            }
            '<' => {
                // Use numeric entity like Pandoc citeproc
                result.push_str("&#60;");
                i += 1;
            }
            '>' => {
                // Use numeric entity like Pandoc citeproc
                result.push_str("&#62;");
                i += 1;
            }
            // Note: Quotes are NOT escaped in CSL output per Pandoc citeproc behavior
            _ => {
                result.push(c);
                i += 1;
            }
        }
    }
    result
}

/// Check if the slice starting at the given position is an entity reference.
/// Matches: &#NN; (numeric) or &name; (named)
fn is_entity_reference(chars: &[char]) -> bool {
    if chars.len() < 3 || chars[0] != '&' {
        return false;
    }

    // Numeric entity: &#digits;
    if chars[1] == '#' {
        let mut i = 2;
        // Could be &#xHH; (hex) or &#NN; (decimal)
        if i < chars.len() && (chars[i] == 'x' || chars[i] == 'X') {
            i += 1;
            // Hex digits
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
        } else {
            // Decimal digits
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        return i > 2 && i < chars.len() && chars[i] == ';';
    }

    // Named entity: &name;
    if chars[1].is_ascii_alphabetic() {
        let mut i = 2;
        while i < chars.len() && chars[i].is_ascii_alphanumeric() {
            i += 1;
        }
        return i > 1 && i < chars.len() && chars[i] == ';';
    }

    false
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
        let outputs = vec![Output::literal("A"), Output::Null, Output::literal("C")];
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
        assert_eq!(
            html,
            "<span style=\"font-variant:small-caps;\">Author</span>"
        );
    }

    #[test]
    fn test_to_inlines_quotes() {
        let mut formatting = Formatting::default();
        formatting.quotes = true;

        let output = Output::formatted(formatting, vec![Output::literal("Title")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        // Unicode curly quotes as per CSL locale conventions
        assert_eq!(html, "\u{201C}Title\u{201D}");
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
        // Uses numeric entity references like Pandoc citeproc
        let output = Output::literal("A < B & C > D");
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "A &#60; B &#38; C &#62; D");
    }

    #[test]
    fn test_to_inlines_bold_with_prefix_suffix() {
        // Default Formatting has affixes_inside=false, so affixes go OUTSIDE formatting
        // bold + prefix="(" + suffix=")" + "[1]" → "(<b>[1]</b>)"
        let mut formatting = Formatting::default();
        formatting.font_weight = Some(quarto_csl::FontWeight::Bold);
        formatting.prefix = Some("(".to_string());
        formatting.suffix = Some(")".to_string());

        let output = Output::formatted(formatting, vec![Output::literal("[1]")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "(<b>[1]</b>)");
    }

    #[test]
    fn test_to_inlines_italic_with_prefix_suffix() {
        // Default Formatting has affixes_inside=false, so affixes go OUTSIDE formatting
        // italic + prefix="[" + suffix="]" + "Title" → "[<i>Title</i>]"
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);
        formatting.prefix = Some("[".to_string());
        formatting.suffix = Some("]".to_string());

        let output = Output::formatted(formatting, vec![Output::literal("Title")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "[<i>Title</i>]");
    }

    #[test]
    fn test_to_inlines_affixes_inside_true() {
        // For layout elements, affixes_inside=true, so affixes go INSIDE formatting
        // bold + prefix="(" + suffix=")" + "[1]" → "<b>([1])</b>"
        let mut formatting = Formatting::default();
        formatting.font_weight = Some(quarto_csl::FontWeight::Bold);
        formatting.prefix = Some("(".to_string());
        formatting.suffix = Some(")".to_string());
        formatting.affixes_inside = true;

        let output = Output::formatted(formatting, vec![Output::literal("[1]")]);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        assert_eq!(html, "<b>([1])</b>");
    }
}

#[cfg(test)]
mod rich_text_tests {
    use super::*;

    #[test]
    fn test_nocase_span_preserves_spaces() {
        let input = "a <span class=\"nocase\">SMITH</span> Pencil";
        let output = parse_csl_rich_text(input);
        let inlines = output.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        // Spaces should be preserved
        assert_eq!(html, "a SMITH Pencil");
    }

    #[test]
    fn test_nocase_with_capitalize_all() {
        let input = "a <span class=\"nocase\">SMITH</span> Pencil";
        let output = parse_csl_rich_text(input);

        // Apply capitalize-all formatting
        let mut formatting = Formatting::default();
        formatting.text_case = Some(quarto_csl::TextCase::CapitalizeAll);
        let formatted = Output::formatted(formatting, vec![output]);

        let inlines = formatted.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        // SMITH should stay unchanged due to nocase, other words should be capitalized
        assert_eq!(html, "A SMITH Pencil");
    }

    #[test]
    fn test_nodecor_with_italic_formatting() {
        // Test that nodecor flips italic to normal when inside italic formatting
        let input = r#"Lessard <span class="nodecor">v.</span> Schmidt"#;
        let output = parse_csl_rich_text(input);

        // Apply italic formatting
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);
        let formatted = Output::formatted(formatting, vec![output]);

        let inlines = formatted.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);
        // The v. should be in a span with font-style:normal to flip the italic
        assert_eq!(
            html,
            r#"<i>Lessard <span style="font-style:normal;">v.</span> Schmidt</i>"#
        );
    }

    #[test]
    fn test_nodecor_debug_structure() {
        // Debug test to understand the Output structure
        let input = r#"Lessard <span class="nodecor">v.</span> Schmidt"#;
        let output = parse_csl_rich_text(input);

        // Check the structure - Output::sequence creates a Formatted with default formatting
        match &output {
            Output::Formatted { children, .. } => {
                assert_eq!(
                    children.len(),
                    3,
                    "Expected 3 children in sequence: {:?}",
                    children
                );
                // Check that one of them is a Tagged with NoDecoration
                let has_nodecor = children.iter().any(|c| {
                    matches!(
                        c,
                        Output::Tagged {
                            tag: Tag::NoDecoration,
                            ..
                        }
                    )
                });
                assert!(
                    has_nodecor,
                    "Expected a NoDecoration tag in the output: {:?}",
                    children
                );
            }
            other => panic!("Expected Formatted (sequence), got {:?}", other),
        }
    }

    #[test]
    fn test_nodecor_from_json_reference() {
        use crate::reference::Reference;

        // Parse a reference from JSON like the CSL tests do
        let json = r#"{
            "id": "ITEM-1",
            "title": "Lessard <span class=\"nodecor\">v.</span> Schmidt",
            "type": "legal_case"
        }"#;

        let reference: Reference = serde_json::from_str(json).unwrap();

        // Get the title value
        let title = reference.get_variable("title").expect("title should exist");

        // Verify the raw title value contains the nodecor span
        assert!(
            title.contains(r#"<span class="nodecor">"#),
            "Title should contain nodecor span, got: {}",
            title
        );

        // Parse it and check the structure
        let output = parse_csl_rich_text(&title);

        // Apply italic formatting
        let mut formatting = Formatting::default();
        formatting.font_style = Some(quarto_csl::FontStyle::Italic);
        let formatted = Output::formatted(formatting, vec![output]);

        let inlines = formatted.to_inlines();
        let html = render_inlines_to_csl_html(&inlines);

        // The v. should be in a span with font-style:normal
        assert_eq!(
            html, r#"<i>Lessard <span style="font-style:normal;">v.</span> Schmidt</i>"#,
            "HTML output doesn't match expected"
        );
    }

    #[test]
    fn test_nodecor_full_csl_pipeline() {
        use crate::reference::Reference;
        use crate::types::Processor;

        // Minimal CSL style with italic title
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="note" version="1.0">
  <info>
    <id />
    <title />
    <updated>2009-08-10T04:49:00+09:00</updated>
  </info>
  <citation>
    <layout>
      <text variable="title"/>
    </layout>
  </citation>
  <bibliography>
    <layout>
      <text variable="title" font-style="italic"/>
    </layout>
  </bibliography>
</style>"#;

        let style = quarto_csl::parse_csl(csl).expect("CSL parse error");
        let mut processor = Processor::new(style);

        // Add reference with nodecor markup in title
        let reference: Reference = serde_json::from_str(
            r#"{
            "id": "ITEM-1",
            "title": "Lessard <span class=\"nodecor\">v.</span> Schmidt",
            "type": "legal_case"
        }"#,
        )
        .unwrap();

        processor.add_reference(reference);

        // Generate bibliography
        let entries = processor
            .generate_bibliography_to_outputs()
            .expect("Bibliography error");

        assert_eq!(entries.len(), 1, "Should have one entry");

        let (_, output) = &entries[0];
        let blocks = output.to_blocks();
        let html = render_blocks_to_csl_html(&blocks);

        // Debug output
        eprintln!("Bibliography HTML: {}", html);

        // Check that the nodecor span is being rendered with font-style:normal
        assert!(
            html.contains(r#"<span style="font-style:normal;">v.</span>"#),
            "Expected nodecor flip-flop, got: {}",
            html
        );
    }
}

#[cfg(test)]
mod punct_tests {
    use super::*;

    #[test]
    fn test_fix_punct_double_period() {
        let input = vec!["Hello.".to_string(), ".World".to_string()];
        let result = fix_punct(input);
        // Period-period -> keep first
        assert_eq!(result.join(""), "Hello.World");
    }

    #[test]
    fn test_fix_punct_double_comma() {
        let input = vec!["Hello,".to_string(), ",World".to_string()];
        let result = fix_punct(input);
        // Comma-comma -> keep first
        assert_eq!(result.join(""), "Hello,World");
    }

    #[test]
    fn test_fix_punct_excl_period() {
        let input = vec!["Hello!".to_string(), ".World".to_string()];
        let result = fix_punct(input);
        // Exclamation-period -> keep first
        assert_eq!(result.join(""), "Hello!World");
    }

    #[test]
    fn test_fix_punct_double_space() {
        let input = vec!["Hello ".to_string(), " World".to_string()];
        let result = fix_punct(input);
        // Space-space -> keep second
        assert_eq!(result.join(""), "Hello World");
    }

    #[test]
    fn test_fix_punct_keep_both() {
        let input = vec!["Hello".to_string(), "World".to_string()];
        let result = fix_punct(input);
        // No punctuation collision
        assert_eq!(result.join(""), "HelloWorld");
    }

    #[test]
    fn test_join_with_smart_delim_basic() {
        let input = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let result = join_with_smart_delim(input, ", ");
        assert_eq!(result, "A, B, C");
    }

    #[test]
    fn test_join_with_smart_delim_skip_before_comma() {
        // If next element starts with comma, skip the delimiter
        let input = vec!["A".to_string(), ", B".to_string()];
        let result = join_with_smart_delim(input, "; ");
        assert_eq!(result, "A, B");
    }

    #[test]
    fn test_join_with_smart_delim_skip_before_period() {
        // If next element starts with period, skip the delimiter
        let input = vec!["A".to_string(), ". B".to_string()];
        let result = join_with_smart_delim(input, ", ");
        assert_eq!(result, "A. B");
    }

    #[test]
    fn test_punctuation_in_quote() {
        use super::{Output, Tag, move_punctuation_inside_quotes};
        use quarto_csl::Formatting;

        // Helper to create a quoted output
        fn quoted(text: &str) -> Output {
            let mut fmt = Formatting::default();
            fmt.quotes = true;
            Output::Formatted {
                formatting: fmt,
                children: vec![Output::Literal(text.to_string())],
            }
        }

        // Test: "Hello" followed by ". world" → "Hello." followed by " world"
        let input = Output::sequence(vec![quoted("Hello"), Output::literal(". world")]);
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(result.render(), "\u{201C}Hello.\u{201D} world");

        // Test: quoted followed by comma
        let input = Output::sequence(vec![quoted("one"), Output::literal(", "), quoted("two")]);
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(result.render(), "\u{201C}one,\u{201D} \u{201C}two\u{201D}");

        // Test: already has punctuation inside - should not double
        let input = Output::sequence(vec![quoted("Hello."), Output::literal(". world")]);
        let result = move_punctuation_inside_quotes(input);
        // Should NOT move because content already ends with period
        assert_eq!(result.render(), "\u{201C}Hello.\u{201D}. world");

        // Test: no trailing punctuation - unchanged
        let input = Output::sequence(vec![quoted("Hello"), Output::literal(" world")]);
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(result.render(), "\u{201C}Hello\u{201D} world");

        // Test: punctuation in a Tagged suffix (matches real citation structure)
        let input = Output::sequence(vec![
            quoted("The Title"),
            Output::tagged(Tag::Suffix, Output::literal(". And more")),
        ]);
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(result.render(), "\u{201C}The Title.\u{201D} And more");

        // Test: complex suffix with nested quotes (like `. And "so it goes"`)
        let complex_suffix =
            Output::sequence(vec![Output::literal(". And "), quoted("so it goes")]);
        let input = Output::sequence(vec![
            quoted("The Title"),
            Output::tagged(Tag::Suffix, complex_suffix),
        ]);
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(
            result.render(),
            "\u{201C}The Title.\u{201D} And \u{201C}so it goes\u{201D}"
        );

        // Test: full citation structure with Item wrapper (matches actual citation output)
        let complex_suffix =
            Output::sequence(vec![Output::literal(". And "), quoted("so it goes")]);
        let inner = Output::sequence(vec![
            quoted("The Title"),
            Output::tagged(Tag::Suffix, complex_suffix),
        ]);
        let input = Output::tagged(
            Tag::Item {
                item_type: super::CitationItemType::NormalCite,
                item_id: "ITEM-1".to_string(),
            },
            inner,
        );
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(
            result.render(),
            "\u{201C}The Title.\u{201D} And \u{201C}so it goes\u{201D}"
        );

        // Test: using parse_csl_rich_text for the suffix (matches actual code path)
        let parsed_suffix = super::parse_csl_rich_text(". And \"so it goes\"");
        let inner = Output::sequence(vec![
            quoted("The Title"),
            Output::tagged(Tag::Suffix, parsed_suffix),
        ]);
        let input = Output::tagged(
            Tag::Item {
                item_type: super::CitationItemType::NormalCite,
                item_id: "ITEM-1".to_string(),
            },
            inner,
        );
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(
            result.render(),
            "\u{201C}The Title.\u{201D} And \u{201C}so it goes\u{201D}"
        );

        // Test: with capitalize_first applied (simulates no-prefix case)
        let parsed_suffix = super::parse_csl_rich_text(". And \"so it goes\"");
        let layout_output = quoted("The Title");
        let inner = Output::sequence(vec![
            layout_output.capitalize_first(), // This is what happens when there's no prefix
            Output::tagged(Tag::Suffix, parsed_suffix),
        ]);
        let input = Output::tagged(
            Tag::Item {
                item_type: super::CitationItemType::NormalCite,
                item_id: "ITEM-1".to_string(),
            },
            inner,
        );
        let result = move_punctuation_inside_quotes(input);
        assert_eq!(
            result.render(),
            "\u{201C}The Title.\u{201D} And \u{201C}so it goes\u{201D}"
        );
    }

    #[test]
    fn test_formatted_with_delimiter() {
        let output = Output::formatted_with_delimiter(
            Formatting::default(),
            vec![
                Output::literal("A"),
                Output::literal("B"),
                Output::literal("C"),
            ],
            ", ",
        );
        assert_eq!(output.render(), "A, B, C");
    }

    // ========================================================================
    // Title case tests
    // ========================================================================

    #[test]
    fn test_title_case_last_word_capitalized() {
        // Last word should ALWAYS be capitalized, even if it's a stop word
        // CSL spec: "stop words are lowercased unless they are the first or last word"
        assert_eq!(
            title_case("intercultural research for"),
            "Intercultural Research For"
        );
        // "the" at the end should be capitalized because it's the last word
        assert_eq!(title_case("a book about the"), "A Book about The");
    }

    #[test]
    fn test_title_case_last_word_stop_word() {
        // Stop words at the end should be capitalized
        assert_eq!(title_case("what is this for"), "What Is This For");
        assert_eq!(
            title_case("something to think about"),
            "Something to Think About"
        );
        assert_eq!(title_case("the way things are"), "The Way Things Are");
    }

    #[test]
    fn test_title_case_name_particles() {
        // Name particles (von, van, de, d) should be treated as stop words
        // They stay lowercase in middle position, capitalize at start/end
        assert_eq!(title_case("john von doe a life"), "John von Doe a Life");
        assert_eq!(title_case("john van doe a life"), "John van Doe a Life");
        assert_eq!(title_case("john de doe a life"), "John de Doe a Life");
    }

    #[test]
    fn test_title_case_about_is_stop_word() {
        // "about" should be a stop word (lowercase in middle)
        assert_eq!(title_case("an about up life"), "An about up Life");
    }

    #[test]
    fn test_title_case_after_colon() {
        // First word after colon should be capitalized
        assert_eq!(
            title_case("john von doe: an about up life"),
            "John von Doe: An about up Life"
        );
    }
}

#[cfg(test)]
mod test_xml_entity_decoding {
    use super::parse_csl_rich_text;

    #[test]
    fn test_rich_text_bold_tag() {
        // Test that <b>friend</b> gets parsed as bold formatting
        let input = "<b>friend</b>";
        let output = parse_csl_rich_text(input);

        // Should be formatted with bold
        let inlines = output.to_inlines();
        let html = super::render_inlines_to_csl_html(&inlines);

        assert_eq!(html, "<b>friend</b>", "Bold tag should be preserved");
    }

    #[test]
    fn test_entity_decoded_bold_tag() {
        // This simulates what we get when XML parser decodes &#60;b&#62;friend&#60;/b&#62;
        // After decoding: <b>friend</b>
        let input = "<b>friend</b>"; // This is what XML should give us
        let output = parse_csl_rich_text(input);

        let inlines = output.to_inlines();
        let html = super::render_inlines_to_csl_html(&inlines);

        assert_eq!(
            html, "<b>friend</b>",
            "Decoded entity should be parsed as bold"
        );
    }
}
