//! Core SASS types matching TypeScript Quarto's SASS architecture.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! The central type is [`SassLayer`]: the smallest unit, organizing
//! SCSS by purpose (uses, defaults, functions, mixins, rules). The
//! pipeline composes `Vec<SassLayer>` directly (see `bundle.rs`).
//!
//! (The TS-architecture `SassBundle`/`SassBundleDark` wrapper types
//! were ported early but never wired — the light/dark epic (bd-0pic6)
//! delivered dark variants through per-variant `ThemeConfig`s
//! instead, and the dead scaffolding was removed in its phase E.)

use serde::{Deserialize, Serialize};

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
}
