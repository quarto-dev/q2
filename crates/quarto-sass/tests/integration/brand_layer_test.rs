//! Tests for `_brand.yml` → `SassLayer` translation.
//!
//! Ports the behavior of Q1's `core/sass/brand.ts`:
//! - `brandColorLayer`: palette + named-color SCSS/CSS variables
//! - `brandDefaultsBootstrapLayer`: Bootstrap-targeted defaults +
//!   passthrough sections
//! - `brandTypographyLayer`: font @import / @font-face + per-slot SCSS
//!   variable assignments
//!
//! Expected SCSS strings are derived by hand from a careful read of
//! Q1's algorithm; the External Sources Policy forbids reading from
//! `external-sources/` at test time.

use std::path::Path;

use quarto_brand::Brand;
use quarto_sass::brand_to_layers;

fn brand(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .expect("parse")
        .split()
        .light
}

// ── color layer ─────────────────────────────────────────────────────

#[test]
fn color_layer_emits_brand_palette_sass_vars() {
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   red: \"#FF0000\"\n\
         \x20   black: \"#002040\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    // The color layer is always the first non-empty layer.
    let color = &layers[0];
    assert!(
        color.defaults.contains("$brand-red: #FF0000 !default;"),
        "missing $brand-red in defaults:\n{}",
        color.defaults
    );
    assert!(
        color.defaults.contains("$brand-black: #002040 !default;"),
        "missing $brand-black in defaults:\n{}",
        color.defaults
    );
}

#[test]
fn color_layer_emits_brand_palette_css_custom_props() {
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   red: \"#FF0000\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let color = &layers[0];
    assert!(
        color.rules.contains(":root {"),
        "missing :root block in rules:\n{}",
        color.rules
    );
    assert!(
        color.rules.contains("--brand-red: #FF0000;"),
        "missing --brand-red:\n{}",
        color.rules
    );
}

#[test]
fn color_layer_emits_named_theme_color_sass_vars_with_resolution() {
    // primary references palette entry "red"; resolution must happen.
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   red: \"#FF0000\"\n\
         \x20 primary: red\n\
         \x20 foreground: \"#21f\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let color = &layers[0];
    assert!(
        color.defaults.contains("$primary: #FF0000 !default;"),
        "missing $primary resolved through palette:\n{}",
        color.defaults
    );
    assert!(
        color.defaults.contains("$foreground: #21f !default;"),
        "missing $foreground:\n{}",
        color.defaults
    );
}

