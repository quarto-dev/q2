//! SASS layer parsing and merging.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module implements the layer boundary parsing logic from TypeScript Quarto.
//! Layer boundaries are special comments that organize SCSS by purpose:
//!
//! ```text
//! /\*-- scss:uses --*/
//! @use "sass:math";
//!
//! /\*-- scss:defaults --*/
//! $primary: blue !default;
//!
//! /\*-- scss:functions --*/
//! @function double($n) { @return $n * 2; }
//!
//! /\*-- scss:mixins --*/
//! @mixin center { display: flex; }
//!
//! /\*-- scss:rules --*/
//! .container { max-width: 1200px; }
//! ```

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::SassError;
use crate::types::SassLayer;

/// Regex pattern for layer boundary markers.
///
/// Matches lines like:
/// - `/*-- scss:uses --*/`
/// - `/*-- scss:defaults --*/`
/// - `/*-- scss:functions --*/`
/// - `/*-- scss:mixins --*/`
/// - `/*-- scss:rules --*/`
///
/// NOTE: This intentionally matches TS Quarto's regex which does NOT allow
/// space after the colon (e.g., `/*-- scss: functions --*/` won't match).
/// This means files like title-block.scss which use non-standard markers
/// will have their content parsed into the default section until a valid
/// marker is found.
///
/// The pattern allows optional whitespace (spaces/tabs) before and after
/// the layer name, but NOT after the colon.
/// Captures the layer type in group 1.
static LAYER_BOUNDARY_LINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^/\*--[ \t]*scss:(uses|functions|rules|defaults|mixins)[ \t]*--\*/$").unwrap()
});

/// Regex for testing if content contains any boundary marker (multiline).
static LAYER_BOUNDARY_TEST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^/\*--[ \t]*scss:(uses|functions|rules|defaults|mixins)[ \t]*--\*/$").unwrap()
});

/// The five layer types supported by Quarto SCSS
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerType {
    Uses,
    Defaults,
    Functions,
    Mixins,
    Rules,
}

/// Parse a SCSS string into a SassLayer by extracting content between boundary markers.
///
/// # Arguments
///
/// * `content` - The raw SCSS content to parse
/// * `hint` - Optional hint for error messages (e.g., file path)
///
/// # Returns
///
/// A `SassLayer` with content organized by section, or an error if no boundary
/// markers are found.
///
/// # Behavior
///
/// - Content before any boundary marker goes into `defaults` (the implicit first layer)
/// - Each boundary marker switches the accumulator to that layer type
/// - Boundary marker lines themselves are NOT included in the output
/// - Multiple occurrences of the same layer type are concatenated
///
/// # Example
///
/// ```
/// use quarto_sass::parse_layer;
///
/// let scss = r#"
/// /*-- scss:defaults --*/
/// $primary: blue !default;
///
/// /*-- scss:rules --*/
/// .container { color: $primary; }
/// "#;
///
/// let layer = parse_layer(scss, Some("theme.scss")).unwrap();
/// assert!(layer.defaults.contains("$primary"));
/// assert!(layer.rules.contains(".container"));
/// ```
pub fn parse_layer(content: &str, hint: Option<&str>) -> Result<SassLayer, SassError> {
    // Verify that at least one boundary marker exists
    if !LAYER_BOUNDARY_TEST.is_match(content) {
        return Err(SassError::NoBoundaryMarkers {
            hint: hint.map(String::from),
        });
    }

    let mut uses: Vec<&str> = Vec::new();
    let mut defaults: Vec<&str> = Vec::new();
    let mut functions: Vec<&str> = Vec::new();
    let mut mixins: Vec<&str> = Vec::new();
    let mut rules: Vec<&str> = Vec::new();

    // Current accumulator - defaults to Defaults (content before first marker goes here)
    let mut current_layer = LayerType::Defaults;

    for line in content.lines() {
        if let Some(captures) = LAYER_BOUNDARY_LINE.captures(line) {
            // This is a boundary marker - switch the accumulator
            let layer_name = captures.get(1).unwrap().as_str();
            current_layer = match layer_name {
                "uses" => LayerType::Uses,
                "defaults" => LayerType::Defaults,
                "functions" => LayerType::Functions,
                "mixins" => LayerType::Mixins,
                "rules" => LayerType::Rules,
                _ => unreachable!("Regex only matches known layer types"),
            };
        } else {
            // Not a boundary marker - add to current accumulator
            match current_layer {
                LayerType::Uses => uses.push(line),
                LayerType::Defaults => defaults.push(line),
                LayerType::Functions => functions.push(line),
                LayerType::Mixins => mixins.push(line),
                LayerType::Rules => rules.push(line),
            }
        }
    }

    Ok(SassLayer {
        uses: uses.join("\n"),
        defaults: defaults.join("\n"),
        functions: functions.join("\n"),
        mixins: mixins.join("\n"),
        rules: rules.join("\n"),
    })
}

