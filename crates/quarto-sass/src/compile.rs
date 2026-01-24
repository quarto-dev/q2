//! High-level SASS compilation API for the render pipeline.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides a simplified API for compiling CSS from theme configuration.
//! It's the main entry point for the render pipeline's SASS compilation needs.
//!
//! # Architecture
//!
//! The compilation flow is:
//! 1. Extract `ThemeConfig` from `ConfigValue` (done by `ThemeConfig::from_config_value`)
//! 2. Process theme specs into layers (done by `process_theme_specs`)
//! 3. Assemble SCSS bundle (done by `assemble_with_user_layers`)
//! 4. Compile SCSS to CSS (done by grass on native, dart-sass on WASM)
//!
//! This module provides functions that orchestrate this entire flow.
//!
//! # Example
//!
//! ```rust,ignore
//! use quarto_sass::{ThemeConfig, ThemeContext, compile_theme_css};
//! use std::path::PathBuf;
//!
//! // From merged config
//! let theme_config = ThemeConfig::from_config_value(&merged_config)?;
//!
//! // Create context for path resolution
//! let context = ThemeContext::native(PathBuf::from("/project/doc"));
//!
//! // Compile to CSS
//! let css = compile_theme_css(&theme_config, &context)?;
//! ```

use std::path::Path;

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::bundle::{assemble_bootstrap, assemble_with_user_layers};
use crate::config::ThemeConfig;
use crate::error::SassError;
use crate::resources::default_load_paths;
use crate::themes::{ThemeContext, process_theme_specs};

// Native-only imports
#[cfg(not(target_arch = "wasm32"))]
use crate::resources::all_resources;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;

/// Cached default Bootstrap CSS (minified).
///
/// This is compiled once and reused for all documents that don't specify a theme.
#[cfg(not(target_arch = "wasm32"))]
static DEFAULT_CSS_CACHE: OnceLock<String> = OnceLock::new();

/// Compile CSS from theme configuration.
///
/// This is the main entry point for the render pipeline. It takes a `ThemeConfig`
/// (extracted from document/project config) and compiles the appropriate CSS.
///
/// # Arguments
///
/// * `config` - The theme configuration (themes and minification setting)
/// * `context` - The theme context for path resolution and runtime access
///
/// # Returns
///
/// Compiled CSS string on success.
///
/// # Errors
///
/// Returns an error if:
/// - Theme files cannot be loaded
/// - SCSS assembly fails
/// - SASS compilation fails
///
/// # Example
///
/// ```rust,ignore
/// use quarto_sass::{ThemeConfig, ThemeContext, compile_theme_css};
/// use std::path::PathBuf;
///
/// let config = ThemeConfig::from_config_value(&merged_config)?;
/// let context = ThemeContext::native(PathBuf::from("/project/doc"));
/// let css = compile_theme_css(&config, &context)?;
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_theme_css(
    config: &ThemeConfig,
    context: &ThemeContext<'_>,
) -> Result<String, SassError> {
    use quarto_system_runtime::sass_native::compile_scss_with_embedded;

    if !config.has_themes() {
        // No custom themes - use default Bootstrap
        return compile_default_css(context.runtime(), config.minified);
    }

    // Process theme specs into layers
    let result = process_theme_specs(&config.themes, context)?;

    // Assemble SCSS
    let scss = assemble_with_user_layers(&result.layers)?;

    // Build load paths: default paths + custom theme directories
    let mut load_paths = default_load_paths();
    load_paths.extend(result.load_paths);
    load_paths.extend(context.load_paths().iter().cloned());

    // Create a combined resource provider from all embedded resources
    let resources = all_resources();

    // Compile
    compile_scss_with_embedded(
        context.runtime(),
        &resources,
        &scss,
        &load_paths,
        config.minified,
    )
    .map_err(|e| SassError::CompilationFailed {
        message: e.to_string(),
    })
}

