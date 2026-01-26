//! SASS bundle assembly for compilation.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module implements the layer assembly logic from TypeScript Quarto.
//! The key insight is that SCSS must be assembled in a specific order:
//!
//! 1. USES: framework → quarto → user
//! 2. FUNCTIONS: framework → quarto → user (Bootstrap functions come first)
//! 3. DEFAULTS: user → quarto → framework (reversed for `!default` semantics)
//! 4. MIXINS: framework → quarto → user
//! 5. RULES: framework → quarto → user
//!
//! This order ensures:
//! - Bootstrap functions like `color-contrast()` are available before theme defaults
//! - User defaults take precedence over framework defaults (via `!default`)
//! - All rules can use all variables and mixins

use std::path::Path;

use crate::error::SassError;
use crate::layer::parse_layer;
use crate::resources::{
    BOOTSTRAP_RESOURCES, QUARTO_BOOTSTRAP_RESOURCES, SASS_UTILS_RESOURCES, THEMES_RESOURCES,
};
use crate::themes::{BuiltInTheme, ThemeContext, ThemeSpec};
use crate::types::SassLayer;

/// Quarto's SASS module imports.
///
/// These create Quarto-namespaced aliases for standard SASS modules.
/// Required for Quarto's Bootstrap functions and rules that use:
/// - `quarto-color.red()`, `quarto-color.blackness()`, etc.
/// - `quarto-map.get()`, `quarto-map.merge()`, etc.
/// - `quarto-math.div()`, `quarto-math.min()`, etc.
const QUARTO_USES: &str = r#"@use "sass:color" as quarto-color;
@use "sass:map" as quarto-map;
@use "sass:math" as quarto-math;
"#;

/// Bootstrap framework layer.
///
/// Contains Bootstrap's core SCSS split into layer sections:
/// - functions: `_functions.scss` + `sass-utils/color-contrast.scss`
/// - defaults: `_variables.scss`
/// - mixins: `_mixins.scss`
/// - rules: `bootstrap.scss`
pub fn load_bootstrap_framework() -> Result<SassLayer, SassError> {
    // Bootstrap functions
    let functions_content = BOOTSTRAP_RESOURCES
        .read_str(Path::new("_functions.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "Bootstrap _functions.scss not found".to_string(),
        })?;

    // Self-contained color-contrast function (critical for theme compatibility)
    let color_contrast_content = SASS_UTILS_RESOURCES
        .read_str(Path::new("color-contrast.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "sass-utils/color-contrast.scss not found".to_string(),
        })?;

    // Bootstrap variables
    let variables_content = BOOTSTRAP_RESOURCES
        .read_str(Path::new("_variables.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "Bootstrap _variables.scss not found".to_string(),
        })?;

    // Bootstrap mixins
    let mixins_content = BOOTSTRAP_RESOURCES
        .read_str(Path::new("_mixins.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "Bootstrap _mixins.scss not found".to_string(),
        })?;

    // Bootstrap rules (main entry point)
    let rules_content = BOOTSTRAP_RESOURCES
        .read_str(Path::new("bootstrap.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "Bootstrap bootstrap.scss not found".to_string(),
        })?;

    Ok(SassLayer {
        uses: String::new(),
        // Functions: Bootstrap functions + self-contained color-contrast
        functions: format!("{}\n\n{}", functions_content, color_contrast_content),
        defaults: variables_content.to_string(),
        mixins: mixins_content.to_string(),
        rules: rules_content.to_string(),
    })
}

/// Quarto Bootstrap customization layer.
///
/// Contains Quarto's Bootstrap customizations:
/// - uses: SASS module imports with Quarto namespaces
/// - defaults: `_bootstrap-customize.scss` + `_bootstrap-variables.scss`
/// - functions: `_bootstrap-functions.scss` (theme-contrast, etc.)
/// - mixins: `_bootstrap-mixins.scss`
/// - rules: `_bootstrap-rules.scss`
pub fn load_quarto_layer() -> Result<SassLayer, SassError> {
    // Quarto customization defaults (has boundary markers)
    let customize_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-customize.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-customize.scss not found".to_string(),
        })?;

    // Parse the customize layer (it has boundary markers)
    let customize_layer = parse_layer(customize_content, Some("_bootstrap-customize.scss"))?;

    // Quarto variables (no boundary markers, just variables)
    let variables_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-variables.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-variables.scss not found".to_string(),
        })?;

    // Quarto functions (no boundary markers, just functions)
    let functions_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-functions.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-functions.scss not found".to_string(),
        })?;

    // Quarto mixins
    let mixins_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-mixins.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-mixins.scss not found".to_string(),
        })?;

    // Quarto rules
    let rules_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-rules.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-rules.scss not found".to_string(),
        })?;

    // Combine defaults: customize layer defaults + Quarto variables
    let combined_defaults = format!("{}\n\n{}", customize_layer.defaults, variables_content);

    Ok(SassLayer {
        uses: QUARTO_USES.to_string(),
        functions: functions_content.to_string(),
        defaults: combined_defaults,
        mixins: mixins_content.to_string(),
        rules: rules_content.to_string(),
    })
}

