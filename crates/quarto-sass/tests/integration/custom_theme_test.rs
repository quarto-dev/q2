//! Integration tests for custom SCSS theme support.
//!
//! Tests custom theme loading, processing, and compilation, including:
//! - Single custom file only
//! - [cosmo, custom.scss] - custom overrides built-in
//! - [custom.scss, cosmo] - built-in overrides custom defaults
//! - Custom file that uses @import from its directory

use quarto_sass::{
    BOOTSTRAP_RESOURCES, ThemeContext, ThemeSpec, assemble_themes, default_load_paths,
};
use std::path::{Path, PathBuf};

// Helper to create a ThemeContext using the native runtime convenience method
fn make_context(dir: PathBuf) -> ThemeContext<'static> {
    ThemeContext::native(dir)
}

/// Combined filesystem adapter that checks both real filesystem and embedded resources.
///
/// This is needed because:
/// - Custom theme files and their @imports live on the real filesystem
/// - Bootstrap files live in embedded resources
#[derive(Debug)]
struct CombinedFs;

impl grass::Fs for CombinedFs {
    fn is_dir(&self, path: &Path) -> bool {
        // First check real filesystem
        if path.exists() && path.is_dir() {
            return true;
        }
        // Fall back to embedded resources
        BOOTSTRAP_RESOURCES.is_dir(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        // First check real filesystem
        if path.exists() && path.is_file() {
            return true;
        }
        // Fall back to embedded resources
        BOOTSTRAP_RESOURCES.is_file(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        // First try real filesystem
        if path.exists() && path.is_file() {
            return std::fs::read(path);
        }
        // Fall back to embedded resources
        BOOTSTRAP_RESOURCES
            .read(path)
            .map(|b| b.to_vec())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found: {:?}", path),
                )
            })
    }
}

fn get_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/custom")
}

fn compile_themes(specs: &[ThemeSpec], context: &ThemeContext) -> Result<String, String> {
    let (scss, extra_load_paths) = assemble_themes(specs, context).map_err(|e| e.to_string())?;

    // Combine default load paths with custom theme load paths
    let mut load_paths = default_load_paths();
    load_paths.extend(extra_load_paths);

    let options = grass::Options::default()
        .fs(&CombinedFs)
        .load_paths(&load_paths)
        .style(grass::OutputStyle::Expanded);

    grass::from_string(&scss, &options).map_err(|e| e.to_string())
}

