//! Unit tests for language term resolution (localization).
//!
//! Covers the merge semantics defined in
//! `claude-notes/plans/2026-07-17-localization-i18n-design.md` (bd-llhlzd7p):
//! shipped-file subtag walk, user `language:` overrides (plain keys,
//! per-language subkeys, custom files), crossref prefix→title fallback, and
//! unknown-key warnings.

use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_core::language::{
    StructuredTermLayer, parse_language_file, resolve_language, structured_layer_from_config,
};
use quarto_error_reporting::DiagnosticKind;

/// Parses YAML as document front matter (strings become PandocInlines, the
/// same shape a real `language:` block has after metadata merging) and
/// converts it to a structured term layer.
fn user_layer(yaml: &str) -> (StructuredTermLayer, DiagnosticCollector) {
    let parsed = quarto_yaml::parse(yaml).expect("test yaml parses");
    let mut diagnostics = DiagnosticCollector::new();
    let config = pampa::pandoc::yaml_to_config_value(
        parsed,
        quarto_config::InterpretationContext::DocumentMetadata,
        &mut diagnostics,
    );
    assert!(
        diagnostics.diagnostics().is_empty(),
        "yaml conversion should not diagnose: {:?}",
        diagnostics.diagnostics()
    );
    let mut layer_diags = DiagnosticCollector::new();
    let layer = structured_layer_from_config(&config, &mut layer_diags);
    (layer, layer_diags)
}

fn resolve_no_extras(lang: &str) -> quarto_core::language::LanguageTerms {
    resolve_language(lang, &[])
}

// ── Shipped-file resolution ────────────────────────────────────────────────

#[test]
fn english_base_resolves() {
    let terms = resolve_no_extras("en");
    assert_eq!(terms.lang(), "en");
    assert_eq!(terms.get("crossref-fig-title"), Some("Figure"));
    assert_eq!(terms.get("toc-title-document"), Some("Table of contents"));
    // Empty-string term values are legal (and distinct from absent).
    assert_eq!(terms.get("search-text-placeholder"), Some(""));
    assert_eq!(terms.get("no-such-term"), None);
}

#[test]
fn plain_language_tag_resolves_shipped_translation() {
    let terms = resolve_no_extras("es");
    assert_eq!(terms.get("callout-note-title"), Some("Nota"));
    assert_eq!(terms.get("crossref-fig-title"), Some("Figura"));
    assert_eq!(terms.get("toc-title-document"), Some("Tabla de contenidos"));
    assert_eq!(terms.get("environment-proof-title"), Some("Demostración"));
}

#[test]
fn subtag_walk_merges_most_general_first() {
    let terms = resolve_no_extras("pt-BR");
    // Overridden in _language-pt-BR.yml:
    assert_eq!(terms.get("code-links-title"), Some("Links de código"));
    // Only in _language-pt.yml (inherited through the walk):
    assert_eq!(
        terms.get("title-block-modified"),
        Some("Data de Modificação")
    );
    // Plain pt keeps its own value when asked for directly:
    let pt = resolve_no_extras("pt");
    assert_eq!(pt.get("code-links-title"), Some("Ligações de código"));
}

#[test]
fn region_and_script_variants_resolve() {
    let de_ch = resolve_no_extras("de-CH");
    assert_eq!(de_ch.get("section-title-footnotes"), Some("Fussnoten"));
    // Inherited from _language-de.yml:
    assert_eq!(de_ch.get("crossref-fig-title"), Some("Abbildung"));

    let zh_tw = resolve_no_extras("zh-TW");
    assert_eq!(zh_tw.get("callout-note-title"), Some("註釋"));
}

#[test]
fn missing_intermediate_layer_is_tolerated() {
    // Upstream ships _language-sr-Latn.yml with no _language-sr.yml.
    let terms = resolve_no_extras("sr-Latn");
    assert_eq!(terms.get("callout-note-title"), Some("Beleška"));
}

#[test]
fn unknown_region_falls_back_to_parent_language() {
    let terms = resolve_no_extras("es-MX");
    assert_eq!(terms.get("callout-note-title"), Some("Nota"));
}

#[test]
fn unknown_language_falls_back_to_english() {
    let terms = resolve_no_extras("xx");
    assert_eq!(terms.get("callout-note-title"), Some("Note"));
    assert_eq!(terms.get("crossref-fig-title"), Some("Figure"));
}

// ── Crossref accessor fallbacks ────────────────────────────────────────────

#[test]
fn crossref_prefix_falls_back_to_title() {
    let en = resolve_no_extras("en");
    // fig has no crossref-fig-prefix in the catalog: falls back to the title.
    assert_eq!(en.crossref_prefix("fig"), Some("Figure"));
    // eq has an explicit prefix key.
    assert_eq!(en.crossref_prefix("eq"), Some("Equation"));
    assert_eq!(en.crossref_title("fig"), Some("Figure"));
    assert_eq!(en.crossref_title("nosuchtype"), None);
    assert_eq!(en.crossref_prefix("nosuchtype"), None);

    let es = resolve_no_extras("es");
    assert_eq!(es.crossref_prefix("fig"), Some("Figura"));
    assert_eq!(es.crossref_prefix("eq"), Some("Ecuación"));
}