/// Load a built-in theme layer.
///
/// Parses the theme's SCSS file to extract layer sections.
pub fn load_theme(theme: BuiltInTheme) -> Result<SassLayer, SassError> {
    let filename = theme.filename();
    let content = THEMES_RESOURCES
        .read_str(Path::new(&filename))
        .ok_or_else(|| SassError::ThemeNotFound(theme.name().to_string()))?;

    parse_layer(content, Some(&filename))
}

/// Load the title block SCSS layer.
///
/// This loads `title-block.scss` from the templates directory and parses it
/// into a `SassLayer`. The title block layer provides styling for:
/// - `.quarto-title-meta` - metadata grid layout
/// - `.quarto-title-meta-heading` - metadata labels
/// - `#title-block-header.quarto-title-block` - overall title block layout
/// - `.abstract`, `.description`, `.keywords` - special sections
///
/// In TS Quarto, this layer is added as a "user" layer that comes after the
/// quarto layer but before any custom user SCSS. This ensures the title block
/// styles can use Bootstrap variables and mixins while still being overridable.
///
/// # Known Issue
///
/// The `title-block.scss` file uses non-standard layer boundary markers that
/// don't match TS Quarto's regex. Specifically:
/// - `/*-- scss: functions --*/` has a space after the colon (not recognized)
/// - `/*-- scss:variables --*/` uses "variables" which isn't a valid layer name
///
/// This means the functions and variables in the file end up in the `defaults`
/// section rather than their intended sections. We match this behavior for
/// parity with TS Quarto.
///
/// See: <https://github.com/quarto-dev/quarto-cli/issues/13960>
///
/// # Returns
///
/// A `SassLayer` with the title block SCSS organized by section.
///
/// # Errors
///
/// Returns an error if `title-block.scss` cannot be found or parsed.
pub fn load_title_block_layer() -> Result<SassLayer, SassError> {
    use crate::resources::TEMPLATES_RESOURCES;

    let content = TEMPLATES_RESOURCES
        .read_str(Path::new("title-block.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "title-block.scss not found in templates resources".to_string(),
        })?;

    parse_layer(content, Some("title-block.scss"))
}

/// Assemble a complete SCSS string for compilation.
///
/// This function implements the correct assembly order from TypeScript Quarto:
/// 1. Functions: framework → quarto → theme
/// 2. Defaults: theme → quarto → framework (reversed!)
/// 3. Mixins: framework → quarto → theme
/// 4. Rules: framework → quarto → theme
///
/// # Arguments
///
/// * `framework` - Bootstrap framework layer
/// * `quarto` - Quarto customization layer
/// * `theme` - Optional theme layer (Bootswatch or custom)
///
/// # Returns
///
/// A complete SCSS string ready for compilation.
pub fn assemble_scss(
    framework: &SassLayer,
    quarto: &SassLayer,
    theme: Option<&SassLayer>,
) -> String {
    let mut parts: Vec<&str> = Vec::new();

    // 1. USES (framework → quarto → theme)
    if !framework.uses.is_empty() {
        parts.push(&framework.uses);
    }
    if !quarto.uses.is_empty() {
        parts.push(&quarto.uses);
    }
    if let Some(t) = theme {
        if !t.uses.is_empty() {
            parts.push(&t.uses);
        }
    }

    // 2. FUNCTIONS (framework → quarto → theme)
    // Framework functions come FIRST so they're available to theme defaults
    if !framework.functions.is_empty() {
        parts.push(&framework.functions);
    }
    if !quarto.functions.is_empty() {
        parts.push(&quarto.functions);
    }
    if let Some(t) = theme {
        if !t.functions.is_empty() {
            parts.push(&t.functions);
        }
    }

    // 3. DEFAULTS (theme → quarto → framework - REVERSED!)
    // Theme defaults come FIRST so they take precedence via !default
    if let Some(t) = theme {
        if !t.defaults.is_empty() {
            parts.push(&t.defaults);
        }
    }
    if !quarto.defaults.is_empty() {
        parts.push(&quarto.defaults);
    }
    if !framework.defaults.is_empty() {
        parts.push(&framework.defaults);
    }

    // 4. MIXINS (framework → quarto → theme)
    if !framework.mixins.is_empty() {
        parts.push(&framework.mixins);
    }
    if !quarto.mixins.is_empty() {
        parts.push(&quarto.mixins);
    }
    if let Some(t) = theme {
        if !t.mixins.is_empty() {
            parts.push(&t.mixins);
        }
    }

    // 5. RULES (framework → quarto → theme)
    if !framework.rules.is_empty() {
        parts.push(&framework.rules);
    }
    if !quarto.rules.is_empty() {
        parts.push(&quarto.rules);
    }
    if let Some(t) = theme {
        if !t.rules.is_empty() {
            parts.push(&t.rules);
        }
    }

    parts.join("\n\n")
}

