//! Font-weight parsing and validation (bd-5fseopxy).
//!
//! brand.yml allows a variable-font weight *range* written as a string
//! (`weight: 400..700`) on `typography.fonts[]` entries. Before this
//! work the string deserialized as `BrandFontWeight::Name("400..700")`
//! and every consumer silently substituted 400. These tests pin the
//! typed range variant and the post-parse validation that turns any
//! unrecognised weight into a hard error naming the offending YAML
//! path — Quarto 1 fails hard on unknown weight keywords; Quarto 2
//! must never fall back to 400.
//!
//! Design (see claude-notes/plans/2026-09-08-brand-font-weight-ranges.md):
//! - Range syntax is numeric only (`N..M`, both in 100..=900, `N <= M`).
//! - Keyword ends (`regular..bold`) are not accepted.
//! - Typography slots (`base`, `headings`, …) take one weight; a range
//!   there is an error.

use quarto_brand::{BrandError, BrandFontWeight, BrandFontWeightAtom, UnifiedBrand};

fn parse(yaml: &str) -> UnifiedBrand {
    UnifiedBrand::from_yaml_str(yaml).unwrap_or_else(|e| panic!("parse:\n{yaml}\n{e}"))
}

/// `weight` of the first `typography.fonts[]` entry (google/bunny).
fn first_font_weight(b: &UnifiedBrand) -> &BrandFontWeight {
    match &b.fonts()[0] {
        quarto_brand::BrandFont::Google(g) | quarto_brand::BrandFont::Bunny(g) => {
            g.weight.as_ref().expect("weight set")
        }
        other => panic!("expected google/bunny font, got {other:?}"),
    }
}

fn google_font(weight_yaml: &str) -> String {
    format!(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: {weight_yaml}\n"
    )
}

/// Run validation and unwrap the `InvalidFontWeight` payload.
fn expect_invalid(yaml: &str) -> (String, String, String) {
    let b = parse(yaml);
    match b.validate() {
        Err(BrandError::InvalidFontWeight {
            path,
            value,
            reason,
        }) => (path.to_string(), value, reason),
        Err(other) => panic!("expected InvalidFontWeight, got {other:?}\n{yaml}"),
        Ok(()) => panic!("expected validation error, got Ok\n{yaml}"),
    }
}

// ── parsing ─────────────────────────────────────────────────────────

#[test]
fn range_string_parses_to_range_variant() {
    let b = parse(&google_font("400..700"));
    match first_font_weight(&b) {
        BrandFontWeight::Range(r) => {
            assert_eq!(r.min, 400);
            assert_eq!(r.max, 700);
        }
        other => panic!("expected Range, got {other:?}"),
    }
}

#[test]
fn quoted_range_string_parses_to_range_variant() {
    let b = parse(&google_font("\"300..800\""));
    match first_font_weight(&b) {
        BrandFontWeight::Range(r) => {
            assert_eq!((r.min, r.max), (300, 800));
        }
        other => panic!("expected Range, got {other:?}"),
    }
}