/// Compile CSS from ConfigValue directly.
///
/// This is a convenience function that combines config extraction and compilation.
/// Use this when you have a merged `ConfigValue` and want to get CSS in one step.
///
/// # Arguments
///
/// * `config` - The merged configuration (project + document)
/// * `document_dir` - Directory containing the input document (for relative path resolution)
/// * `runtime` - The system runtime for file access
///
/// # Returns
///
/// Compiled CSS string on success.
///
/// # Errors
///
/// Returns an error if:
/// - Theme configuration extraction fails
/// - Theme files cannot be loaded
/// - SCSS compilation fails
///
/// # Example
///
/// ```rust,ignore
/// use quarto_sass::compile_css_from_config;
/// use quarto_system_runtime::NativeRuntime;
/// use std::path::PathBuf;
///
/// let runtime = NativeRuntime::new();
/// let css = compile_css_from_config(
///     &merged_config,
///     &PathBuf::from("/project/doc"),
///     &runtime,
/// )?;
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_css_from_config(
    config: &ConfigValue,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<String, SassError> {
    // Extract theme config
    let theme_config = ThemeConfig::from_config_value(config)?;

    // Create context
    let context = ThemeContext::new(document_dir.to_path_buf(), runtime);

    // Compile
    compile_theme_css(&theme_config, &context)
}

/// Compile the default Bootstrap CSS.
///
/// This compiles Bootstrap with Quarto's customizations but without any
/// Bootswatch theme or custom SCSS. The result is cached for performance.
///
/// # Arguments
///
/// * `runtime` - The system runtime for file access
/// * `minified` - Whether to produce minified CSS
///
/// # Returns
///
/// Compiled CSS string on success.
///
/// # Performance
///
/// The minified CSS is cached after first compilation. Subsequent calls
/// return the cached value immediately (if minified=true). Non-minified
/// compilation is not cached.
///
/// # Example
///
/// ```rust,ignore
/// use quarto_sass::compile_default_css;
/// use quarto_system_runtime::NativeRuntime;
///
/// let runtime = NativeRuntime::new();
/// let css = compile_default_css(&runtime, true)?;
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_default_css(
    runtime: &dyn SystemRuntime,
    minified: bool,
) -> Result<String, SassError> {
    use quarto_system_runtime::sass_native::compile_scss_with_embedded;

    // Return cached version if available (only for minified)
    if minified {
        if let Some(cached) = DEFAULT_CSS_CACHE.get() {
            return Ok(cached.clone());
        }
    }

    // Assemble default Bootstrap SCSS
    let scss = assemble_bootstrap()?;

    // Get load paths and resources
    let load_paths = default_load_paths();
    let resources = all_resources();

    // Compile
    let css = compile_scss_with_embedded(runtime, &resources, &scss, &load_paths, minified)
        .map_err(|e| SassError::CompilationFailed {
            message: e.to_string(),
        })?;

    // Cache minified result
    if minified {
        let _ = DEFAULT_CSS_CACHE.set(css.clone());
    }

    Ok(css)
}

/// Clear the default CSS cache.
///
/// This is primarily useful for testing. In production, the cache persists
/// for the lifetime of the process.
#[cfg(test)]
pub fn clear_default_css_cache() {
    // OnceLock doesn't have a clear method, so we can't actually clear it.
    // This function exists for API symmetry but does nothing.
    // In tests, just accept that the cache persists.
}

// =============================================================================
// WASM Implementations
// =============================================================================
//
// WASM uses dart-sass via the JavaScript bridge (async).
// Bootstrap SCSS resources are pre-populated in the VFS by wasm-quarto-hub-client.

/// Compile CSS from theme configuration (WASM version).
///
/// This is the main entry point for the render pipeline. It takes a `ThemeConfig`
/// (extracted from document/project config) and compiles the appropriate CSS.
///
/// # Arguments
///
/// * `config` - The theme configuration (themes and minification setting)
/// * `context` - The theme context for path resolution and runtime access
///
/// # Returns
///
/// Compiled CSS string on success.
#[cfg(target_arch = "wasm32")]
pub async fn compile_theme_css(
    config: &ThemeConfig,
    context: &ThemeContext<'_>,
) -> Result<String, SassError> {
    if !config.has_themes() {
        // No custom themes - use default Bootstrap
        return compile_default_css(context.runtime(), config.minified).await;
    }

    // Process theme specs into layers
    let result = process_theme_specs(&config.themes, context)?;

    // Assemble SCSS
    let scss = assemble_with_user_layers(&result.layers)?;

    // Build load paths: default paths + custom theme directories
    let mut load_paths = default_load_paths();
    load_paths.extend(result.load_paths);
    load_paths.extend(context.load_paths().iter().cloned());

    // Compile via JS bridge
    context
        .runtime()
        .compile_sass(&scss, &load_paths, config.minified)
        .await
        .map_err(|e| SassError::CompilationFailed {
            message: e.to_string(),
        })
}

/// Compile CSS from ConfigValue directly (WASM version).
///
/// This is a convenience function that combines config extraction and compilation.
/// Use this when you have a merged `ConfigValue` and want to get CSS in one step.
#[cfg(target_arch = "wasm32")]
pub async fn compile_css_from_config(
    config: &ConfigValue,
    document_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<String, SassError> {
    // Extract theme config
    let theme_config = ThemeConfig::from_config_value(config)?;

    // Create context
    let context = ThemeContext::new(document_dir.to_path_buf(), runtime);

    // Compile
    compile_theme_css(&theme_config, &context).await
}

/// Compile the default Bootstrap CSS (WASM version).
///
/// This compiles Bootstrap with Quarto's customizations but without any
/// Bootswatch theme or custom SCSS.
///
/// Note: Unlike the native version, this does NOT cache the result.
/// Caching should be handled by the JavaScript layer (SassCacheManager)
/// which uses IndexedDB for persistent caching across sessions.
#[cfg(target_arch = "wasm32")]
pub async fn compile_default_css(
    runtime: &dyn SystemRuntime,
    minified: bool,
) -> Result<String, SassError> {
    // Assemble default Bootstrap SCSS
    let scss = assemble_bootstrap()?;

    // Get load paths (these point to VFS paths populated by wasm-quarto-hub-client)
    let load_paths = default_load_paths();

    // Compile via JS bridge
    runtime
        .compile_sass(&scss, &load_paths, minified)
        .await
        .map_err(|e| SassError::CompilationFailed {
            message: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::ThemeSpec;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;

    #[test]
    fn test_compile_default_css() {
        let runtime = NativeRuntime::new();
        let css = compile_default_css(&runtime, true).unwrap();

        // Should have Bootstrap classes
        assert!(css.contains(".btn"), "Should contain .btn class");
        assert!(
            css.contains(".container"),
            "Should contain .container class"
        );

        // Should be minified (few newlines)
        let newlines = css.matches('\n').count();
        assert!(
            newlines < 100,
            "Minified CSS should have few newlines, got {}",
            newlines
        );

        // Should be a reasonable size
        assert!(
            css.len() > 100_000,
            "Bootstrap CSS should be > 100KB, got {} bytes",
            css.len()
        );
    }

    #[test]
    fn test_compile_default_css_expanded() {
        let runtime = NativeRuntime::new();
        let css = compile_default_css(&runtime, false).unwrap();

        // Should have Bootstrap classes
        assert!(css.contains(".btn"));

        // Should NOT be minified (many newlines)
        let newlines = css.matches('\n').count();
        assert!(
            newlines > 1000,
            "Expanded CSS should have many newlines, got {}",
            newlines
        );
    }

    #[test]
    fn test_compile_theme_css_no_themes() {
        let runtime = NativeRuntime::new();
        let config = ThemeConfig::default_bootstrap();
        let context = ThemeContext::new(PathBuf::from("/doc"), &runtime);

        let css = compile_theme_css(&config, &context).unwrap();

        // Should be Bootstrap CSS
        assert!(css.contains(".btn"));
        assert!(css.contains(".container"));
    }

    #[test]
    fn test_compile_theme_css_builtin_theme() {
        let runtime = NativeRuntime::new();
        let themes = vec![ThemeSpec::parse("cosmo").unwrap()];
        let config = ThemeConfig::new(themes, true);
        let context = ThemeContext::new(PathBuf::from("/doc"), &runtime);

        let css = compile_theme_css(&config, &context).unwrap();

        // Should have Bootstrap classes
        assert!(css.contains(".btn"));
        assert!(css.contains(".container"));
    }

    #[test]
    fn test_compile_theme_css_multiple_themes() {
        let runtime = NativeRuntime::new();
        let themes = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("flatly").unwrap(),
        ];
        let config = ThemeConfig::new(themes, true);
        let context = ThemeContext::new(PathBuf::from("/doc"), &runtime);

        let css = compile_theme_css(&config, &context).unwrap();

        // Should compile successfully with merged themes
        assert!(css.contains(".btn"));
    }

    #[test]
    fn test_compile_css_from_config_empty() {
        use quarto_pandoc_types::ConfigValueKind;
        use quarto_source_map::SourceInfo;

        let runtime = NativeRuntime::new();
        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let css = compile_css_from_config(&config, Path::new("/doc"), &runtime).unwrap();

        // Should produce default Bootstrap CSS
        assert!(css.contains(".btn"));
    }

    #[test]
    fn test_compile_css_from_config_with_theme() {
        use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
        use quarto_source_map::SourceInfo;
        use yaml_rust2::Yaml;

        let runtime = NativeRuntime::new();

        // Build config: { format: { html: { theme: "cosmo" } } }
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let html_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::default(),
            value: theme_value,
        };

        let html_value = ConfigValue {
            value: ConfigValueKind::Map(vec![html_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let format_entry = ConfigMapEntry {
            key: "html".to_string(),
            key_source: SourceInfo::default(),
            value: html_value,
        };

        let format_value = ConfigValue {
            value: ConfigValueKind::Map(vec![format_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "format".to_string(),
            key_source: SourceInfo::default(),
            value: format_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::default(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let css = compile_css_from_config(&config, Path::new("/doc"), &runtime).unwrap();

        // Should compile successfully with theme
        assert!(css.contains(".btn"));
    }

    #[test]
    fn test_compile_default_css_caching() {
        let runtime = NativeRuntime::new();

        // First compilation
        let css1 = compile_default_css(&runtime, true).unwrap();

        // Second compilation (should use cache)
        let css2 = compile_default_css(&runtime, true).unwrap();

        // Should be identical
        assert_eq!(css1, css2);
    }

    #[test]
    fn test_compile_theme_css_with_custom_file() {
        // Use the test fixture
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");

        let runtime = NativeRuntime::new();
        let themes = vec![ThemeSpec::parse("override.scss").unwrap()];
        let config = ThemeConfig::new(themes, true);
        let context = ThemeContext::new(fixture_dir, &runtime);

        let css = compile_theme_css(&config, &context).unwrap();

        // Should have Bootstrap classes
        assert!(css.contains(".btn"));

        // Should have custom rule from the fixture
        assert!(css.contains(".custom-rule"));
    }

    #[test]
    fn test_compile_theme_css_builtin_then_custom() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_dir = PathBuf::from(manifest_dir).join("test-fixtures/custom");

        let runtime = NativeRuntime::new();
        let themes = vec![
            ThemeSpec::parse("cosmo").unwrap(),
            ThemeSpec::parse("override.scss").unwrap(),
        ];
        let config = ThemeConfig::new(themes, true);
        let context = ThemeContext::new(fixture_dir, &runtime);

        let css = compile_theme_css(&config, &context).unwrap();

        // Should have Bootstrap classes
        assert!(css.contains(".btn"));

        // Should have custom rule
        assert!(css.contains(".custom-rule"));
    }
}