// ── User overrides ─────────────────────────────────────────────────────────

#[test]
fn user_plain_key_overrides_shipped_translation() {
    let (layer, diags) = user_layer("toc-title-document: \"Sommaire\"\n");
    assert!(diags.diagnostics().is_empty());
    let terms = resolve_language("fr", &[layer]);
    assert_eq!(terms.get("toc-title-document"), Some("Sommaire"));
    // Untouched keys keep the shipped fr values.
    assert_eq!(
        terms.get("title-block-published"),
        Some("Date de publication")
    );
}

#[test]
fn user_subkeys_apply_only_for_matching_lang() {
    let yaml = "\
en:
  title-block-published: \"Updated\"
fr:
  title-block-published: \"Mis à jour!\"
";
    let (layer, _) = user_layer(yaml);
    let fr = resolve_language("fr", std::slice::from_ref(&layer));
    assert_eq!(fr.get("title-block-published"), Some("Mis à jour!"));

    let en = resolve_language("en", std::slice::from_ref(&layer));
    assert_eq!(en.get("title-block-published"), Some("Updated"));

    // fr-CA walks through fr, picking up the fr subkey.
    let fr_ca = resolve_language("fr-CA", &[layer]);
    assert_eq!(fr_ca.get("title-block-published"), Some("Mis à jour!"));
}

#[test]
fn user_subkey_beats_plain_key_within_a_layer() {
    let yaml = "\
title-block-published: \"Plain\"
fr:
  title-block-published: \"Subkey\"
";
    let (layer, _) = user_layer(yaml);
    let fr = resolve_language("fr", std::slice::from_ref(&layer));
    assert_eq!(fr.get("title-block-published"), Some("Subkey"));
    // For a non-matching lang the plain key still applies.
    let es = resolve_language("es", &[layer]);
    assert_eq!(es.get("title-block-published"), Some("Plain"));
}

#[test]
fn later_layers_override_earlier_layers() {
    let (project, _) = user_layer("toc-title-document: \"From project\"\n");
    let (doc, _) = user_layer("toc-title-document: \"From doc\"\n");
    let terms = resolve_language("en", &[project, doc]);
    assert_eq!(terms.get("toc-title-document"), Some("From doc"));
}

// ── Unknown-key warnings ───────────────────────────────────────────────────

#[test]
fn unknown_key_warns_but_is_preserved() {
    let (layer, diags) = user_layer("my-custom-term: \"Zap\"\n");
    let warnings: Vec<_> = diags
        .diagnostics()
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Warning)
        .collect();
    assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
    assert!(
        warnings[0].title.contains("my-custom-term"),
        "warning should name the key: {}",
        warnings[0].title
    );
    assert!(
        warnings[0].location.is_some(),
        "warning should carry a source location"
    );

    // The key is still resolvable (usable from custom templates).
    let terms = resolve_language("en", &[layer]);
    assert_eq!(terms.get("my-custom-term"), Some("Zap"));
}

#[test]
fn custom_crossref_pattern_keys_do_not_warn() {
    let (_, diags) =
        user_layer("crossref-robot-title: \"Robot\"\ncrossref-robot-prefix: \"Rbt.\"\n");
    assert!(
        diags.diagnostics().is_empty(),
        "crossref-*-title/-prefix are legal for custom types: {:?}",
        diags.diagnostics()
    );
}

#[test]
fn unknown_key_inside_subkey_warns_too() {
    let (_, diags) = user_layer("fr:\n  my-custom-term: \"Zap\"\n");
    let warnings: Vec<_> = diags
        .diagnostics()
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Warning)
        .collect();
    assert_eq!(warnings.len(), 1);
}

// ── Custom language files ──────────────────────────────────────────────────

#[test]
fn custom_file_flat_form() {
    // Q1 docs: a full-translation custom.yml is a flat term map.
    let content = "callout-note-title: \"Nota Bene\"\ntoc-title-document: \"Indice\"\n";
    let mut file_diags = DiagnosticCollector::new();
    let layer = parse_language_file(content, "custom.yml", &mut file_diags).expect("parses");
    let terms = resolve_language("en", &[layer]);
    assert_eq!(terms.get("callout-note-title"), Some("Nota Bene"));
    assert_eq!(terms.get("toc-title-document"), Some("Indice"));
    // Untouched keys fall through to the shipped base.
    assert_eq!(terms.get("callout-tip-title"), Some("Tip"));
}

#[test]
fn custom_file_per_language_form() {
    // Q1 docs example (custom-language.yml).
    let content = "\
en:
  title-block-published: \"Updated\"
fr:
  title-block-published: \"Mis à jour\"
";
    let mut file_diags = DiagnosticCollector::new();
    let layer =
        parse_language_file(content, "custom-language.yml", &mut file_diags).expect("parses");
    let fr = resolve_language("fr", &[layer]);
    assert_eq!(fr.get("title-block-published"), Some("Mis à jour"));
}

#[test]
fn custom_file_rejects_non_string_values() {
    let mut file_diags = DiagnosticCollector::new();
    let err = parse_language_file("callout-note-title: [1, 2]\n", "bad.yml", &mut file_diags);
    assert!(err.is_err(), "arrays are not legal term values");
}
