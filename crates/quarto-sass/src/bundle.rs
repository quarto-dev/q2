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
use crate::themes::BuiltInTheme;
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
}
