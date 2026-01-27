//! SASS compilation using the grass crate (native only).
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! This module provides SASS/SCSS compilation for native targets using the
//! grass crate, a pure Rust implementation that targets dart-sass 1.54.3.
//!
//! Key components:
//! - `RuntimeFs`: Adapter implementing `grass::Fs` for our `SystemRuntime`
//! - `compile_scss`: High-level function for SCSS compilation
//! - Support for embedded resources (Bootstrap SCSS) via `EmbeddedResourceProvider`

use std::fmt::Debug;
use std::io;
use std::path::{Path, PathBuf};

use grass::{Options, OutputStyle};

use crate::traits::{RuntimeError, RuntimeResult, SystemRuntime};

/// Trait for providing embedded SCSS resources.
///
/// This trait allows the SASS compiler to access resources that are
/// embedded at compile time, such as Bootstrap SCSS files.
/// Resources are typically accessed via a virtual path prefix like
/// `/__quarto_resources__/bootstrap/scss/`.
pub trait EmbeddedResourceProvider: Send + Sync {
    /// Check if a path exists as a file in the embedded resources.
    fn is_file(&self, path: &Path) -> bool;

    /// Check if a path exists as a directory in the embedded resources.
    fn is_dir(&self, path: &Path) -> bool;

    /// Read a file's contents from the embedded resources.
    fn read(&self, path: &Path) -> Option<&'static [u8]>;
}

/// Adapter that implements `grass::Fs` using a `SystemRuntime`.
///
/// This allows grass to read files through our runtime abstraction,
/// enabling consistent behavior across different runtime configurations.
///
/// The adapter checks embedded resources first (if provided), then falls
/// back to the runtime for file access.
pub struct RuntimeFs<'a> {
    runtime: &'a dyn SystemRuntime,
    /// Optional embedded resources (e.g., Bootstrap SCSS)
    embedded: Option<&'a dyn EmbeddedResourceProvider>,
}

impl<'a> RuntimeFs<'a> {
    /// Create a new RuntimeFs adapter wrapping the given runtime.
    pub fn new(runtime: &'a dyn SystemRuntime) -> Self {
        Self {
            runtime,
            embedded: None,
        }
    }

    /// Create a new RuntimeFs with embedded resources.
    ///
    /// The embedded resources are checked first when resolving files,
    /// before falling back to the runtime.
    pub fn with_embedded(
        runtime: &'a dyn SystemRuntime,
        embedded: &'a dyn EmbeddedResourceProvider,
    ) -> Self {
        Self {
            runtime,
            embedded: Some(embedded),
        }
    }
}

impl Debug for RuntimeFs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeFs")
            .field("runtime", &"<SystemRuntime>")
            .field("embedded", &self.embedded.is_some())
            .finish()
    }
}

