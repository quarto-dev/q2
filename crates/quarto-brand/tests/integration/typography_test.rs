//! Tests for typography slot lookup and monospace normalization.
//!
//! Mirrors Q1's `Brand.processData()` normalization from
//! `external-sources/quarto-cli/src/core/brand/brand.ts`: when both
//! `monospace` and `monospace-{inline,block}` are set, the more
//! specific one overlays the generic. When only `monospace` is set, it
//! is propagated into both slots so consumers don't have to special-
//! case.

use quarto_brand::Brand;

fn brand(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .expect("parse")
        .split()
        .light
}

#[test]
fn font_slot_returns_specific_when_set() {
    let b = brand(
        "typography:\n\
         \x20 base:\n\
         \x20   family: Arial\n",
    );
    let slot = b.font_slot("base").expect("base slot");
    assert_eq!(slot.family.as_deref(), Some("Arial"));
}

#[test]
fn font_slot_returns_none_when_typography_missing() {
    let b = brand("");
    assert!(b.font_slot("base").is_none());
}

#[test]
fn effective_monospace_inline_from_monospace_only() {
    // If only `monospace` is set, it propagates into inline.
    let b = brand(
        "typography:\n\
         \x20 monospace:\n\
         \x20   family: Fira Code\n\
         \x20   color: \"#222\"\n",
    );
    let m = b.effective_monospace_inline().expect("monospace-inline");
    assert_eq!(m.family.as_deref(), Some("Fira Code"));
    assert_eq!(m.color.as_deref(), Some("#222"));
}

#[test]
fn effective_monospace_inline_merges_specific_over_generic() {
    // monospace sets family + color; monospace-inline overrides color.
    // Q1 spread order is `{ ...monospace, ...monospace-inline }`, so
    // the inline value wins.
    let b = brand(
        "typography:\n\
         \x20 monospace:\n\
         \x20   family: Fira Code\n\
         \x20   color: \"#222\"\n\
         \x20 monospace-inline:\n\
         \x20   color: \"#f32\"\n",
    );
    let m = b.effective_monospace_inline().expect("inline");
    assert_eq!(m.family.as_deref(), Some("Fira Code"));
    assert_eq!(m.color.as_deref(), Some("#f32"));
}

#[test]
fn effective_monospace_block_merges_specific_over_generic() {
    let b = brand(
        "typography:\n\
         \x20 monospace:\n\
         \x20   family: Fira Code\n\
         \x20 monospace-block:\n\
         \x20   size: 8pt\n",
    );
    let m = b.effective_monospace_block().expect("block");
    assert_eq!(m.family.as_deref(), Some("Fira Code"));
    assert_eq!(m.size.as_deref(), Some("8pt"));
}

#[test]
fn effective_monospace_returns_none_when_neither_set() {
    let b = brand("typography:\n  base:\n    family: Arial\n");
    assert!(b.effective_monospace_inline().is_none());
    assert!(b.effective_monospace_block().is_none());
}

#[test]
fn font_slot_accepts_bare_string_as_family_shorthand() {
    // Q1 accepts `base: "Open Sans"` as shorthand for
    // `base: { family: "Open Sans" }`. Same for monospace and the
    // other font slots.
    let b = brand(
        "typography:\n\
         \x20 base: Open Sans\n\
         \x20 monospace: IBM Plex Mono\n",
    );
    let base = b.font_slot("base").expect("base slot");
    assert_eq!(base.family.as_deref(), Some("Open Sans"));
    let mono = b.font_slot("monospace").expect("monospace slot");
    assert_eq!(mono.family.as_deref(), Some("IBM Plex Mono"));
}

#[test]
fn fonts_iterates_in_source_order() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: google\n\
         \x20     family: A\n\
         \x20   - source: google\n\
         \x20     family: B\n",
    );
    let fams: Vec<&str> = b
        .fonts()
        .iter()
        .map(|f| match f {
            quarto_brand::BrandFont::Google(g) => g.family.as_str(),
            _ => "<other>",
        })
        .collect();
    assert_eq!(fams, vec!["A", "B"]);
}
