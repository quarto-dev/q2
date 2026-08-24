//! Unified `_brand.yml` light/dark values: parsing and splitting
//! (bd-unified-brand-split-ep49amad, GH #580).
//!
//! Q1 reference: `splitUnifiedBrand` / `brandHasDarkMode` in
//! `external-sources/quarto-cli/src/core/brand/brand.ts`, and the
//! `brand-color-light-dark` schema in
//! `src/resources/schema/definitions.yml`. The parse form is the
//! *unified* brand ([`UnifiedBrand`]), where every named color slot
//! and typography color accepts either a plain string or a
//! `{light:, dark:}` pair; [`UnifiedBrand::split`] produces the two
//! single-mode [`Brand`]s the render pipeline consumes.

use quarto_brand::{BrandColorValue, LogoEntry, UnifiedBrand};

fn parse(yaml: &str) -> UnifiedBrand {
    UnifiedBrand::from_yaml_str(yaml).expect("unified brand must parse")
}

// ── parsing ─────────────────────────────────────────────────────────

#[test]
fn named_color_accepts_light_dark_pair() {
    // The literal shape from GH #580.
    let brand = parse("color:\n  background:\n    light: \"#b22221\"\n    dark: \"#22b221\"\n");
    let color = brand.color.as_ref().expect("color section");
    match color.background.as_ref().expect("background set") {
        BrandColorValue::LightDark(pair) => {
            assert_eq!(pair.light.as_deref(), Some("#b22221"));
            assert_eq!(pair.dark.as_deref(), Some("#22b221"));
        }
        other => panic!("expected LightDark, got {other:?}"),
    }
}

#[test]
fn named_color_plain_string_still_parses_as_single() {
    let brand = parse("color:\n  primary: \"#3366c1\"\n");
    let color = brand.color.as_ref().expect("color section");
    match color.primary.as_ref().expect("primary set") {
        BrandColorValue::Single(s) => assert_eq!(s, "#3366c1"),
        other => panic!("expected Single, got {other:?}"),
    }
}

#[test]
fn pair_with_unknown_key_is_rejected() {
    let err = UnifiedBrand::from_yaml_str(
        "color:\n  background:\n    light: \"#b22221\"\n    bogus: \"#22b221\"\n",
    );
    assert!(
        err.is_err(),
        "a light/dark pair with an unknown key must not parse: {err:?}"
    );
}

#[test]
fn palette_entry_light_dark_map_is_rejected() {
    // Deliberate limitation, matching Q1's schema (`brand-color-unified`
    // keeps `palette` values plain strings) and the callout in
    // docs/guides/authoring/brand.qmd.
    let err = UnifiedBrand::from_yaml_str(
        "color:\n  palette:\n    accent:\n      light: \"#b22221\"\n      dark: \"#22b221\"\n",
    );
    assert!(
        err.is_err(),
        "palette entries must stay plain strings: {err:?}"
    );
}

#[test]
fn typography_colors_accept_light_dark_pairs() {
    let brand = parse(concat!(
        "typography:\n",
        "  headings:\n",
        "    family: Rubik\n",
        "    color:\n",
        "      light: \"#111143\"\n",
        "      dark: \"#d0d0fe\"\n",
        "  monospace:\n",
        "    background-color:\n",
        "      light: \"#f0f0f1\"\n",
        "      dark: \"#20202f\"\n",
    ));
    let typography = brand.typography.as_ref().expect("typography section");
    let headings = typography.headings.as_ref().expect("headings slot");
    assert_eq!(headings.family.as_deref(), Some("Rubik"));
    match headings.color.as_ref().expect("headings color") {
        BrandColorValue::LightDark(pair) => {
            assert_eq!(pair.light.as_deref(), Some("#111143"));
            assert_eq!(pair.dark.as_deref(), Some("#d0d0fe"));
        }
        other => panic!("expected LightDark, got {other:?}"),
    }
    let monospace = typography.monospace.as_ref().expect("monospace slot");
    match monospace
        .background_color
        .as_ref()
        .expect("monospace background-color")
    {
        BrandColorValue::LightDark(pair) => {
            assert_eq!(pair.light.as_deref(), Some("#f0f0f1"));
            assert_eq!(pair.dark.as_deref(), Some("#20202f"));
        }
        other => panic!("expected LightDark, got {other:?}"),
    }
}

