//! Theme configuration extraction from ConfigValue.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides types and functions for extracting theme configuration
//! from Quarto's configuration system (`ConfigValue`). It handles the mapping
//! from a format-flattened `theme` key to `ThemeSpec` arrays for compilation.
//!
//! After MetadataMergeStage, the merged config is format-flattened so `theme`
//! sits at the top level (not nested under `format.html`).
//!
//! # Configuration Formats
//!
//! The theme configuration after flattening:
//!
//! ```yaml
//! # Single theme (string)
//! theme: cosmo
//!
//! # Multiple themes (array)
//! theme:
//!   - cosmo
//!   - custom.scss
//!
//! # No theme (absent) - uses default Bootstrap
//! {}
//! ```

use std::path::{Path, PathBuf};

use quarto_brand::{Brand, BrandRef};
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

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

    /// Sentinel indicating the user asked to suppress the Bootstrap layer
    /// entirely via `theme: none`.
    ///
    /// This mirrors Quarto 1's `theme: none` behavior: callers should skip
    /// Bootstrap compilation and emit a minimal or empty CSS payload of
    /// their choosing. `themes` is always empty when this is `true`.
    pub suppress_bootstrap: bool,

    /// Unresolved reference to a `_brand.yml` (path or inline block),
    /// or `None` if no brand was configured.
    ///
    /// Resolved into a typed [`quarto_brand::Brand`] via
    /// [`ThemeConfig::resolve`].
    pub brand_ref: Option<BrandRef>,
}

/// Resolved form of [`ThemeConfig`] with the brand file loaded and
/// parsed.
///
/// Produced by [`ThemeConfig::resolve`]. Carries everything the SCSS
/// pipeline needs to compile the document's CSS, including the typed
/// `Brand` value when the configuration requests one.
#[derive(Debug, Clone, Default)]
pub struct ResolvedThemeConfig {
    pub themes: Vec<ThemeSpec>,
    pub minified: bool,
    pub suppress_bootstrap: bool,
    pub brand: Option<Brand>,
    /// Directory the brand file was loaded from (for resolving
    /// relative `@font-face` URLs). `None` when brand was inline.
    pub brand_dir: Option<PathBuf>,
}

impl ThemeConfig {
    /// Create a new ThemeConfig with the given themes.
    pub fn new(themes: Vec<ThemeSpec>, minified: bool) -> Self {
        Self {
            themes,
            minified,
            suppress_bootstrap: false,
            brand_ref: None,
        }
    }

    /// Create config for default Bootstrap theme (no Bootswatch customization).
    ///
    /// This produces Bootstrap CSS with Quarto's customizations but without
    /// any Bootswatch theme applied.
    pub fn default_bootstrap() -> Self {
        Self {
            themes: Vec::new(),
            minified: true,
            suppress_bootstrap: false,
            brand_ref: None,
        }
    }

    /// Extract theme config from a format-flattened ConfigValue.
    ///
    /// Expects `theme` at top level (as produced by MetadataMergeStage).
    /// Supports:
    /// - String: single theme name or path (e.g., `"cosmo"`, `"custom.scss"`)
    /// - Array: multiple themes to layer (e.g., `["cosmo", "custom.scss"]`)
    /// - Null/absent: use default Bootstrap theme
    ///
    /// # Arguments
    ///
    /// * `config` - The format-flattened merged configuration (project + document)
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
        // Look for top-level `theme` (format-flattened by MetadataMergeStage)
        let theme_value = config.get("theme");
        let brand_ref = extract_brand_ref(config.get("brand"))?;

        let mut result = match theme_value {
            None => Self::default_bootstrap(),
            Some(value) => {
                if value.is_null() {
                    Self::default_bootstrap()
                } else if let Some(s) = config_value_as_text(value)
                    && s.eq_ignore_ascii_case("none")
                {
                    // `theme: none` sentinel: suppress Bootstrap entirely.
                    Self {
                        themes: Vec::new(),
                        minified: true,
                        suppress_bootstrap: true,
                        brand_ref: None,
                    }
                } else {
                    let themes = extract_theme_specs(value)?;
                    Self {
                        themes,
                        minified: true,
                        suppress_bootstrap: false,
                        brand_ref: None,
                    }
                }
            }
        };

