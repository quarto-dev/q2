//! Bootswatch theme support.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides support for Bootswatch themes, which are pre-built
//! Bootstrap 5 theme customizations. Each theme provides a different visual
//! style while maintaining Bootstrap's component structure.
//!
//! # Theme Specifications
//!
//! Themes can be specified as either built-in names or custom file paths:
//!
//! - Built-in themes: `"cosmo"`, `"darkly"`, etc.
//! - Custom SCSS files: `"custom.scss"`, `"./themes/brand.scss"`
//! - Multiple layers: `["cosmo", "custom.scss"]`
//!
//! Use [`ThemeSpec::parse()`] to parse a theme string into the appropriate type.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use quarto_system_runtime::{PathKind, SystemRuntime};

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

/// A theme specification - either a built-in name or a file path.
///
/// This enum represents the different ways a theme can be specified in
/// Quarto configuration:
///
/// - Built-in themes like `"cosmo"` or `"darkly"` → [`ThemeSpec::BuiltIn`]
/// - Custom SCSS files like `"custom.scss"` → [`ThemeSpec::Custom`]
///
/// # Parsing Rules
///
/// Strings are parsed according to TypeScript Quarto's rules:
/// - Strings ending in `.scss` or `.css` → treated as file paths
/// - Other strings → treated as built-in theme names
///
/// # Example
///
/// ```
/// use quarto_sass::ThemeSpec;
///
/// // Built-in theme
/// let theme = ThemeSpec::parse("cosmo").unwrap();
/// assert!(theme.is_builtin());
///
/// // Custom file
/// let custom = ThemeSpec::parse("custom.scss").unwrap();
/// assert!(!custom.is_builtin());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSpec {
    /// A built-in Bootswatch theme.
    BuiltIn(BuiltInTheme),
    /// A custom SCSS file path (absolute or relative to document directory).
    Custom(PathBuf),
}

impl ThemeSpec {
    /// Parse a theme string into a ThemeSpec.
    ///
    /// # Resolution Rules
    ///
    /// - Strings ending in `.scss` or `.css` → [`ThemeSpec::Custom`]
    /// - Other strings → looked up as built-in theme names
    ///
    /// # Errors
    ///
    /// Returns [`SassError::UnknownTheme`] if the string is not a file path
    /// (no `.scss`/`.css` extension) and doesn't match any built-in theme name.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_sass::ThemeSpec;
    ///
    /// // Built-in theme
    /// assert!(ThemeSpec::parse("cosmo").is_ok());
    ///
    /// // Custom file (always succeeds, path validity checked later)
    /// assert!(ThemeSpec::parse("custom.scss").is_ok());
    /// assert!(ThemeSpec::parse("/abs/path/theme.css").is_ok());
    ///
    /// // Unknown built-in name
    /// assert!(ThemeSpec::parse("nonexistent").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, SassError> {
        let s_lower = s.to_lowercase();

        // Check for file extensions - matching TS Quarto behavior
        if s_lower.ends_with(".scss") || s_lower.ends_with(".css") {
            // Custom file path - use original case for the path
            Ok(ThemeSpec::Custom(PathBuf::from(s)))
        } else {
            // Try to parse as built-in theme name
            let theme: BuiltInTheme = s.parse()?;
            Ok(ThemeSpec::BuiltIn(theme))
        }
    }

    /// Check if this is a built-in theme.
    ///
    /// Returns `true` for [`ThemeSpec::BuiltIn`], `false` for [`ThemeSpec::Custom`].
    pub fn is_builtin(&self) -> bool {
        matches!(self, ThemeSpec::BuiltIn(_))
    }

    /// Check if this is a custom file path.
    ///
    /// Returns `true` for [`ThemeSpec::Custom`], `false` for [`ThemeSpec::BuiltIn`].
    pub fn is_custom(&self) -> bool {
        matches!(self, ThemeSpec::Custom(_))
    }

    /// Get the built-in theme if this is a built-in spec.
    pub fn as_builtin(&self) -> Option<BuiltInTheme> {
        match self {
            ThemeSpec::BuiltIn(theme) => Some(*theme),
            ThemeSpec::Custom(_) => None,
        }
    }

    /// Get the file path if this is a custom spec.
    pub fn as_custom(&self) -> Option<&Path> {
        match self {
            ThemeSpec::BuiltIn(_) => None,
            ThemeSpec::Custom(path) => Some(path),
        }
    }
}