/// Assemble Bootstrap with a built-in theme.
///
/// This is a convenience function that loads all layers and assembles them
/// in the correct order for compilation.
///
/// # Arguments
///
/// * `theme` - The built-in theme to use.
///
/// # Returns
///
/// A complete SCSS string ready for compilation.
pub fn assemble_with_theme(theme: BuiltInTheme) -> Result<String, SassError> {
    let framework = load_bootstrap_framework()?;
    let quarto = load_quarto_layer()?;
    let theme_layer = load_theme(theme)?;

    Ok(assemble_scss(&framework, &quarto, Some(&theme_layer)))
}

/// Assemble Bootstrap without a theme.
///
/// This produces the base Bootstrap CSS with Quarto customizations.
///
/// # Returns
///
/// A complete SCSS string ready for compilation.
pub fn assemble_bootstrap() -> Result<String, SassError> {
    let framework = load_bootstrap_framework()?;
    let quarto = load_quarto_layer()?;

    Ok(assemble_scss(&framework, &quarto, None))
}

/// Assemble SCSS with multiple user layers.
///
/// This function is designed to work with the output of [`process_theme_specs()`],
/// which returns layers with customization already injected at the correct positions.
///
/// The user layers are merged using [`merge_layers()`], which:
/// - Concatenates `uses`, `functions`, `mixins`, `rules` in order
/// - **Reverses** `defaults` so later layers take precedence
///
/// # Assembly Order
///
/// 1. **USES**: framework → quarto → merged_user
/// 2. **FUNCTIONS**: framework → quarto → merged_user
/// 3. **DEFAULTS**: merged_user → quarto → framework (reversed in assemble_scss)
/// 4. **MIXINS**: framework → quarto → merged_user
/// 5. **RULES**: framework → quarto → merged_user
///
/// # Arguments
///
/// * `user_layers` - The layers from [`process_theme_specs()`], with customization
///   already injected at the appropriate positions.
///
/// # Example
///
/// ```
/// use quarto_sass::{ThemeSpec, ThemeContext, process_theme_specs, assemble_with_user_layers};
/// use std::path::PathBuf;
///
/// let context = ThemeContext::native(PathBuf::from("/doc"));
/// let specs = vec![ThemeSpec::parse("cosmo").unwrap()];
/// let result = process_theme_specs(&specs, &context).unwrap();
/// let scss = assemble_with_user_layers(&result.layers).unwrap();
/// ```
pub fn assemble_with_user_layers(user_layers: &[SassLayer]) -> Result<String, SassError> {
    use crate::layer::merge_layers;

    let framework = load_bootstrap_framework()?;
    let quarto = load_quarto_layer()?;

    if user_layers.is_empty() {
        // No user layers - assemble without theme
        return Ok(assemble_scss(&framework, &quarto, None));
    }

    // Merge user layers - merge_layers() reverses defaults automatically
    let merged_user = merge_layers(user_layers);

    Ok(assemble_scss(&framework, &quarto, Some(&merged_user)))
}