        // `theme: none` is mutually exclusive with brand — Q1 would
        // produce no Bootstrap output anyway, so we mirror that by
        // dropping the brand_ref. The user intent ("don't generate
        // Bootstrap CSS") wins.
        if result.suppress_bootstrap {
            return Ok(result);
        }

        let has_brand_token = result.themes.iter().any(ThemeSpec::is_brand);
        match (brand_ref, has_brand_token) {
            (Some(br), false) => {
                // Auto-inject brand at the end of the theme list.
                result.brand_ref = Some(br);
                result.themes.push(ThemeSpec::Brand);
            }
            (Some(br), true) => {
                // Token already present at user-specified position.
                result.brand_ref = Some(br);
            }
            (None, true) => {
                return Err(SassError::InvalidThemeConfig {
                    message: "`theme:` contains `brand` but no `_brand.yml` was configured \
                              via the `brand:` key"
                        .to_string(),
                    location: config.get("theme").map(|v| v.source_info.clone()),
                });
            }
            (None, false) => {}
        }

        Ok(result)
    }

    /// Resolve any [`BrandRef`] in this config by reading the brand
    /// file (if it's a path) and parsing it into a typed [`Brand`].
    ///
    /// `base_dir` is the directory relative paths in [`BrandRef::Path`]
    /// are resolved against (typically the project root). The
    /// `runtime` provides cross-platform file access — native
    /// `std::fs` on the CLI, the VFS on WASM.
    pub fn resolve(
        self,
        runtime: &dyn SystemRuntime,
        base_dir: &Path,
    ) -> Result<ResolvedThemeConfig, SassError> {
        let (brand, brand_dir) = match self.brand_ref {
            None => (None, None),
            Some(BrandRef::Path(rel_path)) => {
                let full_path = if rel_path.is_absolute() {
                    rel_path.clone()
                } else {
                    base_dir.join(&rel_path)
                };
                let bytes = runtime.file_read(&full_path).map_err(|e| {
                    SassError::Io(std::io::Error::other(format!(
                        "reading brand file {}: {e}",
                        full_path.display()
                    )))
                })?;
                let yaml =
                    std::str::from_utf8(&bytes).map_err(|e| SassError::InvalidThemeConfig {
                        message: format!(
                            "brand file {} is not valid UTF-8: {e}",
                            full_path.display()
                        ),
                        location: None,
                    })?;
                let brand = Brand::from_yaml_str(yaml).map_err(brand_err)?;
                let dir = full_path
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                (Some(brand), Some(dir))
            }
            Some(BrandRef::Inline(value)) => {
                let brand: Brand =
                    serde_yaml::from_value(*value).map_err(|e| SassError::InvalidThemeConfig {
                        message: format!("inline brand block: {e}"),
                        location: None,
                    })?;
                (Some(brand), None)
            }
        };

        Ok(ResolvedThemeConfig {
            themes: self.themes,
            minified: self.minified,
            suppress_bootstrap: self.suppress_bootstrap,
            brand,
            brand_dir,
        })
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

/// Resolve a `_brand.yml` (from the `brand:` key) into SCSS layers, independent
/// of the Bootstrap `theme:` parsing.
///
/// `format: html` resolves brand through [`ThemeConfig::from_config_value`] +
/// the `brand` position marker in `process_theme_specs`, but that path also
/// parses `theme:` against the Bootswatch theme set — which rejects reveal theme
/// names. RevealJS therefore resolves brand on its own via this helper: extract
/// the `brand:` reference, read/parse it into a [`Brand`], and convert it to
/// SCSS layers with [`crate::brand_to_layers`]. Returns an empty vector when no
/// brand is configured.
///
/// `base_dir` resolves a relative `brand:` path (typically the project root);
/// `font_path_prefix` is the directory prefix for `@font-face` URLs (pass an
/// empty path to reference brand-bundled fonts by bare name).
pub fn resolve_brand_layers(
    config: &ConfigValue,
    runtime: &dyn SystemRuntime,
    base_dir: &Path,
    font_path_prefix: &Path,
) -> Result<Vec<crate::SassLayer>, SassError> {
    let Some(brand_ref) = extract_brand_ref(config.get("brand"))? else {
        return Ok(Vec::new());
    };
    // Reuse ThemeConfig's brand resolution (path/inline → typed Brand).
    let resolved = ThemeConfig {
        themes: Vec::new(),
        minified: true,
        suppress_bootstrap: false,
        brand_ref: Some(brand_ref),
    }
    .resolve(runtime, base_dir)?;

    match resolved.brand {
        Some(brand) => crate::brand_layer::brand_to_layers(&brand, font_path_prefix),
        None => Ok(Vec::new()),
    }
}

/// Extract the text content from a ConfigValue, handling both Scalar strings
/// and PandocInlines (which occur when document frontmatter values are parsed
/// as markdown by pampa).
fn config_value_as_text(value: &ConfigValue) -> Option<String> {
    value
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| value.as_plain_text())
}

