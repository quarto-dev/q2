//! Tests for logo lookup and path resolution.
//!
//! Mirrors Q1's `Brand.getLogo`, `resolvePath`, and `getFavicon` from
//! `external-sources/quarto-cli/src/core/brand/brand.ts`.

use std::path::Path;

use quarto_brand::{Brand, BrandLogoResource};

fn brand(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .expect("parse")
        .split()
        .light
}

#[test]
fn favicon_from_small_logo() {
    let b = brand("logo:\n  small: logo.png\n");
    assert_eq!(b.favicon(), Some("logo.png"));
}

#[test]
fn favicon_none_when_no_small_logo() {
    let b = brand("logo:\n  large: big.png\n");
    assert_eq!(b.favicon(), None);
}

#[test]
fn favicon_none_when_small_is_light_dark_pair() {
    // Q1's getFavicon takes the small logo's `.path`; a light/dark pair
    // has no single path. Return None and let the caller decide which
    // variant to use.
    let b = brand(
        "logo:\n\
         \x20 small:\n\
         \x20   light: light.png\n\
         \x20   dark: dark.png\n",
    );
    assert_eq!(b.favicon(), None);
}

#[test]
fn logo_lookup_by_size() {
    let b = brand(
        "logo:\n\
         \x20 small: small.png\n\
         \x20 medium: medium.png\n\
         \x20 large: large.png\n",
    );
    assert_eq!(
        b.logo("small").and_then(|l| l.single_path()),
        Some("small.png")
    );
    assert_eq!(
        b.logo("medium").and_then(|l| l.single_path()),
        Some("medium.png")
    );
    assert_eq!(
        b.logo("large").and_then(|l| l.single_path()),
        Some("large.png")
    );
}

/// P8 orange-book: `small: <name>` where `<name>` matches a key under
/// `logo.images` resolves to that image's path, not the literal string
/// — Q1's `getLogoResource` precedence (`brand.ts`).
#[test]
fn favicon_resolves_small_logo_name_against_images() {
    let b = brand(
        "logo:\n\
         \x20 small: test-logo\n\
         \x20 images:\n\
         \x20   test-logo:\n\
         \x20     path: logo.svg\n",
    );
    assert_eq!(b.favicon(), Some("logo.svg"));
}

/// An explicit `{path, alt}` value is never treated as a name
/// reference, even if its path string collides with an `images` key.
#[test]
fn favicon_does_not_resolve_explicit_resource_as_name_reference() {
    let b = brand(
        "logo:\n\
         \x20 small:\n\
         \x20   path: test-logo\n\
         \x20 images:\n\
         \x20   test-logo:\n\
         \x20     path: logo.svg\n",
    );
    assert_eq!(b.favicon(), Some("test-logo"));
}

#[test]
fn logo_image_by_name() {
    let b = brand(
        "logo:\n\
         \x20 images:\n\
         \x20   main:\n\
         \x20     path: m.png\n\
         \x20     alt: \"Main\"\n",
    );
    let img = b.logo_image("main").expect("image");
    assert_eq!(img.path(), "m.png");
    assert_eq!(img.alt(), Some("Main"));
}

#[test]
fn resolve_path_relative_joins_against_brand_dir() {
    let r = BrandLogoResource::Path("logo.png".to_string());
    let resolved = r.with_path_relative_to(Path::new("assets/brand"));
    assert_eq!(resolved.path(), "assets/brand/logo.png");
}

#[test]
fn resolve_path_external_url_unchanged() {
    let r = BrandLogoResource::Path("https://example.com/logo.png".to_string());
    let resolved = r.with_path_relative_to(Path::new("assets/brand"));
    assert_eq!(resolved.path(), "https://example.com/logo.png");
}

#[test]
fn resolve_path_absolute_unchanged() {
    let r = BrandLogoResource::Path("/abs/path/logo.png".to_string());
    let resolved = r.with_path_relative_to(Path::new("assets/brand"));
    assert_eq!(resolved.path(), "/abs/path/logo.png");
}

#[test]
fn resolve_path_preserves_alt() {
    let r = BrandLogoResource::Explicit(quarto_brand::BrandLogoExplicit {
        path: "logo.png".to_string(),
        alt: Some("Brand".to_string()),
    });
    let resolved = r.with_path_relative_to(Path::new("brand"));
    assert_eq!(resolved.path(), "brand/logo.png");
    assert_eq!(resolved.alt(), Some("Brand"));
}