impl std::fmt::Display for ThemeSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeSpec::BuiltIn(theme) => write!(f, "{}", theme),
            ThemeSpec::Custom(path) => write!(f, "{}", path.display()),
        }
    }
}

/// Context for resolving theme paths and loading custom themes.
///
/// This struct provides the necessary context for resolving relative paths
/// in custom theme specifications and tracking load paths for @import resolution.
///
/// The context holds a reference to a [`SystemRuntime`] for file system access,
/// enabling cross-platform support (native filesystem and WASM VirtualFileSystem).
///
/// # Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use quarto_sass::ThemeContext;
///
/// // On native platforms, use the convenience constructor
/// #[cfg(not(target_arch = "wasm32"))]
/// let context = ThemeContext::native(PathBuf::from("/project/doc"));
/// ```
pub struct ThemeContext<'a> {
    /// Directory containing the input document.
    ///
    /// Relative paths in custom theme specifications are resolved relative to this directory.
    document_dir: PathBuf,

    /// Additional load paths for @import resolution.
    ///
    /// These paths are searched when SCSS files use @import or @use.
    load_paths: Vec<PathBuf>,

    /// Runtime for file system access.
    ///
    /// This enables cross-platform file access - native `std::fs` on CLI,
    /// VirtualFileSystem on WASM (hub-client).
    runtime: &'a dyn SystemRuntime,
}

impl std::fmt::Debug for ThemeContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeContext")
            .field("document_dir", &self.document_dir)
            .field("load_paths", &self.load_paths)
            .field("runtime", &"<SystemRuntime>")
            .finish()
    }
}

impl<'a> ThemeContext<'a> {
    /// Create a new ThemeContext with the given document directory and runtime.
    ///
    /// The document directory is used to resolve relative paths in custom theme specs.
    /// The runtime provides file system access (native or WASM VFS).
    ///
    /// # Arguments
    ///
    /// * `document_dir` - Directory containing the input document
    /// * `runtime` - Runtime for file system access
    pub fn new(document_dir: PathBuf, runtime: &'a dyn SystemRuntime) -> Self {
        Self {
            document_dir,
            load_paths: Vec::new(),
            runtime,
        }
    }

    /// Create a ThemeContext with additional load paths.
    ///
    /// # Arguments
    ///
    /// * `document_dir` - Directory containing the input document
    /// * `load_paths` - Additional paths for @import resolution
    /// * `runtime` - Runtime for file system access
    pub fn with_load_paths(
        document_dir: PathBuf,
        load_paths: Vec<PathBuf>,
        runtime: &'a dyn SystemRuntime,
    ) -> Self {
        Self {
            document_dir,
            load_paths,
            runtime,
        }
    }

    /// Get the document directory.
    pub fn document_dir(&self) -> &Path {
        &self.document_dir
    }

    /// Get the load paths.
    pub fn load_paths(&self) -> &[PathBuf] {
        &self.load_paths
    }

    /// Get the runtime.
    pub fn runtime(&self) -> &dyn SystemRuntime {
        self.runtime
    }

    /// Add a load path.
    pub fn add_load_path(&mut self, path: PathBuf) {
        self.load_paths.push(path);
    }

    /// Resolve a potentially relative path against the document directory.
    ///
    /// - Absolute paths are returned as-is.
    /// - Relative paths are resolved relative to the document directory.
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.document_dir.join(path)
        }
    }
}