/// Extract a [`BrandRef`] from the value at the `brand:` key, if any.
///
/// - String → [`BrandRef::Path`].
/// - Map → check for `light`/`dark` keys. If present, emit a soft
///   warning (light/dark pairs are deferred to a follow-up) and use
///   the `light` half. Otherwise treat the whole map as an inline
///   brand block.
/// - Null / absent → `None`.
fn extract_brand_ref(value: Option<&ConfigValue>) -> Result<Option<BrandRef>, SassError> {
    let Some(value) = value else { return Ok(None) };
    if value.is_null() {
        return Ok(None);
    }

    // Path form.
    if let Some(s) = config_value_as_text(value) {
        return Ok(Some(BrandRef::Path(PathBuf::from(s))));
    }

    // Light/dark pair, or inline block.
    if let Some(entries) = value.as_map_entries() {
        let light = entries.iter().find(|e| e.key == "light");
        let dark = entries.iter().find(|e| e.key == "dark");
        let other = entries.iter().any(|e| e.key != "light" && e.key != "dark");
        if (light.is_some() || dark.is_some()) && !other {
            // Treat as a light/dark pair. Light half is used; the dark
            // side is deferred — see Phase 8 follow-up.
            // TODO(brand light/dark): wire dark variant once Q2 has a
            // light/dark seam.
            if let Some(light_entry) = light {
                return extract_brand_ref(Some(&light_entry.value));
            }
            // Only dark configured — silently ignore for now.
            return Ok(None);
        }

        // Inline brand block: convert the typed ConfigValue back to a
        // serde_yaml::Value so we can hand it to serde_yaml::from_value
        // in `resolve`.
        let yaml_value = config_value_to_yaml_value(value)?;
        return Ok(Some(BrandRef::Inline(Box::new(yaml_value))));
    }

    // Scalar(Yaml::Hash) — synthesized in tests, or produced when the
    // metadata merge stage passes through a hash without lifting it
    // into ConfigValueKind::Map. Treat as inline.
    if value.as_array().is_none()
        && let yaml_value = config_value_to_yaml_value(value)?
    {
        // Only accept if the yaml_value is a mapping; bail otherwise.
        if matches!(yaml_value, serde_yaml::Value::Mapping(_)) {
            return Ok(Some(BrandRef::Inline(Box::new(yaml_value))));
        }
    }

    Err(SassError::InvalidThemeConfig {
        message: "`brand:` must be a path string or a brand block (map)".to_string(),
        location: Some(value.source_info.clone()),
    })
}

/// Convert a `ConfigValue` to a `serde_yaml::Value`, walking the
/// typed structure directly.
///
/// We do **not** round-trip via `serde_yaml::to_string(config_value)`
/// because `ConfigValue`'s `Serialize` impl emits the typed wrapper
/// (variant tags + `source_info` / `merge_op` metadata). The output
/// here is just the underlying YAML shape — what an unannotated YAML
/// parser would have produced — so downstream `serde_yaml::from_value`
/// can deserialize it into the brand types.
fn config_value_to_yaml_value(value: &ConfigValue) -> Result<serde_yaml::Value, SassError> {
    use quarto_pandoc_types::ConfigValueKind;
    Ok(match &value.value {
        ConfigValueKind::Scalar(yaml) => yaml_rust_to_serde(yaml),
        ConfigValueKind::Path(s) | ConfigValueKind::Glob(s) | ConfigValueKind::Expr(s) => {
            serde_yaml::Value::String(s.clone())
        }
        ConfigValueKind::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(config_value_to_yaml_value(item)?);
            }
            serde_yaml::Value::Sequence(out)
        }
        ConfigValueKind::Map(entries) => {
            let mut map = serde_yaml::Mapping::new();
            for entry in entries {
                map.insert(
                    serde_yaml::Value::String(entry.key.clone()),
                    config_value_to_yaml_value(&entry.value)?,
                );
            }
            serde_yaml::Value::Mapping(map)
        }
        ConfigValueKind::PandocInlines(_) | ConfigValueKind::PandocBlocks(_) => {
            return Err(SassError::InvalidThemeConfig {
                message: "brand block must be plain YAML, not Pandoc inlines/blocks".to_string(),
                location: Some(value.source_info.clone()),
            });
        }
    })
}