impl grass::Fs for RuntimeFs<'_> {
    fn is_dir(&self, path: &Path) -> bool {
        // Check embedded resources first
        if let Some(embedded) = self.embedded {
            if embedded.is_dir(path) {
                return true;
            }
        }
        // Fall back to runtime
        self.runtime.is_dir(path).unwrap_or(false)
    }

    fn is_file(&self, path: &Path) -> bool {
        // Check embedded resources first
        if let Some(embedded) = self.embedded {
            if embedded.is_file(path) {
                return true;
            }
        }
        // Fall back to runtime
        self.runtime.is_file(path).unwrap_or(false)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        // Check embedded resources first
        if let Some(embedded) = self.embedded {
            if let Some(content) = embedded.read(path) {
                return Ok(content.to_vec());
            }
        }
        // Fall back to runtime
        self.runtime
            .file_read(path)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

/// Compile SCSS source to CSS using grass.
///
/// # Arguments
///
/// * `runtime` - The runtime to use for file system access
/// * `scss` - The SCSS source code to compile
/// * `load_paths` - Directories to search for @use/@import resolution
/// * `minified` - Whether to produce compressed output
///
/// # Returns
///
/// Compiled CSS string on success, `RuntimeError::SassError` on failure.
pub fn compile_scss(
    runtime: &dyn SystemRuntime,
    scss: &str,
    load_paths: &[PathBuf],
    minified: bool,
) -> RuntimeResult<String> {
    let fs = RuntimeFs::new(runtime);

    let style = if minified {
        OutputStyle::Compressed
    } else {
        OutputStyle::Expanded
    };

    let options = Options::default()
        .fs(&fs)
        .load_paths(load_paths)
        .style(style);

    grass::from_string(scss, &options).map_err(|e| RuntimeError::SassError(e.to_string()))
}

/// Compile SCSS source to CSS using grass with embedded resources.
///
/// This is similar to `compile_scss` but also checks embedded resources
/// (such as Bootstrap SCSS) before falling back to the runtime's file system.
///
/// # Arguments
///
/// * `runtime` - The runtime to use for file system access
/// * `embedded` - Embedded resources provider (e.g., Bootstrap SCSS)
/// * `scss` - The SCSS source code to compile
/// * `load_paths` - Directories to search for @use/@import resolution
/// * `minified` - Whether to produce compressed output
///
/// # Returns
///
/// Compiled CSS string on success, `RuntimeError::SassError` on failure.
pub fn compile_scss_with_embedded(
    runtime: &dyn SystemRuntime,
    embedded: &dyn EmbeddedResourceProvider,
    scss: &str,
    load_paths: &[PathBuf],
    minified: bool,
) -> RuntimeResult<String> {
    let fs = RuntimeFs::with_embedded(runtime, embedded);

    let style = if minified {
        OutputStyle::Compressed
    } else {
        OutputStyle::Expanded
    };

    let options = Options::default()
        .fs(&fs)
        .load_paths(load_paths)
        .style(style);

    grass::from_string(scss, &options).map_err(|e| RuntimeError::SassError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeRuntime;

    #[test]
    fn test_compile_simple_scss() {
        let runtime = NativeRuntime::new();
        let scss = "$primary: #007bff; .btn { color: $primary; }";

        let css = compile_scss(&runtime, scss, &[], false).unwrap();

        assert!(css.contains(".btn"));
        assert!(css.contains("#007bff"));
    }

    #[test]
    fn test_compile_scss_minified() {
        let runtime = NativeRuntime::new();
        let scss = "$primary: blue;\n\n.btn {\n  color: $primary;\n}";

        let css = compile_scss(&runtime, scss, &[], true).unwrap();

        // Minified output should not have extra whitespace
        assert!(!css.contains("\n\n"));
        // But should still have the content
        assert!(css.contains(".btn"));
        assert!(css.contains("blue"));
    }

    #[test]
    fn test_compile_scss_with_functions() {
        let runtime = NativeRuntime::new();
        let scss = r#"
            @function double($n) {
                @return $n * 2;
            }
            .box {
                width: double(50px);
            }
        "#;

        let css = compile_scss(&runtime, scss, &[], false).unwrap();

        assert!(css.contains(".box"));
        assert!(css.contains("100px"));
    }

    #[test]
    fn test_compile_scss_with_mixins() {
        let runtime = NativeRuntime::new();
        let scss = r#"
            @mixin center {
                display: flex;
                justify-content: center;
                align-items: center;
            }
            .container {
                @include center;
            }
        "#;

        let css = compile_scss(&runtime, scss, &[], false).unwrap();

        assert!(css.contains(".container"));
        assert!(css.contains("display: flex"));
        assert!(css.contains("justify-content: center"));
    }

    #[test]
    fn test_compile_scss_error() {
        let runtime = NativeRuntime::new();
        let scss = ".btn { color: $undefined-variable; }";

        let result = compile_scss(&runtime, scss, &[], false);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RuntimeError::SassError(_)));
    }

    #[test]
    fn test_compile_scss_nested_rules() {
        let runtime = NativeRuntime::new();
        let scss = r#"
            .nav {
                background: white;

                .item {
                    padding: 10px;

                    &:hover {
                        background: gray;
                    }
                }
            }
        "#;

        let css = compile_scss(&runtime, scss, &[], false).unwrap();

        assert!(css.contains(".nav"));
        assert!(css.contains(".nav .item"));
        assert!(css.contains(".nav .item:hover"));
    }

    #[test]
    fn test_runtime_fs_debug() {
        let runtime = NativeRuntime::new();
        let fs = RuntimeFs::new(&runtime);
        let debug_str = format!("{:?}", fs);
        assert!(debug_str.contains("RuntimeFs"));
    }

    /// Helper to find the workspace root with resources/scss
    fn find_workspace_root() -> Option<PathBuf> {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|p| p.join("resources/scss").exists())
            .map(|p| p.to_path_buf())
    }

    /// Helper to assemble Bootstrap SCSS in the correct layer order.
    ///
    /// Bootstrap in TS Quarto is not compiled directly from bootstrap.scss.
    /// Instead, it's assembled from separate layer files in the correct order:
    /// 1. Functions (_functions.scss)
    /// 2. Variables (_variables.scss)
    /// 3. Mixins (_mixins.scss)
    /// 4. Rules (bootstrap.scss - which imports all component rules)
    fn assemble_bootstrap_scss(runtime: &NativeRuntime, bootstrap_dir: &Path) -> String {
        let functions = runtime
            .file_read_string(&bootstrap_dir.join("_functions.scss"))
            .unwrap_or_default();
        let variables = runtime
            .file_read_string(&bootstrap_dir.join("_variables.scss"))
            .unwrap_or_default();
        let mixins = runtime
            .file_read_string(&bootstrap_dir.join("_mixins.scss"))
            .unwrap_or_default();
        let rules = runtime
            .file_read_string(&bootstrap_dir.join("bootstrap.scss"))
            .unwrap_or_default();

        // Assemble in correct order: functions, variables, mixins, then rules
        format!(
            "// Functions\n{}\n\n// Variables\n{}\n\n// Mixins\n{}\n\n// Rules\n{}",
            functions, variables, mixins, rules
        )
    }

    /// Test Bootstrap 5.3.1 compilation.
    ///
    /// This verifies that grass can compile the Bootstrap SCSS from TS Quarto.
    /// The test uses the external-sources directory, so it will be skipped if
    /// that directory doesn't exist.
    ///
    /// Note: Bootstrap must be assembled in layers (functions, variables, mixins, rules)
    /// rather than compiled directly from bootstrap.scss, because bootstrap.scss
    /// assumes the other files have already been imported.
    #[test]
    fn test_compile_bootstrap_5_3_1() {
        let runtime = NativeRuntime::new();

        let Some(root) = find_workspace_root() else {
            eprintln!("Skipping Bootstrap test: resources/scss not found");
            return;
        };

        let bootstrap_dir = root.join("resources/scss/bootstrap/dist/scss");

        if !bootstrap_dir.exists() {
            eprintln!(
                "Skipping Bootstrap test: Bootstrap SCSS not found at {:?}",
                bootstrap_dir
            );
            return;
        }

        // Assemble Bootstrap SCSS in the correct layer order
        let bootstrap_scss = assemble_bootstrap_scss(&runtime, &bootstrap_dir);

        // Compile with Bootstrap directory as load path
        let result = compile_scss(&runtime, &bootstrap_scss, &[bootstrap_dir.clone()], false);

        match result {
            Ok(css) => {
                // Basic sanity checks on the compiled CSS
                assert!(
                    css.len() > 100_000,
                    "Bootstrap CSS should be at least 100KB, got {} bytes",
                    css.len()
                );
                assert!(css.contains(".btn"), "Should contain .btn class");
                assert!(
                    css.contains(".container"),
                    "Should contain .container class"
                );
                assert!(css.contains(".navbar"), "Should contain .navbar class");
                assert!(css.contains(".modal"), "Should contain .modal class");
                println!("Bootstrap 5.3.1 compiled successfully: {} bytes", css.len());
            }
            Err(e) => {
                panic!("Bootstrap compilation failed: {}", e);
            }
        }
    }

    /// Test Bootstrap 5.3.1 minified compilation.
    #[test]
    fn test_compile_bootstrap_5_3_1_minified() {
        let runtime = NativeRuntime::new();

        let Some(root) = find_workspace_root() else {
            eprintln!("Skipping Bootstrap minified test: resources/scss not found");
            return;
        };

        let bootstrap_dir = root.join("resources/scss/bootstrap/dist/scss");

        if !bootstrap_dir.exists() {
            eprintln!("Skipping Bootstrap minified test: Bootstrap SCSS not found");
            return;
        }

        // Assemble Bootstrap SCSS in the correct layer order
        let bootstrap_scss = assemble_bootstrap_scss(&runtime, &bootstrap_dir);

        let result = compile_scss(&runtime, &bootstrap_scss, &[bootstrap_dir.clone()], true);

        match result {
            Ok(css) => {
                // Minified should be smaller than expanded (typically ~30% smaller)
                assert!(
                    css.len() > 80_000,
                    "Minified Bootstrap CSS should be at least 80KB"
                );
                // Minified output should not have newlines between rules
                // (some double spaces may exist in strings, so we don't check for that)
                let newline_count = css.matches('\n').count();
                assert!(
                    newline_count < 100,
                    "Minified CSS should have minimal newlines, got {}",
                    newline_count
                );
                println!(
                    "Bootstrap 5.3.1 minified compiled successfully: {} bytes, {} newlines",
                    css.len(),
                    newline_count
                );
            }
            Err(e) => {
                panic!("Bootstrap minified compilation failed: {}", e);
            }
        }
    }
}