// Native-only convenience constructor
#[cfg(not(target_arch = "wasm32"))]
impl ThemeContext<'static> {
    /// Create a ThemeContext using the native runtime.
    ///
    /// This is a convenience constructor for native (non-WASM) targets.
    /// It uses a static `NativeRuntime` for file system access.
    ///
    /// # Arguments
    ///
    /// * `document_dir` - Directory containing the input document
    ///
    /// # Example
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use quarto_sass::ThemeContext;
    ///
    /// let context = ThemeContext::native(PathBuf::from("/project/doc"));
    /// assert_eq!(context.document_dir(), std::path::Path::new("/project/doc"));
    /// ```
    pub fn native(document_dir: PathBuf) -> Self {
        use once_cell::sync::Lazy;
        use quarto_system_runtime::NativeRuntime;

        static NATIVE_RUNTIME: Lazy<NativeRuntime> = Lazy::new(NativeRuntime::new);
        Self::new(document_dir, &*NATIVE_RUNTIME)
    }

    /// Create a ThemeContext with load paths using the native runtime.
    ///
    /// This is a convenience constructor for native (non-WASM) targets.
    ///
    /// # Arguments
    ///
    /// * `document_dir` - Directory containing the input document
    /// * `load_paths` - Additional paths for @import resolution
    pub fn native_with_load_paths(document_dir: PathBuf, load_paths: Vec<PathBuf>) -> Self {
        use once_cell::sync::Lazy;
        use quarto_system_runtime::NativeRuntime;

        static NATIVE_RUNTIME: Lazy<NativeRuntime> = Lazy::new(NativeRuntime::new);
        Self::with_load_paths(document_dir, load_paths, &*NATIVE_RUNTIME)
    }
}

/// Resolved theme with its layer and metadata.
///
/// Contains the parsed SCSS layer along with information about where it came from.
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    /// The parsed SCSS layer.
    pub layer: SassLayer,
    /// Whether this is a built-in theme.
    pub is_builtin: bool,
    /// The source path for custom themes (None for built-in).
    pub source_path: Option<PathBuf>,
}