fn yaml_rust_to_serde(yaml: &yaml_rust2::Yaml) -> serde_yaml::Value {
    use yaml_rust2::Yaml;
    match yaml {
        // `serde_yaml::Number` doesn't have a fallible `from_f64`, so
        // we attempt the parse and fall back to a string for NaN/Inf.
        Yaml::Real(s) => match s.parse::<f64>() {
            Ok(f) if f.is_finite() => serde_yaml::Value::Number(f.into()),
            _ => serde_yaml::Value::String(s.clone()),
        },
        Yaml::Integer(i) => serde_yaml::Value::Number((*i).into()),
        Yaml::String(s) => serde_yaml::Value::String(s.clone()),
        Yaml::Boolean(b) => serde_yaml::Value::Bool(*b),
        Yaml::Array(items) => {
            serde_yaml::Value::Sequence(items.iter().map(yaml_rust_to_serde).collect())
        }
        Yaml::Hash(hash) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in hash {
                map.insert(yaml_rust_to_serde(k), yaml_rust_to_serde(v));
            }
            serde_yaml::Value::Mapping(map)
        }
        Yaml::Alias(_) | Yaml::BadValue => serde_yaml::Value::Null,
        Yaml::Null => serde_yaml::Value::Null,
    }
}

fn brand_err(e: quarto_brand::BrandError) -> SassError {
    SassError::InvalidThemeConfig {
        message: e.to_string(),
        location: None,
    }
}

