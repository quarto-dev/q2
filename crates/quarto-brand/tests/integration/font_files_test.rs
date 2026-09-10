//! Tests for `source: file` font entries (bd-ve916wr8): the published
//! name a local font file gets in the output, and tolerance of keys
//! the brand.yml spec has not settled (`format:`, `display:`).

use quarto_brand::{Brand, BrandFont, BrandFontFileEntry, published_font_name};

fn brand(yaml: &str) -> Brand {
    quarto_brand::UnifiedBrand::from_yaml_str(yaml)
        .expect("parse")
        .split()
        .light
}

fn file_entries(b: &Brand) -> &[BrandFontFileEntry] {
    match b.fonts().first().expect("one font") {
        BrandFont::File(f) => &f.files,
        other => panic!("expected a file font, got {other:?}"),
    }
}

/// Local font files are published beside the theme CSS as
/// `fonts/<basename>`, whatever directory they were authored in — a
/// brand-relative path, a `/`-rooted (project-root) path, or a `../`
/// climb all collapse to the basename.
#[test]
fn published_font_name_is_the_basename_of_any_local_path() {
    assert_eq!(
        published_font_name("assets/sub/Regular.woff2").as_deref(),
        Some("Regular.woff2")
    );
    assert_eq!(
        published_font_name("/assets/Rooted.woff").as_deref(),
        Some("Rooted.woff")
    );
    assert_eq!(
        published_font_name("../shared/Parent.ttf").as_deref(),
        Some("Parent.ttf")
    );
    assert_eq!(published_font_name("Bare.otf").as_deref(), Some("Bare.otf"));
    // Backslash-separated paths (authored on Windows) publish the
    // same way.
    assert_eq!(
        published_font_name("assets\\Win.woff2").as_deref(),
        Some("Win.woff2")
    );
}

/// External URLs are served by whoever hosts them: nothing to publish.
#[test]
fn published_font_name_is_none_for_external_urls() {
    assert_eq!(
        published_font_name("https://fonts.example.com/dir/Remote.woff2"),
        None
    );
    assert_eq!(published_font_name("//cdn.example.com/Remote.woff2"), None);
}

/// `format:` / `display:` on a file entry are not part of the settled
/// brand.yml spec. They must not fail the parse (decision 5: warn and
/// ignore), and the entry must expose them so the consumer can warn.
#[test]
fn explicit_file_entry_tolerates_unknown_keys_and_reports_them() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: file\n\
         \x20     family: F\n\
         \x20     files:\n\
         \x20       - path: Regular.woff2\n\
         \x20         weight: 400\n\
         \x20         format: woff2\n\
         \x20         display: swap\n",
    );
    let entries = file_entries(&b);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), "Regular.woff2");
    assert_eq!(
        entries[0].unknown_keys(),
        vec!["format", "display"],
        "unknown keys, in authored order"
    );
}

#[test]
fn file_entries_without_unknown_keys_report_none() {
    let b = brand(
        "typography:\n\
         \x20 fonts:\n\
         \x20   - source: file\n\
         \x20     family: F\n\
         \x20     files:\n\
         \x20       - Bare.woff2\n\
         \x20       - path: Explicit.woff2\n\
         \x20         style: italic\n",
    );
    let entries = file_entries(&b);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].unknown_keys().is_empty());
    assert!(entries[1].unknown_keys().is_empty());
}
