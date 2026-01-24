//! Core SASS types matching TypeScript Quarto's SASS architecture.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! The type hierarchy is:
//! - SassLayer: Smallest unit, organizes SCSS by purpose (uses, defaults, functions, mixins, rules)
//! - SassBundleLayers: Groups layers by audience (framework, quarto, user) with load paths
//! - SassBundle: Complete bundle with metadata (dependency, dark mode, HTML attributes)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A single SASS layer with organized sections.
///
/// Each section corresponds to a layer boundary marker in SCSS files:
/// - `/*-- scss:uses --*/` → uses
/// - `/*-- scss:defaults --*/` → defaults
/// - `/*-- scss:functions --*/` → functions
/// - `/*-- scss:mixins --*/` → mixins
/// - `/*-- scss:rules --*/` → rules
///
/// When compiling SCSS, sections are ordered as:
/// 1. uses (framework → quarto → user)
/// 2. functions (framework → quarto → user)
/// 3. defaults (user → quarto.reverse() → framework.reverse())
/// 4. mixins (framework → quarto → user)
/// 5. rules (framework → quarto → user)
///
/// Note: Only defaults are reversed because SASS `!default` means
/// "only set if not already set" - first definition wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SassLayer {
    /// @use imports (e.g., `@use "sass:math"`)
    pub uses: String,

    /// SASS variable defaults (with `!default` flag)
    pub defaults: String,

    /// SASS function definitions
    pub functions: String,

    /// SASS mixin definitions
    pub mixins: String,

    /// CSS/SASS rules
    pub rules: String,
}

impl SassLayer {
    /// Create a new empty layer
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all sections are empty
    pub fn is_empty(&self) -> bool {
        self.uses.is_empty()
            && self.defaults.is_empty()
            && self.functions.is_empty()
            && self.mixins.is_empty()
            && self.rules.is_empty()
    }

    /// Check if any section has content
    pub fn has_content(&self) -> bool {
        !self.is_empty()
    }
}

/// Bundle of layers organized by audience.
///
/// The layers represent different sources of SCSS:
/// - `framework`: Bootstrap, Reveal.js, or other framework SCSS
/// - `quarto`: Quarto's built-in SCSS
/// - `user`: User-provided customizations (can be multiple layers)
///
/// The `load_paths` specify directories to search for @use/@import resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SassBundleLayers {
    /// Unique identifier for this bundle (used for caching)
    pub key: String,

    /// Framework layer (Bootstrap, Reveal.js, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<SassLayer>,

    /// Quarto's built-in layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarto: Option<SassLayer>,

    /// User customization layers (multiple allowed)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user: Vec<SassLayer>,

    /// Paths to search for @use/@import resolution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_paths: Vec<PathBuf>,
}

/// Dark mode variant layers.
///
/// Used when a document has both light and dark themes.
/// The `default` flag indicates whether dark mode is the default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SassBundleDark {
    /// Framework dark mode layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<SassLayer>,

    /// Quarto dark mode layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarto: Option<SassLayer>,

    /// User dark mode layers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user: Vec<SassLayer>,

    /// Whether dark mode is the default
    #[serde(default)]
    pub default: bool,
}

/// Complete SASS bundle with metadata.
///
/// This is the top-level type used for SASS compilation.
/// It includes:
/// - All layers from `SassBundleLayers`
/// - Dependency information (which framework is being used)
/// - Optional dark mode variant
/// - HTML attributes to apply to the compiled CSS link
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SassBundle {
    /// Unique identifier for this bundle
    pub key: String,

    /// Framework layer (Bootstrap, Reveal.js, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<SassLayer>,

    /// Quarto's built-in layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarto: Option<SassLayer>,

    /// User customization layers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user: Vec<SassLayer>,

    /// Paths to search for @use/@import resolution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_paths: Vec<PathBuf>,

    /// Which framework this bundle depends on (e.g., "bootstrap")
    pub dependency: String,

    /// Dark mode variant layers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<SassBundleDark>,

    /// HTML attributes for the compiled CSS (e.g., {"data-theme": "custom"})
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attribs: HashMap<String, String>,
}

