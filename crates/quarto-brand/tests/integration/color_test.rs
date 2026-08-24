//! Tests for color name resolution.
//!
//! Mirrors Q1's `Brand.getColor` semantics from
//! `external-sources/quarto-cli/src/core/brand/brand.ts`:
//!
//! - If `name` is in `color.palette`, recursively resolve the
//!   palette's value.
//! - Else if `name` is one of the named theme colors (`primary`,
//!   `secondary`, ...) and that slot is set on `color.*`, recursively
//!   resolve the slot's value.
//! - Else, treat `name` as a raw CSS color value and return it
//!   verbatim.
//! - Cycle detection caps at 100 hops.

use quarto_brand::{Brand, BrandError};

fn brand(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .expect("parse")
        .split()
        .light
}

#[test]
fn resolve_raw_color_passthrough() {
    let b = brand("color:\n  primary: red\n");
    // "red" is not in any palette and not a named theme color we
    // recurse into; it's a raw CSS color name → passthrough.
    assert_eq!(b.resolve_color("red").unwrap(), "red");
    assert_eq!(b.resolve_color("#abc").unwrap(), "#abc");
    assert_eq!(b.resolve_color("rgb(0, 0, 0)").unwrap(), "rgb(0, 0, 0)");
}

#[test]
fn resolve_palette_alias() {
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   brand-red: \"#FF0000\"\n\
         \x20 primary: brand-red\n",
    );
    assert_eq!(b.resolve_color("brand-red").unwrap(), "#FF0000");
}

#[test]
fn resolve_named_theme_color_to_palette() {
    // Kitchen-sink-style: palette defines red; primary references red.
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   red: \"#FF0000\"\n\
         \x20 primary: red\n",
    );
    assert_eq!(b.resolve_color("primary").unwrap(), "#FF0000");
}

#[test]
fn resolve_named_theme_color_when_unset_is_passthrough() {
    // `primary` is not set on color.* and not in palette: passthrough.
    let b = brand("color:\n  secondary: blue\n");
    assert_eq!(b.resolve_color("primary").unwrap(), "primary");
}

#[test]
fn resolve_multi_step_alias() {
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   inner: \"#abcdef\"\n\
         \x20   outer: inner\n\
         \x20 primary: outer\n",
    );
    assert_eq!(b.resolve_color("primary").unwrap(), "#abcdef");
}

#[test]
fn resolve_cycle_in_palette_errors() {
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   a: b\n\
         \x20   b: a\n",
    );
    let err = b.resolve_color("a").unwrap_err();
    assert!(matches!(err, BrandError::CircularColorReference { .. }));
}

#[test]
fn resolve_empty_color_section_is_passthrough() {
    let b = brand("");
    assert_eq!(b.resolve_color("red").unwrap(), "red");
}

#[test]
fn resolve_color_quiet_does_not_warn_on_unknown() {
    // Quiet mode: just passes through, no error.
    let b = brand("color: {}\n");
    assert_eq!(
        b.resolve_color_quiet("not-a-real-color"),
        "not-a-real-color"
    );
}
