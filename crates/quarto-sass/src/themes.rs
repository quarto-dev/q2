//! Bootswatch theme support.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides support for Bootswatch themes, which are pre-built
//! Bootstrap 5 theme customizations. Each theme provides a different visual
//! style while maintaining Bootstrap's component structure.

use std::path::Path;
use std::str::FromStr;

use crate::error::SassError;
use crate::layer::parse_layer;
use crate::resources::THEMES_RESOURCES;
use crate::types::SassLayer;

/// Built-in Bootswatch themes.
///
/// These are the 25 themes available in Bootswatch 5.3.1, matching
/// the themes bundled with TypeScript Quarto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltInTheme {
    Cerulean,
    Cosmo,
    Cyborg,
    Darkly,
    Flatly,
    Journal,
    Litera,
    Lumen,
    Lux,
    Materia,
    Minty,
    Morph,
    Pulse,
    Quartz,
    Sandstone,
    Simplex,
    Sketchy,
    Slate,
    Solar,
    Spacelab,
    Superhero,
    United,
    Vapor,
    Yeti,
    Zephyr,
}

impl BuiltInTheme {
    /// Get the theme name as a lowercase string (used for file lookups).
    pub fn name(&self) -> &'static str {
        match self {
            BuiltInTheme::Cerulean => "cerulean",
            BuiltInTheme::Cosmo => "cosmo",
            BuiltInTheme::Cyborg => "cyborg",
            BuiltInTheme::Darkly => "darkly",
            BuiltInTheme::Flatly => "flatly",
            BuiltInTheme::Journal => "journal",
            BuiltInTheme::Litera => "litera",
            BuiltInTheme::Lumen => "lumen",
            BuiltInTheme::Lux => "lux",
            BuiltInTheme::Materia => "materia",
            BuiltInTheme::Minty => "minty",
            BuiltInTheme::Morph => "morph",
            BuiltInTheme::Pulse => "pulse",
            BuiltInTheme::Quartz => "quartz",
            BuiltInTheme::Sandstone => "sandstone",
            BuiltInTheme::Simplex => "simplex",
            BuiltInTheme::Sketchy => "sketchy",
            BuiltInTheme::Slate => "slate",
            BuiltInTheme::Solar => "solar",
            BuiltInTheme::Spacelab => "spacelab",
            BuiltInTheme::Superhero => "superhero",
            BuiltInTheme::United => "united",
            BuiltInTheme::Vapor => "vapor",
            BuiltInTheme::Yeti => "yeti",
            BuiltInTheme::Zephyr => "zephyr",
        }
    }

    /// Get the SCSS filename for this theme.
    pub fn filename(&self) -> String {
        format!("{}.scss", self.name())
    }

    /// Get all available built-in themes.
    pub fn all() -> &'static [BuiltInTheme] {
        &[
            BuiltInTheme::Cerulean,
            BuiltInTheme::Cosmo,
            BuiltInTheme::Cyborg,
            BuiltInTheme::Darkly,
            BuiltInTheme::Flatly,
            BuiltInTheme::Journal,
            BuiltInTheme::Litera,
            BuiltInTheme::Lumen,
            BuiltInTheme::Lux,
            BuiltInTheme::Materia,
            BuiltInTheme::Minty,
            BuiltInTheme::Morph,
            BuiltInTheme::Pulse,
            BuiltInTheme::Quartz,
            BuiltInTheme::Sandstone,
            BuiltInTheme::Simplex,
            BuiltInTheme::Sketchy,
            BuiltInTheme::Slate,
            BuiltInTheme::Solar,
            BuiltInTheme::Spacelab,
            BuiltInTheme::Superhero,
            BuiltInTheme::United,
            BuiltInTheme::Vapor,
            BuiltInTheme::Yeti,
            BuiltInTheme::Zephyr,
        ]
    }

    /// Check if a theme name is known to be dark-themed by default.
    ///
    /// Dark themes typically have a dark background and light text.
    pub fn is_dark(&self) -> bool {
        matches!(
            self,
            BuiltInTheme::Cyborg
                | BuiltInTheme::Darkly
                | BuiltInTheme::Slate
                | BuiltInTheme::Solar
                | BuiltInTheme::Superhero
                | BuiltInTheme::Vapor
        )
    }
}

impl FromStr for BuiltInTheme {
    type Err = SassError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "cerulean" => Ok(BuiltInTheme::Cerulean),
            "cosmo" => Ok(BuiltInTheme::Cosmo),
            "cyborg" => Ok(BuiltInTheme::Cyborg),
            "darkly" => Ok(BuiltInTheme::Darkly),
            "flatly" => Ok(BuiltInTheme::Flatly),
            "journal" => Ok(BuiltInTheme::Journal),
            "litera" => Ok(BuiltInTheme::Litera),
            "lumen" => Ok(BuiltInTheme::Lumen),
            "lux" => Ok(BuiltInTheme::Lux),
            "materia" => Ok(BuiltInTheme::Materia),
            "minty" => Ok(BuiltInTheme::Minty),
            "morph" => Ok(BuiltInTheme::Morph),
            "pulse" => Ok(BuiltInTheme::Pulse),
            "quartz" => Ok(BuiltInTheme::Quartz),
            "sandstone" => Ok(BuiltInTheme::Sandstone),
            "simplex" => Ok(BuiltInTheme::Simplex),
            "sketchy" => Ok(BuiltInTheme::Sketchy),
            "slate" => Ok(BuiltInTheme::Slate),
            "solar" => Ok(BuiltInTheme::Solar),
            "spacelab" => Ok(BuiltInTheme::Spacelab),
            "superhero" => Ok(BuiltInTheme::Superhero),
            "united" => Ok(BuiltInTheme::United),
            "vapor" => Ok(BuiltInTheme::Vapor),
            "yeti" => Ok(BuiltInTheme::Yeti),
            "zephyr" => Ok(BuiltInTheme::Zephyr),
            _ => Err(SassError::UnknownTheme(s.to_string())),
        }
    }
}