impl SassBundle {
    /// Create a new bundle with a key and dependency
    pub fn new(key: impl Into<String>, dependency: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            dependency: dependency.into(),
            ..Default::default()
        }
    }

    /// Convert to SassBundleLayers (loses dependency, dark, and attribs)
    pub fn into_layers(self) -> SassBundleLayers {
        SassBundleLayers {
            key: self.key,
            framework: self.framework,
            quarto: self.quarto,
            user: self.user,
            load_paths: self.load_paths,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sass_layer_default_is_empty() {
        let layer = SassLayer::default();
        assert!(layer.is_empty());
        assert!(!layer.has_content());
    }

    #[test]
    fn test_sass_layer_with_content() {
        let layer = SassLayer {
            defaults: "$color: red;".to_string(),
            ..Default::default()
        };
        assert!(!layer.is_empty());
        assert!(layer.has_content());
    }

    #[test]
    fn test_sass_bundle_new() {
        let bundle = SassBundle::new("my-bundle", "bootstrap");
        assert_eq!(bundle.key, "my-bundle");
        assert_eq!(bundle.dependency, "bootstrap");
        assert!(bundle.framework.is_none());
        assert!(bundle.user.is_empty());
    }

    #[test]
    fn test_sass_layer_serde_roundtrip() {
        let layer = SassLayer {
            uses: "@use 'sass:math';".to_string(),
            defaults: "$primary: blue !default;".to_string(),
            functions: "@function double($n) { @return $n * 2; }".to_string(),
            mixins: "@mixin center { display: flex; }".to_string(),
            rules: ".container { max-width: 1200px; }".to_string(),
        };

        let json = serde_json::to_string(&layer).unwrap();
        let parsed: SassLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(layer, parsed);
    }

    #[test]
    fn test_sass_layer_new() {
        let layer = SassLayer::new();
        assert!(layer.is_empty());
        assert_eq!(layer.uses, "");
        assert_eq!(layer.defaults, "");
        assert_eq!(layer.functions, "");
        assert_eq!(layer.mixins, "");
        assert_eq!(layer.rules, "");
    }

    #[test]
    fn test_sass_bundle_into_layers() {
        let bundle = SassBundle {
            key: "test-bundle".to_string(),
            dependency: "bootstrap".to_string(),
            framework: Some(SassLayer {
                defaults: "$fw: 1;".to_string(),
                ..Default::default()
            }),
            quarto: Some(SassLayer {
                defaults: "$q: 2;".to_string(),
                ..Default::default()
            }),
            user: vec![SassLayer {
                rules: ".user { color: red; }".to_string(),
                ..Default::default()
            }],
            load_paths: vec![PathBuf::from("/path/to/scss")],
            dark: Some(SassBundleDark::default()),
            attribs: HashMap::from([("data-theme".to_string(), "custom".to_string())]),
        };

        let layers = bundle.into_layers();

        assert_eq!(layers.key, "test-bundle");
        assert!(layers.framework.is_some());
        assert!(layers.quarto.is_some());
        assert_eq!(layers.user.len(), 1);
        assert_eq!(layers.load_paths.len(), 1);
        // Note: dependency, dark, and attribs are lost in conversion
    }

    #[test]
    fn test_sass_bundle_layers_serde_roundtrip() {
        let layers = SassBundleLayers {
            key: "layers-test".to_string(),
            framework: Some(SassLayer {
                functions: "@function fw() { @return 1; }".to_string(),
                ..Default::default()
            }),
            quarto: None,
            user: vec![
                SassLayer {
                    defaults: "$user1: 1;".to_string(),
                    ..Default::default()
                },
                SassLayer {
                    defaults: "$user2: 2;".to_string(),
                    ..Default::default()
                },
            ],
            load_paths: vec![PathBuf::from("/scss"), PathBuf::from("/bootstrap")],
        };

        let json = serde_json::to_string(&layers).unwrap();
        let parsed: SassBundleLayers = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.key, "layers-test");
        assert!(parsed.framework.is_some());
        assert!(parsed.quarto.is_none());
        assert_eq!(parsed.user.len(), 2);
        assert_eq!(parsed.load_paths.len(), 2);
    }

    #[test]
    fn test_sass_bundle_dark_serde_roundtrip() {
        let dark = SassBundleDark {
            framework: Some(SassLayer {
                defaults: "$dark-bg: #222;".to_string(),
                ..Default::default()
            }),
            quarto: None,
            user: vec![SassLayer {
                rules: ".dark { background: black; }".to_string(),
                ..Default::default()
            }],
            default: true,
        };

        let json = serde_json::to_string(&dark).unwrap();
        let parsed: SassBundleDark = serde_json::from_str(&json).unwrap();

        assert!(parsed.framework.is_some());
        assert!(parsed.quarto.is_none());
        assert_eq!(parsed.user.len(), 1);
        assert!(parsed.default);
    }

    #[test]
    fn test_sass_bundle_full_serde_roundtrip() {
        let bundle = SassBundle {
            key: "full-bundle".to_string(),
            dependency: "bootstrap".to_string(),
            framework: Some(SassLayer {
                uses: "@use 'sass:color';".to_string(),
                defaults: "$primary: blue;".to_string(),
                functions: "@function f() { @return 1; }".to_string(),
                mixins: "@mixin m() { color: red; }".to_string(),
                rules: ".fw { display: block; }".to_string(),
            }),
            quarto: Some(SassLayer {
                defaults: "$quarto-var: 1;".to_string(),
                ..Default::default()
            }),
            user: vec![SassLayer {
                rules: ".custom { margin: 0; }".to_string(),
                ..Default::default()
            }],
            load_paths: vec![PathBuf::from("/scss")],
            dark: Some(SassBundleDark {
                framework: None,
                quarto: None,
                user: vec![],
                default: false,
            }),
            attribs: HashMap::from([
                ("data-theme".to_string(), "custom".to_string()),
                ("id".to_string(), "main-styles".to_string()),
            ]),
        };

        let json = serde_json::to_string_pretty(&bundle).unwrap();
        let parsed: SassBundle = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.key, "full-bundle");
        assert_eq!(parsed.dependency, "bootstrap");
        assert!(parsed.framework.is_some());
        assert!(parsed.quarto.is_some());
        assert_eq!(parsed.user.len(), 1);
        assert_eq!(parsed.load_paths.len(), 1);
        assert!(parsed.dark.is_some());
        assert_eq!(parsed.attribs.len(), 2);
    }

    #[test]
    fn test_serde_skip_empty_fields() {
        // Empty bundle should serialize without optional fields
        let bundle = SassBundle::new("minimal", "bootstrap");
        let json = serde_json::to_string(&bundle).unwrap();

        // These fields should be absent due to skip_serializing_if
        assert!(!json.contains("framework"));
        assert!(!json.contains("quarto"));
        assert!(!json.contains("user"));
        assert!(!json.contains("load_paths"));
        assert!(!json.contains("dark"));
        assert!(!json.contains("attribs"));

        // These required fields should be present
        assert!(json.contains("key"));
        assert!(json.contains("dependency"));
    }

    #[test]
    fn test_sass_bundle_layers_default() {
        let layers = SassBundleLayers::default();
        assert_eq!(layers.key, "");
        assert!(layers.framework.is_none());
        assert!(layers.quarto.is_none());
        assert!(layers.user.is_empty());
        assert!(layers.load_paths.is_empty());
    }

    #[test]
    fn test_sass_bundle_dark_default() {
        let dark = SassBundleDark::default();
        assert!(dark.framework.is_none());
        assert!(dark.quarto.is_none());
        assert!(dark.user.is_empty());
        assert!(!dark.default);
    }
}
