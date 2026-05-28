//! Integration tests for parsing the committed `_brand.yml` fixtures.
//!
//! These fixtures were copied once from Quarto 1's test corpus on
//! 2026-05-20; see `tests/fixtures/README.md`. The tests verify that
//! every fixture deserializes cleanly into our typed `Brand` model with
//! `deny_unknown_fields` enforcement.

use std::path::PathBuf;

use quarto_brand::Brand;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture(rel_path: &str) -> Brand {
    let path = fixtures_dir().join(rel_path);
    let yaml =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Brand::from_yaml_str(&yaml).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn parse_kitchen_sink() {
    let brand = load_fixture("brand-yaml/kitchen-sink/_brand.yml");

    let color = brand.color.as_ref().expect("color section");
    let palette = color.palette.as_ref().expect("palette");
    assert_eq!(palette.get("red").map(String::as_str), Some("#FF0000"));
    assert_eq!(palette.get("black").map(String::as_str), Some("#002040"));
    assert_eq!(color.primary.as_deref(), Some("red"));
    assert_eq!(color.foreground.as_deref(), Some("#21f"));
    assert_eq!(color.background.as_deref(), Some("#e6f8ff"));

    let typography = brand.typography.as_ref().expect("typography");
    assert_eq!(typography.fonts.len(), 3);
    // base font has size: 12pt, weight: 400, line-height: 0.9
    let base = typography.base.as_ref().expect("base font");
    let base_family = base.family.as_deref();
    assert_eq!(base_family, Some("EB Garamond"));
}

#[test]
fn parse_monospace_colors() {
    let brand = load_fixture("brand-yaml/monospace-colors/_brand.yml");

    let color = brand.color.as_ref().expect("color section");
    assert_eq!(color.background.as_deref(), Some("#e6f8ff"));

    let typography = brand.typography.as_ref().expect("typography");
    let mono = typography.monospace.as_ref().expect("monospace");
    assert_eq!(mono.color.as_deref(), Some("#eee"));
    assert_eq!(mono.background_color.as_deref(), Some("#339d2c"));
}

#[test]
fn parse_palette_colors() {
    let brand = load_fixture("brand-yaml/palette-colors/_brand.yml");

    let color = brand.color.as_ref().expect("color");
    let palette = color.palette.as_ref().expect("palette");
    assert_eq!(palette.len(), 5);
    assert_eq!(palette.get("orangeblue").map(String::as_str), Some("#ccc"));
    assert!(palette.contains_key("branded-monospace-inline-foreground"));
}

#[test]
fn parse_basic_brand() {
    let brand = load_fixture("use-brand/basic-brand/_brand.yml");
    assert_eq!(
        brand
            .meta
            .as_ref()
            .and_then(|m| m.name.as_ref().and_then(|n| n.full_name())),
        Some("Basic Test Brand")
    );

    let logo = brand.logo.as_ref().expect("logo");
    let small = logo.small.as_ref().expect("small logo");
    assert_eq!(small.single_path(), Some("logo.png"));
}

#[test]
fn parse_multi_file_brand() {
    let brand = load_fixture("use-brand/multi-file-brand/_brand.yml");

    let logo = brand.logo.as_ref().expect("logo");
    let images = logo.images.as_ref().expect("images");
    assert!(images.contains_key("main"));
    assert!(images.contains_key("favicon"));

    let typography = brand.typography.as_ref().expect("typography");
    assert_eq!(typography.fonts.len(), 1);
}

#[test]
fn parse_nested_brand() {
    let brand = load_fixture("use-brand/nested-brand/_brand.yml");

    let logo = brand.logo.as_ref().expect("logo");
    let small = logo.small.as_ref().expect("small");
    let (light, dark) = small.light_dark_paths().expect("light/dark logo");
    assert_eq!(light, Some("images/logo.png"));
    assert_eq!(dark, Some("images/header.png"));
}

#[test]
fn unknown_top_level_key_is_rejected() {
    let yaml = "color:\n  primary: red\nnot_a_real_key: oops\n";
    let err = Brand::from_yaml_str(yaml).expect_err("should reject unknown key");
    let msg = err.to_string();
    assert!(
        msg.contains("not_a_real_key") || msg.contains("unknown field"),
        "error message should mention the offending key: {msg}"
    );
}
