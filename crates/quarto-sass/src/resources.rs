//! Embedded SCSS resources for SASS compilation.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides access to Bootstrap 5.3.1 SCSS files that are embedded
//! at compile time. These resources are available under a virtual path prefix
//! `/__quarto_resources__/bootstrap/scss/`.
//!
//! # Usage
//!
//! For native compilation with grass:
//! ```ignore
//! use quarto_sass::resources::BOOTSTRAP_RESOURCES;
//!
//! // Check if a file exists
//! if BOOTSTRAP_RESOURCES.is_file("_variables.scss") {
//!     let content = BOOTSTRAP_RESOURCES.read("_variables.scss");
//! }
//! ```
//!
//! For WASM, the resources should be pre-populated into the VFS at startup.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use include_dir::{Dir, include_dir};

/// Bootstrap 5.3.1 SCSS directory embedded at compile time.
///
/// This includes all SCSS files from the Bootstrap distribution used by TS Quarto.
static BOOTSTRAP_DIR: Dir<'static> = include_dir!(
    "$CARGO_MANIFEST_DIR/../../external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/scss"
);

/// Bootstrap sass-utils directory embedded at compile time.
///
/// Contains utility functions like color-contrast.scss that have inline fallback
/// variable definitions, making them work even before Bootstrap variables are loaded.
static SASS_UTILS_DIR: Dir<'static> = include_dir!(
    "$CARGO_MANIFEST_DIR/../../external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/sass-utils"
);

/// Bootswatch themes directory embedded at compile time.
///
/// Contains 25 Bootswatch theme files that customize Bootstrap's appearance.
static THEMES_DIR: Dir<'static> = include_dir!(
    "$CARGO_MANIFEST_DIR/../../external-sources/quarto-cli/src/resources/formats/html/bootstrap/themes"
);

/// Quarto Bootstrap customization files embedded at compile time.
///
/// Contains Quarto's Bootstrap customization layer and additional functions.
static QUARTO_BOOTSTRAP_DIR: Dir<'static> = include_dir!(
    "$CARGO_MANIFEST_DIR/../../external-sources/quarto-cli/src/resources/formats/html/bootstrap"
);

/// Virtual path prefix for embedded resources.
///
/// Files embedded via `EmbeddedResources` are accessible under this prefix.
/// For example, Bootstrap's `_variables.scss` is at:
/// `/__quarto_resources__/bootstrap/scss/_variables.scss`
pub const RESOURCE_PATH_PREFIX: &str = "/__quarto_resources__";

/// Embedded SCSS resources with compile-time inclusion.
///
/// This type provides access to SCSS files embedded via `include_dir!`.
/// It maintains a lazy-initialized index of all files and directories
/// for efficient lookups.
pub struct EmbeddedResources {
    /// The embedded directory tree.
    dir: &'static Dir<'static>,
    /// The path prefix within the virtual filesystem (e.g., "bootstrap/scss").
    prefix: &'static str,
    /// Lazy-initialized set of all file paths (relative to prefix).
    files: OnceLock<HashSet<String>>,
    /// Lazy-initialized set of all directory paths (relative to prefix).
    directories: OnceLock<HashSet<String>>,
}

impl EmbeddedResources {
    /// Create a new EmbeddedResources instance.
    ///
    /// This is `const` so it can be used in static initialization.
    pub const fn new(dir: &'static Dir<'static>, prefix: &'static str) -> Self {
        Self {
            dir,
            prefix,
            files: OnceLock::new(),
            directories: OnceLock::new(),
        }
    }

    /// Get the path prefix for these resources.
    pub fn prefix(&self) -> &str {
        self.prefix
    }

    /// Get the full virtual path prefix including the resource root.
    ///
    /// Returns e.g., `/__quarto_resources__/bootstrap/scss`.
    pub fn full_prefix(&self) -> String {
        format!("{}/{}", RESOURCE_PATH_PREFIX, self.prefix)
    }

    /// Check if a path exists as a file.
    ///
    /// The path can be:
    /// - Relative to the prefix (e.g., "_variables.scss")
    /// - Relative to RESOURCE_PATH_PREFIX (e.g., "bootstrap/scss/_variables.scss")
    /// - Absolute with RESOURCE_PATH_PREFIX (e.g., "/__quarto_resources__/bootstrap/scss/_variables.scss")
    pub fn is_file(&self, path: &Path) -> bool {
        let relative = self.strip_prefix(path);
        self.files().contains(relative.as_str())
    }

