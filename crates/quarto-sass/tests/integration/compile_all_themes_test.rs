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

/// Compile a built-in theme to CSS.
///
/// **Scope warning:** `assemble_with_theme` covers Bootstrap + the
/// Quarto layer + the theme only. It does NOT include the
/// always-present built-in user layers (`title-block.scss`,
/// `highlight.scss`) that the real render path adds via
/// `compile_default_css` / `compile_with_doc_vars`. Assertions about
/// content from those layers will fail against this helper's output
/// no matter what the `.scss` files say — assemble with
/// `assemble_with_user_layers` + the relevant `load_*_layer()`
/// instead (see `test_compiled_css_resets_source_code_pre_margin`).
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

/// Test compiled CSS contains editorial mark styling.
///
/// Editorial marks syntax:
/// [++ text] -> <span class="quarto-insert">
/// [-- text] -> <span class="quarto-delete">
/// [!! text] -> <span class="quarto-highlight">
/// [>> text] -> <span class="quarto-edit-comment">
#[test]
fn test_compiled_css_has_editorial_marks() {
    let css = compile_theme(BuiltInTheme::Cosmo).expect("Cosmo should compile");

    // Should contain editorial mark rules
    assert!(
        css.contains(".quarto-insert {"),
        "Should have .quarto-insert rule"
    );
    assert!(
        css.contains(".quarto-delete {"),
        "Should have .quarto-delete rule"
    );
    assert!(
        css.contains(".quarto-highlight {"),
        "Should have .quarto-highlight rule"
    );
    assert!(
        css.contains(".quarto-edit-comment {"),
        "Should have .quarto-edit-comment rule"
    );

    // Check .quarto-insert has expected properties (background color and no underline)
    let ins_section = css
        .split(".quarto-insert {")
        .nth(1)
        .expect("quarto-insert section should exist");
    assert!(
        ins_section.contains("background-color:"),
        ".quarto-insert should have background-color"
    );
    assert!(
        ins_section.contains("text-decoration: none"),
        ".quarto-insert should have text-decoration: none"
    );

    // Check .quarto-delete has strikethrough
    let del_section = css
        .split(".quarto-delete {")
        .nth(1)
        .expect("quarto-delete section should exist");
    assert!(
        del_section.contains("text-decoration: line-through"),
        ".quarto-delete should have line-through"
    );

    // Check .quarto-edit-comment has italic
    let comment_section = css
        .split(".quarto-edit-comment {")
        .nth(1)
        .expect("quarto-edit-comment section should exist");
    assert!(
        comment_section.contains("font-style: italic"),
        ".quarto-edit-comment should have italic style"
    );
}

/// Collect the bodies of all top-level rules whose selector list starts
/// at a line beginning with `selector` (expanded output style puts each
/// top-level selector at the start of a line).
fn rule_bodies<'a>(css: &'a str, selector: &str) -> Vec<&'a str> {
    let needle = format!("\n{selector} {{");
    css.split(&needle)
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(body, _)| body))
        .collect()
}

/// Regression test for bd-jby1i: highlighted code blocks rendered a
/// stray empty line at the bottom because Bootstrap reboot's
/// `pre { margin-bottom: 1rem }` was trapped inside the block
/// formatting context created by `div.sourceCode { overflow-y: hidden }`.
/// Quarto 1 inherits the counteracting rules from Pandoc's baseline
/// highlighting CSS (`$highlighting-css$`); Quarto 2 must ship them in
/// `highlight.scss`:
///
///   pre.sourceCode { margin: 0; }    -- kill the trapped margin
///   div.sourceCode { margin: 1em 0; } -- div takes over outer spacing
#[test]
fn test_compiled_css_resets_source_code_pre_margin() {
    // The highlight layer is not part of `assemble_with_theme`; the
    // render path (`compile_default_css` / `compile_with_doc_vars`)
    // passes it as an always-present user layer. Mirror that here.
    use quarto_sass::bundle::{assemble_with_user_layers, load_highlight_layer};

    let highlight = load_highlight_layer(None).expect("highlight layer should load");
    let scss = assemble_with_user_layers(&[highlight]).expect("assembly should succeed");

    let load_paths = default_load_paths();
    let options = grass::Options::default()
        .fs(&EmbeddedFs)
        .load_paths(&load_paths)
        .style(grass::OutputStyle::Expanded);
    let css = grass::from_string(&scss, &options).expect("default + highlight should compile");

    let pre_bodies = rule_bodies(&css, "pre.sourceCode");
    assert!(
        pre_bodies.iter().any(|body| body.contains("margin: 0")),
        "some pre.sourceCode rule should reset margin to 0 \
         (bd-jby1i: otherwise Bootstrap's pre margin-bottom is trapped \
         inside div.sourceCode's BFC and renders as a stray empty line); \
         found bodies: {pre_bodies:?}"
    );

    let div_bodies = rule_bodies(&css, "div.sourceCode");
    assert!(
        div_bodies.iter().any(|body| body.contains("margin: 1em 0")),
        "some div.sourceCode rule should carry the outer margin (1em 0) \
         that replaces the pre margin reset by bd-jby1i's fix; \
         found bodies: {div_bodies:?}"
    );
}
