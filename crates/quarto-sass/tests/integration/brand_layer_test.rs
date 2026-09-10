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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
         \x20       - path: assets/sub/regular.woff2\n\
         \x20         weight: 400\n\
         \x20         style: normal\n",
    );
    let layers = brand_to_layers(&b).unwrap();
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
    // bd-ve916wr8: the file is published beside the theme CSS as
    // `fonts/<basename>`, so the URL is that constant — never the
    // brand-relative source path, and never document-relative.
    assert!(
        typ.uses
            .contains("src: url('fonts/regular.woff2') format('woff2');"),
        "published-name URL with a format() hint:\n{}",
        typ.uses
    );
    assert!(
        !typ.uses.contains("assets/sub"),
        "source directory must not leak into the URL:\n{}",
        typ.uses
    );
    assert!(
        typ.uses.contains("font-weight: 400"),
        "weight:\n{}",
        typ.uses
    );
}

fn file_font_uses(path: &str) -> String {
    let b = brand(&format!(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: file\n\
         \x20     family: F\n\
         \x20     files:\n\
         \x20       - {path}\n"
    ));
    let layers = brand_to_layers(&b).unwrap();
    layers.last().expect("typography layer").uses.clone()
}

/// A leading `/` means the project root (path-resolution contract);
/// `../` climbs out of the brand dir. Both are *source* locations —
/// the published URL is still `fonts/<basename>` (bd-ve916wr8
/// decision 3).
#[test]
fn file_font_face_rooted_and_parent_source_paths_publish_by_basename() {
    let rooted = file_font_uses("/assets/Rooted.woff");
    assert!(
        rooted.contains("src: url('fonts/Rooted.woff') format('woff');"),
        "rooted path:\n{rooted}"
    );
    let parent = file_font_uses("../shared/Parent.ttf");
    assert!(
        parent.contains("src: url('fonts/Parent.ttf') format('truetype');"),
        "parent path:\n{parent}"
    );
    let otf = file_font_uses("Plain.otf");
    assert!(
        otf.contains("src: url('fonts/Plain.otf') format('opentype');"),
        "otf:\n{otf}"
    );
}

#[test]
fn file_font_face_external_url_passes_through_without_format_hint() {
    let uses = file_font_uses("https://fonts.example.com/dir/Remote.woff2");
    assert!(
        uses.contains("src: url('https://fonts.example.com/dir/Remote.woff2');"),
        "external URL verbatim:\n{uses}"
    );
    assert!(
        !uses.contains("format("),
        "no format() hint for an external URL:\n{uses}"
    );
}

#[test]
fn file_font_face_unknown_extension_has_no_format_hint() {
    let uses = file_font_uses("assets/Mystery.font");
    assert!(
        uses.contains("src: url('fonts/Mystery.font');"),
        "unknown extension still published by basename:\n{uses}"
    );
    assert!(!uses.contains("format("), "no format() hint:\n{uses}");
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
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
    let layers = brand_to_layers(&b).unwrap();
    let typ = layers.last().expect("typography layer");
    // Bootstrap monospace var
    assert!(
        typ.defaults
            .contains("$font-family-monospace: \"Fira Code\" !default;"),
        "expected $font-family-monospace:\n{}",
        typ.defaults
    );
}

// ── font weight ranges (bd-5fseopxy) ────────────────────────────────
//
// brand.yml variable-font ranges (`weight: 400..700`). Google serves a
// true variable axis via `wght@N..M`; Bunny has no range syntax
// (`inter:400..700` silently serves 400 only — checked 2026-09-08), so
// a range is expanded to discrete weights there; a local variable font
// file declares its axis with CSS `font-weight: N M`.

fn typography_uses(yaml: &str) -> String {
    let b = brand(yaml);
    let layers = brand_to_layers(&b).unwrap();
    layers.last().expect("typography layer").uses.clone()
}

#[test]
fn google_weight_range_emits_wght_axis() {
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: 400..700\n\
         \x20     style: normal\n",
    );
    assert!(
        uses.contains("family=EB+Garamond:wght@400..700&display=swap"),
        "expected the wght axis range form:\n{uses}"
    );
}

