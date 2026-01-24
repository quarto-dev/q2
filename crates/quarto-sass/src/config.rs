//! Theme configuration extraction from ConfigValue.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides types and functions for extracting theme configuration
//! from Quarto's configuration system (`ConfigValue`). It handles the mapping
//! from `format.html.theme` to `ThemeSpec` arrays for compilation.
//!
//! # Configuration Formats
//!
//! The theme configuration in YAML can take several forms:
//!
//! ```yaml
//! # Single theme (string)
//! format:
//!   html:
//!     theme: cosmo
//!
//! # Multiple themes (array)
//! format:
//!   html:
//!     theme:
//!       - cosmo
//!       - custom.scss
//!
//! # No theme (absent) - uses default Bootstrap
//! format:
//!   html: {}
//! ```

use quarto_pandoc_types::ConfigValue;

use crate::error::SassError;
use crate::themes::ThemeSpec;

/// Extracted theme configuration from document/project metadata.
///
/// This type represents the parsed theme configuration ready for compilation.
/// It's extracted from `ConfigValue` via [`ThemeConfig::from_config_value()`].
///
/// # Example
///
/// ```rust,ignore
/// use quarto_sass::ThemeConfig;
/// use quarto_pandoc_types::ConfigValue;
///
/// // From merged project + document config
/// let config = ThemeConfig::from_config_value(&merged_config)?;
///
/// // Or use the default (Bootstrap default theme)
/// let default_config = ThemeConfig::default_bootstrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ThemeConfig {
    /// Theme specifications (built-in names or file paths).
    ///
    /// Empty means use default Bootstrap theme (no Bootswatch customization).
    pub themes: Vec<ThemeSpec>,

    /// Whether to produce minified CSS.
    ///
    /// Defaults to `true` for consistency with TypeScript Quarto.
    pub minified: bool,
}

impl ThemeConfig {
    /// Create a new ThemeConfig with the given themes.
    pub fn new(themes: Vec<ThemeSpec>, minified: bool) -> Self {
        Self { themes, minified }
    }

    /// Create config for default Bootstrap theme (no Bootswatch customization).
    ///
    /// This produces Bootstrap CSS with Quarto's customizations but without
    /// any Bootswatch theme applied.
    pub fn default_bootstrap() -> Self {
        Self {
            themes: Vec::new(),
            minified: true,
        }
    }

    /// Extract theme config from merged ConfigValue.
    ///
    /// Looks for `format.html.theme` in the config. Supports:
    /// - String: single theme name or path (e.g., `"cosmo"`, `"custom.scss"`)
    /// - Array: multiple themes to layer (e.g., `["cosmo", "custom.scss"]`)
    /// - Null/absent: use default Bootstrap theme
    ///
    /// # Arguments
    ///
    /// * `config` - The merged configuration (project + document)
    ///
    /// # Returns
    ///
    /// A `ThemeConfig` ready for compilation.
    ///
    /// # Errors
    ///
    /// Returns `SassError::InvalidThemeConfig` if the theme configuration
    /// has an unexpected structure (e.g., a map instead of string/array).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use quarto_sass::ThemeConfig;
    ///
    /// let config = ThemeConfig::from_config_value(&merged_config)?;
    /// println!("Themes: {:?}", config.themes);
    /// println!("Minified: {}", config.minified);
    /// ```
    pub fn from_config_value(config: &ConfigValue) -> Result<Self, SassError> {
        // Navigate to format.html.theme
        let theme_value = config
            .get("format")
            .and_then(|format| format.get("html"))
            .and_then(|html| html.get("theme"));

        match theme_value {
            None => {
                // No theme specified - use default Bootstrap
                Ok(Self::default_bootstrap())
            }
            Some(value) => {
                // Check for null
                if value.is_null() {
                    return Ok(Self::default_bootstrap());
                }

                // Try to extract themes
                let themes = extract_theme_specs(value)?;
                Ok(Self {
                    themes,
                    minified: true, // Always minified for TS Quarto parity
                })
            }
        }
    }

    /// Check if this config specifies any themes.
    ///
    /// Returns `false` if the config uses the default Bootstrap theme
    /// (no Bootswatch or custom themes).
    pub fn has_themes(&self) -> bool {
        !self.themes.is_empty()
    }

    /// Get the theme specifications.
    pub fn theme_specs(&self) -> &[ThemeSpec] {
        &self.themes
    }
}

