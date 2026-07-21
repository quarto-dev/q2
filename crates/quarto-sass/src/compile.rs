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

use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::SystemRuntime;

use crate::bundle::assemble_with_user_layers;
use crate::config::ThemeConfig;
use crate::error::SassError;
use crate::resources::default_load_paths;
use crate::themes::{ThemeContext, process_theme_specs};

// Native-only imports
#[cfg(not(target_arch = "wasm32"))]
use crate::resources::all_resources;

use std::sync::OnceLock;

/// Cached default Bootstrap CSS (minified).
///
/// Compiled once per process and reused for every render of a document
/// that doesn't specify a theme. Both the native and WASM entry points
/// consult this cache; on WASM it's critical for keystroke-rate renders
/// in hub-client because the underlying dart-sass bridge call is
/// expensive (~100-500 ms per compile).
static DEFAULT_CSS_CACHE: OnceLock<String> = OnceLock::new();

/// Assemble the SCSS bundle for a themed configuration.
///
/// This extracts the assembly step from `compile_theme_css`: processing theme
/// specs into layers, loading the title block layer, and assembling the final
/// SCSS string. It also computes the load paths needed for compilation.
///
/// Only call this when `config.has_themes()` is true. For the default (no theme)
/// case, use `DEFAULT_CSS` directly instead of compiling.
///
/// # Returns
///
/// A tuple of `(scss_string, load_paths)` ready for compilation.
pub fn assemble_theme_scss(
    config: &ThemeConfig,
    context: &ThemeContext<'_>,
) -> Result<(String, Vec<PathBuf>), SassError> {
    use crate::bundle::{
        load_copy_code_layer, load_embed_example_layer, load_highlight_layer,
        load_title_block_layer,
    };

    // Process theme specs into layers
    let result = process_theme_specs(&config.themes, context)?;

    // Build user layers: title block + syntax-highlight defaults first
    // (like TS Quarto's order for built-in user layers), then any theme
    // layers from the config. User themes can override any `.hl-*` or
    // title-block rule by declaring the same selector in a later layer.
    let highlight_layer = load_highlight_layer()?;
    let embed_example_layer = load_embed_example_layer()?;
    let copy_code_layer = load_copy_code_layer()?;
    let mut user_layers = Vec::new();
    // `title-block-style: plain|none` drops the title-block layer
    // (bd-gx9cic8z P6); all other built-in layers are unconditional.
    if config.title_block_layer {
        user_layers.push(load_title_block_layer()?);
    }
    user_layers.extend([highlight_layer, embed_example_layer, copy_code_layer]);
    user_layers.extend(result.layers);

    // Assemble SCSS
    let scss = assemble_with_user_layers(&user_layers)?;

    // Build load paths: default paths + custom theme directories
    let mut load_paths = default_load_paths();
    load_paths.extend(result.load_paths);
    load_paths.extend(context.load_paths().iter().cloned());

    Ok((scss, load_paths))
}

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

    let (scss, load_paths) = assemble_theme_scss(config, context)?;

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