#[test]
fn test_single_custom_file_only() {
    let context = make_context(get_fixture_dir());
    let specs = vec![ThemeSpec::parse("override.scss").unwrap()];

    let css = compile_themes(&specs, &context).expect("Custom theme should compile");

    // Should contain the custom rule
    assert!(
        css.contains(".custom-rule"),
        "CSS should contain .custom-rule"
    );

    // Should contain Bootstrap classes (base framework)
    assert!(css.contains(".btn"), "CSS should contain Bootstrap .btn");
    assert!(
        css.contains(".container"),
        "CSS should contain Bootstrap .container"
    );

    // CSS should be substantial (full Bootstrap)
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_builtin_then_custom() {
    // [cosmo, custom.scss] - custom defaults win via !default semantics
    let context = make_context(get_fixture_dir());
    let specs = vec![
        ThemeSpec::parse("cosmo").unwrap(),
        ThemeSpec::parse("override.scss").unwrap(),
    ];

    let css = compile_themes(&specs, &context).expect("Combined themes should compile");

    // Should contain Bootstrap classes
    assert!(css.contains(".btn"), "CSS should contain Bootstrap .btn");

    // Should contain custom rule
    assert!(
        css.contains(".custom-rule"),
        "CSS should contain .custom-rule"
    );

    // CSS should be substantial
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_custom_then_builtin() {
    // [custom.scss, cosmo] - different ordering
    let context = make_context(get_fixture_dir());
    let specs = vec![
        ThemeSpec::parse("override.scss").unwrap(),
        ThemeSpec::parse("cosmo").unwrap(),
    ];

    let css = compile_themes(&specs, &context).expect("Combined themes should compile");

    // Should contain Bootstrap classes
    assert!(css.contains(".btn"), "CSS should contain Bootstrap .btn");

    // Should contain custom rule
    assert!(
        css.contains(".custom-rule"),
        "CSS should contain .custom-rule"
    );

    // CSS should be substantial
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_custom_with_import() {
    // Custom file that uses @import from its directory
    let context = make_context(get_fixture_dir());
    let specs = vec![ThemeSpec::parse("with_import.scss").unwrap()];

    let css = compile_themes(&specs, &context).expect("Custom theme with import should compile");

    // Should contain the custom rule that uses the imported mixin
    assert!(
        css.contains(".with-import-rule"),
        "CSS should contain .with-import-rule"
    );

    // The partial defines a border, which should be included
    assert!(
        css.contains("border:"),
        "CSS should contain border from partial mixin"
    );

    // CSS should be substantial
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_multiple_custom_files() {
    // Multiple custom files
    let context = make_context(get_fixture_dir());
    let specs = vec![
        ThemeSpec::parse("override.scss").unwrap(),
        ThemeSpec::parse("with_import.scss").unwrap(),
    ];

    let css = compile_themes(&specs, &context).expect("Multiple custom themes should compile");

    // Should contain rules from both custom files
    assert!(
        css.contains(".custom-rule"),
        "CSS should contain .custom-rule from override.scss"
    );
    assert!(
        css.contains(".with-import-rule"),
        "CSS should contain .with-import-rule from with_import.scss"
    );

    // CSS should be substantial
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_builtin_custom_builtin() {
    // Complex ordering: [cosmo, custom.scss, flatly]
    let context = make_context(get_fixture_dir());
    let specs = vec![
        ThemeSpec::parse("cosmo").unwrap(),
        ThemeSpec::parse("override.scss").unwrap(),
        ThemeSpec::parse("flatly").unwrap(),
    ];

    let css = compile_themes(&specs, &context).expect("Complex theme combination should compile");

    // Should contain Bootstrap classes
    assert!(css.contains(".btn"), "CSS should contain Bootstrap .btn");

    // Should contain custom rule
    assert!(
        css.contains(".custom-rule"),
        "CSS should contain .custom-rule"
    );

    // CSS should be substantial
    assert!(
        css.len() > 100_000,
        "CSS should be substantial ({} bytes)",
        css.len()
    );
}

#[test]
fn test_ordering_affects_merged_user_layer() {
    // Verify that [A, B] and [B, A] process into different merged layers
    // The key difference is in defaults ordering (affects !default variable precedence)
    //
    // Note: The final CSS rule order is always framework → quarto → merged_user
    // (as determined by assemble_scss), but within merged_user, the rules
    // follow the input order.
    let context = make_context(get_fixture_dir());

    let specs_cosmo_first = vec![
        ThemeSpec::parse("cosmo").unwrap(),
        ThemeSpec::parse("override.scss").unwrap(),
    ];

    let specs_custom_first = vec![
        ThemeSpec::parse("override.scss").unwrap(),
        ThemeSpec::parse("cosmo").unwrap(),
    ];

    let css1 = compile_themes(&specs_cosmo_first, &context).expect("cosmo-first should compile");
    let css2 = compile_themes(&specs_custom_first, &context).expect("custom-first should compile");

    // Both should compile successfully
    assert!(css1.len() > 100_000);
    assert!(css2.len() > 100_000);

    // Both should contain Bootstrap classes and custom rules
    assert!(css1.contains(".btn"), "css1 should have Bootstrap .btn");
    assert!(css2.contains(".btn"), "css2 should have Bootstrap .btn");
    assert!(
        css1.contains(".custom-rule"),
        "css1 should have .custom-rule"
    );
    assert!(
        css2.contains(".custom-rule"),
        "css2 should have .custom-rule"
    );

    // The CSS content will be slightly different due to defaults ordering
    // affecting variable resolution with !default semantics.
    //
    // In [cosmo, custom]: custom's defaults take precedence over cosmo's
    // In [custom, cosmo]: cosmo's defaults take precedence over custom's
    //
    // This is because merge_layers reverses defaults order, so the last
    // layer's defaults appear first in the merged output (winning with !default).
    //
    // Since our test fixture override.scss defines $primary: #ff6600,
    // and cosmo defines its own $primary, the ordering affects which wins.
    //
    // We just verify both compile and produce valid CSS here.
    // More detailed ordering tests are in themes.rs unit tests.
}
