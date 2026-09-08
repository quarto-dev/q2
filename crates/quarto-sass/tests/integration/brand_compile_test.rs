//! Compile-time check: brand layers must produce valid SCSS that
//! grass can compile. This catches malformed variable values
//! (unquoted multi-word family names, broken color references, etc.)
//! that string-search tests can miss.
//!
//! Only runs on native (grass is the native SASS compiler).

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use quarto_brand::Brand;
use quarto_sass::brand_to_layers;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("quarto-brand/tests/fixtures")
}

fn load_brand(rel: &str) -> Brand {
    let path = fixtures_dir().join(rel);
    let yaml =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    quarto_brand::UnifiedBrand::from_yaml_str(&yaml)
        .unwrap_or_else(|e| panic!("parse: {e}"))
        .split()
        .light
}

/// Concatenate all layers into a single SCSS string in the same order
/// `assemble_with_user_layers` does — uses first, then defaults, etc.
fn flatten_layers(layers: &[quarto_sass::SassLayer]) -> String {
    let uses = layers
        .iter()
        .map(|l| l.uses.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let defaults = layers
        .iter()
        .map(|l| l.defaults.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let rules = layers
        .iter()
        .map(|l| l.rules.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    format!("{uses}\n{defaults}\n{rules}\n")
}

fn assert_compiles(scss: &str) {
    let opts = grass::Options::default();
    match grass::from_string(scss, &opts) {
        Ok(_) => {}
        Err(e) => panic!("SCSS failed to compile:\n{e}\n--- input ---\n{scss}\n"),
    }
}

#[test]
fn kitchen_sink_brand_compiles() {
    let brand = load_brand("brand-yaml/kitchen-sink/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

#[test]
fn monospace_colors_brand_compiles() {
    let brand = load_brand("brand-yaml/monospace-colors/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

#[test]
fn palette_colors_brand_compiles() {
    let brand = load_brand("brand-yaml/palette-colors/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

#[test]
fn basic_brand_compiles() {
    let brand = load_brand("use-brand/basic-brand/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

#[test]
fn multi_file_brand_compiles() {
    let brand = load_brand("use-brand/multi-file-brand/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("brand")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

#[test]
fn nested_brand_compiles() {
    let brand = load_brand("use-brand/nested-brand/_brand.yml");
    let layers = brand_to_layers(&brand, Path::new("")).expect("brand_to_layers");
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}

// ── font weight ranges (bd-5fseopxy) ────────────────────────────────

fn brand_from_str(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .unwrap_or_else(|e| panic!("parse: {e}"))
        .split()
        .light
}

/// The range forms must produce SCSS grass accepts: `wght@400..700`
/// inside an `@import url(...)` and `font-weight: 300 800` in
/// `@font-face`. Before the fix the file form emitted
/// `font-weight: 300..800;`, which grass rejects (`expected ";"`).
#[test]
fn weight_range_brand_compiles() {
    let b = brand_from_str(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: 400..700\n\
         \x20   - family: Local Var\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - path: LocalVar-VariableFont_wght.woff2\n\
         \x20         weight: 300..800\n\
         \x20 base: EB Garamond\n\
         \x20 headings:\n\
         \x20   family: EB Garamond\n\
         \x20   weight: 600\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let scss = flatten_layers(&layers);
    assert_compiles(&scss);
}