/// Compile theme CSS with an additional doc-derived SassLayer of
/// `$variable: value;` assignments prepended to the user layers.
///
/// This is the entry point used by `CompileThemeCssStage` to thread
/// per-document metadata (e.g. `$sidebar-border` for docked sidebars,
/// future `$sidebar-bg` / `$navbar-bg` / etc.) into the SCSS bundle.
///
/// `doc_vars` slots in as the **last** user layer so it lands at the
/// front of the merged-defaults section and wins the `!default` race
/// against the framework's defaults — no `!default` flag needed in
/// `doc_vars` itself. Mirrors Q1's `format-html-scss.ts` synthesis.
///
/// When `doc_vars.is_empty()`, this is byte-equivalent to:
/// - `compile_default_css(...)` if `!config.has_themes()`,
/// - `compile_theme_css(config, context)` otherwise.
///
/// Notably, when `doc_vars` is non-empty the in-process
/// `DEFAULT_CSS_CACHE` is **bypassed** because the compiled output now
/// depends on metadata. Cross-session caching is the caller's
/// responsibility (the stage handles it via `cache_get_lru` /
/// `cache_set_lru` keyed on a hash that includes doc-vars).
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_with_doc_vars(
    config: &ThemeConfig,
    context: &ThemeContext<'_>,
    doc_vars: &crate::SassLayer,
) -> Result<String, SassError> {
    use crate::bundle::{
        load_copy_code_layer, load_embed_example_layer, load_highlight_layer,
        load_title_block_layer,
    };
    use crate::themes::process_theme_specs;
    use quarto_system_runtime::sass_native::compile_scss_with_embedded;

    // Fast paths: no doc-vars to inject — defer to existing entry points
    // so we keep the OnceLock cache for the no-theme case. When the
    // title-block layer is dropped (`title-block-style: plain|none`),
    // the shared default bundle no longer matches, so fall through to
    // a direct assembly instead (the themed path honors the flag via
    // `assemble_theme_scss`).
    if doc_vars.is_empty() {
        if config.has_themes() {
            return compile_theme_css(config, context);
        }
        if config.title_block_layer {
            return compile_default_css(context.runtime(), config.minified);
        }
    }

    // Build user layers: title-block (unless dropped by
    // `title-block-style: plain|none`) + highlight built-ins,
    // matching `compile_default_css` and `assemble_theme_scss`, then any
    // theme layers, then doc_vars LAST so it lands at the top of the
    // merged-defaults section and wins the `!default` race.
    let highlight_layer = load_highlight_layer()?;
    let embed_example_layer = load_embed_example_layer()?;
    let copy_code_layer = load_copy_code_layer()?;
    let mut user_layers = Vec::new();
    if config.title_block_layer {
        user_layers.push(load_title_block_layer()?);
    }
    user_layers.extend([highlight_layer, embed_example_layer, copy_code_layer]);

    let mut load_paths = default_load_paths();
    if config.has_themes() {
        let result = process_theme_specs(&config.themes, context)?;
        user_layers.extend(result.layers);
        load_paths.extend(result.load_paths);
    }
    load_paths.extend(context.load_paths().iter().cloned());

    user_layers.push(doc_vars.clone());

    let scss = crate::assemble_with_user_layers(&user_layers)?;
    let resources = all_resources();
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
/// Use this when you have a format-flattened `ConfigValue` (as produced by
/// MetadataMergeStage) and want to get CSS in one step.
///
/// # Arguments
///
/// * `config` - The format-flattened merged configuration (theme at top level)
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
    use crate::bundle::{
        load_copy_code_layer, load_embed_example_layer, load_highlight_layer,
        load_title_block_layer,
    };
    use quarto_system_runtime::sass_native::compile_scss_with_embedded;

    // Return cached version if available (only for minified)
    if minified && let Some(cached) = DEFAULT_CSS_CACHE.get() {
        return Ok(cached.clone());
    }

    // Load built-in user layers: title block styling + default syntax-
    // highlight colors. Both ship with Quarto and are always included.
    let title_block_layer = load_title_block_layer()?;
    let highlight_layer = load_highlight_layer()?;
    let embed_example_layer = load_embed_example_layer()?;
    let copy_code_layer = load_copy_code_layer()?;

    // Assemble SCSS: Bootstrap + Quarto + title block + highlight + embed-example defaults
    let scss = assemble_with_user_layers(&[
        title_block_layer,
        highlight_layer,
        embed_example_layer,
        copy_code_layer,
    ])?;

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

/// Compile Quarto's reveal.js theme CSS (native).
///
/// Assembles the reveal framework + Quarto reveal layers (see
/// [`crate::assemble_reveal_scss`]) into a single SCSS bundle and compiles it in
/// one `grass` pass (decision D1 — unified compilation). The result is a
/// self-contained reveal theme stylesheet: reveal's base rules driven by
/// `--r-*` custom properties carrying Quarto's overridden values, plus Quarto's
/// look-fixing rule overrides.
///
/// `theme_layers` are the resolved built-in/user theme layers (empty = the
/// white-equivalent default). `load_paths` are extra directories for resolving
/// `@use`/`@import` inside *user* theme files (built-in reveal SCSS needs none —
/// it only uses the built-in `sass:color` / `sass:meta` modules and is otherwise
/// self-contained).
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_reveal_theme_css(
    runtime: &dyn SystemRuntime,
    minified: bool,
    theme_layers: &[crate::SassLayer],
    load_paths: &[PathBuf],
) -> Result<String, SassError> {
    use quarto_system_runtime::sass_native::compile_scss;

    let scss = crate::bundle::assemble_reveal_scss(theme_layers)?;
    compile_scss(runtime, &scss, load_paths, minified).map_err(|e| SassError::CompilationFailed {
        message: e.to_string(),
    })
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

    let (scss, load_paths) = assemble_theme_scss(config, context)?;

    // Compile via JS bridge
    context
        .runtime()
        .compile_sass(&scss, &load_paths, config.minified)
        .await
        .map_err(|e| SassError::CompilationFailed {
            message: e.to_string(),
        })
}

/// WASM mirror of [`compile_with_doc_vars`]. See native version for full
/// documentation on the doc-vars layer's role in the user-layer ordering.
#[cfg(target_arch = "wasm32")]
pub async fn compile_with_doc_vars(
    config: &ThemeConfig,
    context: &ThemeContext<'_>,
    doc_vars: &crate::SassLayer,
) -> Result<String, SassError> {
    use crate::bundle::{
        load_copy_code_layer, load_embed_example_layer, load_highlight_layer,
        load_title_block_layer,
    };
    use crate::themes::process_theme_specs;

    // Same fast-path rule as the native variant: the shared default
    // bundle only matches when the title-block layer is included
    // (`title-block-style: plain|none` drops it — bd-gx9cic8z P6).
    if doc_vars.is_empty() {
        if config.has_themes() {
            return compile_theme_css(config, context).await;
        }
        if config.title_block_layer {
            return compile_default_css(context.runtime(), config.minified).await;
        }
    }

    let highlight_layer = load_highlight_layer()?;
    let embed_example_layer = load_embed_example_layer()?;
    let copy_code_layer = load_copy_code_layer()?;
    let mut user_layers = Vec::new();
    if config.title_block_layer {
        user_layers.push(load_title_block_layer()?);
    }
    user_layers.extend([highlight_layer, embed_example_layer, copy_code_layer]);

    let mut load_paths = default_load_paths();
    if config.has_themes() {
        let result = process_theme_specs(&config.themes, context)?;
        user_layers.extend(result.layers);
        load_paths.extend(result.load_paths);
    }
    load_paths.extend(context.load_paths().iter().cloned());

    user_layers.push(doc_vars.clone());

    let scss = crate::assemble_with_user_layers(&user_layers)?;

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
/// Use this when you have a format-flattened `ConfigValue` (as produced by
/// MetadataMergeStage) and want to get CSS in one step.
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
/// Cached in-process via [`DEFAULT_CSS_CACHE`] (the same `OnceLock` the
/// native entry uses). First call per WASM module lifetime compiles;
/// subsequent calls return a clone of the cached string in nanoseconds.
/// The dart-sass JS bridge is expensive (~100-500 ms), so this cache is
/// critical for hub-client's keystroke-rate renders on documents with
/// no theme.
///
/// Cross-session persistence is handled at a higher layer by
/// `CompileThemeCssStage` routing through `runtime.cache_get`/`cache_set`
/// (see the fix plan at
/// `claude-notes/plans/2026-04-18-wasm-scss-cache-regression.md`).
#[cfg(target_arch = "wasm32")]
pub async fn compile_default_css(
    runtime: &dyn SystemRuntime,
    minified: bool,
) -> Result<String, SassError> {
    use crate::bundle::{
        load_copy_code_layer, load_embed_example_layer, load_highlight_layer,
        load_title_block_layer,
    };

    // Return cached version if available (only for minified, matching
    // native). Minified is always true in practice for hub-client.
    if minified {
        if let Some(cached) = DEFAULT_CSS_CACHE.get() {
            return Ok(cached.clone());
        }
    }

    // Load built-in user layers: title block styling + default syntax-
    // highlight colors. Both ship with Quarto and are always included.
    // This mirrors the native `compile_default_css`. Without the
    // highlight layer, documents without an explicit `theme:` frontmatter
    // entry would render code blocks with `hl-*` span classes but no
    // associated colors.
    let title_block_layer = load_title_block_layer()?;
    let highlight_layer = load_highlight_layer()?;
    let embed_example_layer = load_embed_example_layer()?;
    let copy_code_layer = load_copy_code_layer()?;

    // Assemble SCSS: Bootstrap + Quarto + title block + highlight + embed-example defaults
    let scss = assemble_with_user_layers(&[
        title_block_layer,
        highlight_layer,
        embed_example_layer,
        copy_code_layer,
    ])?;

    // Get load paths (these point to VFS paths populated by wasm-quarto-hub-client)
    let load_paths = default_load_paths();

    // Compile via JS bridge
    let css = runtime
        .compile_sass(&scss, &load_paths, minified)
        .await
        .map_err(|e| SassError::CompilationFailed {
            message: e.to_string(),
        })?;

    // Cache minified result
    if minified {
        let _ = DEFAULT_CSS_CACHE.set(css.clone());
    }

    Ok(css)
}

/// Compile Quarto's reveal.js theme CSS (WASM mirror of the native entry).
///
/// See the native [`compile_reveal_theme_css`] for the architecture. Compiles
/// via the dart-sass JS bridge. `load_paths` resolve `@use`/`@import` inside
/// user theme files; built-in reveal SCSS needs none.
#[cfg(target_arch = "wasm32")]
pub async fn compile_reveal_theme_css(
    runtime: &dyn SystemRuntime,
    minified: bool,
    theme_layers: &[crate::SassLayer],
    load_paths: &[PathBuf],
) -> Result<String, SassError> {
    let scss = crate::bundle::assemble_reveal_scss(theme_layers)?;
    runtime
        .compile_sass(&scss, load_paths, minified)
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

        // Should have title block styles
        assert!(
            css.contains(".quarto-title-meta"),
            "Should contain .quarto-title-meta class from title-block.scss"
        );

        // Should have the base #title-block-header rule ported from TS Quarto's
        // _quarto-rules.scss: the unconditional bottom margin that separates the
        // title block from the first article section. Without it the title block
        // sits flush against the content (bd-btjkyylx). This is distinct from the
        // responsive `body.nav-sidebar #title-block-header{margin-block-end:0}`
        // override, which collapses this margin on small screens — the `1rem`
        // value uniquely identifies the base rule.
        assert!(
            css.contains("#title-block-header{margin-block-end:1rem"),
            "Should contain the base #title-block-header{{margin-block-end:1rem}} \
             rule ported from _quarto-rules.scss (bd-btjkyylx)"
        );

        // Should have the task-list rules ported from TS Quarto's
        // _quarto-rules.scss (bd-obkvhlam): the ul.task-list indent and the
        // checkbox right-margin that space `<input type="checkbox">` from the
        // item text emitted by the HTML writer's task-list rendering.
        assert!(
            css.contains("ul.task-list{padding-left:1em}"),
            "Should contain the ul.task-list rule ported from _quarto-rules.scss (bd-obkvhlam)"
        );
        assert!(
            css.contains("input[type=checkbox]{margin-right:.5ch}"),
            "Should contain the checkbox margin rule ported from _quarto-rules.scss (bd-obkvhlam)"
        );

        // Should have default syntax-highlight rules for `.hl-*` classes
        // emitted by the HTML writer for tree-sitter captures.
        assert!(
            css.contains(".hl-keyword"),
            "Should contain .hl-keyword rule from highlight.scss"
        );
        assert!(
            css.contains(".hl-function-builtin"),
            "Should contain nested-capture .hl-function-builtin rule"
        );

        // Should have code-copy-button rules from the shared copy-code.scss
        // layer (bd-lg6t6qfy extracted these out of _bootstrap-rules.scss; the
        // HTML output must be unchanged by that extraction).
        assert!(
            css.contains(".code-copy-button"),
            "Should contain .code-copy-button rule from copy-code.scss"
        );
        assert!(
            css.contains(".code-copy-outer-scaffold"),
            "Should contain the .code-copy-outer-scaffold positioning context"
        );

        // Should have Quarto page-footer layout rules (ported from Q1).
        assert!(
            css.contains(".nav-footer"),
            "Should contain .nav-footer layout from ported page-footer SCSS"
        );
        assert!(
            css.contains(".nav-footer-left"),
            "Should contain .nav-footer-left responsive rules"
        );
        assert!(
            css.contains(".footer-items"),
            "Should contain .footer-items rule for inline flex layout"
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
    fn test_compile_reveal_theme_includes_highlight_rules() {
        // bd-ehyyfpjj: `format: revealjs` code blocks receive `hl-*` span
        // annotations from `CodeHighlightStage`, but render UNCOLORED unless
        // the compiled reveal theme CSS also carries the `.hl-*` colour
        // rules. Every HTML compile bundles `highlight.scss` via
        // `load_highlight_layer`; the reveal path must include it too, or
        // render/preview/hub-client all show plain (uncolored) code.
        let runtime = NativeRuntime::new();
        // Empty theme layers → the default (white-equivalent) reveal theme.
        let css = compile_reveal_theme_css(&runtime, true, &[], &[]).unwrap();
        assert!(
            css.contains(".hl-keyword"),
            "reveal theme CSS must contain .hl-keyword from highlight.scss"
        );
        assert!(
            css.contains(".hl-function-builtin"),
            "reveal theme CSS must contain the nested-capture .hl-function-builtin rule"
        );
    }

    #[test]
    fn test_compile_reveal_theme_includes_copy_code_rules() {
        // bd-lg6t6qfy: `format: revealjs` emits the same copy-button scaffold
        // (`.code-copy-outer-scaffold` / `.code-copy-button`) as HTML, but the
        // reveal theme CSS historically shipped none of the styling — so the
        // button rendered as an empty UA-bordered box (bd-fu1a5g6l suppressed
        // it). Every HTML compile bundles `copy-code.scss` via
        // `load_copy_code_layer`; `assemble_reveal_scss` must include it too,
        // or render/preview/hub-client show an unstyled (or suppressed) button.
        let runtime = NativeRuntime::new();
        // Empty theme layers → the default (white-equivalent) reveal theme.
        let css = compile_reveal_theme_css(&runtime, true, &[], &[]).unwrap();
        assert!(
            css.contains(".code-copy-button"),
            "reveal theme CSS must contain .code-copy-button from copy-code.scss"
        );
        assert!(
            css.contains(".code-copy-outer-scaffold"),
            "reveal theme CSS must contain the .code-copy-outer-scaffold scaffold rule"
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
            source_info: SourceInfo::for_test(),
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

        // Build flattened config: { theme: "cosmo" }
        let theme_value = ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String("cosmo".to_string())),
            source_info: SourceInfo::for_test(),
            merge_op: quarto_pandoc_types::MergeOp::Concat,
        };

        let root_entry = ConfigMapEntry {
            key: "theme".to_string(),
            key_source: SourceInfo::for_test(),
            value: theme_value,
        };

        let config = ConfigValue {
            value: ConfigValueKind::Map(vec![root_entry]),
            source_info: SourceInfo::for_test(),
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

    /// Phase 1 of the sidebar-vertical-border port (bd-k8y0).
    ///
    /// Q1 emits `.sidebar.sidebar-navigation:not(.rollup) { border-right:
    /// 1px solid $table-border-color !important; }` when `$sidebar-border`
    /// is truthy (`quarto-cli/.../quarto-nav.scss:552-556`). The rule is
    /// what produces the faint vertical line between a docked sidebar and
    /// main content on every Q1 docked-sidebar website.
    ///
    /// `$sidebar-border` defaults to `false !default` in
    /// `_bootstrap-variables.scss:173`, so this test injects an
    /// unconditional `$sidebar-border: true;` via a user layer (which lands
    /// before the framework defaults — see `assemble_with_user_layers`
    /// ordering in `bundle.rs:283-295`). That is exactly the seam Phase 2
    /// of the plan will use to thread doc-derived variables through.
    #[test]
    fn test_sidebar_border_rule_emits_when_variable_is_true() {
        use crate::bundle::assemble_with_user_layers;
        use crate::layer::parse_layer_from_parts;
        use quarto_system_runtime::sass_native::compile_scss_with_embedded;

        let runtime = NativeRuntime::new();
        let resources = all_resources();
        let load_paths = default_load_paths();

        // Doc-vars layer: a non-`!default` assignment, like Q1's
        // synthesized `$sidebar-border: <bool>;` snippet from
        // format-html-scss.ts. No `!default` so it is unconditional and
        // wins against the framework's `$sidebar-border: false !default;`.
        let doc_vars = parse_layer_from_parts("", "$sidebar-border: true;", "", "", "");
        let scss = assemble_with_user_layers(&[doc_vars]).unwrap();

        let css =
            compile_scss_with_embedded(&runtime, &resources, &scss, &load_paths, true).unwrap();

        // The rule should fire. Match on the selector + the property so
        // we don't hinge the test on the exact color (which inherits from
        // `$table-border-color` and may shift if the framework default
        // changes).
        assert!(
            css.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "$sidebar-border=true must produce a .sidebar.sidebar-navigation:not(.rollup) rule"
        );
        // Look for the border-right within the surrounding rule body.
        let rule_idx = css
            .find(".sidebar.sidebar-navigation:not(.rollup)")
            .expect("selector present");
        let rule_tail = &css[rule_idx..rule_idx.saturating_add(400).min(css.len())];
        assert!(
            rule_tail.contains("border-right:1px solid")
                || rule_tail.contains("border-right: 1px solid"),
            "rule body must declare a 1px border-right, got: {}",
            rule_tail
        );
        assert!(
            rule_tail.contains("!important"),
            "rule must carry !important to match Q1, got: {}",
            rule_tail
        );
    }

    /// Mirror of the previous test for the off path: when the framework
    /// default `$sidebar-border: false !default;` is left untouched (no
    /// doc-vars layer overriding it), no rule should be emitted. This
    /// guards against accidentally hardcoding the rule.
    #[test]
    fn test_sidebar_border_rule_absent_when_variable_is_false() {
        let runtime = NativeRuntime::new();
        let css = compile_default_css(&runtime, true).unwrap();
        assert!(
            !css.contains(".sidebar.sidebar-navigation:not(.rollup)"),
            "no .sidebar.sidebar-navigation:not(.rollup) rule should appear when \
             $sidebar-border is false (its framework default)"
        );
    }

    /// Q1 parity: `#quarto-sidebar > * { padding-right: 1em }` puts a
    /// 1em gutter between the sidebar content (toggle dongles, links)
    /// and the right edge of the sidebar column — visually separating
    /// the controls from the `$sidebar-border` separator. See
    /// `quarto-cli/.../quarto-nav.scss:623-628`.
    #[test]
    fn test_quarto_sidebar_children_have_right_padding() {
        let runtime = NativeRuntime::new();
        let css = compile_default_css(&runtime, true).unwrap();
        // grass minifies whitespace, but the selector + property pair
        // should appear together in some form. We tolerate the variants
        // grass might emit (with or without a space inside `1em`).
        let needle1 = "#quarto-sidebar>*{padding-right:1em}";
        let needle2 = "#quarto-sidebar > * { padding-right: 1em }";
        assert!(
            css.contains(needle1) || css.contains(needle2),
            "missing `#quarto-sidebar > * {{ padding-right: 1em }}` (Q1 parity), \
             expected to find `{}` (or expanded form) in compiled default CSS",
            needle1
        );
    }

    /// Q1 parity: `.quarto-container { min-height: calc(100vh - 132px) }`
    /// stretches the page area (and therefore the sidebar grid cell)
    /// to at least viewport-height minus the navbar/footer composite.
    /// Without it, a short page leaves the sidebar (and its border)
    /// stopping partway down the viewport. See
    /// `quarto-cli/.../quarto-nav.scss:53-55`.
    #[test]
    fn test_quarto_container_min_height_fills_viewport() {
        let runtime = NativeRuntime::new();
        let css = compile_default_css(&runtime, true).unwrap();
        // Tolerate grass's spacing variations inside the calc() expr.
        assert!(
            css.contains(".quarto-container{min-height:calc(100vh - 132px)}")
                || css.contains(".quarto-container{min-height:calc(100vh-132px)}")
                || css.contains(".quarto-container { min-height: calc(100vh - 132px) }"),
            "missing `.quarto-container {{ min-height: calc(100vh - 132px) }}` (Q1 parity)"
        );
    }
}