/// Extract theme specifications from a ConfigValue.
///
/// Handles both string and array formats.
fn extract_theme_specs(value: &ConfigValue) -> Result<Vec<ThemeSpec>, SassError> {
    // Handle string value (single theme)
    if let Some(s) = value.as_str() {
        let spec = ThemeSpec::parse(s)?;
        return Ok(vec![spec]);
    }

    // Handle array value (multiple themes)
    if let Some(items) = value.as_array() {
        let mut specs = Vec::with_capacity(items.len());
        for item in items {
            if let Some(s) = item.as_str() {
                specs.push(ThemeSpec::parse(s)?);
            } else {
                // Array item is not a string
                return Err(SassError::InvalidThemeConfig {
                    message: "theme array must contain only strings".to_string(),
                });
            }
        }
        return Ok(specs);
    }

    // Neither string nor array - invalid
    Err(SassError::InvalidThemeConfig {
        message: "theme must be a string or array of strings".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    /// Helper to create a config with format.html.theme set to a string
    fn config_with_theme_string(theme: &str) -> ConfigValue {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Helper to create a config with format.html.theme set to an array
    fn config_with_theme_array(themes: &[&str]) -> ConfigValue {
        let items: Vec<ConfigValue> = themes
            .iter()
            .map(|s| ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
                source_info: SourceInfo::default(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            })
            .collect();

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Helper to create an empty config (no theme)
    fn empty_config() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Helper to create config with format.html but no theme
    fn config_without_theme() -> ConfigValue {
        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    // === ThemeConfig tests ===

    #[test]
    fn test_theme_config_default_bootstrap() {
        let config = ThemeConfig::default_bootstrap();
        assert!(config.themes.is_empty());
        assert!(config.minified);
        assert!(!config.has_themes());
    }

    #[test]
    fn test_theme_config_new() {
        let themes = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("custom.scss").unwrap(),
        ];
        let config = ThemeConfig::new(themes, false);

        assert_eq!(config.themes.len(), 2);
        assert!(!config.minified);
        assert!(config.has_themes());
    }

    // === from_config_value tests ===

    #[test]
    fn test_from_config_value_string_builtin() {
        let config = config_with_theme_string("cosmo");
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
        assert_eq!(
            theme_config.themes[0].as_builtin(),
            Some(crate::themes::BuiltInTheme::Cosmo)
        );
        assert!(theme_config.minified);
    }

    #[test]
    fn test_from_config_value_string_custom() {
        let config = config_with_theme_string("custom.scss");
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_custom());
        assert_eq!(
            theme_config.themes[0].as_custom().map(|p| p.to_str()),
            Some(Some("custom.scss"))
        );
    }

    #[test]
    fn test_from_config_value_array_single() {
        let config = config_with_theme_array(&["darkly"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
    }

    #[test]
    fn test_from_config_value_array_multiple() {
        let config = config_with_theme_array(&["cosmo", "custom.scss"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_builtin());
        assert!(theme_config.themes[1].is_custom());
    }

    #[test]
    fn test_from_config_value_empty_config() {
        let config = empty_config();
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert!(theme_config.themes.is_empty());
        assert!(!theme_config.has_themes());
    }

    #[test]
    fn test_from_config_value_no_theme() {
        let config = config_without_theme();
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert!(theme_config.themes.is_empty());
    }

    #[test]
    fn test_from_config_value_null_theme() {
        // Create config with theme: null
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert!(theme_config.themes.is_empty());
    }

    #[test]
    fn test_from_config_value_unknown_theme() {
        let config = config_with_theme_string("nonexistent");
        let result = ThemeConfig::from_config_value(&config);

        assert!(result.is_err());
        match result {
            Err(SassError::UnknownTheme(name)) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected UnknownTheme error"),
        }
    }

    #[test]
    fn test_from_config_value_invalid_type() {
        // Create config with theme as a map (invalid)
        let theme_value = ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let result = ThemeConfig::from_config_value(&config);
        assert!(result.is_err());
        match result {
            Err(SassError::InvalidThemeConfig { message }) => {
                assert!(message.contains("string or array"));
            }
            _ => panic!("Expected InvalidThemeConfig error"),
        }
    }

    #[test]
    fn test_from_config_value_array_with_non_string() {
        // Create config with theme array containing a non-string
        let items = vec![
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
                source_info: SourceInfo::default(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::Integer(42)),
                source_info: SourceInfo::default(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
        ];

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let result = ThemeConfig::from_config_value(&config);
        assert!(result.is_err());
        match result {
            Err(SassError::InvalidThemeConfig { message }) => {
                assert!(message.contains("only strings"));
            }
            _ => panic!("Expected InvalidThemeConfig error"),
        }
    }

    // === theme_specs accessor test ===

    #[test]
    fn test_theme_specs() {
        let config = config_with_theme_array(&["cosmo", "flatly"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        let specs = theme_config.theme_specs();
        assert_eq!(specs.len(), 2);
        assert!(specs[0].is_builtin());
        assert!(specs[1].is_builtin());
    }
}
