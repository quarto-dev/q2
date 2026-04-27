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

use quarto_pandoc_types::config_value::ConfigValue;

/// Returns `true` if the top-level metadata key is explicitly set to the boolean
/// `false`. This is the "affirmative disable" convention used across `toc`,
/// `navbar`, and `page-footer`: setting the key to `false` in any layer that
/// wins the metadata merge suppresses the feature regardless of any other
/// structured data (e.g. pre-populated `navigation.toc`, `navigation.navbar`,
/// `navigation.footer`) that may also be present in the merged metadata.
///
/// Returns `false` if the key is absent, non-boolean, or set to `true`.
pub fn is_feature_disabled(meta: &ConfigValue, key: &str) -> bool {
    meta.get(key).and_then(|v| v.as_bool()) == Some(false)
}

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
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    fn meta_with(key: &str, value: ConfigValue) -> ConfigValue {
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::default(),
                value,
            }],
            SourceInfo::default(),
        )
    }

    fn bool_value(b: bool) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Boolean(b)),
            source_info: SourceInfo::default(),
            merge_op: Default::default(),
        }
    }

    #[test]
    fn test_is_feature_disabled_false() {
        let meta = meta_with("toc", bool_value(false));
        assert!(is_feature_disabled(&meta, "toc"));
    }

    #[test]
    fn test_is_feature_disabled_true_is_not_disabled() {
        let meta = meta_with("toc", bool_value(true));
        assert!(!is_feature_disabled(&meta, "toc"));
    }

    #[test]
    fn test_is_feature_disabled_absent_is_not_disabled() {
        let meta = ConfigValue::default();
        assert!(!is_feature_disabled(&meta, "toc"));
    }

    #[test]
    fn test_is_feature_disabled_non_bool_is_not_disabled() {
        // `toc: "auto"` is a valid value that means enabled; it must not trip
        // the disabled check.
        let meta = meta_with(
            "toc",
            ConfigValue::new_string("auto", SourceInfo::default()),
        );
        assert!(!is_feature_disabled(&meta, "toc"));
    }

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
