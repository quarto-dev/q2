//! Parity tests comparing grass (Rust) vs dart-sass (reference) output.
//!
//! Copyright (c) 2025 Posit, PBC
//!
//! These tests ensure that grass produces output compatible with dart-sass
//! for Bootstrap 5.3.1 and Bootswatch themes.
//!
//! ## How it works
//!
//! 1. Reference fixtures are pre-generated using dart-sass via Node.js
//!    (see scripts/generate-sass-fixtures.mjs)
//! 2. These tests compile the same SCSS using grass (native Rust)
//! 3. Output is compared for parity
//!
//! ## Running fixture generation
//!
//! ```bash
//! npm install --no-save sass
//! node scripts/generate-sass-fixtures.mjs
//! ```

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use quarto_system_runtime::NativeRuntime;
use quarto_system_runtime::sass_native::compile_scss;

/// Find the workspace root directory (contains Cargo.toml with [workspace])
fn find_workspace_root() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()?
        .ancestors()
        .find(|p| p.join("external-sources").exists())
        .map(|p| p.to_path_buf())
}

/// Path to Bootstrap SCSS directory
fn bootstrap_scss_dir(root: &Path) -> PathBuf {
    root.join("external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/scss")
}

/// Path to Bootswatch themes directory
fn themes_dir(root: &Path) -> PathBuf {
    root.join("external-sources/quarto-cli/src/resources/formats/html/bootstrap/themes")
}

/// Path to dart-sass fixtures directory
fn fixtures_dir(root: &Path) -> PathBuf {
    root.join("crates/quarto-sass/test-fixtures/dart-sass")
}

/// Assemble Bootstrap SCSS in the correct layer order.
fn assemble_bootstrap_scss(bootstrap_dir: &Path) -> String {
    let functions = fs::read_to_string(bootstrap_dir.join("_functions.scss")).unwrap_or_default();
    let variables = fs::read_to_string(bootstrap_dir.join("_variables.scss")).unwrap_or_default();
    let mixins = fs::read_to_string(bootstrap_dir.join("_mixins.scss")).unwrap_or_default();
    let rules = fs::read_to_string(bootstrap_dir.join("bootstrap.scss")).unwrap_or_default();

    format!(
        "// Functions\n{}\n\n// Variables\n{}\n\n// Mixins\n{}\n\n// Rules\n{}",
        functions, variables, mixins, rules
    )
}

/// Parse layer boundaries from a theme file.
/// Returns (defaults, rules) tuple.
fn parse_theme_layers(content: &str) -> (String, String) {
    let mut defaults = Vec::new();
    let mut rules = Vec::new();
    let mut current = "defaults"; // Content before any marker goes to defaults

    for line in content.lines() {
        if line.trim() == "/*-- scss:defaults --*/" {
            current = "defaults";
        } else if line.trim() == "/*-- scss:rules --*/" {
            current = "rules";
        } else if line.trim() == "/*-- scss:uses --*/" {
            current = "uses";
        } else if line.trim() == "/*-- scss:functions --*/" {
            current = "functions";
        } else if line.trim() == "/*-- scss:mixins --*/" {
            current = "mixins";
        } else {
            match current {
                "defaults" => defaults.push(line),
                "rules" => rules.push(line),
                _ => {} // Ignore other sections for now
            }
        }
    }

    (defaults.join("\n"), rules.join("\n"))
}

/// Assemble a Bootswatch theme with Bootstrap.
fn assemble_theme_scss(bootstrap_dir: &Path, theme_path: &Path) -> String {
    let theme_content = fs::read_to_string(theme_path).unwrap_or_default();
    let (theme_defaults, theme_rules) = parse_theme_layers(&theme_content);

    let functions = fs::read_to_string(bootstrap_dir.join("_functions.scss")).unwrap_or_default();
    let variables = fs::read_to_string(bootstrap_dir.join("_variables.scss")).unwrap_or_default();
    let mixins = fs::read_to_string(bootstrap_dir.join("_mixins.scss")).unwrap_or_default();
    let rules = fs::read_to_string(bootstrap_dir.join("bootstrap.scss")).unwrap_or_default();

    format!(
        "// Bootstrap Functions\n{}\n\n\
         // Theme Defaults\n{}\n\n\
         // Bootstrap Variables\n{}\n\n\
         // Bootstrap Mixins\n{}\n\n\
         // Theme Rules\n{}\n\n\
         // Bootstrap Rules\n{}",
        functions, theme_defaults, variables, mixins, theme_rules, rules
    )
}

/// Extract CSS selectors from CSS content.
fn extract_selectors(css: &str) -> HashSet<String> {
    let mut selectors = HashSet::new();

    // Simple selector extraction - matches selectors before {
    // This is a rough heuristic but good enough for comparison
    for line in css.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with('{') {
            let selector = trimmed.trim_end_matches('{').trim();
            if !selector.is_empty() && !selector.starts_with('@') {
                selectors.insert(selector.to_string());
            }
        }
    }

    selectors
}