    /// Check if a path exists as a directory.
    ///
    /// Accepts paths in the same formats as `is_file`.
    pub fn is_dir(&self, path: &Path) -> bool {
        let relative = self.strip_prefix(path);
        if relative.is_empty() {
            return true; // Root of embedded resources is a directory
        }
        self.directories().contains(relative.as_str())
    }

    /// Read a file's contents.
    ///
    /// Returns `None` if the file doesn't exist or isn't a file.
    /// Accepts paths in the same formats as `is_file`.
    pub fn read(&self, path: &Path) -> Option<&'static [u8]> {
        let relative = self.strip_prefix(path);
        self.dir.get_file(&relative).map(|f| f.contents())
    }

    /// Read a file's contents as a string.
    ///
    /// Returns `None` if the file doesn't exist or isn't valid UTF-8.
    pub fn read_str(&self, path: &Path) -> Option<&'static str> {
        let relative = self.strip_prefix(path);
        self.dir.get_file(&relative).and_then(|f| f.contents_utf8())
    }

    /// Get an iterator over all file paths (relative to the prefix).
    pub fn file_paths(&self) -> impl Iterator<Item = &String> {
        self.files().iter()
    }

    /// Get the number of embedded files.
    pub fn file_count(&self) -> usize {
        self.files().len()
    }

    /// Strip the resource prefix from a path to get the relative path.
    fn strip_prefix(&self, path: &Path) -> String {
        let path_str = path.to_string_lossy();

        // Strip absolute resource prefix if present
        let without_abs_prefix = path_str
            .strip_prefix(RESOURCE_PATH_PREFIX)
            .unwrap_or(&path_str);
        let without_abs_prefix = without_abs_prefix.trim_start_matches('/');

        // Strip the resource-specific prefix if present
        let relative = without_abs_prefix
            .strip_prefix(self.prefix)
            .unwrap_or(without_abs_prefix);
        let relative = relative.trim_start_matches('/');

        relative.to_string()
    }

    /// Lazily build and return the set of all file paths.
    fn files(&self) -> &HashSet<String> {
        self.files.get_or_init(|| {
            let mut files = HashSet::new();
            collect_files(self.dir, &mut files);
            files
        })
    }

    /// Lazily build and return the set of all directory paths.
    fn directories(&self) -> &HashSet<String> {
        self.directories.get_or_init(|| {
            let mut dirs = HashSet::new();
            collect_directories(self.dir, &mut dirs);
            dirs
        })
    }
}

// Note: EmbeddedResources automatically derives Send + Sync because all fields are:
// - dir: &'static Dir - static references are Send + Sync
// - prefix: &'static str - static string slices are Send + Sync
// - files/directories: OnceLock<HashSet<String>> - OnceLock<T> is Send + Sync when T is

// Implement EmbeddedResourceProvider trait for native targets
#[cfg(not(target_arch = "wasm32"))]
impl quarto_system_runtime::EmbeddedResourceProvider for EmbeddedResources {
    fn is_file(&self, path: &Path) -> bool {
        EmbeddedResources::is_file(self, path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        EmbeddedResources::is_dir(self, path)
    }

    fn read(&self, path: &Path) -> Option<&'static [u8]> {
        EmbeddedResources::read(self, path)
    }
}

/// Recursively collect all file paths from an embedded directory.
///
/// Note: `file.path()` returns the full path relative to the root of the
/// include_dir, so we don't need to track or add prefixes.
fn collect_files(dir: &Dir<'static>, files: &mut HashSet<String>) {
    for file in dir.files() {
        files.insert(file.path().to_string_lossy().to_string());
    }

    for subdir in dir.dirs() {
        collect_files(subdir, files);
    }
}

/// Recursively collect all directory paths from an embedded directory.
///
/// Note: `dir.path()` returns the full path relative to the root of the
/// include_dir, so we don't need to track or add prefixes.
fn collect_directories(dir: &Dir<'static>, dirs: &mut HashSet<String>) {
    for subdir in dir.dirs() {
        dirs.insert(subdir.path().to_string_lossy().to_string());
        collect_directories(subdir, dirs);
    }
}

/// Bootstrap 5.3.1 SCSS resources.
///
/// These files are embedded at compile time from the TS Quarto distribution.
/// Access them via the virtual path prefix `/__quarto_resources__/bootstrap/scss/`.
pub static BOOTSTRAP_RESOURCES: EmbeddedResources =
    EmbeddedResources::new(&BOOTSTRAP_DIR, "bootstrap/scss");

