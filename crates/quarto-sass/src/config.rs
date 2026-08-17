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

use quarto_brand::{Brand, BrandRef, ResolvedBrand};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;
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

    /// Source location of each entry in `themes`, parallel by index
    /// (`theme_locations[i]` locates `themes[i]`; consumers should
    /// index with `.get(i)` rather than assume equal length).
    ///
    /// Populated by [`ThemeConfig::from_config_value`] from the YAML
    /// values; `None` for entries without a source (programmatic
    /// construction, the auto-injected `brand` token). Kept as a
    /// parallel field — rather than inside [`ThemeSpec`] — so the
    /// spec stays a pure value type ([`Eq`], [`std::fmt::Display`],
    /// cache-key identity) unpolluted by provenance (bd-of20unsb).
    pub theme_locations: Vec<Option<SourceInfo>>,

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

    /// Whether to include the built-in title-block SCSS layer
    /// (`templates/title-block.scss`) in the compiled bundle.
    ///
    /// `true` by default; `false` when the document sets
    /// `title-block-style: plain | none | false` — Q1's
    /// `documentTitleScssLayer` behavior (bd-gx9cic8z P6). The
    /// document keeps its markup (for `plain`); only the styled look
    /// is dropped.
    pub title_block_layer: bool,

    /// Unresolved reference to a `_brand.yml` (path or inline block),
    /// or `None` if no brand was configured.
    ///
    /// Resolved into a typed [`quarto_brand::Brand`] via
    /// [`ThemeConfig::resolve`].
    pub brand_ref: Option<BrandRef>,

    /// The dark half of a `theme: {light: …, dark: …}` map, parsed and
    /// ready for a dark-variant compilation. `None` when the theme was
    /// not a map or the map had no `dark:` entry (a light-only map is
    /// an honored, if redundant, spelling of the plain form).
    ///
    /// The fields of [`ThemeConfig`] itself always describe the light
    /// variant; options that apply to the whole configuration
    /// (`minified`, `title_block_layer`, `brand_ref`) are not
    /// duplicated here.
    pub dark: Option<DarkThemeConfig>,

    /// Resolved syntax-highlight palette for THIS (light) variant,
    /// from the `highlight-style:` key (bd-0pic6 phase B). Adaptive
    /// names are resolved at parse time: a scalar `a11y` becomes
    /// `a11y-light` here and `a11y-dark` on [`DarkThemeConfig`]; for a
    /// single-variant config the palette follows the built-in themes'
    /// darkness (`theme: darkly` + `a11y` → `a11y-dark`). `None` →
    /// the default palette.
    pub highlight_style: Option<HighlightStyle>,
}

/// The parsed `dark:` half of a `theme: {light: …, dark: …}` pair
/// (bd-0pic6 light/dark epic, phase A1).
///
/// Mirrors the variant-specific subset of [`ThemeConfig`]: its own
/// spec list, per-entry locations, and `none`-sentinel flag.
#[derive(Debug, Clone, Default)]
pub struct DarkThemeConfig {
    /// Theme specifications for the dark variant. Empty means default
    /// Bootstrap (no Bootswatch customization) — e.g. `{dark: none}`
    /// sets `suppress_bootstrap` instead.
    pub themes: Vec<ThemeSpec>,

    /// Source location of each entry in `themes`, parallel by index
    /// (same contract as [`ThemeConfig::theme_locations`]).
    pub theme_locations: Vec<Option<SourceInfo>>,

    /// The dark half used the `none` sentinel (`{…, dark: none}`):
    /// no Bootstrap output for the dark variant.
    pub suppress_bootstrap: bool,

    /// Whether dark is the *author-default* variant: true when the
    /// `dark:` key is written before `light:` in the map (or is the
    /// only key). This is Q1's key-order rule
    /// (`format-html-info.ts::darkModeDefaultMetadata`: the first key
    /// of the map decides). Drives the emitted stylesheet order and
    /// the toggle's initial state.
    pub is_default: bool,

    /// Location of the `dark:` key itself, for diagnostics that need
    /// to point at the dark half as a whole.
    pub key_location: Option<SourceInfo>,

    /// Resolved syntax-highlight palette for the dark variant (from
    /// `highlight-style:`, adaptive names already resolved — see
    /// [`ThemeConfig::highlight_style`]). `None` → the default
    /// palette.
    pub highlight_style: Option<HighlightStyle>,

    /// The dark variant's brand reference (bd-0pic6 phase C): the
    /// `dark:` half of a `brand: {light:, dark:}` pair, or a copy of
    /// the light brand when the brand has no dark half (Q1's
    /// per-layer fallback — a bundle with no dark opinion contributes
    /// its light layers to the dark compile).
    pub brand_ref: Option<BrandRef>,
}

/// A resolved syntax-highlight palette request for one variant
/// (bd-0pic6 phase B).
///
/// `name` is the palette identifier after adaptive resolution
/// (`a11y` → `a11y-light` / `a11y-dark` depending on the variant's
/// darkness). The name is NOT validated here — quarto-sass carries it
/// as data; the compile falls back to the default palette for unknown
/// names and `CompileThemeCssStage` emits the user-facing warning
/// (same division of labor as the theme diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightStyle {
    pub name: String,
    /// Location of the YAML value that produced this name, for
    /// diagnostics.
    pub location: Option<SourceInfo>,
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
    /// The dark variant, carried through unchanged from
    /// [`ThemeConfig::dark`]. (The brand is resolved once and shared
    /// by both variants until the brand light/dark seam lands —
    /// bd-ld-c-brand-seam-wef8ww3n.)
    pub dark: Option<DarkThemeConfig>,
}

impl ThemeConfig {
    /// Create a new ThemeConfig with the given themes.
    pub fn new(themes: Vec<ThemeSpec>, minified: bool) -> Self {
        let theme_locations = vec![None; themes.len()];
        Self {
            themes,
            theme_locations,
            minified,
            suppress_bootstrap: false,
            title_block_layer: true,
            brand_ref: None,
            dark: None,
            highlight_style: None,
        }
    }

    /// Create config for default Bootstrap theme (no Bootswatch customization).
    ///
    /// This produces Bootstrap CSS with Quarto's customizations but without
    /// any Bootswatch theme applied.
    pub fn default_bootstrap() -> Self {
        Self {
            themes: Vec::new(),
            theme_locations: Vec::new(),
            minified: true,
            suppress_bootstrap: false,
            title_block_layer: true,
            brand_ref: None,
            dark: None,
            highlight_style: None,
        }
    }

