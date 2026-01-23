//! Integration tests for compiling Bootstrap using embedded resources.
//!
//! These tests verify that Bootstrap 5.3.1 can be compiled using the
//! embedded SCSS files without needing the external-sources directory.

use std::path::PathBuf;

use quarto_sass::{BOOTSTRAP_RESOURCES, RESOURCE_PATH_PREFIX};
use quarto_system_runtime::{NativeRuntime, compile_scss_with_embedded};

/// Helper to assemble Bootstrap SCSS from embedded resources in the correct layer order.
///
/// Bootstrap must be assembled in layers (functions, variables, mixins, rules)
/// rather than compiled directly from bootstrap.scss.
fn assemble_bootstrap_scss_from_embedded() -> String {
    let functions = BOOTSTRAP_RESOURCES
        .read_str(std::path::Path::new("_functions.scss"))
        .unwrap_or_default();
    let variables = BOOTSTRAP_RESOURCES
        .read_str(std::path::Path::new("_variables.scss"))
        .unwrap_or_default();
    let mixins = BOOTSTRAP_RESOURCES
        .read_str(std::path::Path::new("_mixins.scss"))
        .unwrap_or_default();
    let rules = BOOTSTRAP_RESOURCES
        .read_str(std::path::Path::new("bootstrap.scss"))
        .unwrap_or_default();

    format!(
        "// Functions\n{}\n\n// Variables\n{}\n\n// Mixins\n{}\n\n// Rules\n{}",
        functions, variables, mixins, rules
    )
}

/// Test Bootstrap 5.3.1 compilation using embedded resources.
///
/// This test verifies that Bootstrap can be compiled using only the
/// embedded SCSS files, without needing external filesystem access.
#[test]
fn test_compile_bootstrap_from_embedded() {
    let runtime = NativeRuntime::new();

    // Assemble Bootstrap SCSS from embedded resources
    let bootstrap_scss = assemble_bootstrap_scss_from_embedded();

    // Compile using embedded resources
    // Use the full path prefix for load paths
    let load_paths = vec![PathBuf::from(format!(
        "{}/bootstrap/scss",
        RESOURCE_PATH_PREFIX
    ))];

    let result = compile_scss_with_embedded(
        &runtime,
        &BOOTSTRAP_RESOURCES,
        &bootstrap_scss,
        &load_paths,
        false,
    );

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
            println!(
                "Bootstrap 5.3.1 compiled from embedded resources: {} bytes",
                css.len()
            );
        }
        Err(e) => {
            panic!(
                "Bootstrap compilation from embedded resources failed: {}",
                e
            );
        }
    }
}

/// Test Bootstrap 5.3.1 minified compilation using embedded resources.
#[test]
fn test_compile_bootstrap_from_embedded_minified() {
    let runtime = NativeRuntime::new();

    // Assemble Bootstrap SCSS from embedded resources
    let bootstrap_scss = assemble_bootstrap_scss_from_embedded();

    // Compile using embedded resources with minification
    let load_paths = vec![PathBuf::from(format!(
        "{}/bootstrap/scss",
        RESOURCE_PATH_PREFIX
    ))];

    let result = compile_scss_with_embedded(
        &runtime,
        &BOOTSTRAP_RESOURCES,
        &bootstrap_scss,
        &load_paths,
        true,
    );

    match result {
        Ok(css) => {
            // Minified should be smaller than expanded
            assert!(
                css.len() > 80_000,
                "Minified Bootstrap CSS should be at least 80KB, got {} bytes",
                css.len()
            );

            // Minified output should have minimal newlines
            let newline_count = css.matches('\n').count();
            assert!(
                newline_count < 100,
                "Minified CSS should have minimal newlines, got {}",
                newline_count
            );

            println!(
                "Bootstrap 5.3.1 minified from embedded resources: {} bytes, {} newlines",
                css.len(),
                newline_count
            );
        }
        Err(e) => {
            panic!(
                "Bootstrap minified compilation from embedded resources failed: {}",
                e
            );
        }
    }
}

/// Test that embedded resources are accessible via the standard path prefix.
#[test]
fn test_embedded_resource_path_resolution() {
    // Check that files are accessible via multiple path formats
    assert!(
        BOOTSTRAP_RESOURCES.is_file(std::path::Path::new("_variables.scss")),
        "Should find _variables.scss with relative path"
    );

    assert!(
        BOOTSTRAP_RESOURCES.is_file(std::path::Path::new("bootstrap/scss/_variables.scss")),
        "Should find _variables.scss with prefix path"
    );

    assert!(
        BOOTSTRAP_RESOURCES.is_file(std::path::Path::new(
            "/__quarto_resources__/bootstrap/scss/_variables.scss"
        )),
        "Should find _variables.scss with full absolute path"
    );
}