/// Bootstrap sass-utils resources.
///
/// Contains utility functions like `color-contrast.scss` that have inline
/// fallback variable definitions. This is critical for theme compatibility -
/// the self-contained `color-contrast()` function works even before Bootstrap
/// variables are loaded.
pub static SASS_UTILS_RESOURCES: EmbeddedResources =
    EmbeddedResources::new(&SASS_UTILS_DIR, "bootstrap/sass-utils");

/// Bootswatch theme resources.
///
/// Contains 25 Bootswatch theme files (cerulean, cosmo, cyborg, darkly, etc.)
/// that customize Bootstrap's appearance.
pub static THEMES_RESOURCES: EmbeddedResources =
    EmbeddedResources::new(&THEMES_DIR, "bootstrap/themes");

/// Quarto Bootstrap customization resources.
///
/// Contains Quarto's Bootstrap customization files:
/// - `_bootstrap-customize.scss` - Quarto defaults for heading sizes, etc.
/// - `_bootstrap-functions.scss` - Quarto color functions (theme-contrast, etc.)
/// - `_bootstrap-mixins.scss` - Quarto mixins
/// - `_bootstrap-rules.scss` - Quarto CSS rules
/// - `_bootstrap-variables.scss` - Quarto variables
pub static QUARTO_BOOTSTRAP_RESOURCES: EmbeddedResources =
    EmbeddedResources::new(&QUARTO_BOOTSTRAP_DIR, "bootstrap/quarto");

/// Get the default load paths for SASS compilation.
///
/// Returns paths that should be added to the SASS compiler's load paths
/// for Bootstrap compilation to work correctly.
pub fn default_load_paths() -> Vec<std::path::PathBuf> {
    vec![
        std::path::PathBuf::from(BOOTSTRAP_RESOURCES.full_prefix()),
        std::path::PathBuf::from(SASS_UTILS_RESOURCES.full_prefix()),
    ]
}