// ── splitting ───────────────────────────────────────────────────────

#[test]
fn split_plain_string_lands_in_both_halves() {
    let split = parse("color:\n  primary: \"#3366c1\"\n").split();
    let light = split.light.color.as_ref().expect("light color");
    let dark = split.dark.color.as_ref().expect("dark color");
    assert_eq!(light.primary.as_deref(), Some("#3366c1"));
    assert_eq!(dark.primary.as_deref(), Some("#3366c1"));
    assert!(!split.enables_dark_mode);
}

#[test]
fn split_pair_distributes_halves() {
    let split =
        parse("color:\n  background:\n    light: \"#b22221\"\n    dark: \"#22b221\"\n").split();
    assert_eq!(
        split
            .light
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        Some("#b22221")
    );
    assert_eq!(
        split
            .dark
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        Some("#22b221")
    );
    assert!(split.enables_dark_mode);
}

#[test]
fn split_light_only_pair_omits_slot_from_dark_half() {
    // Q1: `{light: X}` puts X in the light half and OMITS the slot
    // from the dark half — no fallback to the light value.
    let split = parse("color:\n  background:\n    light: \"#b22221\"\n").split();
    assert_eq!(
        split
            .light
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        Some("#b22221")
    );
    assert_eq!(
        split
            .dark
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        None,
        "dark half must not inherit the light-only value"
    );
    assert!(
        !split.enables_dark_mode,
        "a light-only pair does not enable dark mode (Q1: enablesDarkMode checks `dark`)"
    );
}

#[test]
fn split_dark_only_pair_omits_slot_from_light_half() {
    let split = parse("color:\n  background:\n    dark: \"#22b221\"\n").split();
    assert_eq!(
        split
            .light
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        None
    );
    assert_eq!(
        split
            .dark
            .color
            .as_ref()
            .and_then(|c| c.background.as_deref()),
        Some("#22b221")
    );
    assert!(split.enables_dark_mode);
}

#[test]
fn split_shares_palette_meta_and_defaults() {
    let split = parse(concat!(
        "meta:\n",
        "  name: Acme\n",
        "color:\n",
        "  palette:\n",
        "    brand-blue: \"#0066cd\"\n",
        "  primary: brand-blue\n",
        "defaults:\n",
        "  bootstrap:\n",
        "    enable-rounded: false\n",
    ))
    .split();
    for half in [&split.light, &split.dark] {
        assert_eq!(
            half.color
                .as_ref()
                .and_then(|c| c.palette.as_ref())
                .and_then(|p| p.get("brand-blue"))
                .map(String::as_str),
            Some("#0066cd"),
            "palette must be shared by both halves"
        );
        assert_eq!(
            half.meta
                .as_ref()
                .and_then(|m| m.name.as_ref())
                .and_then(|n| n.full_name()),
            Some("Acme"),
            "meta must be shared by both halves"
        );
        assert!(
            half.defaults
                .as_ref()
                .is_some_and(|d| d.bootstrap().is_some()),
            "defaults must be shared by both halves"
        );
    }
}

#[test]
fn split_specializes_typography_colors_and_keeps_shared_fields() {
    let split = parse(concat!(
        "typography:\n",
        "  fonts:\n",
        "    - family: Rubik\n",
        "      source: google\n",
        "  headings:\n",
        "    family: Rubik\n",
        "    weight: 600\n",
        "    color:\n",
        "      light: \"#111143\"\n",
        "      dark: \"#d0d0fe\"\n",
    ))
    .split();
    for (half, expected) in [(&split.light, "#111143"), (&split.dark, "#d0d0fe")] {
        let typography = half.typography.as_ref().expect("typography survives split");
        assert_eq!(typography.fonts.len(), 1, "fonts list shared");
        let headings = typography.headings.as_ref().expect("headings slot");
        assert_eq!(headings.family.as_deref(), Some("Rubik"));
        assert!(headings.weight.is_some(), "non-color fields shared");
        assert_eq!(headings.color.as_deref(), Some(expected));
    }
    assert!(split.enables_dark_mode);
}