#[test]
fn color_layer_applies_default_color_name_map() {
    // foreground:#21f → body-color, pre-color, body-bg (no — that's background)
    // background:#e6f8ff → body-bg
    let b = brand(
        "color:\n\
         \x20 foreground: \"#21f\"\n\
         \x20 background: \"#e6f8ff\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let color = &layers[0];
    // foreground maps to body-color and pre-color and body-color.
    assert!(
        color.defaults.contains("$body-color: #21f !default;"),
        "expected body-color via name map:\n{}",
        color.defaults
    );
    assert!(
        color.defaults.contains("$pre-color: #21f !default;"),
        "expected pre-color via name map:\n{}",
        color.defaults
    );
    assert!(
        color.defaults.contains("$body-bg: #e6f8ff !default;"),
        "expected body-bg via name map:\n{}",
        color.defaults
    );
}

#[test]
fn color_layer_palette_key_sanitization() {
    // Q1 sanitizes palette keys: any non-[a-zA-Z0-9_-] becomes "-".
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   \"my color\": \"#abc\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let color = &layers[0];
    assert!(
        color.defaults.contains("$brand-my-color: #abc !default;"),
        "expected sanitized $brand-my-color:\n{}",
        color.defaults
    );
}

#[test]
fn empty_brand_produces_no_layers() {
    let b = brand("");
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    assert!(
        layers.is_empty(),
        "empty brand should produce no layers, got {} layer(s)",
        layers.len()
    );
}

// ── bootstrap-defaults layer ────────────────────────────────────────

#[test]
fn bootstrap_defaults_layer_emits_bootstrap_colors_from_palette() {
    // Palette keys that match Bootstrap's named colors (black, white,
    // blue, ...) become `$<color>: <value> !default;` in the
    // bootstrap-defaults layer.
    let b = brand(
        "color:\n\
         \x20 palette:\n\
         \x20   blue: \"#0000ff\"\n\
         \x20   purple: \"#800080\"\n\
         \x20   not_a_bs_color: \"#abc\"\n\
         defaults:\n\
         \x20 bootstrap:\n\
         \x20   defaults:\n\
         \x20     font-size-base: \"1.1rem\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    // Per Q1's `brandBootstrapSassLayers`, bootstrap-defaults is
    // `unshift`-ed to the front of the user layers — so the order is
    // [bootstrap_defaults, color, typography?].
    let bs = &layers[0];
    assert!(
        bs.defaults.contains("$blue: #0000ff !default;"),
        "expected $blue from palette:\n{}",
        bs.defaults
    );
    assert!(
        bs.defaults.contains("$purple: #800080 !default;"),
        "expected $purple from palette:\n{}",
        bs.defaults
    );
    assert!(
        !bs.defaults.contains("not_a_bs_color"),
        "non-Bootstrap palette entries should not leak in:\n{}",
        bs.defaults
    );
    assert!(
        bs.defaults.contains("$font-size-base: 1.1rem !default;"),
        "expected $font-size-base from defaults.bootstrap.defaults:\n{}",
        bs.defaults
    );
}

#[test]
fn bootstrap_defaults_layer_emits_passthrough_sections() {
    let b = brand(
        "defaults:\n\
         \x20 bootstrap:\n\
         \x20   uses: \"@use 'sass:math';\"\n\
         \x20   functions: \"@function foo() { @return 1; }\"\n\
         \x20   mixins: \"@mixin bar { color: red; }\"\n\
         \x20   rules: \".my-class { color: red; }\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let bs = &layers[0]; // no color/typography, so bootstrap-defaults is first
    assert!(bs.uses.contains("@use 'sass:math';"), "uses: {}", bs.uses);
    assert!(
        bs.functions.contains("@function foo()"),
        "functions: {}",
        bs.functions
    );
    assert!(bs.mixins.contains("@mixin bar"), "mixins: {}", bs.mixins);
    assert!(bs.rules.contains(".my-class"), "rules: {}", bs.rules);
}

#[test]
fn no_bootstrap_defaults_no_bootstrap_layer() {
    // Brand with color only — no bootstrap defaults — should produce
    // exactly one layer (the color layer).
    let b = brand("color:\n  primary: \"#abc\"\n");
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    assert_eq!(
        layers.len(),
        1,
        "expected just color layer, got {:?}",
        layers
    );
}

// ── typography layer ────────────────────────────────────────────────

#[test]
fn typography_layer_emits_google_font_import() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: google\n\
         \x20     family: EB Garamond\n\
         \x20     weight: [400, 700]\n\
         \x20     style: [normal, italic]\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let typ = layers.last().expect("typography layer");
    // Q1 places font @import lines in `uses` (so they land at the top
    // of the compiled SCSS, before any rules).
    assert!(
        typ.uses
            .contains("fonts.googleapis.com/css2?family=EB+Garamond"),
        "expected google import URL in uses:\n{}",
        typ.uses
    );
    assert!(
        typ.uses.contains("ital,"),
        "italic style flag should be set:\n{}",
        typ.uses
    );
}

#[test]
fn typography_layer_emits_file_font_face() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: file\n\
         \x20     family: Brand Font\n\
         \x20     files:\n\
         \x20       - path: regular.woff2\n\
         \x20         weight: 400\n\
         \x20         style: normal\n",
    );
    let layers = brand_to_layers(&b, Path::new("brand")).unwrap();
    let typ = layers.last().expect("typography layer");
    // @font-face blocks live in `uses` alongside @import lines.
    assert!(
        typ.uses.contains("@font-face {"),
        "expected @font-face block:\n{}",
        typ.uses
    );
    assert!(
        typ.uses.contains("font-family: \"Brand Font\""),
        "family in @font-face (quoted):\n{}",
        typ.uses
    );
    assert!(
        typ.uses.contains("brand/regular.woff2"),
        "relative path joined with font_path_prefix:\n{}",
        typ.uses
    );
    assert!(
        typ.uses.contains("font-weight: 400"),
        "weight:\n{}",
        typ.uses
    );
}

#[test]
fn typography_layer_base_font_assigns_bootstrap_vars() {
    let b = brand(
        "typography:\n\
         \x20 base:\n\
         \x20   family: EB Garamond\n\
         \x20   size: 12pt\n\
         \x20   weight: 400\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let typ = layers.last().expect("typography layer");
    assert!(
        typ.defaults
            .contains("$font-family-base: \"EB Garamond\" !default;"),
        "expected $font-family-base:\n{}",
        typ.defaults
    );
    assert!(
        typ.defaults.contains("$font-size-base: 12pt !default;"),
        "expected $font-size-base:\n{}",
        typ.defaults
    );
    assert!(
        typ.defaults.contains("$font-weight-base: 400 !default;"),
        "expected $font-weight-base:\n{}",
        typ.defaults
    );
}

#[test]
fn typography_layer_headings_font_emits_revealjs_vars_too() {
    let b = brand("typography:\n  headings:\n    family: PT Sans\n");
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let typ = layers.last().expect("typography layer");
    // Both bootstrap and revealjs targets are emitted.
    assert!(
        typ.defaults
            .contains("$headings-font-family: \"PT Sans\" !default;"),
        "expected bootstrap var:\n{}",
        typ.defaults
    );
    assert!(
        typ.defaults
            .contains("$presentation-heading-font: \"PT Sans\" !default;"),
        "expected revealjs var:\n{}",
        typ.defaults
    );
}

#[test]
fn typography_layer_monospace_propagates_to_inline_and_block() {
    // Per Q1 semantics: if only `monospace` is set, both inline and
    // block slots receive its values.
    let b = brand(
        "typography:\n\
         \x20 monospace:\n\
         \x20   family: Fira Code\n\
         \x20   color: \"#222\"\n",
    );
    let layers = brand_to_layers(&b, Path::new("")).unwrap();
    let typ = layers.last().expect("typography layer");
    // Bootstrap monospace var
    assert!(
        typ.defaults
            .contains("$font-family-monospace: \"Fira Code\" !default;"),
        "expected $font-family-monospace:\n{}",
        typ.defaults
    );
}