    /// Extract theme config from a format-flattened ConfigValue.
    ///
    /// Expects `theme` at top level (as produced by MetadataMergeStage).
    /// Supports:
    /// - String: single theme name or path (e.g., `"cosmo"`, `"custom.scss"`)
    /// - Array: multiple themes to layer (e.g., `["cosmo", "custom.scss"]`)
    /// - Map with only `light:`/`dark:` keys (Q1's dual-theme form):
    ///   each half is parsed like a top-level theme value; the light
    ///   half fills the top-level fields, the dark half becomes
    ///   [`ThemeConfig::dark`] (with the key-order rule deciding
    ///   [`DarkThemeConfig::is_default`]).
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
    /// has an unexpected structure (e.g., a map with keys other than
    /// `light`/`dark`).
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
        let brand_refs = extract_brand_refs(config.get("brand"))?;

        let mut result = match theme_value {
            None => Self::default_bootstrap(),
            Some(value) => match light_dark_pair(value) {
                Some(pair) => {
                    // The pair form is top-level only: each half goes
                    // through the same value parsing as a plain
                    // `theme:` (string / array / null / `none`
                    // sentinel), where a nested map is invalid.
                    let mut cfg = match pair.light {
                        Some(light_value) => Self::from_theme_value(light_value)?,
                        // Only `dark:` configured → default Bootstrap
                        // for the light variant.
                        None => Self::default_bootstrap(),
                    };
                    cfg.dark = match pair.dark {
                        Some((dark_value, key_source)) => {
                            let dark_cfg = Self::from_theme_value(dark_value)?;
                            Some(DarkThemeConfig {
                                themes: dark_cfg.themes,
                                theme_locations: dark_cfg.theme_locations,
                                suppress_bootstrap: dark_cfg.suppress_bootstrap,
                                is_default: pair.dark_first,
                                key_location: Some(key_source),
                                highlight_style: None,
                                brand_ref: None,
                            })
                        }
                        None => None,
                    };
                    cfg
                }
                None => Self::from_theme_value(value)?,
            },
        };

        // `title-block-style: plain | none | false` drops the
        // title-block SCSS layer (Q1's `documentTitleScssLayer`
        // returns no layer for those values; bd-gx9cic8z P6). Markup
        // consequences live in quarto-core; this crate only controls
        // the layer's presence in the bundle.
        result.title_block_layer = title_block_layer_enabled(config);

        // `theme: none` is mutually exclusive with brand, per variant —
        // Q1 would produce no Bootstrap output anyway, so we mirror
        // that by dropping the brand_ref when *every* configured
        // variant suppresses Bootstrap. The user intent ("don't
        // generate Bootstrap CSS") wins.
        let all_variants_suppressed =
            result.suppress_bootstrap && result.dark.as_ref().is_none_or(|d| d.suppress_bootstrap);
        if all_variants_suppressed {
            return Ok(result);
        }

        // A dark brand ENABLES dark mode (Q1's `enablesDarkMode`):
        // when `brand: {…, dark: …}` exists without a dark theme
        // half, synthesize the dark variant from the light theme list
        // — it then flows through dual compilation, link emission,
        // and the toggle like a `theme:`-declared pair. Synthesis
        // happens BEFORE brand-token injection so each variant gets
        // its own marker. The author default falls back to the brand
        // map's key order (the theme map's order wins when both maps
        // exist, because a theme-declared pair sets `is_default`
        // above and this branch is skipped).
        if brand_refs.dark.is_some() && result.dark.is_none() && !result.suppress_bootstrap {
            result.dark = Some(DarkThemeConfig {
                themes: result.themes.clone(),
                theme_locations: result.theme_locations.clone(),
                suppress_bootstrap: false,
                is_default: brand_refs.dark_first,
                key_location: None,
                highlight_style: None,
                brand_ref: None,
            });
        }

        // Per-variant brand refs: the dark variant falls back to the
        // light brand when the brand has no dark half (Q1's per-layer
        // fallback). Auto-inject the position marker at the end of
        // each variant's list that doesn't already name it (and isn't
        // suppressed); naming `brand` in a variant that has no brand
        // is an error.
        let light_ref = brand_refs.light;
        let dark_ref = brand_refs.dark.or_else(|| light_ref.clone());

        let light_has_token = result.themes.iter().any(ThemeSpec::is_brand);
        match (&light_ref, light_has_token) {
            (Some(_), false) if !result.suppress_bootstrap => {
                result.themes.push(ThemeSpec::Brand);
                result.theme_locations.push(None);
            }
            (None, true) => {
                return Err(SassError::InvalidThemeConfig {
                    message: "`theme:` contains `brand` but no `_brand.yml` was configured \
                              via the `brand:` key"
                        .to_string(),
                    location: config.get("theme").map(|v| v.source_info.clone()),
                });
            }
            _ => {}
        }
        result.brand_ref = light_ref;

        if let Some(dark) = result.dark.as_mut() {
            let dark_has_token = dark.themes.iter().any(ThemeSpec::is_brand);
            match (&dark_ref, dark_has_token) {
                (Some(_), false) if !dark.suppress_bootstrap => {
                    dark.themes.push(ThemeSpec::Brand);
                    dark.theme_locations.push(None);
                }
                (None, true) => {
                    return Err(SassError::InvalidThemeConfig {
                        message: "`theme:` contains `brand` but no `_brand.yml` was configured \
                                  via the `brand:` key"
                            .to_string(),
                        location: config.get("theme").map(|v| v.source_info.clone()),
                    });
                }
                _ => {}
            }
            dark.brand_ref = dark_ref;
        }

        // `highlight-style:` (bd-0pic6 phase B) — after theme parsing
        // so adaptive-name resolution can consult the variants.
        parse_highlight_style(config, &mut result)?;