#[test]
fn google_weight_range_with_italic_emits_ital_wght_pairs() {
    // Default style is [normal, italic]; each italic value pairs with
    // the whole range, exactly as discrete weights do.
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: 400..700\n",
    );
    assert!(
        uses.contains("family=EB+Garamond:ital,wght@0,400..700;1,400..700&display=swap"),
        "expected ital,wght pairs over the range:\n{uses}"
    );
}

#[test]
fn google_keyword_list_still_emits_discrete_weights() {
    // Regression guard for the pre-existing list behavior alongside
    // the new range path.
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: [regular, bold]\n\
         \x20     style: normal\n",
    );
    assert!(
        uses.contains("family=EB+Garamond:wght@400;700&display=swap"),
        "expected discrete weights:\n{uses}"
    );
}

#[test]
fn bunny_weight_range_expands_to_hundreds() {
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Inter\n\
         \x20     source: bunny\n\
         \x20     weight: 400..700\n\
         \x20     style: normal\n",
    );
    assert!(
        uses.contains("family=Inter:400,500,600,700&display=swap"),
        "expected the range expanded to discrete weights:\n{uses}"
    );
}

#[test]
fn bunny_weight_range_with_italic_expands_both_halves() {
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Inter\n\
         \x20     source: bunny\n\
         \x20     weight: 400..600\n",
    );
    assert!(
        uses.contains("family=Inter:400i,500i,600i,400,500,600&display=swap"),
        "expected italic and normal expansions:\n{uses}"
    );
}

#[test]
fn bunny_non_round_range_keeps_its_ends() {
    // Endpoints are kept verbatim; the multiples of 100 strictly
    // between them are filled in.
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Inter\n\
         \x20     source: bunny\n\
         \x20     weight: 450..620\n\
         \x20     style: normal\n",
    );
    assert!(
        uses.contains("family=Inter:450,500,600,620&display=swap"),
        "expected ends plus interior hundreds:\n{uses}"
    );
}

#[test]
fn file_weight_range_emits_css_font_weight_pair() {
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Local Var\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - path: LocalVar-VariableFont_wght.woff2\n\
         \x20         weight: 300..800\n",
    );
    assert!(
        uses.contains("font-weight: 300 800;"),
        "expected the CSS variable-font weight pair:\n{uses}"
    );
    assert!(
        !uses.contains("300..800"),
        "the YAML range syntax must not leak into CSS:\n{uses}"
    );
}

#[test]
fn file_keyword_weight_still_maps_to_number() {
    let uses = typography_uses(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Local\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - path: local-bold.woff2\n\
         \x20         weight: bold\n",
    );
    assert!(uses.contains("font-weight: 700;"), "{uses}");
}

/// Emission is defensive: even when a caller skipped
/// `Brand::validate`, a range on a typography slot must not be written
/// into SCSS (`$headings-font-weight: 500..700` fails the whole theme
/// compile).
#[test]
fn slot_weight_range_is_an_error_not_scss() {
    let b = brand(
        "typography:\n\
         \x20 headings:\n\
         \x20   family: A\n\
         \x20   weight: 500..700\n",
    );
    match brand_to_layers(&b) {
        Err(quarto_sass::SassError::InvalidBrandFontWeight { path, value, .. }) => {
            assert_eq!(path, "typography.headings.weight");
            assert_eq!(value, "500..700");
        }
        Err(other) => panic!("expected InvalidBrandFontWeight, got {other:?}"),
        Ok(layers) => panic!(
            "expected an error, got layers:\n{}",
            layers.last().map_or("", |l| l.defaults.as_str())
        ),
    }
}

/// Same defensive contract for the silent-400 path: an unknown
/// keyword on a google font is an error, never `wght@400`.
#[test]
fn unknown_google_weight_keyword_is_an_error_not_400() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: Bold\n",
    );
    match brand_to_layers(&b) {
        Err(quarto_sass::SassError::InvalidBrandFontWeight { path, value, .. }) => {
            assert_eq!(path, "typography.fonts[0].weight");
            assert_eq!(value, "Bold");
        }
        Err(other) => panic!("expected InvalidBrandFontWeight, got {other:?}"),
        Ok(layers) => panic!(
            "expected an error, got:\n{}",
            layers.last().map_or("", |l| l.uses.as_str())
        ),
    }
}