#[test]
fn split_typography_light_only_color_omits_dark_side() {
    let split = parse(concat!(
        "typography:\n",
        "  headings:\n",
        "    family: Rubik\n",
        "    color:\n",
        "      light: \"#111143\"\n",
    ))
    .split();
    let dark_headings = split
        .dark
        .typography
        .as_ref()
        .and_then(|t| t.headings.as_ref())
        .expect("headings slot survives in the dark half");
    assert_eq!(dark_headings.family.as_deref(), Some("Rubik"));
    assert_eq!(dark_headings.color, None);
    assert!(!split.enables_dark_mode);
}

#[test]
fn split_carries_logo_pair_through_unchanged() {
    // Deliberate divergence from Q1's splitLogo: q2's logo consumers
    // (favicon, navbar image) run once per document and need both
    // sides — `LogoEntry::LightDark` models that already, so the
    // split leaves logo entries intact in both halves.
    let split = parse(concat!(
        "logo:\n",
        "  small:\n",
        "    light: light-logo.png\n",
        "    dark: dark-logo.png\n",
        "  medium: plain-logo.png\n",
    ))
    .split();
    for half in [&split.light, &split.dark] {
        let logo = half.logo.as_ref().expect("logo section survives split");
        match logo.small.as_ref().expect("small slot") {
            LogoEntry::LightDark { light, dark } => {
                assert_eq!(light.as_ref().map(|r| r.path()), Some("light-logo.png"));
                assert_eq!(dark.as_ref().map(|r| r.path()), Some("dark-logo.png"));
            }
            other => panic!("logo pair must be carried through, got {other:?}"),
        }
        match logo.medium.as_ref().expect("medium slot") {
            LogoEntry::Single(r) => assert_eq!(r.path(), "plain-logo.png"),
            other => panic!("single logo must stay single, got {other:?}"),
        }
    }
    assert!(
        split.enables_dark_mode,
        "a logo dark half enables dark mode (Q1 parity)"
    );
}

// ── has_dark_mode ───────────────────────────────────────────────────

#[test]
fn has_dark_mode_false_for_all_plain_brand() {
    let brand = parse(concat!(
        "color:\n",
        "  primary: \"#3366c1\"\n",
        "  background: \"#fefefd\"\n",
        "typography:\n",
        "  headings:\n",
        "    color: \"#111143\"\n",
        "logo:\n",
        "  small: logo.png\n",
    ));
    assert!(!brand.has_dark_mode());
}

#[test]
fn has_dark_mode_true_for_color_dark_value() {
    assert!(parse("color:\n  primary:\n    dark: \"#22b221\"\n").has_dark_mode());
}

#[test]
fn has_dark_mode_false_for_light_only_pair() {
    assert!(!parse("color:\n  primary:\n    light: \"#b22221\"\n").has_dark_mode());
}

#[test]
fn has_dark_mode_true_for_typography_color_dark_value() {
    assert!(
        parse("typography:\n  headings:\n    color:\n      dark: \"#d0d0fe\"\n").has_dark_mode()
    );
}

#[test]
fn has_dark_mode_true_for_typography_background_color_dark_value() {
    assert!(
        parse("typography:\n  monospace:\n    background-color:\n      dark: \"#20202f\"\n")
            .has_dark_mode()
    );
}

#[test]
fn has_dark_mode_true_for_logo_dark_side() {
    assert!(
        parse("logo:\n  small:\n    light: l.png\n    dark: d.png\n").has_dark_mode(),
        "a logo with a dark side enables dark mode"
    );
}

#[test]
fn has_dark_mode_false_for_logo_light_only() {
    assert!(!parse("logo:\n  small:\n    light: l.png\n").has_dark_mode());
}
