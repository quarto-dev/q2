//! Catalog integrity tests for the embedded language term files
//! (`resources/language/_language*.yml`).
//!
//! These guard the re-copy workflow documented in `resources/language/README.md`:
//! if an upstream update introduces malformed files, non-string values, or new
//! stray keys, these tests fail and prompt a review.
//!
//! Part of the localization epic bd-llhlzd7p
//! (`claude-notes/plans/2026-07-17-localization-i18n-design.md`).

use quarto_core::language::{
    BASE_LANGUAGE_FILE, embedded_language_file, embedded_language_file_names, parse_term_file,
};

/// Keys present in upstream per-language files but absent from the base
/// catalog. These are dead entries upstream (Quarto 1 silently filters
/// unknown keys); we keep our copies verbatim for provenance fidelity.
/// A new entry appearing here after a re-copy deserves a look — it is
/// either an upstream typo or a new key missing from `_language.yml`.
const KNOWN_UPSTREAM_STRAYS: &[(&str, &str)] = &[
    ("_language-lt.yml", "search"),
    ("_language-sv.yml", "callout-danger-title"),
];

fn is_crossref_pattern(key: &str) -> bool {
    key.starts_with("crossref-") && (key.ends_with("-title") || key.ends_with("-prefix"))
}

#[test]
fn embedded_catalog_has_expected_shape() {
    let names = embedded_language_file_names();
    assert!(
        names.contains(&BASE_LANGUAGE_FILE),
        "base {BASE_LANGUAGE_FILE} must be embedded"
    );
    // 34 per-language variants shipped from Quarto 1 at the time of the copy.
    let variants = names.iter().filter(|n| **n != BASE_LANGUAGE_FILE).count();
    assert_eq!(
        variants, 34,
        "expected exactly 34 per-language files, found {variants}: {names:?}"
    );
}

#[test]
fn base_catalog_contains_keys_consumed_by_transforms() {
    let base = parse_term_file(
        embedded_language_file(BASE_LANGUAGE_FILE).expect("base file embedded"),
        BASE_LANGUAGE_FILE,
    )
    .expect("base file parses");

    // Keys the v1 consumers depend on (plan section D4). Exact values are
    // asserted so a silent upstream rename/redefinition fails loudly.
    let expected = [
        ("callout-note-title", "Note"),
        ("callout-tip-title", "Tip"),
        ("callout-warning-title", "Warning"),
        ("callout-important-title", "Important"),
        ("callout-caution-title", "Caution"),
        ("crossref-fig-title", "Figure"),
        ("crossref-tbl-title", "Table"),
        ("crossref-lst-title", "Listing"),
        ("crossref-eq-prefix", "Equation"),
        ("crossref-sec-prefix", "Section"),
        ("crossref-thm-title", "Theorem"),
        ("crossref-lem-title", "Lemma"),
        ("crossref-cor-title", "Corollary"),
        ("crossref-prp-title", "Proposition"),
        ("crossref-cnj-title", "Conjecture"),
        ("crossref-def-title", "Definition"),
        ("crossref-exm-title", "Example"),
        ("crossref-exr-title", "Exercise"),
        ("environment-proof-title", "Proof"),
        ("environment-solution-title", "Solution"),
        ("environment-remark-title", "Remark"),
        ("toc-title-document", "Table of contents"),
        ("section-title-abstract", "Abstract"),
        ("title-block-author-single", "Author"),
        ("title-block-author-plural", "Authors"),
        ("title-block-published", "Published"),
    ];
    for (key, value) in expected {
        assert_eq!(
            base.terms.get(key).map(|t| t.value.as_str()),
            Some(value),
            "base catalog key {key:?}"
        );
    }
}

#[test]
fn every_embedded_file_is_a_flat_string_map_with_known_keys() {
    let base = parse_term_file(
        embedded_language_file(BASE_LANGUAGE_FILE).expect("base file embedded"),
        BASE_LANGUAGE_FILE,
    )
    .expect("base file parses");

    for name in embedded_language_file_names() {
        let content = embedded_language_file(name).expect("listed file is readable");
        let layer = parse_term_file(content, name)
            .unwrap_or_else(|e| panic!("{name} failed to parse as a term file: {e}"));
        assert!(
            !layer.terms.is_empty(),
            "{name} parsed to an empty term map"
        );
        if name == BASE_LANGUAGE_FILE {
            continue;
        }
        for key in layer.terms.keys() {
            let known = base.terms.contains_key(key.as_str())
                || is_crossref_pattern(key)
                || KNOWN_UPSTREAM_STRAYS.contains(&(name, key.as_str()));
            assert!(
                known,
                "{name}: key {key:?} is not in the base catalog, does not match \
                 crossref-*-title/-prefix, and is not a documented upstream stray"
            );
        }
    }
}

// Note: variant files do NOT always have a parent file — upstream ships
// `_language-sr-Latn.yml` with no `_language-sr.yml`. The subtag walk must
// tolerate missing intermediate layers; that behavior is unit-tested with the
// resolution engine (phase 2 of the plan).