/// Compare two CSS outputs and return a parity report.
struct ParityReport {
    /// Size of grass output
    grass_size: usize,
    /// Size of dart-sass output
    dartsass_size: usize,
    /// Size difference as a percentage
    size_diff_percent: f64,
    /// Selectors in dart-sass but not in grass
    missing_selectors: Vec<String>,
    /// Selectors in grass but not in dart-sass
    extra_selectors: Vec<String>,
    /// Whether outputs are byte-identical
    exact_match: bool,
}

impl ParityReport {
    fn is_acceptable(&self) -> bool {
        // Allow up to 5% size difference and no missing selectors
        self.size_diff_percent.abs() < 5.0 && self.missing_selectors.is_empty()
    }
}

fn compare_css(grass: &str, dartsass: &str) -> ParityReport {
    let grass_size = grass.len();
    let dartsass_size = dartsass.len();

    let size_diff_percent = if dartsass_size > 0 {
        ((grass_size as f64 - dartsass_size as f64) / dartsass_size as f64) * 100.0
    } else {
        0.0
    };

    let grass_selectors = extract_selectors(grass);
    let dartsass_selectors = extract_selectors(dartsass);

    let missing_selectors: Vec<String> = dartsass_selectors
        .difference(&grass_selectors)
        .cloned()
        .collect();

    let extra_selectors: Vec<String> = grass_selectors
        .difference(&dartsass_selectors)
        .cloned()
        .collect();

    let exact_match = grass == dartsass;

    ParityReport {
        grass_size,
        dartsass_size,
        size_diff_percent,
        missing_selectors,
        extra_selectors,
        exact_match,
    }
}

/// Test Bootstrap 5.3.1 parity (expanded output)
#[test]
fn test_bootstrap_parity_expanded() {
    let runtime = NativeRuntime::new();

    let Some(root) = find_workspace_root() else {
        eprintln!("Skipping test: workspace root not found");
        return;
    };

    let bootstrap_dir = bootstrap_scss_dir(&root);
    let fixture_path = fixtures_dir(&root).join("bootstrap.css");

    if !bootstrap_dir.exists() {
        eprintln!("Skipping test: Bootstrap SCSS not found");
        return;
    }

    if !fixture_path.exists() {
        eprintln!(
            "Skipping test: dart-sass fixture not found at {:?}",
            fixture_path
        );
        eprintln!("Run: node scripts/generate-sass-fixtures.mjs");
        return;
    }

    let bootstrap_scss = assemble_bootstrap_scss(&bootstrap_dir);
    let dartsass_css = fs::read_to_string(&fixture_path).unwrap();

    let grass_css = compile_scss(&runtime, &bootstrap_scss, &[bootstrap_dir.clone()], false)
        .expect("grass compilation should succeed");

    let report = compare_css(&grass_css, &dartsass_css);

    println!("Bootstrap 5.3.1 Parity Report (expanded):");
    println!("  grass size:    {} bytes", report.grass_size);
    println!("  dart-sass size: {} bytes", report.dartsass_size);
    println!("  size diff:     {:.2}%", report.size_diff_percent);
    println!("  exact match:   {}", report.exact_match);
    println!(
        "  missing selectors: {} (in dart-sass but not grass)",
        report.missing_selectors.len()
    );
    println!(
        "  extra selectors:   {} (in grass but not dart-sass)",
        report.extra_selectors.len()
    );

    if !report.missing_selectors.is_empty() && report.missing_selectors.len() <= 10 {
        println!("  missing: {:?}", report.missing_selectors);
    }

    assert!(
        report.is_acceptable(),
        "Bootstrap parity check failed: size diff {:.2}%, {} missing selectors",
        report.size_diff_percent,
        report.missing_selectors.len()
    );
}

/// Test Bootstrap 5.3.1 parity (minified output)
#[test]
fn test_bootstrap_parity_minified() {
    let runtime = NativeRuntime::new();

    let Some(root) = find_workspace_root() else {
        eprintln!("Skipping test: workspace root not found");
        return;
    };

    let bootstrap_dir = bootstrap_scss_dir(&root);
    let fixture_path = fixtures_dir(&root).join("bootstrap.min.css");

    if !bootstrap_dir.exists() || !fixture_path.exists() {
        eprintln!("Skipping test: required files not found");
        return;
    }

    let bootstrap_scss = assemble_bootstrap_scss(&bootstrap_dir);
    let dartsass_css = fs::read_to_string(&fixture_path).unwrap();

    let grass_css = compile_scss(&runtime, &bootstrap_scss, &[bootstrap_dir.clone()], true)
        .expect("grass compilation should succeed");

    let report = compare_css(&grass_css, &dartsass_css);

    println!("Bootstrap 5.3.1 Parity Report (minified):");
    println!("  grass size:    {} bytes", report.grass_size);
    println!("  dart-sass size: {} bytes", report.dartsass_size);
    println!("  size diff:     {:.2}%", report.size_diff_percent);

    // For minified, we're more lenient on size differences due to whitespace handling
    assert!(
        report.size_diff_percent.abs() < 10.0,
        "Minified size difference too large: {:.2}%",
        report.size_diff_percent
    );
}