impl std::fmt::Display for BuiltInTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Load a built-in theme's SCSS layer from embedded resources.
///
/// Returns a `SassLayer` with the theme's defaults, functions, mixins, and rules
/// parsed from the embedded theme file.
///
/// # Arguments
///
/// * `theme` - The built-in theme to load.
///
/// # Errors
///
/// Returns an error if the theme file cannot be read or parsed.
pub fn load_theme_layer(theme: BuiltInTheme) -> Result<SassLayer, SassError> {
    let filename = theme.filename();
    let content = THEMES_RESOURCES
        .read_str(Path::new(&filename))
        .ok_or_else(|| SassError::ThemeNotFound(theme.name().to_string()))?;

    parse_layer(content, Some(&filename))
}

/// Resolve a theme name to a theme layer.
///
/// This function handles both built-in theme names (e.g., "cosmo") and returns
/// the parsed SCSS layer. For custom themes from files, use `parse_layer` directly.
///
/// # Arguments
///
/// * `name` - The theme name (case-insensitive).
///
/// # Returns
///
/// Returns a tuple of `(BuiltInTheme, SassLayer)` on success.
///
/// # Errors
///
/// Returns an error if the theme name is unknown or the theme cannot be loaded.
pub fn resolve_theme(name: &str) -> Result<(BuiltInTheme, SassLayer), SassError> {
    let theme: BuiltInTheme = name.parse()?;
    let layer = load_theme_layer(theme)?;
    Ok((theme, layer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_theme_name() {
        assert_eq!(BuiltInTheme::Cerulean.name(), "cerulean");
        assert_eq!(BuiltInTheme::Darkly.name(), "darkly");
        assert_eq!(BuiltInTheme::Zephyr.name(), "zephyr");
    }

    #[test]
    fn test_builtin_theme_from_str() {
        assert_eq!(
            "cerulean".parse::<BuiltInTheme>().unwrap(),
            BuiltInTheme::Cerulean
        );
        assert_eq!(
            "DARKLY".parse::<BuiltInTheme>().unwrap(),
            BuiltInTheme::Darkly
        );
        assert_eq!(
            "Slate".parse::<BuiltInTheme>().unwrap(),
            BuiltInTheme::Slate
        );
        assert!("nonexistent".parse::<BuiltInTheme>().is_err());
    }

    #[test]
    fn test_builtin_theme_all() {
        let all = BuiltInTheme::all();
        assert_eq!(all.len(), 25);
        assert!(all.contains(&BuiltInTheme::Cerulean));
        assert!(all.contains(&BuiltInTheme::Zephyr));
    }

    #[test]
    fn test_builtin_theme_is_dark() {
        assert!(BuiltInTheme::Cyborg.is_dark());
        assert!(BuiltInTheme::Darkly.is_dark());
        assert!(BuiltInTheme::Slate.is_dark());
        assert!(!BuiltInTheme::Cerulean.is_dark());
        assert!(!BuiltInTheme::Cosmo.is_dark());
    }

    #[test]
    fn test_load_theme_layer() {
        let layer = load_theme_layer(BuiltInTheme::Cerulean).unwrap();
        // Cerulean should have defaults
        assert!(!layer.defaults.is_empty());
        assert!(layer.defaults.contains("$theme"));
    }

    #[test]
    fn test_load_theme_slate() {
        // Slate is one of the "problematic" themes with custom functions
        let layer = load_theme_layer(BuiltInTheme::Slate).unwrap();
        assert!(!layer.defaults.is_empty());
        // Slate redefines lighten/darken functions
        assert!(layer.defaults.contains("@function lighten"));
        assert!(layer.defaults.contains("@function darken"));
    }

    #[test]
    fn test_resolve_theme() {
        let (theme, layer) = resolve_theme("cosmo").unwrap();
        assert_eq!(theme, BuiltInTheme::Cosmo);
        assert!(!layer.defaults.is_empty());
    }

    #[test]
    fn test_resolve_theme_case_insensitive() {
        let (theme1, _) = resolve_theme("COSMO").unwrap();
        let (theme2, _) = resolve_theme("Cosmo").unwrap();
        let (theme3, _) = resolve_theme("cosmo").unwrap();
        assert_eq!(theme1, theme2);
        assert_eq!(theme2, theme3);
    }

    #[test]
    fn test_resolve_unknown_theme() {
        let result = resolve_theme("nonexistent");
        assert!(result.is_err());
    }
}
