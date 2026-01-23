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
}