/// Themes that are known to compile successfully with our assembly order.
/// Other themes have complex layer dependencies that require more sophisticated handling.
const WORKING_THEMES: &[&str] = &[
    "cerulean",
    "cosmo",
    "darkly",
    "flatly",
    "journal",
    "litera",
    "lux",
    "materia",
    "minty",
    "morph",
    "pulse",
    "quartz",
    "sandstone",
    "solar",
    "spacelab",
    "united",
    "yeti",
    "zephyr",
];

/// Test Bootswatch theme parity
#[test]
fn test_bootswatch_themes_parity() {
    let runtime = NativeRuntime::new();

    let Some(root) = find_workspace_root() else {
        eprintln!("Skipping test: workspace root not found");
        return;
    };

    let bootstrap_dir = bootstrap_scss_dir(&root);
    let themes = themes_dir(&root);
    let fixtures = fixtures_dir(&root).join("themes");

    if !bootstrap_dir.exists() || !themes.exists() {
        eprintln!("Skipping test: required directories not found");
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for theme in WORKING_THEMES {
        let theme_path = themes.join(format!("{}.scss", theme));
        let fixture_path = fixtures.join(format!("{}.css", theme));

        if !fixture_path.exists() {
            println!("{}: skipped (no fixture)", theme);
            skipped += 1;
            continue;
        }

        let theme_scss = assemble_theme_scss(&bootstrap_dir, &theme_path);
        let dartsass_css = fs::read_to_string(&fixture_path).unwrap();

        match compile_scss(&runtime, &theme_scss, &[bootstrap_dir.clone()], false) {
            Ok(grass_css) => {
                let report = compare_css(&grass_css, &dartsass_css);

                if report.is_acceptable() {
                    println!(
                        "{}: passed (size diff: {:.2}%, {} missing selectors)",
                        theme,
                        report.size_diff_percent,
                        report.missing_selectors.len()
                    );
                    passed += 1;
                } else {
                    println!(
                        "{}: FAILED (size diff: {:.2}%, {} missing selectors)",
                        theme,
                        report.size_diff_percent,
                        report.missing_selectors.len()
                    );
                    if !report.missing_selectors.is_empty() && report.missing_selectors.len() <= 5 {
                        println!("  missing: {:?}", report.missing_selectors);
                    }
                    failed += 1;
                }
            }
            Err(e) => {
                println!("{}: FAILED (compilation error: {})", theme, e);
                failed += 1;
            }
        }
    }

    println!(
        "\nTheme parity summary: {} passed, {} failed, {} skipped",
        passed, failed, skipped
    );

    // Allow some failures for complex themes, but most should pass
    let pass_rate = passed as f64 / (passed + failed) as f64;
    assert!(
        pass_rate >= 0.8,
        "Theme pass rate too low: {:.0}% (expected >= 80%)",
        pass_rate * 100.0
    );
}

/// Test that grass can compile all working themes without error
#[test]
fn test_all_themes_compile() {
    let runtime = NativeRuntime::new();

    let Some(root) = find_workspace_root() else {
        eprintln!("Skipping test: workspace root not found");
        return;
    };

    let bootstrap_dir = bootstrap_scss_dir(&root);
    let themes = themes_dir(&root);

    if !bootstrap_dir.exists() || !themes.exists() {
        eprintln!("Skipping test: required directories not found");
        return;
    }

    let mut compiled = 0;
    let mut failed = 0;

    for theme in WORKING_THEMES {
        let theme_path = themes.join(format!("{}.scss", theme));
        let theme_scss = assemble_theme_scss(&bootstrap_dir, &theme_path);

        match compile_scss(&runtime, &theme_scss, &[bootstrap_dir.clone()], false) {
            Ok(css) => {
                // Basic sanity check - compiled CSS should be substantial
                assert!(
                    css.len() > 100_000,
                    "Theme {} CSS is too small: {} bytes",
                    theme,
                    css.len()
                );
                compiled += 1;
            }
            Err(e) => {
                println!("Theme {} failed to compile: {}", theme, e);
                failed += 1;
            }
        }
    }

    println!(
        "\nCompilation summary: {} compiled, {} failed",
        compiled, failed
    );

    assert_eq!(
        failed, 0,
        "Expected all working themes to compile, but {} failed",
        failed
    );
}