/// Load a custom theme from the filesystem (or VFS on WASM).
///
/// Reads the SCSS file at the specified path and parses it into a [`SassLayer`].
/// Uses the runtime from the context for cross-platform file access.
///
/// # Arguments
///
/// * `path` - The path to the SCSS file (can be relative or absolute).
/// * `context` - The theme context for path resolution and file access.
///
/// # Returns
///
/// A tuple of `(SassLayer, PathBuf)` where the `PathBuf` is the directory
/// containing the theme file (for @import resolution).
///
/// # Errors
///
/// - [`SassError::CustomThemeNotFound`] if the file doesn't exist.
/// - [`SassError::InvalidScssFile`] if the file doesn't have layer boundaries.
/// - [`SassError::Io`] for other I/O errors.
///
/// # Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use quarto_sass::{ThemeContext, load_custom_theme};
///
/// // On native, use the convenience constructor
/// #[cfg(not(target_arch = "wasm32"))]
/// let context = ThemeContext::native(PathBuf::from("/project"));
/// #[cfg(not(target_arch = "wasm32"))]
/// let (layer, theme_dir) = load_custom_theme(
///     std::path::Path::new("themes/custom.scss"),
///     &context,
/// ).unwrap();
/// ```
pub fn load_custom_theme(
    path: &Path,
    context: &ThemeContext<'_>,
) -> Result<(SassLayer, PathBuf), SassError> {
    // Resolve the path against the document directory
    let resolved_path = context.resolve_path(path);

    // Check if file exists using runtime
    let exists = context
        .runtime()
        .path_exists(&resolved_path, Some(PathKind::File))
        .map_err(|e| {
            SassError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

    if !exists {
        return Err(SassError::CustomThemeNotFound {
            path: resolved_path,
        });
    }

    // Read the file using runtime
    let content = context
        .runtime()
        .file_read_string(&resolved_path)
        .map_err(|e| {
            SassError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;

    // Parse the layer
    let layer = parse_layer(&content, Some(&resolved_path.to_string_lossy())).map_err(|e| {
        // Convert NoBoundaryMarkers to InvalidScssFile for better error messages
        match e {
            SassError::NoBoundaryMarkers { .. } => SassError::InvalidScssFile {
                path: resolved_path.clone(),
            },
            other => other,
        }
    })?;

    // Get the directory containing the theme file for @import resolution
    let theme_dir = resolved_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    Ok((layer, theme_dir))
}

/// Resolve a [`ThemeSpec`] to a [`ResolvedTheme`].
///
/// This function handles both built-in and custom themes:
/// - Built-in themes are loaded from embedded resources.
/// - Custom themes are loaded from the filesystem.
///
/// # Arguments
///
/// * `spec` - The theme specification to resolve.
/// * `context` - The theme context for path resolution.
///
/// # Errors
///
/// Returns an error if the theme cannot be loaded.
pub fn resolve_theme_spec(
    spec: &ThemeSpec,
    context: &ThemeContext<'_>,
) -> Result<ResolvedTheme, SassError> {
    match spec {
        ThemeSpec::BuiltIn(theme) => {
            let layer = load_theme_layer(*theme)?;
            Ok(ResolvedTheme {
                layer,
                is_builtin: true,
                source_path: None,
            })
        }
        ThemeSpec::Custom(path) => {
            let (layer, _theme_dir) = load_custom_theme(path, context)?;
            let resolved_path = context.resolve_path(path);
            Ok(ResolvedTheme {
                layer,
                is_builtin: false,
                source_path: Some(resolved_path),
            })
        }
    }
}

/// Result of processing theme specifications.
///
/// Contains the processed layers (with customization injection already applied)
/// and any additional load paths collected from custom themes.
#[derive(Debug, Clone, Default)]
pub struct ThemeLayerResult {
    /// Layers in order (with customization already injected after built-in themes).
    pub layers: Vec<SassLayer>,
    /// Load paths collected from custom theme directories.
    ///
    /// These should be added to the SASS compiler's load paths for @import resolution.
    pub load_paths: Vec<PathBuf>,
}

/// Load the Quarto customization layer.
///
/// This is the layer that gets injected after each built-in theme (or at the beginning
/// if only custom files are specified). It contains Quarto's heading size customizations
/// and other Bootstrap overrides.
///
/// This is separate from `load_quarto_layer()` which loads the full Quarto layer
/// including functions, mixins, and rules.
pub fn load_quarto_customization_layer() -> Result<SassLayer, SassError> {
    use crate::resources::QUARTO_BOOTSTRAP_RESOURCES;

    let customize_content = QUARTO_BOOTSTRAP_RESOURCES
        .read_str(Path::new("_bootstrap-customize.scss"))
        .ok_or_else(|| SassError::CompilationFailed {
            message: "_bootstrap-customize.scss not found".to_string(),
        })?;

    parse_layer(customize_content, Some("_bootstrap-customize.scss"))
}

/// Process theme specifications into layers with customization injection.
///
/// This is the Rust equivalent of TypeScript Quarto's `layerTheme()` function.
/// It processes theme specs in order and injects the Quarto customization layer
/// at the appropriate positions.
///
/// # Customization Injection Rules
///
/// - After **each** built-in theme, inject the Quarto customization layer.
/// - If **no** built-in themes, inject customization at the **beginning**.
///
/// This ensures that Quarto's heading sizes and other defaults take precedence
/// over theme defaults (in the SASS `!default` sense) while allowing user
/// customization to override everything.
///
/// # Example
///
/// ```text
/// Input: ["cosmo", "custom.scss"]
/// Output layers: [cosmoLayer, customizeLayer, customLayer]
/// Merged defaults: custom → customize → cosmo (custom wins)
/// Merged rules: cosmo → customize → custom (custom comes last)
///
/// Input: ["custom.scss", "cosmo"]
/// Output layers: [customLayer, cosmoLayer, customizeLayer]
/// Merged defaults: customize → cosmo → custom (customize wins for its vars)
/// Merged rules: custom → cosmo → customize (customize comes last)
///
/// Input: ["custom.scss"]  (no built-in)
/// Output layers: [customizeLayer, customLayer]
/// Merged defaults: custom → customize (custom wins)
/// Merged rules: customize → custom (custom comes last)
/// ```
///
/// # Arguments
///
/// * `specs` - The theme specifications to process.
/// * `context` - The theme context for path resolution.
///
/// # Errors
///
/// Returns an error if any theme cannot be loaded.
pub fn process_theme_specs(
    specs: &[ThemeSpec],
    context: &ThemeContext<'_>,
) -> Result<ThemeLayerResult, SassError> {
    let mut layers = Vec::new();
    let mut load_paths = Vec::new();
    let mut any_builtin = false;

    // Load the customization layer once (we may need to clone it multiple times)
    let customize_layer = load_quarto_customization_layer()?;

    for spec in specs {
        match spec {
            ThemeSpec::BuiltIn(theme) => {
                // Load the built-in theme
                let theme_layer = load_theme_layer(*theme)?;
                layers.push(theme_layer);

                // Inject customization AFTER each built-in theme
                layers.push(customize_layer.clone());
                any_builtin = true;
            }
            ThemeSpec::Custom(path) => {
                // Load the custom theme
                let (layer, theme_dir) = load_custom_theme(path, context)?;
                layers.push(layer);

                // Add the theme directory to load paths for @import resolution
                load_paths.push(theme_dir);
            }
        }
    }

    // If no built-in themes, inject customization at the beginning
    if !any_builtin && !layers.is_empty() {
        layers.insert(0, customize_layer);
    }

    Ok(ThemeLayerResult { layers, load_paths })
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

    // ThemeSpec tests

    #[test]
    fn test_theme_spec_parse_builtin() {
        let spec = ThemeSpec::parse("cosmo").unwrap();
        assert!(spec.is_builtin());
        assert!(!spec.is_custom());
        assert_eq!(spec.as_builtin(), Some(BuiltInTheme::Cosmo));
        assert_eq!(spec.as_custom(), None);
    }

    #[test]
    fn test_theme_spec_parse_builtin_case_insensitive() {
        let spec1 = ThemeSpec::parse("COSMO").unwrap();
        let spec2 = ThemeSpec::parse("Cosmo").unwrap();
        let spec3 = ThemeSpec::parse("cosmo").unwrap();
        assert_eq!(spec1, spec2);
        assert_eq!(spec2, spec3);
        assert!(spec1.is_builtin());
    }

    #[test]
    fn test_theme_spec_parse_custom_scss() {
        let spec = ThemeSpec::parse("custom.scss").unwrap();
        assert!(spec.is_custom());
        assert!(!spec.is_builtin());
        assert_eq!(spec.as_custom(), Some(Path::new("custom.scss")));
        assert_eq!(spec.as_builtin(), None);
    }

    #[test]
    fn test_theme_spec_parse_custom_css() {
        let spec = ThemeSpec::parse("theme.css").unwrap();
        assert!(spec.is_custom());
        assert_eq!(spec.as_custom(), Some(Path::new("theme.css")));
    }

    #[test]
    fn test_theme_spec_parse_custom_preserves_case() {
        let spec = ThemeSpec::parse("MyTheme.SCSS").unwrap();
        assert!(spec.is_custom());
        // Path should preserve original case
        assert_eq!(spec.as_custom(), Some(Path::new("MyTheme.SCSS")));
    }

    #[test]
    fn test_theme_spec_parse_custom_relative_path() {
        let spec = ThemeSpec::parse("./themes/brand.scss").unwrap();
        assert!(spec.is_custom());
        assert_eq!(spec.as_custom(), Some(Path::new("./themes/brand.scss")));
    }

    #[test]
    fn test_theme_spec_parse_custom_absolute_path() {
        let spec = ThemeSpec::parse("/abs/path/theme.scss").unwrap();
        assert!(spec.is_custom());
        assert_eq!(spec.as_custom(), Some(Path::new("/abs/path/theme.scss")));
    }

    #[test]
    fn test_theme_spec_parse_unknown_builtin() {
        let result = ThemeSpec::parse("nonexistent");
        assert!(result.is_err());
        // Verify it's the right error type
        match result {
            Err(SassError::UnknownTheme(name)) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected UnknownTheme error"),
        }
    }

    #[test]
    fn test_theme_spec_display() {
        let builtin = ThemeSpec::parse("cosmo").unwrap();
        assert_eq!(format!("{}", builtin), "cosmo");

        let custom = ThemeSpec::parse("my/theme.scss").unwrap();
        assert_eq!(format!("{}", custom), "my/theme.scss");
    }

    // ThemeContext tests

    #[test]
    fn test_theme_context_new() {
        let context = ThemeContext::native(PathBuf::from("/project/doc"));
        assert_eq!(context.document_dir(), Path::new("/project/doc"));
        assert!(context.load_paths().is_empty());
    }

    #[test]
    fn test_theme_context_with_load_paths() {
        let load_paths = vec![PathBuf::from("/lib1"), PathBuf::from("/lib2")];
        let context = ThemeContext::native_with_load_paths(PathBuf::from("/doc"), load_paths);
        assert_eq!(context.document_dir(), Path::new("/doc"));
        assert_eq!(context.load_paths().len(), 2);
        assert_eq!(context.load_paths()[0], PathBuf::from("/lib1"));
    }

    #[test]
    fn test_theme_context_resolve_relative_path() {
        let context = ThemeContext::native(PathBuf::from("/project/doc"));
        let resolved = context.resolve_path(Path::new("themes/custom.scss"));
        assert_eq!(resolved, PathBuf::from("/project/doc/themes/custom.scss"));
    }

    #[test]
    fn test_theme_context_resolve_absolute_path() {
        let context = ThemeContext::native(PathBuf::from("/project/doc"));
        let resolved = context.resolve_path(Path::new("/abs/path/theme.scss"));
        assert_eq!(resolved, PathBuf::from("/abs/path/theme.scss"));
    }

    #[test]
    fn test_theme_context_add_load_path() {
        let mut context = ThemeContext::native(PathBuf::from("/doc"));
        context.add_load_path(PathBuf::from("/lib"));
        assert_eq!(context.load_paths().len(), 1);
        assert_eq!(context.load_paths()[0], PathBuf::from("/lib"));
    }

    // load_custom_theme tests

    #[test]
    fn test_load_custom_theme_success() {
        // Use the test fixture directory
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");

        let context = ThemeContext::native(fixture_dir);
        let (layer, theme_dir) = load_custom_theme(Path::new("override.scss"), &context).unwrap();

        // Verify the layer was parsed correctly
        assert!(layer.defaults.contains("$test-custom-var"));
        assert!(layer.defaults.contains("from-custom"));
        assert!(layer.rules.contains(".custom-rule"));

        // Verify the theme directory is correct
        assert!(theme_dir.ends_with("test-fixtures/custom"));
    }

    #[test]
    fn test_load_custom_theme_not_found() {
        let context = ThemeContext::native(PathBuf::from("/nonexistent"));
        let result = load_custom_theme(Path::new("missing.scss"), &context);

        assert!(result.is_err());
        match result {
            Err(SassError::CustomThemeNotFound { path }) => {
                assert!(path.ends_with("missing.scss"));
            }
            _ => panic!("Expected CustomThemeNotFound error"),
        }
    }

    #[test]
    fn test_load_custom_theme_no_boundaries() {
        // Use the test fixture without boundary markers
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");

        let context = ThemeContext::native(fixture_dir);
        let result = load_custom_theme(Path::new("no_boundaries.scss"), &context);

        assert!(result.is_err());
        match result {
            Err(SassError::InvalidScssFile { path }) => {
                assert!(path.ends_with("no_boundaries.scss"));
            }
            _ => panic!("Expected InvalidScssFile error"),
        }
    }

    // resolve_theme_spec tests

    #[test]
    fn test_resolve_theme_spec_builtin() {
        let context = ThemeContext::native(PathBuf::from("/doc"));
        let spec = ThemeSpec::parse("cosmo").unwrap();
        let resolved = resolve_theme_spec(&spec, &context).unwrap();

        assert!(resolved.is_builtin);
        assert!(resolved.source_path.is_none());
        assert!(!resolved.layer.defaults.is_empty());
    }

    #[test]
    fn test_resolve_theme_spec_custom() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");

        let context = ThemeContext::native(fixture_dir);
        let spec = ThemeSpec::parse("override.scss").unwrap();
        let resolved = resolve_theme_spec(&spec, &context).unwrap();

        assert!(!resolved.is_builtin);
        assert!(resolved.source_path.is_some());
        assert!(resolved.layer.defaults.contains("$test-custom-var"));
    }

    // process_theme_specs tests

    #[test]
    fn test_load_quarto_customization_layer() {
        let layer = load_quarto_customization_layer().unwrap();
        // Should contain Quarto's heading size customizations
        assert!(
            layer.defaults.contains("$h1-font-size"),
            "Customization layer should have heading sizes"
        );
    }

    #[test]
    fn test_process_theme_specs_builtin_only() {
        // Single built-in theme should produce: [theme, customize]
        let context = ThemeContext::native(PathBuf::from("/doc"));
        let specs = vec![ThemeSpec::parse("cosmo").unwrap()];

        let result = process_theme_specs(&specs, &context).unwrap();

        // Should have 2 layers: cosmo + customize
        assert_eq!(result.layers.len(), 2);

        // First layer should be cosmo
        assert!(result.layers[0].defaults.contains("$theme: \"cosmo\""));

        // Second layer should be customization (has heading sizes)
        assert!(result.layers[1].defaults.contains("$h1-font-size"));

        // No load paths for built-in themes
        assert!(result.load_paths.is_empty());
    }

    #[test]
    fn test_process_theme_specs_custom_only() {
        // Custom-only themes should produce: [customize, custom]
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs = vec![ThemeSpec::parse("override.scss").unwrap()];
        let result = process_theme_specs(&specs, &context).unwrap();

        // Should have 2 layers: customize (at beginning) + custom
        assert_eq!(result.layers.len(), 2);

        // First layer should be customization (injected at beginning)
        assert!(result.layers[0].defaults.contains("$h1-font-size"));

        // Second layer should be custom
        assert!(result.layers[1].defaults.contains("$test-custom-var"));

        // Should have one load path (the custom theme's directory)
        assert_eq!(result.load_paths.len(), 1);
    }

    #[test]
    fn test_process_theme_specs_builtin_then_custom() {
        // [builtin, custom] should produce: [builtin, customize, custom]
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("override.scss").unwrap(),
        ];
        let result = process_theme_specs(&specs, &context).unwrap();

        // Should have 3 layers: cosmo + customize + custom
        assert_eq!(result.layers.len(), 3);

        // Verify order
        assert!(result.layers[0].defaults.contains("$theme: \"cosmo\""));
        assert!(result.layers[1].defaults.contains("$h1-font-size")); // customize
        assert!(result.layers[2].defaults.contains("$test-custom-var")); // custom
    }

    #[test]
    fn test_process_theme_specs_custom_then_builtin() {
        // [custom, builtin] should produce: [custom, builtin, customize]
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs = vec![
            ThemeSpec::parse("override.scss").unwrap(),
            ThemeSpec::parse("cosmo").unwrap(),
        ];
        let result = process_theme_specs(&specs, &context).unwrap();

        // Should have 3 layers: custom + cosmo + customize
        assert_eq!(result.layers.len(), 3);

        // Verify order
        assert!(result.layers[0].defaults.contains("$test-custom-var")); // custom
        assert!(result.layers[1].defaults.contains("$theme: \"cosmo\"")); // cosmo
        assert!(result.layers[2].defaults.contains("$h1-font-size")); // customize
    }

    #[test]
    fn test_process_theme_specs_multiple_builtins() {
        // Multiple built-ins: customization is injected after EACH
        let context = ThemeContext::native(PathBuf::from("/doc"));
        let specs = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("flatly").unwrap(),
        ];

        let result = process_theme_specs(&specs, &context).unwrap();

        // Should have 4 layers: cosmo + customize + flatly + customize
        assert_eq!(result.layers.len(), 4);

        assert!(result.layers[0].defaults.contains("$theme: \"cosmo\""));
        assert!(result.layers[1].defaults.contains("$h1-font-size")); // customize after cosmo
        assert!(result.layers[2].defaults.contains("$theme: \"flatly\""));
        assert!(result.layers[3].defaults.contains("$h1-font-size")); // customize after flatly
    }

    #[test]
    fn test_process_theme_specs_empty() {
        // Empty specs should produce empty result
        let context = ThemeContext::native(PathBuf::from("/doc"));
        let specs: Vec<ThemeSpec> = vec![];

        let result = process_theme_specs(&specs, &context).unwrap();

        assert!(result.layers.is_empty());
        assert!(result.load_paths.is_empty());
    }

    #[test]
    fn test_process_theme_specs_ordering_affects_defaults() {
        // Verify that [A, B] and [B, A] produce different layer orderings
        // which will affect the merged defaults due to !default semantics
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");
        let context = ThemeContext::native(fixture_dir);

        let specs_builtin_first = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("override.scss").unwrap(),
        ];

        let specs_custom_first = vec![
            ThemeSpec::parse("override.scss").unwrap(),
            ThemeSpec::parse("cosmo").unwrap(),
        ];

        let result1 = process_theme_specs(&specs_builtin_first, &context).unwrap();
        let result2 = process_theme_specs(&specs_custom_first, &context).unwrap();

        // Both should have 3 layers
        assert_eq!(result1.layers.len(), 3);
        assert_eq!(result2.layers.len(), 3);

        // But the order should be different
        // [cosmo, custom.scss] -> [cosmo, customize, custom]
        // [custom.scss, cosmo] -> [custom, cosmo, customize]

        // result1: custom comes last (after customize)
        assert!(result1.layers[2].defaults.contains("$test-custom-var"));

        // result2: custom comes first
        assert!(result2.layers[0].defaults.contains("$test-custom-var"));
    }
}