/// High-level function to compile themes from specifications.
///
/// This is the main entry point for compiling custom themes. It:
/// 1. Processes theme specs into layers (with customization injection)
/// 2. Assembles the SCSS bundle
/// 3. Returns the assembled SCSS string ready for compilation
///
/// Note: This function does NOT compile the SCSS - it only assembles it.
/// The actual compilation is performed by grass (or dart-sass for WASM).
///
/// # Arguments
///
/// * `specs` - Theme specifications to process.
/// * `context` - Theme context for path resolution.
///
/// # Returns
///
/// Returns a tuple of `(scss_string, load_paths)`:
/// - `scss_string`: The assembled SCSS ready for compilation
/// - `load_paths`: Additional load paths collected from custom themes
///
/// # Errors
///
/// Returns an error if any theme cannot be loaded or the bundle cannot be assembled.
///
/// # Example
///
/// ```no_run
/// use quarto_sass::{ThemeSpec, ThemeContext, assemble_themes};
/// use std::path::PathBuf;
///
/// let context = ThemeContext::new(PathBuf::from("/project/doc"));
/// let specs = vec![
///     ThemeSpec::parse("cosmo").unwrap(),
///     ThemeSpec::parse("custom.scss").unwrap(),
/// ];
///
/// let (scss, load_paths) = assemble_themes(&specs, &context).unwrap();
/// // Now compile with grass or dart-sass, adding load_paths to the compiler
/// ```
pub fn assemble_themes(
    specs: &[ThemeSpec],
    context: &ThemeContext<'_>,
) -> Result<(String, Vec<std::path::PathBuf>), SassError> {
    use crate::themes::process_theme_specs;

    // Process specs into layers (with customization injection)
    let result = process_theme_specs(specs, context)?;

    // Assemble SCSS
    let scss = assemble_with_user_layers(&result.layers)?;

    Ok((scss, result.load_paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_bootstrap_framework() {
        let framework = load_bootstrap_framework().unwrap();
        assert!(!framework.functions.is_empty());
        assert!(!framework.defaults.is_empty());
        assert!(!framework.mixins.is_empty());
        assert!(!framework.rules.is_empty());

        // Should contain Bootstrap functions
        assert!(framework.functions.contains("@function"));

        // Should contain self-contained color-contrast
        assert!(
            framework
                .functions
                .contains("$color-contrast-dark: $black !default")
        );
    }

    #[test]
    fn test_load_quarto_layer() {
        let quarto = load_quarto_layer().unwrap();
        // Quarto has customizations
        assert!(!quarto.defaults.is_empty());
        assert!(quarto.defaults.contains("$h1-font-size"));
    }

    #[test]
    fn test_load_theme() {
        let theme = load_theme(BuiltInTheme::Cerulean).unwrap();
        assert!(!theme.defaults.is_empty());
        assert!(theme.defaults.contains("$theme"));
    }

    #[test]
    fn test_assemble_scss_order() {
        let framework = SassLayer {
            uses: "// framework uses".to_string(),
            functions: "// framework functions".to_string(),
            defaults: "// framework defaults".to_string(),
            mixins: "// framework mixins".to_string(),
            rules: "// framework rules".to_string(),
        };

        let quarto = SassLayer {
            uses: "// quarto uses".to_string(),
            functions: "// quarto functions".to_string(),
            defaults: "// quarto defaults".to_string(),
            mixins: "// quarto mixins".to_string(),
            rules: "// quarto rules".to_string(),
        };

        let theme = SassLayer {
            uses: "// theme uses".to_string(),
            functions: "// theme functions".to_string(),
            defaults: "// theme defaults".to_string(),
            mixins: "// theme mixins".to_string(),
            rules: "// theme rules".to_string(),
        };

        let assembled = assemble_scss(&framework, &quarto, Some(&theme));

        // Verify order: uses (f→q→t), functions (f→q→t), defaults (t→q→f!), mixins (f→q→t), rules (f→q→t)
        let uses_start = assembled.find("// framework uses").unwrap();
        let uses_quarto = assembled.find("// quarto uses").unwrap();
        let uses_theme = assembled.find("// theme uses").unwrap();
        assert!(uses_start < uses_quarto);
        assert!(uses_quarto < uses_theme);

        let funcs_start = assembled.find("// framework functions").unwrap();
        let funcs_quarto = assembled.find("// quarto functions").unwrap();
        let funcs_theme = assembled.find("// theme functions").unwrap();
        assert!(funcs_start < funcs_quarto);
        assert!(funcs_quarto < funcs_theme);

        // DEFAULTS are REVERSED: theme → quarto → framework
        let defs_theme = assembled.find("// theme defaults").unwrap();
        let defs_quarto = assembled.find("// quarto defaults").unwrap();
        let defs_framework = assembled.find("// framework defaults").unwrap();
        assert!(defs_theme < defs_quarto);
        assert!(defs_quarto < defs_framework);

        let mix_start = assembled.find("// framework mixins").unwrap();
        let mix_quarto = assembled.find("// quarto mixins").unwrap();
        let mix_theme = assembled.find("// theme mixins").unwrap();
        assert!(mix_start < mix_quarto);
        assert!(mix_quarto < mix_theme);

        let rules_start = assembled.find("// framework rules").unwrap();
        let rules_quarto = assembled.find("// quarto rules").unwrap();
        let rules_theme = assembled.find("// theme rules").unwrap();
        assert!(rules_start < rules_quarto);
        assert!(rules_quarto < rules_theme);
    }

    #[test]
    fn test_assemble_bootstrap() {
        let scss = assemble_bootstrap().unwrap();
        // Should contain Bootstrap functions
        assert!(scss.contains("@function"));
        // Should contain Bootstrap variables
        assert!(scss.contains("$primary"));
        // Should contain Bootstrap rules
        assert!(scss.contains("@import"));
    }

    #[test]
    fn test_assemble_with_theme() {
        let scss = assemble_with_theme(BuiltInTheme::Cerulean).unwrap();
        // Should contain theme variables
        assert!(scss.contains("$theme: \"cerulean\""));
        // Should contain Bootstrap
        assert!(scss.contains("@function"));
    }

    #[test]
    fn test_assemble_slate_theme() {
        // Slate is a "problematic" theme that redefines lighten/darken
        // This test verifies the assembly order allows it to compile
        let scss = assemble_with_theme(BuiltInTheme::Slate).unwrap();

        // Should contain slate's theme marker
        assert!(scss.contains("$theme: \"slate\""));

        // Should contain the custom functions
        assert!(scss.contains("@function lighten"));
        assert!(scss.contains("@function darken"));

        // Critical: color-contrast function should come BEFORE slate's defaults
        // because slate's defaults call color-contrast()
        let color_contrast_pos = scss.find("@function color-contrast").unwrap();
        let slate_lighten_pos = scss.find("@function lighten").unwrap();
        assert!(
            color_contrast_pos < slate_lighten_pos,
            "color-contrast() must be defined before theme functions"
        );
    }

    // assemble_with_user_layers tests

    #[test]
    fn test_assemble_with_user_layers_empty() {
        // Empty layers should produce base Bootstrap + Quarto
        let scss = assemble_with_user_layers(&[]).unwrap();

        // Should contain Bootstrap
        assert!(scss.contains("@function"));
        assert!(scss.contains("$primary"));
    }

    #[test]
    fn test_assemble_with_user_layers_single() {
        use crate::themes::load_theme_layer;

        let cosmo_layer = load_theme_layer(BuiltInTheme::Cosmo).unwrap();
        let scss = assemble_with_user_layers(&[cosmo_layer]).unwrap();

        // Should contain theme
        assert!(scss.contains("$theme: \"cosmo\""));
        // Should contain Bootstrap
        assert!(scss.contains("@function"));
    }

    #[test]
    fn test_assemble_with_user_layers_multiple() {
        use crate::layer::parse_layer_from_parts;
        use crate::themes::load_theme_layer;

        let cosmo_layer = load_theme_layer(BuiltInTheme::Cosmo).unwrap();

        // Create a custom layer with a test variable
        let custom_layer = parse_layer_from_parts(
            "",
            "$test-custom-var: \"custom-value\" !default;",
            "",
            "",
            ".custom-rule { content: \"test\"; }",
        );

        let scss = assemble_with_user_layers(&[cosmo_layer, custom_layer]).unwrap();

        // Should contain both theme and custom
        assert!(scss.contains("$theme: \"cosmo\""));
        assert!(scss.contains("$test-custom-var"));
        assert!(scss.contains(".custom-rule"));
    }

    #[test]
    fn test_assemble_with_user_layers_defaults_reversed() {
        use crate::layer::parse_layer_from_parts;

        // Create two layers with the same variable
        let layer1 = parse_layer_from_parts("", "$myvar: layer1 !default;", "", "", ".layer1 {}");
        let layer2 = parse_layer_from_parts("", "$myvar: layer2 !default;", "", "", ".layer2 {}");

        // Assemble [layer1, layer2]
        let scss = assemble_with_user_layers(&[layer1, layer2]).unwrap();

        // Due to merge_layers reversing defaults, layer2's defaults should come
        // BEFORE layer1's defaults in the merged user layer. Then assemble_scss
        // reverses again (user → quarto → framework), so layer2's $myvar should
        // appear before layer1's in the final output.
        //
        // This means layer2's !default value wins (first definition wins with !default)
        let pos1 = scss.find("$myvar: layer1").unwrap();
        let pos2 = scss.find("$myvar: layer2").unwrap();

        // layer2's default should come first (win)
        assert!(
            pos2 < pos1,
            "layer2's defaults should come before layer1's (layer2 wins with !default)"
        );

        // Rules should be in original order: layer1 then layer2
        let rule1 = scss.find(".layer1").unwrap();
        let rule2 = scss.find(".layer2").unwrap();
        assert!(rule1 < rule2, "Rules should be in original order");
    }

    // assemble_themes tests

    #[test]
    fn test_assemble_themes_builtin() {
        use std::path::PathBuf;

        let context = ThemeContext::native(PathBuf::from("/doc"));
        let specs = vec![ThemeSpec::parse("cosmo").unwrap()];

        let (scss, load_paths) = assemble_themes(&specs, &context).unwrap();

        // Should contain theme
        assert!(scss.contains("$theme: \"cosmo\""));
        // Should contain Quarto customization (heading sizes)
        assert!(scss.contains("$h1-font-size"));
        // No load paths for built-in themes
        assert!(load_paths.is_empty());
    }

    #[test]
    fn test_assemble_themes_custom() {
        use std::path::PathBuf;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs = vec![ThemeSpec::parse("override.scss").unwrap()];
        let (scss, load_paths) = assemble_themes(&specs, &context).unwrap();

        // Should contain custom theme
        assert!(scss.contains("$test-custom-var"));
        // Should contain Quarto customization
        assert!(scss.contains("$h1-font-size"));
        // Should have load paths
        assert_eq!(load_paths.len(), 1);
    }

    #[test]
    fn test_assemble_themes_builtin_and_custom() {
        use std::path::PathBuf;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("override.scss").unwrap(),
        ];

        let (scss, load_paths) = assemble_themes(&specs, &context).unwrap();

        // Should contain both
        assert!(scss.contains("$theme: \"cosmo\""));
        assert!(scss.contains("$test-custom-var"));
        // Should contain Quarto customization
        assert!(scss.contains("$h1-font-size"));
        // Should have load paths from custom
        assert_eq!(load_paths.len(), 1);
    }

    // Title block layer tests

    #[test]
    fn test_load_title_block_layer() {
        let layer = load_title_block_layer().unwrap();

        // NOTE: The title-block.scss file uses non-standard markers:
        // - `/*-- scss: functions --*/` (space after colon - not recognized)
        // - `/*-- scss:variables --*/` ("variables" not a valid layer name)
        // - `/*-- scss:rules --*/` (valid!)
        //
        // This matches TS Quarto's behavior where only /*-- scss:rules --*/
        // is recognized, so everything before it goes into defaults.
        //
        // See: https://github.com/quarto-dev/quarto-cli/issues/13960

        // The functions and variables end up in defaults (before the rules marker)
        assert!(
            layer.defaults.contains("@function bannerColor"),
            "Functions should be in defaults (non-standard marker)"
        );
        assert!(
            layer.defaults.contains("$title-banner-color"),
            "Variables should be in defaults (non-standard marker)"
        );

        // Rules are correctly parsed
        assert!(
            layer.rules.contains(".quarto-title-meta"),
            "Should contain .quarto-title-meta rules"
        );
        assert!(
            layer.rules.contains("#title-block-header"),
            "Should contain #title-block-header rules"
        );

        // Functions section should be empty (marker not recognized)
        assert!(
            layer.functions.is_empty(),
            "Functions section should be empty (non-standard marker not recognized)"
        );
    }

    #[test]
    fn test_title_block_layer_in_scss_assembly() {
        let title_block_layer = load_title_block_layer().unwrap();
        let scss = assemble_with_user_layers(&[title_block_layer]).unwrap();

        // Should contain title block rules
        assert!(
            scss.contains(".quarto-title-meta"),
            "Assembled SCSS should contain .quarto-title-meta"
        );
        assert!(
            scss.contains(".quarto-title-meta-heading"),
            "Assembled SCSS should contain .quarto-title-meta-heading"
        );
    }
}