/// Extract theme specifications from a ConfigValue.
///
/// Handles both string and array formats. Theme values from document
/// frontmatter may arrive as PandocInlines (parsed as markdown by pampa),
/// while values from `_quarto.yml` / `_metadata.yml` arrive as Scalar strings.
/// Both are handled transparently.
fn extract_theme_specs(value: &ConfigValue) -> Result<Vec<ThemeSpec>, SassError> {
    // Handle string value (single theme) — covers both Scalar and PandocInlines
    if let Some(s) = config_value_as_text(value) {
        let spec = ThemeSpec::parse(&s).map_err(|e| e.with_location(value.source_info.clone()))?;
        return Ok(vec![spec]);
    }

    // Handle array value (multiple themes)
    if let Some(items) = value.as_array() {
        let mut specs = Vec::with_capacity(items.len());
        for item in items {
            if let Some(s) = config_value_as_text(item) {
                specs.push(
                    ThemeSpec::parse(&s).map_err(|e| e.with_location(item.source_info.clone()))?,
                );
            } else {
                return Err(SassError::InvalidThemeConfig {
                    message: "theme array must contain only strings".to_string(),
                    location: Some(value.source_info.clone()),
                });
            }
        }
        return Ok(specs);
    }

    // Neither string nor array - invalid
    Err(SassError::InvalidThemeConfig {
        message: "theme must be a string or array of strings".to_string(),
        location: Some(value.source_info.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
    use quarto_source_map::SourceInfo;
    use yaml_rust2::Yaml;

    /// Helper to create an empty config (no theme)
    fn empty_config() -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
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
        let config = flattened_config_with_theme_string("cosmo");
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
        let config = flattened_config_with_theme_string("custom.scss");
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
        let config = flattened_config_with_theme_array(&["darkly"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
    }

    #[test]
    fn test_from_config_value_array_multiple() {
        let config = flattened_config_with_theme_array(&["cosmo", "custom.scss"]);
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
    fn test_from_config_value_null_theme() {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert!(theme_config.themes.is_empty());
    }

    #[test]
    fn test_from_config_value_unknown_theme() {
        let config = flattened_config_with_theme_string("nonexistent");
        let result = ThemeConfig::from_config_value(&config);

        assert!(result.is_err());
        match result {
            Err(SassError::UnknownTheme { name, .. }) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected UnknownTheme error"),
        }
    }

    #[test]
    fn test_from_config_value_invalid_type() {
        // Create config with theme as a map (invalid)
        let theme_value = ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let result = ThemeConfig::from_config_value(&config);
        assert!(result.is_err());
        match result {
            Err(SassError::InvalidThemeConfig { message, .. }) => {
                assert!(message.contains("string or array"));
            }
            _ => panic!("Expected InvalidThemeConfig error"),
        }
    }

    #[test]
    fn test_from_config_value_array_with_non_string() {
        let items = vec![
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::Integer(42)),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
        ];

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let result = ThemeConfig::from_config_value(&config);
        assert!(result.is_err());
        match result {
            Err(SassError::InvalidThemeConfig { message, .. }) => {
                assert!(message.contains("only strings"));
            }
            _ => panic!("Expected InvalidThemeConfig error"),
        }
    }

    // === theme_specs accessor test ===

    #[test]
    fn test_theme_specs() {
        let config = flattened_config_with_theme_array(&["cosmo", "flatly"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        let specs = theme_config.theme_specs();
        assert_eq!(specs.len(), 2);
        assert!(specs[0].is_builtin());
        assert!(specs[1].is_builtin());
    }

    // === PandocInlines tests (document frontmatter parsed by pampa) ===

    #[test]
    fn test_from_config_value_pandoc_inlines_theme() {
        use quarto_pandoc_types::inline::{Inline, Str};

        // Simulate pampa parsing `theme: cosmo` as PandocInlines
        let str_node = Inline::Str(Str {
            text: "cosmo".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let theme_value = ConfigValue::new_inlines(vec![str_node], SourceInfo::for_test());

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
        assert_eq!(
            theme_config.themes[0].as_builtin(),
            Some(crate::themes::BuiltInTheme::Cosmo)
        );
    }

    // === Flattened config helpers (post-MetadataMergeStage format) ===

    /// Helper to create a flattened config with theme at top level (string).
    /// This is the format produced by MetadataMergeStage: `{ theme: "darkly" }`
    fn flattened_config_with_theme_string(theme: &str) -> ConfigValue {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(theme.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Helper to create a flattened config with theme at top level (array).
    /// This is the format produced by MetadataMergeStage: `{ theme: ["cosmo", "custom.scss"] }`
    fn flattened_config_with_theme_array(themes: &[&str]) -> ConfigValue {
        let items: Vec<ConfigValue> = themes
            .iter()
            .map(|s| ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            })
            .collect();

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    // === Flattened config tests (post-MetadataMergeStage) ===

    #[test]
    fn test_from_flattened_config_single_theme() {
        let config = flattened_config_with_theme_string("darkly");
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
        assert_eq!(
            theme_config.themes[0].as_builtin(),
            Some(crate::themes::BuiltInTheme::Darkly)
        );
        assert!(theme_config.minified);
    }

    #[test]
    fn test_from_flattened_config_array_theme() {
        let config = flattened_config_with_theme_array(&["cosmo", "custom.scss"]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_builtin());
        assert_eq!(
            theme_config.themes[0].as_builtin(),
            Some(crate::themes::BuiltInTheme::Cosmo)
        );
        assert!(theme_config.themes[1].is_custom());
    }

    #[test]
    fn test_from_flattened_config_no_theme() {
        let config = empty_config();
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert!(theme_config.themes.is_empty());
        assert!(!theme_config.has_themes());
    }

    #[test]
    fn test_theme_none_sets_suppress_bootstrap_flag() {
        // `theme: none` is a Q1-compatible sentinel: it suppresses Bootstrap
        // entirely and asks the caller to emit a minimal CSS fallback (or
        // none at all). It must not be parsed as a BuiltInTheme.
        let config = flattened_config_with_theme_string("none");
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        assert!(theme_config.suppress_bootstrap);
        assert!(theme_config.themes.is_empty());
        assert!(!theme_config.has_themes());
    }

    #[test]
    fn test_theme_none_is_case_insensitive() {
        // YAML bare `None`, `NONE`, `None` should all map to the sentinel.
        for name in ["none", "None", "NONE"] {
            let config = flattened_config_with_theme_string(name);
            let theme_config = ThemeConfig::from_config_value(&config)
                .unwrap_or_else(|e| panic!("{name} should parse: {e:?}"));
            assert!(
                theme_config.suppress_bootstrap,
                "{name} should set suppress_bootstrap"
            );
        }
    }

    #[test]
    fn test_default_theme_config_does_not_suppress_bootstrap() {
        // A plain missing-theme config must NOT set suppress_bootstrap, so
        // the caller compiles the default Bootstrap+Quarto layer.
        let config = empty_config();
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert!(!theme_config.suppress_bootstrap);
    }

    #[test]
    fn test_from_flattened_config_null_theme() {
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::Null),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert!(theme_config.themes.is_empty());
        assert!(!theme_config.has_themes());
    }

    // === Source-location propagation tests (bd-pgczr) ===
    //
    // These guard that the SourceInfo carried by the offending
    // ConfigValue makes it onto SassError::InvalidThemeConfig.location,
    // which downstream uses to render an ariadne span pointing at the
    // offending key in _quarto.yml.

    /// `theme:` set to a map (the unsupported `light:/dark:` shape)
    /// must produce a SassError carrying the offending value's
    /// SourceInfo, not None.
    #[test]
    fn test_invalid_theme_map_carries_location() {
        // Distinctive offsets so we can assert the right source_info
        // propagated (not just any non-default value).
        let theme_source = SourceInfo::original(quarto_source_map::FileId(7), 100, 200);

        let theme_value = ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: theme_source.clone(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        match ThemeConfig::from_config_value(&config) {
            Err(SassError::InvalidThemeConfig { message, location }) => {
                assert!(message.contains("string or array"));
                assert_eq!(
                    location.as_ref(),
                    Some(&theme_source),
                    "expected location to carry the offending value's source_info",
                );
            }
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    /// `theme: [<non-string>]` must produce a SassError carrying the
    /// offending *array's* SourceInfo.
    #[test]
    fn test_invalid_theme_array_carries_location() {
        let theme_source = SourceInfo::original(quarto_source_map::FileId(7), 300, 400);

        let items = vec![
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::Integer(42)),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
        ];

        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: theme_source.clone(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        match ThemeConfig::from_config_value(&config) {
            Err(SassError::InvalidThemeConfig { message, location }) => {
                assert!(message.contains("only strings"));
                assert_eq!(location.as_ref(), Some(&theme_source));
            }
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    // === Source-location propagation for UnknownTheme (bd-1pwy8) ===

    /// A `theme: <unknown name>` value (scalar) must surface a
    /// SassError::UnknownTheme carrying the offending value's
    /// SourceInfo. Without this, the diagnostic falls back to a
    /// span-less "SASS error" line at the CLI.
    #[test]
    fn test_unknown_theme_scalar_carries_location() {
        let theme_source = SourceInfo::original(quarto_source_map::FileId(7), 500, 510);

        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("default".to_string())),
            source_info: theme_source.clone(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };
        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        match ThemeConfig::from_config_value(&config) {
            Err(SassError::UnknownTheme { name, location }) => {
                assert_eq!(name, "default");
                assert_eq!(
                    location.as_ref(),
                    Some(&theme_source),
                    "expected scalar value's source_info to propagate",
                );
            }
            other => panic!("expected UnknownTheme error, got: {:?}", other),
        }
    }

    /// `theme: [<unknown>, cosmo]` — when an array item is an
    /// unknown built-in name, the *item's* source_info should
    /// propagate (not the array's), so the diagnostic points at
    /// the offending element.
    #[test]
    fn test_unknown_theme_in_array_carries_item_location() {
        let item_source = SourceInfo::original(quarto_source_map::FileId(7), 600, 610);

        let items = vec![
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String("nosuchtheme".to_string())),
                source_info: item_source.clone(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
            ConfigValue {
                value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
                source_info: SourceInfo::for_test(),
                merge_op: quarto_pandoc_types::MergeOp::Concat,
            },
        ];
        let theme_value = ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };
        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };
        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        match ThemeConfig::from_config_value(&config) {
            Err(SassError::UnknownTheme { name, location }) => {
                assert_eq!(name, "nosuchtheme");
                assert_eq!(location.as_ref(), Some(&item_source));
            }
            other => panic!("expected UnknownTheme error, got: {:?}", other),
        }
    }
}