/// Get all embedded resource providers.
///
/// Returns references to all embedded resource collections for iteration.
pub fn all_resources() -> [&'static EmbeddedResources; 4] {
    [
        &BOOTSTRAP_RESOURCES,
        &SASS_UTILS_RESOURCES,
        &THEMES_RESOURCES,
        &QUARTO_BOOTSTRAP_RESOURCES,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_resources_embedded() {
        // Should have embedded files
        assert!(
            BOOTSTRAP_RESOURCES.file_count() > 0,
            "Bootstrap SCSS should have files"
        );
        println!(
            "Embedded {} Bootstrap SCSS files",
            BOOTSTRAP_RESOURCES.file_count()
        );
    }

    #[test]
    fn test_bootstrap_core_files_exist() {
        // Check for essential Bootstrap files
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("_variables.scss")),
            "_variables.scss should exist"
        );
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("_functions.scss")),
            "_functions.scss should exist"
        );
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("_mixins.scss")),
            "_mixins.scss should exist"
        );
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("bootstrap.scss")),
            "bootstrap.scss should exist"
        );
    }

    #[test]
    fn test_bootstrap_subdirectories() {
        // Check for subdirectories
        assert!(
            BOOTSTRAP_RESOURCES.is_dir(Path::new("mixins")),
            "mixins/ should exist"
        );
        assert!(
            BOOTSTRAP_RESOURCES.is_dir(Path::new("forms")),
            "forms/ should exist"
        );
        assert!(
            BOOTSTRAP_RESOURCES.is_dir(Path::new("utilities")),
            "utilities/ should exist"
        );
    }

    #[test]
    fn test_read_bootstrap_file() {
        let content = BOOTSTRAP_RESOURCES.read(Path::new("_variables.scss"));
        assert!(content.is_some(), "Should be able to read _variables.scss");

        let content = content.unwrap();
        assert!(content.len() > 1000, "Variables file should be substantial");

        // Check it's valid UTF-8 SCSS
        let content_str = BOOTSTRAP_RESOURCES.read_str(Path::new("_variables.scss"));
        assert!(
            content_str.is_some(),
            "Should be able to read as UTF-8 string"
        );
        assert!(
            content_str.unwrap().contains("$primary"),
            "Should contain $primary variable"
        );
    }

    #[test]
    fn test_path_prefix_stripping() {
        // Test various path formats
        assert!(BOOTSTRAP_RESOURCES.is_file(Path::new("_variables.scss")));
        assert!(BOOTSTRAP_RESOURCES.is_file(Path::new("bootstrap/scss/_variables.scss")));
        assert!(BOOTSTRAP_RESOURCES.is_file(Path::new(
            "/__quarto_resources__/bootstrap/scss/_variables.scss"
        )));
    }

    #[test]
    fn test_full_prefix() {
        assert_eq!(
            BOOTSTRAP_RESOURCES.full_prefix(),
            "/__quarto_resources__/bootstrap/scss"
        );
    }

    #[test]
    fn test_default_load_paths() {
        let paths = default_load_paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            paths[0].to_string_lossy(),
            "/__quarto_resources__/bootstrap/scss"
        );
        assert_eq!(
            paths[1].to_string_lossy(),
            "/__quarto_resources__/bootstrap/sass-utils"
        );
    }

    #[test]
    fn test_sass_utils_resources() {
        // Check color-contrast.scss exists (critical for theme compatibility)
        assert!(
            SASS_UTILS_RESOURCES.is_file(Path::new("color-contrast.scss")),
            "color-contrast.scss should exist"
        );

        // Check it contains the self-contained color-contrast function
        let content = SASS_UTILS_RESOURCES
            .read_str(Path::new("color-contrast.scss"))
            .unwrap();
        assert!(
            content.contains("@function color-contrast"),
            "Should contain color-contrast function"
        );
        // The self-contained version has inline variable definitions
        assert!(
            content.contains("$color-contrast-dark: $black !default"),
            "Should have inline fallback for $color-contrast-dark"
        );
    }

    #[test]
    fn test_themes_resources() {
        // Should have 25 Bootswatch themes
        let theme_count = THEMES_RESOURCES.file_count();
        assert!(
            theme_count >= 25,
            "Should have at least 25 theme files, found {}",
            theme_count
        );

        // Check some specific themes exist
        let themes = ["cerulean", "darkly", "slate", "cyborg", "vapor"];
        for theme in themes {
            let path = format!("{}.scss", theme);
            assert!(
                THEMES_RESOURCES.is_file(Path::new(&path)),
                "{} theme should exist",
                theme
            );
        }
    }

    #[test]
    fn test_theme_content_has_layer_markers() {
        // Themes should have layer boundary markers
        let content = THEMES_RESOURCES.read_str(Path::new("slate.scss")).unwrap();
        assert!(
            content.contains("/*-- scss:defaults --*/"),
            "Theme should have defaults layer marker"
        );
    }

    #[test]
    fn test_quarto_bootstrap_resources() {
        // Check Quarto Bootstrap customization files exist
        assert!(
            QUARTO_BOOTSTRAP_RESOURCES.is_file(Path::new("_bootstrap-customize.scss")),
            "_bootstrap-customize.scss should exist"
        );
        assert!(
            QUARTO_BOOTSTRAP_RESOURCES.is_file(Path::new("_bootstrap-functions.scss")),
            "_bootstrap-functions.scss should exist"
        );
        assert!(
            QUARTO_BOOTSTRAP_RESOURCES.is_file(Path::new("_bootstrap-mixins.scss")),
            "_bootstrap-mixins.scss should exist"
        );
        assert!(
            QUARTO_BOOTSTRAP_RESOURCES.is_file(Path::new("_bootstrap-rules.scss")),
            "_bootstrap-rules.scss should exist"
        );
    }

    #[test]
    fn test_quarto_customize_layer() {
        // The customize layer should have heading size customizations
        let content = QUARTO_BOOTSTRAP_RESOURCES
            .read_str(Path::new("_bootstrap-customize.scss"))
            .unwrap();
        assert!(
            content.contains("$h1-font-size"),
            "Should customize heading sizes"
        );
        assert!(
            content.contains("/*-- scss:defaults --*/"),
            "Should have layer boundary marker"
        );
    }

    #[test]
    fn test_all_resources() {
        let resources = all_resources();
        assert_eq!(resources.len(), 4);
        // Verify all have some files
        for res in resources {
            assert!(res.file_count() > 0, "Resource should have files");
        }
    }

    #[test]
    fn test_file_not_found() {
        assert!(!BOOTSTRAP_RESOURCES.is_file(Path::new("nonexistent.scss")));
        assert!(
            BOOTSTRAP_RESOURCES
                .read(Path::new("nonexistent.scss"))
                .is_none()
        );
    }

    #[test]
    fn test_dir_not_found() {
        assert!(!BOOTSTRAP_RESOURCES.is_dir(Path::new("nonexistent_dir")));
    }

    #[test]
    fn test_nested_file() {
        // Check a file in a subdirectory
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("mixins/_buttons.scss")),
            "File should exist at mixins/_buttons.scss"
        );
        // Check another nested file
        assert!(
            BOOTSTRAP_RESOURCES.is_file(Path::new("forms/_form-control.scss")),
            "File should exist at forms/_form-control.scss"
        );
    }
}
