//! Integration test: Compile all 25 Bootswatch themes.
//!
//! This test verifies that the new bundle assembly order allows all themes
//! to compile successfully, including the 7 "problematic" themes that
//! previously failed (cyborg, slate, superhero, lumen, simplex, sketchy, vapor).

use quarto_sass::{BOOTSTRAP_RESOURCES, BuiltInTheme, assemble_with_theme, default_load_paths};
use std::path::Path;

/// Adapter that implements `grass::Fs` for our embedded resources.
#[derive(Debug)]
struct EmbeddedFs;

impl grass::Fs for EmbeddedFs {
    fn is_dir(&self, path: &Path) -> bool {
        BOOTSTRAP_RESOURCES.is_dir(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        BOOTSTRAP_RESOURCES.is_file(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
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

fn compile_theme(theme: BuiltInTheme) -> Result<String, String> {
    let scss = assemble_with_theme(theme).map_err(|e| e.to_string())?;

    let load_paths = default_load_paths();

    let options = grass::Options::default()
        .fs(&EmbeddedFs)
        .load_paths(&load_paths)
        .style(grass::OutputStyle::Expanded);

    grass::from_string(&scss, &options).map_err(|e| e.to_string())
}

#[test]
fn test_compile_all_themes() {
    let mut failures: Vec<(BuiltInTheme, String)> = Vec::new();
    let mut successes: Vec<BuiltInTheme> = Vec::new();

    for theme in BuiltInTheme::all() {
        match compile_theme(*theme) {
            Ok(css) => {
                // Basic sanity check: CSS should have some content
                assert!(
                    css.len() > 100_000,
                    "{}: CSS too small ({} bytes)",
                    theme,
                    css.len()
                );
                successes.push(*theme);
            }
            Err(e) => {
                failures.push((*theme, e));
            }
        }
    }

    if !failures.is_empty() {
        let failure_msgs: Vec<String> = failures
            .iter()
            .map(|(theme, err)| format!("  {}: {}", theme, err))
            .collect();
        panic!(
            "{} of {} themes failed to compile:\n{}",
            failures.len(),
            BuiltInTheme::all().len(),
            failure_msgs.join("\n")
        );
    }

    println!("Successfully compiled all {} themes:", successes.len());
    for theme in &successes {
        println!("  ✓ {}", theme);
    }
}

/// Test the specific "problematic" themes that previously failed.
#[test]
fn test_previously_problematic_themes() {
    let problematic = [
        BuiltInTheme::Cyborg,
        BuiltInTheme::Slate,
        BuiltInTheme::Superhero,
        BuiltInTheme::Lumen,
        BuiltInTheme::Simplex,
        BuiltInTheme::Sketchy,
        BuiltInTheme::Vapor,
    ];

    for theme in problematic {
        let result = compile_theme(theme);
        assert!(
            result.is_ok(),
            "Theme {} should compile but failed: {:?}",
            theme,
            result.err()
        );
        println!("✓ {} compiles successfully", theme);
    }
}

/// Test that slate's custom lighten/darken functions work.
#[test]
fn test_slate_custom_functions() {
    let css = compile_theme(BuiltInTheme::Slate).expect("Slate should compile");

    // Slate's custom lighten/darken should produce valid CSS
    // The theme uses these to create contrast-based colors
    assert!(css.contains("color:"), "Should have color properties");
    assert!(
        css.contains("background"),
        "Should have background properties"
    );
}

/// Test that cyborg's color-contrast calls work.
#[test]
fn test_cyborg_color_contrast() {
    let css = compile_theme(BuiltInTheme::Cyborg).expect("Cyborg should compile");

    // Cyborg is a dark theme with custom color contrast
    assert!(css.len() > 200_000, "Cyborg CSS should be substantial");
}

/// Test compiled CSS contains expected Bootstrap classes.
#[test]
fn test_compiled_css_has_bootstrap_classes() {
    let css = compile_theme(BuiltInTheme::Cerulean).expect("Cerulean should compile");

    // Should contain Bootstrap component classes
    assert!(css.contains(".btn"), "Should have button classes");
    assert!(css.contains(".container"), "Should have container classes");
    assert!(css.contains(".nav"), "Should have nav classes");
    assert!(css.contains(".form-control"), "Should have form classes");
}