/// Create a SassLayer directly from individual section strings.
///
/// This is useful when loading layers from a directory structure where
/// each section is a separate file (e.g., `_defaults.scss`, `_rules.scss`).
///
/// # Arguments
///
/// * `uses` - Content for @use imports
/// * `defaults` - Content for variable defaults
/// * `functions` - Content for function definitions
/// * `mixins` - Content for mixin definitions
/// * `rules` - Content for CSS rules
///
/// # Example
///
/// ```
/// use quarto_sass::parse_layer_from_parts;
///
/// let layer = parse_layer_from_parts(
///     "",
///     "$primary: blue !default;",
///     "",
///     "",
///     ".btn { color: $primary; }",
/// );
/// assert!(layer.defaults.contains("$primary"));
/// assert!(layer.rules.contains(".btn"));
/// ```
pub fn parse_layer_from_parts(
    uses: impl Into<String>,
    defaults: impl Into<String>,
    functions: impl Into<String>,
    mixins: impl Into<String>,
    rules: impl Into<String>,
) -> SassLayer {
    SassLayer {
        uses: uses.into(),
        defaults: defaults.into(),
        functions: functions.into(),
        mixins: mixins.into(),
        rules: rules.into(),
    }
}

/// Merge multiple SassLayers into one.
///
/// # Merging Rules
///
/// - `uses`: Concatenated in order (first layer first)
/// - `defaults`: **Reversed** order (last layer first) - see note below
/// - `functions`: Concatenated in order
/// - `mixins`: Concatenated in order
/// - `rules`: Concatenated in order
///
/// # Why Defaults are Reversed
///
/// SASS `!default` means "only set if not already set" - the first definition wins.
/// When merging layers where earlier layers should have lower precedence (e.g.,
/// framework < quarto < user), we reverse the defaults so that higher-precedence
/// layers appear first in the output.
///
/// For example, if we merge `[framework, quarto, user]`:
/// - Uses output: framework + quarto + user
/// - Defaults output: user + quarto + framework (reversed!)
/// - Rules output: framework + quarto + user
///
/// This ensures user defaults override quarto defaults which override framework defaults.
///
/// # Example
///
/// ```
/// use quarto_sass::{merge_layers, SassLayer};
///
/// let framework = SassLayer {
///     defaults: "$primary: blue !default;".to_string(),
///     rules: ".framework { }".to_string(),
///     ..Default::default()
/// };
///
/// let user = SassLayer {
///     defaults: "$primary: red !default;".to_string(),
///     rules: ".user { }".to_string(),
///     ..Default::default()
/// };
///
/// let merged = merge_layers(&[framework, user]);
///
/// // Defaults are reversed: user comes first, so $primary will be red
/// assert!(merged.defaults.starts_with("$primary: red"));
///
/// // Rules are in order: framework first, then user
/// assert!(merged.rules.contains(".framework"));
/// assert!(merged.rules.contains(".user"));
/// ```
pub fn merge_layers(layers: &[SassLayer]) -> SassLayer {
    let mut uses: Vec<&str> = Vec::new();
    let mut defaults: Vec<&str> = Vec::new();
    let mut functions: Vec<&str> = Vec::new();
    let mut mixins: Vec<&str> = Vec::new();
    let mut rules: Vec<&str> = Vec::new();

    for layer in layers {
        if !layer.uses.is_empty() {
            uses.push(&layer.uses);
        }
        if !layer.defaults.is_empty() {
            defaults.push(&layer.defaults);
        }
        if !layer.functions.is_empty() {
            functions.push(&layer.functions);
        }
        if !layer.mixins.is_empty() {
            mixins.push(&layer.mixins);
        }
        if !layer.rules.is_empty() {
            rules.push(&layer.rules);
        }
    }

    // Reverse defaults order - first layer in input becomes last in output
    // This is because SASS !default means "only set if not already set"
    defaults.reverse();

    SassLayer {
        uses: uses.join("\n"),
        defaults: defaults.join("\n"),
        functions: functions.join("\n"),
        mixins: mixins.join("\n"),
        rules: rules.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_layer() {
        let scss = r#"/*-- scss:defaults --*/
$primary: blue !default;

/*-- scss:rules --*/
.container { color: $primary; }
"#;

        let layer = parse_layer(scss, None).unwrap();
        assert!(layer.defaults.contains("$primary: blue !default;"));
        assert!(layer.rules.contains(".container"));
        assert!(layer.uses.is_empty());
        assert!(layer.functions.is_empty());
        assert!(layer.mixins.is_empty());
    }

    #[test]
    fn test_parse_all_sections() {
        let scss = r#"/*-- scss:uses --*/
@use "sass:math";

/*-- scss:defaults --*/
$primary: blue !default;

/*-- scss:functions --*/
@function double($n) { @return $n * 2; }

/*-- scss:mixins --*/
@mixin center { display: flex; justify-content: center; }

/*-- scss:rules --*/
.container { max-width: 1200px; }
"#;

        let layer = parse_layer(scss, None).unwrap();
        assert!(layer.uses.contains("@use \"sass:math\""));
        assert!(layer.defaults.contains("$primary: blue !default"));
        assert!(layer.functions.contains("@function double"));
        assert!(layer.mixins.contains("@mixin center"));
        assert!(layer.rules.contains(".container"));
    }

    #[test]
    fn test_parse_content_before_first_marker_goes_to_defaults() {
        let scss = r#"// This comment is before any marker
$early-var: 123;

/*-- scss:defaults --*/
$primary: blue !default;

/*-- scss:rules --*/
.container { }
"#;

        let layer = parse_layer(scss, None).unwrap();
        // Content before first marker goes to defaults
        assert!(layer.defaults.contains("$early-var: 123"));
        assert!(layer.defaults.contains("$primary: blue !default"));
    }

    #[test]
    fn test_parse_no_boundary_markers_error() {
        let scss = r#"
$primary: blue;
.container { color: $primary; }
"#;

        let result = parse_layer(scss, Some("theme.scss"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("theme.scss"));
        assert!(err.to_string().contains("boundary"));
    }

    #[test]
    fn test_parse_whitespace_in_boundary() {
        // TS Quarto allows spaces/tabs around the layer name
        let scss = r#"/*--  scss:defaults  --*/
$primary: blue !default;
"#;

        let layer = parse_layer(scss, None).unwrap();
        assert!(layer.defaults.contains("$primary: blue !default"));
    }

    #[test]
    fn test_parse_multiple_same_layer_concatenated() {
        let scss = r#"/*-- scss:defaults --*/
$first: 1;

/*-- scss:rules --*/
.rule1 { }

/*-- scss:defaults --*/
$second: 2;

/*-- scss:rules --*/
.rule2 { }
"#;

        let layer = parse_layer(scss, None).unwrap();
        assert!(layer.defaults.contains("$first: 1"));
        assert!(layer.defaults.contains("$second: 2"));
        assert!(layer.rules.contains(".rule1"));
        assert!(layer.rules.contains(".rule2"));
    }

    #[test]
    fn test_merge_layers_defaults_reversed() {
        let framework = SassLayer {
            defaults: "$primary: framework-blue !default;".to_string(),
            rules: ".framework { }".to_string(),
            ..Default::default()
        };

        let quarto = SassLayer {
            defaults: "$primary: quarto-blue !default;".to_string(),
            rules: ".quarto { }".to_string(),
            ..Default::default()
        };

        let user = SassLayer {
            defaults: "$primary: user-blue !default;".to_string(),
            rules: ".user { }".to_string(),
            ..Default::default()
        };

        let merged = merge_layers(&[framework, quarto, user]);

        // Defaults should be reversed: user, quarto, framework
        let defaults_lines: Vec<&str> = merged.defaults.lines().collect();
        assert!(defaults_lines[0].contains("user-blue"));
        assert!(defaults_lines[1].contains("quarto-blue"));
        assert!(defaults_lines[2].contains("framework-blue"));

        // Rules should be in order: framework, quarto, user
        let rules_lines: Vec<&str> = merged.rules.lines().collect();
        assert!(rules_lines[0].contains(".framework"));
        assert!(rules_lines[1].contains(".quarto"));
        assert!(rules_lines[2].contains(".user"));
    }

    #[test]
    fn test_merge_layers_empty_sections_skipped() {
        let layer1 = SassLayer {
            defaults: "$a: 1;".to_string(),
            ..Default::default()
        };

        let layer2 = SassLayer {
            defaults: "".to_string(), // Empty - should be skipped
            rules: ".b { }".to_string(),
            ..Default::default()
        };

        let layer3 = SassLayer {
            defaults: "$c: 3;".to_string(),
            ..Default::default()
        };

        let merged = merge_layers(&[layer1, layer2, layer3]);

        // Defaults: layer3, layer1 (layer2 skipped because empty)
        // This means there should be exactly one newline between them
        assert_eq!(merged.defaults, "$c: 3;\n$a: 1;");

        // Rules: only layer2
        assert_eq!(merged.rules, ".b { }");
    }

    #[test]
    fn test_merge_empty_layers() {
        let merged = merge_layers(&[]);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_parse_layer_from_parts() {
        let layer = parse_layer_from_parts(
            "@use 'sass:math';",
            "$primary: blue;",
            "@function f() {}",
            "@mixin m() {}",
            ".rule {}",
        );

        assert_eq!(layer.uses, "@use 'sass:math';");
        assert_eq!(layer.defaults, "$primary: blue;");
        assert_eq!(layer.functions, "@function f() {}");
        assert_eq!(layer.mixins, "@mixin m() {}");
        assert_eq!(layer.rules, ".rule {}");
    }

    #[test]
    fn test_boundary_marker_not_included_in_output() {
        let scss = r#"/*-- scss:defaults --*/
$primary: blue;
/*-- scss:rules --*/
.container { }
"#;

        let layer = parse_layer(scss, None).unwrap();

        // Boundary markers should NOT appear in output
        assert!(!layer.defaults.contains("/*--"));
        assert!(!layer.defaults.contains("--*/"));
        assert!(!layer.rules.contains("/*--"));
        assert!(!layer.rules.contains("--*/"));
    }
}