        Ok(result)
    }

    /// Parse a single theme *value* — the top-level `theme:` value, or
    /// the `light:` half of a `light:`/`dark:` pair. Handles null, the
    /// `none` sentinel, a string, and an array of strings; anything
    /// else (including a map — the pair form is recognized one level
    /// up, top-level only) is `SassError::InvalidThemeConfig`.
    fn from_theme_value(value: &ConfigValue) -> Result<Self, SassError> {
        if value.is_null() {
            return Ok(Self::default_bootstrap());
        }
        if let Some(s) = config_value_as_text(value)
            && s.eq_ignore_ascii_case("none")
        {
            // `theme: none` sentinel: suppress Bootstrap entirely.
            return Ok(Self {
                themes: Vec::new(),
                theme_locations: Vec::new(),
                minified: true,
                suppress_bootstrap: true,
                title_block_layer: true,
                brand_ref: None,
                dark: None,
                highlight_style: None,
            });
        }
        let located = extract_theme_specs(value)?;
        let (themes, theme_locations) = located
            .into_iter()
            .map(|(spec, loc)| (spec, Some(loc)))
            .unzip();
        Ok(Self {
            themes,
            theme_locations,
            minified: true,
            suppress_bootstrap: false,
            title_block_layer: true,
            brand_ref: None,
            dark: None,
            highlight_style: None,
        })
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
            dark: self.dark,
        })
    }

    /// Project the dark half into a standalone, light-shaped
    /// [`ThemeConfig`] so the entire pure compile pipeline
    /// (`process_theme_specs` → `assemble_theme_scss` →
    /// `compile_with_doc_vars`) can run unchanged for the dark
    /// variant. Whole-config options (`minified`,
    /// `title_block_layer`, `brand_ref`) carry over; the projection
    /// has no nested dark half of its own.
    ///
    /// Returns `None` when no dark half is configured.
    pub fn dark_variant(&self) -> Option<ThemeConfig> {
        self.dark.as_ref().map(|d| ThemeConfig {
            themes: d.themes.clone(),
            theme_locations: d.theme_locations.clone(),
            minified: self.minified,
            suppress_bootstrap: d.suppress_bootstrap,
            title_block_layer: self.title_block_layer,
            brand_ref: d.brand_ref.clone(),
            dark: None,
            highlight_style: d.highlight_style.clone(),
        })
    }

    /// Whether any configured variant ships Bootstrap. `theme: none`
    /// suppression is per-variant (`{light: none, dark: darkly}`
    /// still needs Bootstrap CSS + JS for the dark variant), so the
    /// Bootstrap-JS decision must consider both halves.
    pub fn ships_bootstrap(&self) -> bool {
        !self.suppress_bootstrap || self.dark.as_ref().is_some_and(|d| !d.suppress_bootstrap)
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

/// Resolve the `brand:` key of a config into a typed [`Brand`] plus the
/// directory it was read from.
///
/// This is the single entry point for "what brand does this config
/// name?", shared by every brand consumer so the reference-extraction
/// rules (path form, inline block, light/dark pair) are stated once.
/// Returns `Ok(None)` when no brand is configured.
///
/// `base_dir` resolves a relative `brand:` path — typically the project
/// root.
///
/// Consumers differ in *which* config they ask about, and the difference
/// is meaningful: the theme path passes merged document metadata (a
/// document may brand itself in its frontmatter), while site-level
/// consumers such as the favicon fallback pass project metadata only.
pub fn resolve_brand(
    config: &ConfigValue,
    runtime: &dyn SystemRuntime,
    base_dir: &Path,
) -> Result<Option<ResolvedBrand>, SassError> {
    // Single-variant consumers (reveal, favicon fallback) use the
    // LIGHT brand; per-variant selection is the HTML dual-compile
    // path's concern (bd-0pic6 phase C).
    let Some(brand_ref) = extract_brand_refs(config.get("brand"))?.light else {
        return Ok(None);
    };
    // Reuse ThemeConfig's brand resolution (path/inline → typed Brand).
    let resolved = ThemeConfig {
        themes: Vec::new(),
        theme_locations: Vec::new(),
        minified: true,
        suppress_bootstrap: false,
        title_block_layer: true,
        brand_ref: Some(brand_ref),
        dark: None,
        highlight_style: None,
    }
    .resolve(runtime, base_dir)?;

    Ok(resolved
        .brand
        .map(|brand| ResolvedBrand::new(brand, resolved.brand_dir)))
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
    match resolve_brand(config, runtime, base_dir)? {
        Some(resolved) => crate::brand_layer::brand_to_layers(&resolved.brand, font_path_prefix),
        None => Ok(Vec::new()),
    }
}

/// Whether the title-block SCSS layer should be included, per the
/// `title-block-style` option: `plain`, `none`, and `false` drop it
/// (Q1's `documentTitleScssLayer`); anything else — including absent
/// and unrecognized values — keeps it. Mirrors quarto-core's
/// `TitleBlockStyle` parsing (this crate sits below quarto-core, so
/// the two readers are deliberately duplicated; both are covered by
/// the P6 test matrix).
fn title_block_layer_enabled(config: &ConfigValue) -> bool {
    let Some(value) = config.get("title-block-style") else {
        return true;
    };
    if let Some(b) = value.as_bool() {
        return b;
    }
    match config_value_as_text(value).as_deref() {
        Some(s) => !matches!(s.to_lowercase().as_str(), "plain" | "none" | "false"),
        None => true,
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

/// The per-variant brand references extracted from the `brand:` key
/// (bd-0pic6 phase C).
struct BrandRefs {
    light: Option<BrandRef>,
    dark: Option<BrandRef>,
    /// Whether `dark:` is the brand map's first key (or its only
    /// key) — Q1's fallback author-default rule when the `theme:` map
    /// doesn't decide (`format-html-info.ts::darkModeDefaultMetadata`).
    dark_first: bool,
}

/// Extract the per-variant [`BrandRef`]s from the value at the
/// `brand:` key, if any.
///
/// - String → light [`BrandRef::Path`] (the dark variant falls back
///   to it at the call site).
/// - Map with only `light`/`dark` keys → each half extracted as its
///   own single-brand value (path or inline block).
/// - Any other map → an inline (light) brand block.
/// - Null / absent → neither.
fn extract_brand_refs(value: Option<&ConfigValue>) -> Result<BrandRefs, SassError> {
    let none = BrandRefs {
        light: None,
        dark: None,
        dark_first: false,
    };
    let Some(value) = value else { return Ok(none) };
    if value.is_null() {
        return Ok(none);
    }

    if let Some(entries) = value.as_map_entries() {
        let light = entries.iter().find(|e| e.key == "light");
        let dark = entries.iter().find(|e| e.key == "dark");
        let other = entries.iter().any(|e| e.key != "light" && e.key != "dark");
        if (light.is_some() || dark.is_some()) && !other {
            return Ok(BrandRefs {
                light: match light {
                    Some(entry) => Some(extract_single_brand_ref(&entry.value)?),
                    None => None,
                },
                dark: match dark {
                    Some(entry) => Some(extract_single_brand_ref(&entry.value)?),
                    None => None,
                },
                dark_first: entries.first().is_some_and(|e| e.key == "dark"),
            });
        }
    }

    Ok(BrandRefs {
        light: Some(extract_single_brand_ref(value)?),
        dark: None,
        dark_first: false,
    })
}

/// Extract one [`BrandRef`] from a single brand value (a path string
/// or an inline brand block) — the halves of a `{light:, dark:}` pair
/// and the plain single-brand form both go through here.
fn extract_single_brand_ref(value: &ConfigValue) -> Result<BrandRef, SassError> {
    // Path form.
    if let Some(s) = config_value_as_text(value) {
        return Ok(BrandRef::Path(PathBuf::from(s)));
    }

    // Inline brand block: convert the typed ConfigValue back to a
    // serde_yaml::Value so we can hand it to serde_yaml::from_value
    // in `resolve`.
    if value.as_map_entries().is_some() {
        let yaml_value = config_value_to_yaml_value(value)?;
        return Ok(BrandRef::Inline(Box::new(yaml_value)));
    }

    // Scalar(Yaml::Hash) — synthesized in tests, or produced when the
    // metadata merge stage passes through a hash without lifting it
    // into ConfigValueKind::Map. Treat as inline.
    if value.as_array().is_none()
        && let yaml_value = config_value_to_yaml_value(value)?
    {
        // Only accept if the yaml_value is a mapping; bail otherwise.
        if matches!(yaml_value, serde_yaml::Value::Mapping(_)) {
            return Ok(BrandRef::Inline(Box::new(yaml_value)));
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

/// Adaptive highlight styles: bare names that resolve to a
/// variant-specific palette (Q1 ships `<name>-light.theme` /
/// `<name>-dark.theme` pairs for these). Stage-1 curated set
/// (bd-0pic6 phase B); the general `.theme`-translator follow-up
/// grows this list.
const ADAPTIVE_HIGHLIGHT_STYLES: &[&str] = &["a11y"];

/// Resolve an adaptive highlight-style name for a variant's darkness;
/// non-adaptive names pass through unchanged (unknown ones fall back
/// to the default palette at compile time, with a stage-side warning).
fn resolve_adaptive_highlight(name: &str, dark: bool) -> String {
    if ADAPTIVE_HIGHLIGHT_STYLES.contains(&name) {
        format!("{name}-{}", if dark { "dark" } else { "light" })
    } else {
        name.to_string()
    }
}

/// Darkness of a variant judged by its built-in themes (any dark
/// Bootswatch theme ⇒ dark), falling back to `fallback` when the list
/// has no built-ins (custom-SCSS-only variants can't be judged
/// statically — Q1 greps the compiled CSS's darkness sentinel, which
/// isn't available before the compile this decision feeds).
fn builtin_darkness(themes: &[ThemeSpec], fallback: bool) -> bool {
    let mut saw_builtin = false;
    let mut any_dark = false;
    for spec in themes {
        if let ThemeSpec::BuiltIn(b) = spec {
            saw_builtin = true;
            any_dark |= b.is_dark();
        }
    }
    if saw_builtin { any_dark } else { fallback }
}

/// Parse the `highlight-style:` key (scalar or `{light:, dark:}` map)
/// into per-variant [`HighlightStyle`] entries on `result`
/// (bd-0pic6 phase B).
///
/// Adaptive-name resolution: with a theme pair, each half's ROLE
/// decides (the quarto-web shape darkens a cosmo base via custom
/// SCSS, so built-in darkness would mislead); for a single-variant
/// config the built-in themes decide (`theme: darkly` + `a11y` →
/// `a11y-dark`).
fn parse_highlight_style(config: &ConfigValue, result: &mut ThemeConfig) -> Result<(), SassError> {
    let Some(value) = config.get("highlight-style") else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }

    let has_pair = result.dark.is_some();
    let light_is_dark = if has_pair {
        false
    } else {
        builtin_darkness(&result.themes, false)
    };

    if let Some(pair) = light_dark_pair(value) {
        if let Some(light_value) = pair.light {
            let Some(name) = config_value_as_text(light_value) else {
                return Err(SassError::InvalidThemeConfig {
                    message: "`highlight-style:` entries must be strings".to_string(),
                    location: Some(light_value.source_info.clone()),
                });
            };
            result.highlight_style = Some(HighlightStyle {
                name: resolve_adaptive_highlight(&name, light_is_dark),
                location: Some(light_value.source_info.clone()),
            });
        }
        if let Some((dark_value, _key_source)) = pair.dark {
            let Some(name) = config_value_as_text(dark_value) else {
                return Err(SassError::InvalidThemeConfig {
                    message: "`highlight-style:` entries must be strings".to_string(),
                    location: Some(dark_value.source_info.clone()),
                });
            };
            // A dark highlight palette needs a dark theme variant to
            // ride on; without one it has no compile to affect.
            if let Some(dark_half) = result.dark.as_mut() {
                dark_half.highlight_style = Some(HighlightStyle {
                    name: resolve_adaptive_highlight(&name, true),
                    location: Some(dark_value.source_info.clone()),
                });
            }
        }
        return Ok(());
    }

    let Some(name) = config_value_as_text(value) else {
        return Err(SassError::InvalidThemeConfig {
            message: "`highlight-style:` must be a string or a map with only \
                      `light:`/`dark:` keys"
                .to_string(),
            location: Some(value.source_info.clone()),
        });
    };
    result.highlight_style = Some(HighlightStyle {
        name: resolve_adaptive_highlight(&name, light_is_dark),
        location: Some(value.source_info.clone()),
    });
    if let Some(dark_half) = result.dark.as_mut() {
        dark_half.highlight_style = Some(HighlightStyle {
            name: resolve_adaptive_highlight(&name, true),
            location: Some(value.source_info.clone()),
        });
    }
    Ok(())
}

/// The recognized halves of a `theme: {light: …, dark: …}` map.
///
/// Produced by [`light_dark_pair`]; consumed by
/// [`ThemeConfig::from_config_value`]'s pair branch.
struct LightDarkPair<'a> {
    /// The `light:` entry's value, if present.
    light: Option<&'a ConfigValue>,
    /// The `dark:` entry's value and its key's location, if present.
    dark: Option<(&'a ConfigValue, SourceInfo)>,
    /// Whether `dark:` is the map's first key (or its only key) —
    /// Q1's key-order rule for the author-default variant
    /// (`format-html-info.ts::darkModeDefaultMetadata`). Meaningful
    /// only when `dark` is `Some`.
    dark_first: bool,
}

/// Detect Q1's dual-theme pair form: a **non-empty** map whose keys
/// are only `light`/`dark` (at least one present). Mirrors the shape
/// check `extract_brand_ref` applies to `brand:` maps. Returns `None`
/// for every other value — including an empty map or a map with extra
/// keys, which stay invalid-config errors downstream.
fn light_dark_pair(value: &ConfigValue) -> Option<LightDarkPair<'_>> {
    let entries = value.as_map_entries()?;
    if entries.is_empty() || entries.iter().any(|e| e.key != "light" && e.key != "dark") {
        return None;
    }
    Some(LightDarkPair {
        light: entries.iter().find(|e| e.key == "light").map(|e| &e.value),
        dark: entries
            .iter()
            .find(|e| e.key == "dark")
            .map(|e| (&e.value, e.key_source.clone())),
        dark_first: entries.first().is_some_and(|e| e.key == "dark"),
    })
}

/// Extract theme specifications from a ConfigValue.
///
/// Handles both string and array formats. Theme values from document
/// frontmatter may arrive as PandocInlines (parsed as markdown by pampa),
/// while values from `_quarto.yml` / `_metadata.yml` arrive as Scalar strings.
/// Both are handled transparently.
fn extract_theme_specs(value: &ConfigValue) -> Result<Vec<(ThemeSpec, SourceInfo)>, SassError> {
    // Handle string value (single theme) — covers both Scalar and PandocInlines
    if let Some(s) = config_value_as_text(value) {
        let spec = ThemeSpec::parse(&s).map_err(|e| e.with_location(value.source_info.clone()))?;
        return Ok(vec![(spec, value.source_info.clone())]);
    }

    // Handle array value (multiple themes)
    if let Some(items) = value.as_array() {
        let mut specs = Vec::with_capacity(items.len());
        for item in items {
            if let Some(s) = config_value_as_text(item) {
                specs.push((
                    ThemeSpec::parse(&s).map_err(|e| e.with_location(item.source_info.clone()))?,
                    item.source_info.clone(),
                ));
            } else {
                return Err(SassError::InvalidThemeConfig {
                    message: "theme array must contain only strings".to_string(),
                    location: Some(value.source_info.clone()),
                });
            }
        }
        return Ok(specs);
    }

    // Neither string nor array - invalid. (The `light:`/`dark:` map
    // form is recognized before this function is reached; a map
    // arriving here has extra keys, is empty, or is nested inside a
    // pair's half.)
    Err(SassError::InvalidThemeConfig {
        message: "theme must be a string or array of strings, \
                  or a map with only `light:`/`dark:` keys"
            .to_string(),
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
        assert!(config.title_block_layer);
    }

    /// Helper: a config map with a single scalar entry.
    fn config_with_scalar(key: &str, value: Yaml) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue {
                    value: ConfigValueKind::Scalar(value),
                    source_info: SourceInfo::for_test(),
                    merge_op: quarto_pandoc_types::MergeOp::Concat,
                },
            }]),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    #[test]
    fn test_title_block_layer_flag_matrix() {
        // bd-gx9cic8z P6: plain / none / false drop the layer; the
        // default, unknown values (incl. Q1's unsupported
        // `manuscript`), and `true` keep it.
        let keep = |cfg: &ConfigValue| {
            ThemeConfig::from_config_value(cfg)
                .unwrap()
                .title_block_layer
        };
        assert!(keep(&empty_config()));
        for (val, expect) in [
            ("plain", false),
            ("none", false),
            ("default", true),
            ("manuscript", true),
        ] {
            assert_eq!(
                keep(&config_with_scalar(
                    "title-block-style",
                    Yaml::String(val.to_string())
                )),
                expect,
                "title-block-style: {val}"
            );
        }
        assert!(!keep(&config_with_scalar(
            "title-block-style",
            Yaml::Boolean(false)
        )));
        assert!(keep(&config_with_scalar(
            "title-block-style",
            Yaml::Boolean(true)
        )));
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

    // === Light/dark theme map tests (bd-0pic6 epic, phase A1) ===
    //
    // Q1's `theme: {light: […], dark: […]}` map form. Both halves are
    // parsed: the light half fills the top-level fields, the dark half
    // becomes `ThemeConfig::dark` (specs, per-entry locations, `none`
    // sentinel, key-order `is_default`, and the `dark:` key's own
    // location for diagnostics). Maps with keys other than
    // `light`/`dark` stay Q-14-1 errors.

    /// Compact scalar ConfigValue builder for map-form tests.
    fn scalar_value(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Compact string-array ConfigValue builder for map-form tests.
    fn array_value(items: &[&str]) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Array(items.iter().map(|s| scalar_value(s)).collect()),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Map entry with a default (for_test) key source.
    fn map_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: SourceInfo::for_test(),
            value,
        }
    }

    /// A map-kind ConfigValue from entries.
    fn map_value(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(entries),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        }
    }

    /// Root config `{ theme: <value> }` (post-MetadataMergeStage shape).
    fn config_with_theme_value(theme_value: ConfigValue) -> ConfigValue {
        map_value(vec![map_entry("theme", theme_value)])
    }

    #[test]
    fn test_theme_map_light_dark_parses_both_halves() {
        // The canonical shape: both halves are lists. The light list
        // becomes the top-level specs; the dark list is parsed into
        // `ThemeConfig::dark` with per-entry locations and the dark
        // key's own location for diagnostics.
        let dark_key_source = SourceInfo::original(quarto_source_map::FileId(9), 40, 44);
        let theme_value = map_value(vec![
            map_entry("light", array_value(&["custom.scss", "cosmo"])),
            ConfigMapEntry {
                key: "dark".to_string(),
                key_source: dark_key_source.clone(),
                value: array_value(&["dark.scss"]),
            },
        ]);

        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_custom());
        assert_eq!(
            theme_config.themes[0].as_custom().and_then(|p| p.to_str()),
            Some("custom.scss")
        );
        assert!(theme_config.themes[1].is_builtin());
        assert!(!theme_config.suppress_bootstrap);
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert_eq!(
            dark.key_location.as_ref(),
            Some(&dark_key_source),
            "dark.key_location should carry the dark entry's key source",
        );
        assert_eq!(dark.themes.len(), 1, "dark half spec list parsed");
        assert!(dark.themes[0].is_custom());
        assert_eq!(
            dark.themes[0].as_custom().and_then(|p| p.to_str()),
            Some("dark.scss")
        );
        assert_eq!(dark.theme_locations.len(), 1);
        assert!(dark.theme_locations[0].is_some());
        assert!(!dark.suppress_bootstrap);
        assert!(!dark.is_default, "light listed first ⇒ light is default");
    }

    #[test]
    fn test_theme_map_light_scalar_only_no_warning() {
        // D6: a light-only map is fully honored — nothing is ignored,
        // so nothing to warn about. Scalar form.
        let theme_value = map_value(vec![map_entry("light", scalar_value("cosmo"))]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
        assert!(theme_config.dark.is_none());
    }

    #[test]
    fn test_theme_map_light_list_only_no_warning() {
        // D6, list form.
        let theme_value = map_value(vec![map_entry(
            "light",
            array_value(&["custom.scss", "cosmo"]),
        )]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_custom());
        assert!(theme_config.themes[1].is_builtin());
        assert!(theme_config.dark.is_none());
    }

    #[test]
    fn test_theme_map_dark_only_defaults_light_and_is_default_dark() {
        // Only `dark:` configured → the light variant is default
        // Bootstrap (NOT suppressed — the page still gets default
        // styling), the dark variant carries the spec, and dark is the
        // author default (it is the first — only — key).
        let theme_value = map_value(vec![map_entry("dark", scalar_value("darkly"))]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert!(theme_config.themes.is_empty());
        assert!(!theme_config.suppress_bootstrap);
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert_eq!(dark.themes.len(), 1);
        assert!(dark.themes[0].is_builtin());
        assert!(dark.is_default, "dark-only map ⇒ dark is the default");
    }

    #[test]
    fn test_theme_map_dark_first_is_default() {
        // Q1's key-order rule: `{dark: …, light: …}` (dark written
        // first) makes dark the author-default variant.
        let theme_value = map_value(vec![
            map_entry("dark", scalar_value("darkly")),
            map_entry("light", scalar_value("cosmo")),
        ]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert!(dark.is_default, "dark listed first ⇒ dark is default");
        assert_eq!(dark.themes.len(), 1);
        assert!(dark.themes[0].is_builtin());
    }

    #[test]
    fn test_theme_map_dark_none_suppresses_dark_bootstrap() {
        // The `none` sentinel is honored inside the dark half, per
        // variant: the light variant compiles normally, the dark
        // variant suppresses Bootstrap.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("none")),
        ]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert!(!theme_config.suppress_bootstrap);
        assert_eq!(theme_config.themes.len(), 1);
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert!(dark.suppress_bootstrap);
        assert!(dark.themes.is_empty());
    }

    #[test]
    fn test_theme_map_light_none_dark_still_parsed() {
        // `{light: none, dark: darkly}`: suppression is per-variant.
        // The light half suppresses Bootstrap; the dark half still
        // carries its spec for the dark compile.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("none")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert!(theme_config.suppress_bootstrap);
        assert!(theme_config.themes.is_empty());
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert!(!dark.suppress_bootstrap);
        assert_eq!(dark.themes.len(), 1);
        assert!(dark.themes[0].is_builtin());
    }

    #[test]
    fn test_theme_map_nested_map_in_dark_errors() {
        // The pair form is top-level only; a nested map inside `dark:`
        // is invalid, same as inside `light:`.
        let inner = map_value(vec![map_entry("dark", scalar_value("darkly"))]);
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", inner),
        ]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::InvalidThemeConfig { .. }) => {}
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_unknown_dark_theme_errors() {
        // The dark half goes through the same spec parsing as the
        // light half — unknown names error rather than being ignored.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("nosuchtheme")),
        ]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::UnknownTheme { name, .. }) => assert_eq!(name, "nosuchtheme"),
            other => panic!("expected UnknownTheme error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_dark_pandoc_inlines_scalar() {
        // Frontmatter path: pampa parses `dark: darkly` as
        // PandocInlines. The dark half must go through the same text
        // extraction as the light half.
        use quarto_pandoc_types::inline::{Inline, Str};
        let str_node = Inline::Str(Str {
            text: "darkly".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let dark_value = ConfigValue::new_inlines(vec![str_node], SourceInfo::for_test());
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", dark_value),
        ]);

        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert_eq!(dark.themes.len(), 1);
        assert!(dark.themes[0].is_builtin());
    }

    #[test]
    fn test_theme_map_dark_brand_token_without_brand_errors() {
        // Naming `brand` in the dark list without a `brand:` key is an
        // error, same as in the light list.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", array_value(&["darkly", "brand"])),
        ]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::InvalidThemeConfig { message, .. }) => {
                assert!(message.contains("brand"), "message: {message}");
            }
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_explicit_brand_token_position_in_dark() {
        // An explicit `brand` token in the dark list controls the
        // brand layers' position in the dark variant; the light list
        // (without a token) still gets the auto-injected marker at the
        // end.
        let theme_value = map_value(vec![
            map_entry("light", array_value(&["cosmo"])),
            map_entry("dark", array_value(&["brand", "darkly"])),
        ]);
        let config = map_value(vec![
            map_entry("theme", theme_value),
            map_entry("brand", scalar_value("_brand.yml")),
        ]);

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert!(theme_config.brand_ref.is_some());
        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_builtin());
        assert!(theme_config.themes[1].is_brand(), "auto-injected in light");
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert_eq!(dark.themes.len(), 2);
        assert!(dark.themes[0].is_brand(), "explicit token keeps position");
        assert!(dark.themes[1].is_builtin());
    }

    #[test]
    fn test_dark_variant_projects_standalone_config() {
        // `dark_variant()` projects the dark half into a standalone,
        // light-shaped ThemeConfig so the pure compile pipeline can
        // run unchanged for the dark variant.
        let theme_value = map_value(vec![
            map_entry("light", array_value(&["cosmo", "light.scss"])),
            map_entry("dark", array_value(&["darkly", "dark.scss"])),
        ]);
        let config = map_value(vec![
            map_entry("theme", theme_value),
            map_entry("brand", scalar_value("_brand.yml")),
        ]);
        let theme_config = ThemeConfig::from_config_value(&config).unwrap();

        let dark_cfg = theme_config
            .dark_variant()
            .expect("dark half present ⇒ dark variant config");
        // dark specs (incl. the auto-injected brand token) become the
        // top-level list of the projected config.
        assert_eq!(dark_cfg.themes.len(), 3);
        assert!(dark_cfg.themes[0].is_builtin());
        assert!(dark_cfg.themes[1].is_custom());
        assert!(dark_cfg.themes[2].is_brand());
        assert_eq!(dark_cfg.theme_locations.len(), 3);
        // Whole-config options carry over; the projection has no
        // nested dark half of its own.
        assert_eq!(dark_cfg.minified, theme_config.minified);
        assert_eq!(dark_cfg.title_block_layer, theme_config.title_block_layer);
        assert!(dark_cfg.brand_ref.is_some());
        assert!(dark_cfg.dark.is_none());
        assert!(!dark_cfg.suppress_bootstrap);

        // No dark half ⇒ no projection.
        let light_only =
            ThemeConfig::from_config_value(&config_with_theme_value(scalar_value("cosmo")))
                .unwrap();
        assert!(light_only.dark_variant().is_none());
    }

    #[test]
    fn test_dark_variant_carries_dark_none_suppression() {
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("none")),
        ]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();
        let dark_cfg = theme_config.dark_variant().unwrap();
        assert!(dark_cfg.suppress_bootstrap);
        assert!(dark_cfg.themes.is_empty());
    }

    #[test]
    fn test_ships_bootstrap_considers_both_variants() {
        // Bootstrap ships iff ANY variant ships it.
        let plain = ThemeConfig::from_config_value(&config_with_theme_value(scalar_value("cosmo")))
            .unwrap();
        assert!(plain.ships_bootstrap());

        let none =
            ThemeConfig::from_config_value(&config_with_theme_value(scalar_value("none"))).unwrap();
        assert!(!none.ships_bootstrap());

        // {light: none, dark: darkly}: the dark variant still needs
        // Bootstrap JS.
        let light_none_dark = map_value(vec![
            map_entry("light", scalar_value("none")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let cfg =
            ThemeConfig::from_config_value(&config_with_theme_value(light_none_dark)).unwrap();
        assert!(cfg.ships_bootstrap());

        // Both halves none → nothing ships.
        let both_none = map_value(vec![
            map_entry("light", scalar_value("none")),
            map_entry("dark", scalar_value("none")),
        ]);
        let cfg = ThemeConfig::from_config_value(&config_with_theme_value(both_none)).unwrap();
        assert!(!cfg.ships_bootstrap());
    }

    // === highlight-style tests (bd-0pic6 phase B) ===

    /// Root config `{ theme: <value>, highlight-style: <value> }`.
    fn config_with_theme_and_highlight(theme: ConfigValue, highlight: ConfigValue) -> ConfigValue {
        map_value(vec![
            map_entry("theme", theme),
            map_entry("highlight-style", highlight),
        ])
    }

    #[test]
    fn test_highlight_style_scalar_adaptive_resolves_per_variant() {
        // Q1's adaptive names: a scalar `a11y` resolves to the
        // variant-matching palette on each half of a theme pair.
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let cfg = ThemeConfig::from_config_value(&config_with_theme_and_highlight(
            theme,
            scalar_value("a11y"),
        ))
        .unwrap();

        assert_eq!(
            cfg.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-light")
        );
        let dark = cfg.dark.as_ref().unwrap();
        assert_eq!(
            dark.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-dark")
        );
    }

    #[test]
    fn test_highlight_style_map_form_per_half() {
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let highlight = map_value(vec![
            map_entry("light", scalar_value("a11y")),
            map_entry("dark", scalar_value("othername")),
        ]);
        let cfg =
            ThemeConfig::from_config_value(&config_with_theme_and_highlight(theme, highlight))
                .unwrap();

        // The light half of the map resolves adaptively for the light
        // variant; the dark half's raw (non-adaptive) name is carried
        // as-is (unknown names fall back at compile time + warn).
        assert_eq!(
            cfg.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-light")
        );
        assert_eq!(
            cfg.dark
                .as_ref()
                .unwrap()
                .highlight_style
                .as_ref()
                .map(|h| h.name.as_str()),
            Some("othername")
        );
    }

    #[test]
    fn test_highlight_style_single_dark_builtin_resolves_dark() {
        // No theme pair: the adaptive palette follows the built-in
        // theme's darkness (theme: darkly → a11y-dark), Q1's
        // sentinel-driven behavior approximated via
        // BuiltInTheme::is_dark.
        let cfg = ThemeConfig::from_config_value(&config_with_theme_and_highlight(
            scalar_value("darkly"),
            scalar_value("a11y"),
        ))
        .unwrap();
        assert_eq!(
            cfg.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-dark")
        );
        assert!(cfg.dark.is_none());

        let cfg = ThemeConfig::from_config_value(&config_with_theme_and_highlight(
            scalar_value("cosmo"),
            scalar_value("a11y"),
        ))
        .unwrap();
        assert_eq!(
            cfg.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-light")
        );
    }

    #[test]
    fn test_highlight_style_absent_is_none() {
        let cfg = ThemeConfig::from_config_value(&config_with_theme_value(scalar_value("cosmo")))
            .unwrap();
        assert!(cfg.highlight_style.is_none());
    }

    #[test]
    fn test_highlight_style_unknown_name_carried_with_location() {
        let cfg = ThemeConfig::from_config_value(&config_with_theme_and_highlight(
            scalar_value("cosmo"),
            scalar_value("nosuchstyle"),
        ))
        .unwrap();
        let hl = cfg.highlight_style.as_ref().expect("carried");
        assert_eq!(hl.name, "nosuchstyle");
        assert!(hl.location.is_some(), "location carried for diagnostics");
    }

    #[test]
    fn test_dark_variant_carries_highlight_style() {
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let cfg = ThemeConfig::from_config_value(&config_with_theme_and_highlight(
            theme,
            scalar_value("a11y"),
        ))
        .unwrap();
        let dark_cfg = cfg.dark_variant().unwrap();
        assert_eq!(
            dark_cfg.highlight_style.as_ref().map(|h| h.name.as_str()),
            Some("a11y-dark")
        );
    }

    // === brand light/dark seam tests (bd-0pic6 phase C) ===

    #[test]
    fn test_brand_pair_sets_per_variant_refs() {
        // `brand: {light: a.yml, dark: b.yml}` + a theme pair: each
        // variant carries its own BrandRef.
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let brand = map_value(vec![
            map_entry("light", scalar_value("brand-light.yml")),
            map_entry("dark", scalar_value("brand-dark.yml")),
        ]);
        let config = map_value(vec![map_entry("theme", theme), map_entry("brand", brand)]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();

        match cfg.brand_ref.as_ref() {
            Some(BrandRef::Path(p)) => assert_eq!(p.to_str(), Some("brand-light.yml")),
            other => panic!("light brand_ref: {:?}", other),
        }
        let dark = cfg.dark.as_ref().unwrap();
        match dark.brand_ref.as_ref() {
            Some(BrandRef::Path(p)) => assert_eq!(p.to_str(), Some("brand-dark.yml")),
            other => panic!("dark brand_ref: {:?}", other),
        }
        // Brand token auto-injected into both variants.
        assert!(cfg.themes.iter().any(ThemeSpec::is_brand));
        assert!(dark.themes.iter().any(ThemeSpec::is_brand));
    }

    #[test]
    fn test_single_brand_falls_back_to_light_for_dark_variant() {
        // A single `brand: a.yml` with a theme pair: the dark variant
        // uses the same brand (Q1's per-layer fallback — a bundle with
        // no dark opinion contributes its light layers to the dark
        // compile).
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let config = map_value(vec![
            map_entry("theme", theme),
            map_entry("brand", scalar_value("_brand.yml")),
        ]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();
        match cfg.dark.as_ref().unwrap().brand_ref.as_ref() {
            Some(BrandRef::Path(p)) => assert_eq!(p.to_str(), Some("_brand.yml")),
            other => panic!("dark brand_ref fallback: {:?}", other),
        }
    }

    #[test]
    fn test_brand_pair_synthesizes_dark_variant() {
        // A dark brand ENABLES dark mode even when `theme:` has no
        // dark half (Q1: `enablesDarkMode`): the dark variant is
        // synthesized from the light theme list + the dark brand.
        let config = map_value(vec![
            map_entry("theme", scalar_value("cosmo")),
            map_entry(
                "brand",
                map_value(vec![
                    map_entry("light", scalar_value("brand-light.yml")),
                    map_entry("dark", scalar_value("brand-dark.yml")),
                ]),
            ),
        ]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();

        let dark = cfg.dark.as_ref().expect("dark variant synthesized");
        // Same theme list as light (cosmo + auto-injected brand token).
        assert_eq!(dark.themes.len(), 2);
        assert!(dark.themes[0].is_builtin());
        assert!(dark.themes[1].is_brand());
        assert!(!dark.is_default, "light listed first in the brand map");
        match dark.brand_ref.as_ref() {
            Some(BrandRef::Path(p)) => assert_eq!(p.to_str(), Some("brand-dark.yml")),
            other => panic!("dark brand_ref: {:?}", other),
        }
    }

    #[test]
    fn test_brand_pair_dark_first_sets_default() {
        // Q1's fallback rule: when `theme:` doesn't decide the author
        // default, the `brand:` map's key order does.
        let config = map_value(vec![
            map_entry("theme", scalar_value("cosmo")),
            map_entry(
                "brand",
                map_value(vec![
                    map_entry("dark", scalar_value("brand-dark.yml")),
                    map_entry("light", scalar_value("brand-light.yml")),
                ]),
            ),
        ]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();
        assert!(cfg.dark.as_ref().unwrap().is_default);
    }

    #[test]
    fn test_theme_map_order_wins_over_brand_order() {
        // When BOTH maps exist, the theme map's key order decides the
        // author default (Q1 checks the theme map first).
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let brand = map_value(vec![
            map_entry("dark", scalar_value("brand-dark.yml")),
            map_entry("light", scalar_value("brand-light.yml")),
        ]);
        let config = map_value(vec![map_entry("theme", theme), map_entry("brand", brand)]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();
        assert!(
            !cfg.dark.as_ref().unwrap().is_default,
            "theme map wrote light first — brand order must not override"
        );
    }

    #[test]
    fn test_dark_only_brand_synthesizes_dark_default() {
        // `brand: {dark: b.yml}`: no light brand; the synthesized dark
        // variant carries the brand and is the author default (dark is
        // the map's first — only — key).
        let config = map_value(vec![
            map_entry("theme", scalar_value("cosmo")),
            map_entry(
                "brand",
                map_value(vec![map_entry("dark", scalar_value("brand-dark.yml"))]),
            ),
        ]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();

        assert!(cfg.brand_ref.is_none(), "light variant has no brand");
        assert!(
            !cfg.themes.iter().any(ThemeSpec::is_brand),
            "no token in the light list without a light brand"
        );
        let dark = cfg.dark.as_ref().expect("dark synthesized");
        assert!(dark.is_default);
        assert!(dark.brand_ref.is_some());
        assert!(dark.themes.iter().any(ThemeSpec::is_brand));
    }

    #[test]
    fn test_dark_variant_projection_carries_brand_ref() {
        let theme = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let brand = map_value(vec![
            map_entry("light", scalar_value("brand-light.yml")),
            map_entry("dark", scalar_value("brand-dark.yml")),
        ]);
        let config = map_value(vec![map_entry("theme", theme), map_entry("brand", brand)]);
        let cfg = ThemeConfig::from_config_value(&config).unwrap();
        let dark_cfg = cfg.dark_variant().unwrap();
        match dark_cfg.brand_ref.as_ref() {
            Some(BrandRef::Path(p)) => assert_eq!(p.to_str(), Some("brand-dark.yml")),
            other => panic!("projected dark brand_ref: {:?}", other),
        }
    }

    #[test]
    fn test_resolve_carries_dark_through() {
        // ThemeConfig::resolve must not drop the dark half — the
        // compile stage consumes the resolved form.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
        ]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();
        let runtime = quarto_system_runtime::NativeRuntime::new();
        let resolved = theme_config
            .resolve(&runtime, Path::new("."))
            .expect("resolve without brand does no I/O");
        let dark = resolved.dark.as_ref().expect("dark half carried through");
        assert_eq!(dark.themes.len(), 1);
        assert!(dark.themes[0].is_builtin());
    }

    #[test]
    fn test_theme_map_extra_keys_still_errors() {
        // A map with keys beyond light/dark is not the pair form —
        // it stays a Q-14-1 invalid-config error.
        let theme_value = map_value(vec![
            map_entry("light", scalar_value("cosmo")),
            map_entry("dark", scalar_value("darkly")),
            map_entry("contrast", scalar_value("high")),
        ]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::InvalidThemeConfig { message, .. }) => {
                assert!(message.contains("string or array"), "message: {message}");
            }
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_nested_map_in_light_errors() {
        // The pair form is top-level only; a nested map inside
        // `light:` is invalid.
        let inner = map_value(vec![map_entry("light", scalar_value("cosmo"))]);
        let theme_value = map_value(vec![map_entry("light", inner)]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::InvalidThemeConfig { .. }) => {}
            other => panic!("expected InvalidThemeConfig error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_light_none_suppresses_bootstrap() {
        // The `none` sentinel is honored inside the light half, same
        // as `theme: none` at top level.
        let theme_value = map_value(vec![map_entry("light", scalar_value("none"))]);
        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();

        assert!(theme_config.suppress_bootstrap);
        assert!(theme_config.themes.is_empty());
    }

    #[test]
    fn test_theme_map_unknown_light_theme_errors() {
        // The light half goes through the same spec parsing as a
        // top-level theme value — unknown names still error.
        let theme_value = map_value(vec![map_entry("light", scalar_value("nosuchtheme"))]);
        match ThemeConfig::from_config_value(&config_with_theme_value(theme_value)) {
            Err(SassError::UnknownTheme { name, .. }) => assert_eq!(name, "nosuchtheme"),
            other => panic!("expected UnknownTheme error, got: {:?}", other),
        }
    }

    #[test]
    fn test_theme_map_with_brand_auto_injects_into_both_halves() {
        // brand auto-inject composes with the map form: the Brand
        // token is appended to each variant's spec list (Q1 splices
        // brand into light and dark independently).
        let theme_value = map_value(vec![
            map_entry("light", array_value(&["cosmo"])),
            map_entry("dark", array_value(&["darkly"])),
        ]);
        let config = map_value(vec![
            map_entry("theme", theme_value),
            map_entry("brand", scalar_value("_brand.yml")),
        ]);

        let theme_config = ThemeConfig::from_config_value(&config).unwrap();
        assert_eq!(theme_config.themes.len(), 2);
        assert!(theme_config.themes[0].is_builtin());
        assert!(theme_config.themes[1].is_brand());
        assert!(theme_config.brand_ref.is_some());
        let dark = theme_config.dark.as_ref().expect("dark half parsed");
        assert_eq!(dark.themes.len(), 2);
        assert!(dark.themes[0].is_builtin());
        assert!(dark.themes[1].is_brand(), "auto-injected in dark too");
    }

    #[test]
    fn test_theme_map_light_pandoc_inlines_scalar() {
        // Frontmatter path: pampa parses `light: cosmo` as
        // PandocInlines, not a YAML scalar. The light half must go
        // through the same text extraction as top-level themes.
        use quarto_pandoc_types::inline::{Inline, Str};
        let str_node = Inline::Str(Str {
            text: "cosmo".to_string(),
            source_info: SourceInfo::for_test(),
        });
        let light_value = ConfigValue::new_inlines(vec![str_node], SourceInfo::for_test());
        let theme_value = map_value(vec![map_entry("light", light_value)]);

        let theme_config =
            ThemeConfig::from_config_value(&config_with_theme_value(theme_value)).unwrap();
        assert_eq!(theme_config.themes.len(), 1);
        assert!(theme_config.themes[0].is_builtin());
    }
}
