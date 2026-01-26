/*
 * config.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Shared configuration enums for transforms.
 */

//! Shared configuration types for AST transforms.
//!
//! These types correspond to Quarto schema options and are used by multiple
//! transforms to read configuration consistently.

/// Where footnotes/references should be placed.
///
/// Corresponds to the `reference-location` option in Quarto schema.
/// Schema source: `document-footnotes.yml`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferenceLocation {
    /// Footnotes at end of document (default)
    #[default]
    Document,
    /// Footnotes at end of each section (Pandoc handles this)
    Section,
    /// Footnotes at end of each block (Pandoc handles this)
    Block,
    /// Footnotes in margins (sidenotes)
    Margin,
}

impl ReferenceLocation {
    /// Parse from string value.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "section" => Self::Section,
            "block" => Self::Block,
            "margin" => Self::Margin,
            _ => Self::Document,
        }
    }

    /// Convert to string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Section => "section",
            Self::Block => "block",
            Self::Margin => "margin",
        }
    }
}

/// Appendix styling behavior.
///
/// Corresponds to the `appendix-style` option in Quarto schema.
/// Schema source: `document-layout.yml`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppendixStyle {
    /// Standard appendix processing (default)
    #[default]
    Default,
    /// Minimal appendix styling
    Plain,
    /// Disable appendix processing
    None,
}

impl AppendixStyle {
    /// Parse from string or bool value.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plain" => Self::Plain,
            "none" | "false" => Self::None,
            _ => Self::Default,
        }
    }

    /// Parse from bool value.
    pub fn from_bool(b: bool) -> Self {
        if b { Self::Default } else { Self::None }
    }

    /// Convert to string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plain => "plain",
            Self::None => "none",
        }
    }

    /// Check if appendix processing is enabled.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_location_from_str() {
        assert_eq!(
            ReferenceLocation::from_str("document"),
            ReferenceLocation::Document
        );
        assert_eq!(
            ReferenceLocation::from_str("Document"),
            ReferenceLocation::Document
        );
        assert_eq!(
            ReferenceLocation::from_str("DOCUMENT"),
            ReferenceLocation::Document
        );
        assert_eq!(
            ReferenceLocation::from_str("section"),
            ReferenceLocation::Section
        );
        assert_eq!(
            ReferenceLocation::from_str("block"),
            ReferenceLocation::Block
        );
        assert_eq!(
            ReferenceLocation::from_str("margin"),
            ReferenceLocation::Margin
        );
        assert_eq!(
            ReferenceLocation::from_str("Margin"),
            ReferenceLocation::Margin
        );
        assert_eq!(
            ReferenceLocation::from_str("unknown"),
            ReferenceLocation::Document
        );
        assert_eq!(ReferenceLocation::from_str(""), ReferenceLocation::Document);
    }

    #[test]
    fn test_reference_location_as_str() {
        assert_eq!(ReferenceLocation::Document.as_str(), "document");
        assert_eq!(ReferenceLocation::Section.as_str(), "section");
        assert_eq!(ReferenceLocation::Block.as_str(), "block");
        assert_eq!(ReferenceLocation::Margin.as_str(), "margin");
    }

    #[test]
    fn test_reference_location_default() {
        assert_eq!(ReferenceLocation::default(), ReferenceLocation::Document);
    }

    #[test]
    fn test_appendix_style_from_str() {
        assert_eq!(AppendixStyle::from_str("default"), AppendixStyle::Default);
        assert_eq!(AppendixStyle::from_str("Default"), AppendixStyle::Default);
        assert_eq!(AppendixStyle::from_str("plain"), AppendixStyle::Plain);
        assert_eq!(AppendixStyle::from_str("Plain"), AppendixStyle::Plain);
        assert_eq!(AppendixStyle::from_str("none"), AppendixStyle::None);
        assert_eq!(AppendixStyle::from_str("None"), AppendixStyle::None);
        assert_eq!(AppendixStyle::from_str("false"), AppendixStyle::None);
        assert_eq!(AppendixStyle::from_str("unknown"), AppendixStyle::Default);
    }

    #[test]
    fn test_appendix_style_from_bool() {
        assert_eq!(AppendixStyle::from_bool(true), AppendixStyle::Default);
        assert_eq!(AppendixStyle::from_bool(false), AppendixStyle::None);
    }

    #[test]
    fn test_appendix_style_is_enabled() {
        assert!(AppendixStyle::Default.is_enabled());
        assert!(AppendixStyle::Plain.is_enabled());
        assert!(!AppendixStyle::None.is_enabled());
    }

    #[test]
    fn test_appendix_style_default() {
        assert_eq!(AppendixStyle::default(), AppendixStyle::Default);
    }
}
