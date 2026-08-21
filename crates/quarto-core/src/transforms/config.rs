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

/// Resolve a website-style boolean feature flag that can be set at
/// either the top level (`page-navigation:`) or under `website.`
/// (`website.page-navigation:`) of `_quarto.yml`. Document frontmatter
/// is naturally merged into top-level metadata, so a frontmatter-level
/// override appears at the top-level path and wins.
///
/// Precedence (first match wins):
/// 1. Top-level `<key>` in the merged metadata (covers doc frontmatter
///    *and* project top-level placement).
/// 2. `website.<key>` (covers project `website:` scope placement).
/// 3. The supplied `default`.
pub fn resolve_website_bool(meta: &ConfigValue, key: &str, default: bool) -> bool {
    if let Some(v) = meta.get(key).and_then(|v| v.as_bool()) {
        return v;
    }
    if let Some(v) = meta.get_path(&["website", key]).and_then(|v| v.as_bool()) {
        return v;
    }
    default
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
    // Infallible parser with a default fallback; `FromStr` would force a
    // `Result`/`Err` this enum doesn't need.
    #[allow(clippy::should_implement_trait)]
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
    // Infallible parser with a default fallback; `FromStr` would force a
    // `Result`/`Err` this enum doesn't need.
    #[allow(clippy::should_implement_trait)]
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

/// Title-block styling behavior (bd-gx9cic8z P6).
///
/// Corresponds to the `title-block-style` option in Quarto schema.
/// Schema source: `document-layout.yml`. Q1's fourth value,
/// `manuscript`, is deliberately unsupported (epic design decision
/// Q6) and falls back to `Default` like any unknown value — the
/// `AppendixStyle` convention (no dedicated warning machinery).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleBlockStyle {
    /// The styled Quarto title block (default).
    #[default]
    Default,
    /// Styled DOM without the title-block SCSS layer.
    Plain,
    /// Pandoc's fallback title block; banner disabled; no SCSS layer.
    None,
}

impl TitleBlockStyle {
    /// Parse from string value.
    // Infallible parser with a default fallback; `FromStr` would force a
    // `Result`/`Err` this enum doesn't need.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "plain" => Self::Plain,
            "none" | "false" => Self::None,
            _ => Self::Default,
        }
    }

    /// Parse from bool value (`title-block-style: false` is Q1's
    /// second spelling of `none`).
    pub fn from_bool(b: bool) -> Self {
        if b { Self::Default } else { Self::None }
    }

    /// Read the option from document metadata.
    pub fn from_meta(meta: &quarto_pandoc_types::ConfigValue) -> Self {
        let Some(value) = meta.get("title-block-style") else {
            return Self::Default;
        };
        if let Some(b) = value.as_bool() {
            return Self::from_bool(b);
        }
        value
            .as_plain_text()
            .map(|s| Self::from_str(&s))
            .unwrap_or_default()
    }

    /// Convert to string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plain => "plain",
            Self::None => "none",
        }
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
                key_source: SourceInfo::for_test(),
                value,
            }],
            SourceInfo::for_test(),
        )
    }

    fn bool_value(b: bool) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::scalar(Yaml::Boolean(b)),
            source_info: SourceInfo::for_test(),
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
            ConfigValue::new_string("auto", SourceInfo::for_test()),
        );
        assert!(!is_feature_disabled(&meta, "toc"));
    }

    #[test]
    fn resolve_website_bool_returns_default_when_absent() {
        let meta = ConfigValue::default();
        assert!(!resolve_website_bool(&meta, "page-navigation", false));
        assert!(resolve_website_bool(&meta, "page-navigation", true));
    }

    #[test]
    fn resolve_website_bool_top_level_wins() {
        let meta = meta_with("page-navigation", bool_value(true));
        assert!(resolve_website_bool(&meta, "page-navigation", false));
    }

    #[test]
    fn resolve_website_bool_website_scope_used_when_top_level_absent() {
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "page-navigation"], bool_value(true));
        assert!(resolve_website_bool(&meta, "page-navigation", false));
    }

    #[test]
    fn resolve_website_bool_top_level_overrides_website_scope() {
        // Frontmatter / project top-level beats website scope.
        let mut meta = meta_with("page-navigation", bool_value(false));
        meta.insert_path(&["website", "page-navigation"], bool_value(true));
        assert!(!resolve_website_bool(&meta, "page-navigation", true));
    }

    #[test]
    fn resolve_website_bool_top_level_false_disables() {
        let meta = meta_with("page-navigation", bool_value(false));
        assert!(!resolve_website_bool(&meta, "page-navigation", true));
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

    fn string_value(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::scalar(Yaml::String(s.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        }
    }

    #[test]
    fn test_title_block_style_from_meta_matrix() {
        use TitleBlockStyle as S;
        // Absent → default.
        let empty = ConfigValue::new_map(vec![], SourceInfo::for_test());
        assert_eq!(S::from_meta(&empty), S::Default);
        for (val, expect) in [
            ("plain", S::Plain),
            ("none", S::None),
            ("default", S::Default),
            // Q1's manuscript is deliberately unsupported (epic Q6) —
            // silent fallback like any unknown value.
            ("manuscript", S::Default),
            ("garbage", S::Default),
        ] {
            let meta = meta_with("title-block-style", string_value(val));
            assert_eq!(S::from_meta(&meta), expect, "title-block-style: {val}");
        }
        // Q1's boolean spelling: `false` = none, `true` = default.
        assert_eq!(
            S::from_meta(&meta_with("title-block-style", bool_value(false))),
            S::None
        );
        assert_eq!(
            S::from_meta(&meta_with("title-block-style", bool_value(true))),
            S::Default
        );
    }
}