#[test]
fn number_keyword_and_list_still_parse_as_before() {
    let b = parse(&google_font("400"));
    assert!(matches!(
        first_font_weight(&b),
        BrandFontWeight::Number(400)
    ));

    let b = parse(&google_font("bold"));
    assert!(matches!(first_font_weight(&b), BrandFontWeight::Name(s) if s == "bold"));

    let b = parse(&google_font("[400, bold]"));
    match first_font_weight(&b) {
        BrandFontWeight::List(items) => {
            assert_eq!(items.len(), 2);
            assert!(matches!(items[0], BrandFontWeightAtom::Number(400)));
            assert!(matches!(&items[1], BrandFontWeightAtom::Name(s) if s == "bold"));
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn malformed_range_strings_parse_as_names() {
    // Syntactically not `N..M` — these stay `Name` so validation can
    // report them as unknown weights rather than as bad ranges.
    for s in [
        "400..",
        "..700",
        "regular..bold",
        "400...700",
        "400..700..900",
    ] {
        let b = parse(&google_font(s));
        assert!(
            matches!(first_font_weight(&b), BrandFontWeight::Name(n) if n == s),
            "{s:?} should parse as Name, got {:?}",
            first_font_weight(&b)
        );
    }
}

#[test]
fn range_round_trips_through_serialization() {
    let b = parse(&google_font("400..700"));
    let json = serde_json::to_string(first_font_weight(&b)).unwrap();
    assert_eq!(json, "\"400..700\"");
    let back: BrandFontWeight = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, BrandFontWeight::Range(r) if r.min == 400 && r.max == 700));
}

// ── keyword table ───────────────────────────────────────────────────

#[test]
fn weight_name_to_number_matches_q1_table() {
    use quarto_brand::weight_name_to_number as w;
    assert_eq!(w("thin"), Some(100));
    assert_eq!(w("extra-light"), Some(200));
    assert_eq!(w("ultra-light"), Some(200));
    assert_eq!(w("light"), Some(300));
    assert_eq!(w("normal"), Some(400));
    assert_eq!(w("regular"), Some(400));
    assert_eq!(w("medium"), Some(500));
    assert_eq!(w("semi-bold"), Some(600));
    assert_eq!(w("demi-bold"), Some(600));
    assert_eq!(w("bold"), Some(700));
    assert_eq!(w("extra-bold"), Some(800));
    assert_eq!(w("ultra-bold"), Some(800));
    assert_eq!(w("black"), Some(900));
    // Case-sensitive and closed, like Q1's `brandFontWeightValue`.
    assert_eq!(w("Bold"), None);
    assert_eq!(w("semibold"), None);
    assert_eq!(w("bolder"), None);
    assert_eq!(w("lighter"), None);
}

// ── validation: accepted shapes ─────────────────────────────────────

#[test]
fn validate_accepts_every_supported_weight_shape() {
    let b = parse(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: A\n\
         \x20     source: google\n\
         \x20     weight: 400..700\n\
         \x20   - family: B\n\
         \x20     source: bunny\n\
         \x20     weight: [thin, 450, black]\n\
         \x20   - family: C\n\
         \x20     source: google\n\
         \x20     weight: semi-bold\n\
         \x20   - family: D\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - path: d-var.woff2\n\
         \x20         weight: 300..800\n\
         \x20       - path: d-bold.woff2\n\
         \x20         weight: bold\n\
         \x20       - d-plain.woff2\n\
         \x20   - family: E\n\
         \x20     source: system\n\
         \x20 base:\n\
         \x20   family: A\n\
         \x20   weight: 450\n\
         \x20 headings:\n\
         \x20   family: A\n\
         \x20   weight: bold\n",
    );
    b.validate().expect("all weights valid");
}

#[test]
fn validate_accepts_degenerate_and_non_round_ranges() {
    parse(&google_font("400..400")).validate().unwrap();
    parse(&google_font("350..640")).validate().unwrap();
    parse(&google_font("100..900")).validate().unwrap();
}

#[test]
fn validate_accepts_brand_without_typography() {
    parse("color:\n  palette:\n    red: \"#ff0000\"\n")
        .validate()
        .unwrap();
    parse("").validate().unwrap();
}

// ── validation: rejected ranges ─────────────────────────────────────

#[test]
fn validate_rejects_reversed_range() {
    let (path, value, reason) = expect_invalid(&google_font("700..400"));
    assert_eq!(path, "typography.fonts[0].weight");
    assert_eq!(value, "700..400");
    assert!(
        reason.contains("700") && reason.contains("400"),
        "reason should name both ends: {reason}"
    );
}

#[test]
fn validate_rejects_range_ends_outside_100_900() {
    let (path, value, _) = expect_invalid(&google_font("50..700"));
    assert_eq!(path, "typography.fonts[0].weight");
    assert_eq!(value, "50..700");

    let (_, value, reason) = expect_invalid(&google_font("400..1000"));
    assert_eq!(value, "400..1000");
    assert!(
        reason.contains("100") && reason.contains("900"),
        "reason should state the allowed span: {reason}"
    );
}

#[test]
fn validate_rejects_keyword_range_ends() {
    let (path, value, reason) = expect_invalid(&google_font("regular..bold"));
    assert_eq!(path, "typography.fonts[0].weight");
    assert_eq!(value, "regular..bold");
    assert!(
        reason.contains(".."),
        "reason should mention the N..M range form: {reason}"
    );
}

#[test]
fn validate_rejects_half_open_ranges() {
    for s in ["400..", "..700"] {
        let (path, value, _) = expect_invalid(&google_font(s));
        assert_eq!(path, "typography.fonts[0].weight");
        assert_eq!(value, s);
    }
}

// ── validation: rejected keywords and numbers ───────────────────────

#[test]
fn validate_rejects_unknown_keywords() {
    for s in ["bolder", "lighter", "Bold", "semibold", "heavy"] {
        let (path, value, reason) = expect_invalid(&google_font(s));
        assert_eq!(path, "typography.fonts[0].weight", "{s}");
        assert_eq!(value, s);
        assert!(
            reason.contains("bold") || reason.contains("keyword"),
            "reason should point at the keyword table: {reason}"
        );
    }
}

#[test]
fn validate_rejects_numbers_outside_100_900() {
    let (_, value, _) = expect_invalid(&google_font("50"));
    assert_eq!(value, "50");
    let (_, value, _) = expect_invalid(&google_font("1000"));
    assert_eq!(value, "1000");
    let (_, value, _) = expect_invalid(&google_font("0"));
    assert_eq!(value, "0");
}

#[test]
fn validate_reports_list_item_index() {
    let (path, value, _) = expect_invalid(&google_font("[400, semibold, 700]"));
    assert_eq!(path, "typography.fonts[0].weight[1]");
    assert_eq!(value, "semibold");
}

// ── validation: paths for other surfaces ────────────────────────────

#[test]
fn validate_reports_later_font_index() {
    let (path, value, _) = expect_invalid(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: A\n\
         \x20     source: google\n\
         \x20   - family: B\n\
         \x20     source: bunny\n\
         \x20     weight: Bold\n",
    );
    assert_eq!(path, "typography.fonts[1].weight");
    assert_eq!(value, "Bold");
}

#[test]
fn validate_reports_file_entry_path() {
    let (path, value, _) = expect_invalid(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: D\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - d-plain.woff2\n\
         \x20       - path: d-var.woff2\n\
         \x20         weight: 800..300\n",
    );
    assert_eq!(path, "typography.fonts[0].files[1].weight");
    assert_eq!(value, "800..300");
}

#[test]
fn validate_rejects_range_on_typography_slot() {
    let (path, value, reason) = expect_invalid(
        "typography:\n\
         \x20 headings:\n\
         \x20   family: A\n\
         \x20   weight: 500..700\n",
    );
    assert_eq!(path, "typography.headings.weight");
    assert_eq!(value, "500..700");
    assert!(
        reason.contains("single"),
        "reason should say slots take a single weight: {reason}"
    );
}

#[test]
fn validate_rejects_unknown_keyword_on_every_slot() {
    for slot in [
        "base",
        "headings",
        "link",
        "monospace",
        "monospace-inline",
        "monospace-block",
    ] {
        let (path, value, _) = expect_invalid(&format!(
            "typography:\n\
             \x20 {slot}:\n\
             \x20   weight: heavy\n"
        ));
        assert_eq!(path, format!("typography.{slot}.weight"));
        assert_eq!(value, "heavy");
    }
}

#[test]
fn validate_reports_first_error_in_document_order() {
    // fonts[] come before the slots in the YAML path order we walk;
    // a single error is reported and it is the first one.
    let (path, ..) = expect_invalid(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: A\n\
         \x20     source: google\n\
         \x20     weight: Bold\n\
         \x20 base:\n\
         \x20   weight: heavy\n",
    );
    assert_eq!(path, "typography.fonts[0].weight");
}

#[test]
fn invalid_font_weight_error_display_names_path_and_value() {
    let b = parse(&google_font("Bold"));
    let err = b.validate().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("typography.fonts[0].weight"), "{msg}");
    assert!(msg.contains("Bold"), "{msg}");
}
