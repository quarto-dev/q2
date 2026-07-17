//! Language term files and resolution (localization).
//!
//! Quarto's own messages stay in English; *rendered documents* localize.
//! The terms that appear in rendered output (callout titles, crossref
//! titles/prefixes, the TOC title, title-block labels, …) are defined in
//! per-language YAML files under `resources/language/` (embedded into the
//! binary at compile time) and can be overridden by users through the
//! `language:` metadata key. Key names are compatible with Quarto 1
//! (`crossref-fig-title`, `callout-note-title`, `title-block-published`, …).
//!
//! Design: `claude-notes/plans/2026-07-17-localization-i18n-design.md`
//! (braid strand bd-llhlzd7p).

use std::collections::BTreeMap;

use include_dir::{Dir, include_dir};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_config::InterpretationContext;
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
use quarto_source_map::SourceInfo;
use yaml_rust2::Yaml;

/// Embedded copies of `resources/language/_language*.yml` (see the README in
/// that directory for provenance and the update procedure).
static LANGUAGE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../resources/language");

/// Name of the base (English-defaults) term file. This file is also the
/// authoritative catalog of known term keys.
pub const BASE_LANGUAGE_FILE: &str = "_language.yml";

/// Returns the contents of an embedded language file by name
/// (e.g. `_language-fr.yml`), or `None` if no such file ships.
pub fn embedded_language_file(name: &str) -> Option<&'static str> {
    LANGUAGE_DIR.get_file(name).and_then(|f| f.contents_utf8())
}

/// Names of all embedded `_language*.yml` files, sorted.
pub fn embedded_language_file_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = LANGUAGE_DIR
        .files()
        .filter_map(|f| f.path().file_name().and_then(|n| n.to_str()))
        .filter(|n| n.starts_with("_language") && n.ends_with(".yml"))
        .collect();
    names.sort_unstable();
    names
}

/// One resolved term: the display string plus where it was defined
/// (an embedded file, a project/user YAML file, or inline metadata).
#[derive(Debug, Clone, PartialEq)]
pub struct TermEntry {
    /// The display string, e.g. `"Table des matières"`.
    pub value: String,
    /// Source location of the value for diagnostics.
    pub source: SourceInfo,
}

/// A flat `term-key → value` layer parsed from one term file.
///
/// Layers stack during resolution: English base, then each BCP 47 subtag
/// variant, then project-root files, then user `language:` overrides.
#[derive(Debug, Clone, Default)]
pub struct TermLayer {
    /// Term entries in key-sorted order (deterministic iteration).
    pub terms: BTreeMap<String, TermEntry>,
}

/// Errors from parsing a term file.
#[derive(Debug, thiserror::Error)]
pub enum TermFileError {
    #[error("{filename}: failed to parse YAML: {message}")]
    Yaml { filename: String, message: String },
    #[error("{filename}: expected a top-level map of term keys to strings")]
    NotAMap { filename: String },
    #[error("{filename}: value for key {key:?} is not a plain string")]
    NonStringValue { filename: String, key: String },
}

/// Parses a term file's content into a flat [`TermLayer`].
///
/// Term files are read with [`InterpretationContext::ProjectConfig`]
/// semantics: bare strings stay literal strings (they are display text, not
/// markdown). Every value must be a plain scalar string; maps, arrays, and
/// non-string scalars are rejected. (Per-language *subkey maps*, which are
/// legal in user `language:` metadata, are handled at resolution time — a
/// term *file* is always flat.)
pub fn parse_term_file(content: &str, filename: &str) -> Result<TermLayer, TermFileError> {
    let yaml = quarto_yaml::parse_file(content, filename).map_err(|e| TermFileError::Yaml {
        filename: filename.to_string(),
        message: e.to_string(),
    })?;
    let mut diagnostics = DiagnosticCollector::new();
    let config = pampa::pandoc::yaml_to_config_value(
        yaml,
        InterpretationContext::ProjectConfig,
        &mut diagnostics,
    );
    term_layer_from_config(&config, filename)
}

/// A structured term layer: plain term keys plus per-language sublayers.
///
/// This is the shape of user-facing `language:` config — a map whose scalar
/// entries are unconditional term overrides and whose map entries are keyed
/// by a BCP 47 tag and apply only when the document's `lang` matches:
///
/// ```yaml
/// language:
///   title-block-published: "Updated"    # applies unconditionally
///   fr:
///     title-block-published: "Mis à jour"   # applies when lang is fr / fr-*
/// ```
#[derive(Debug, Clone, Default)]
pub struct StructuredTermLayer {
    /// Unconditional term overrides.
    pub terms: BTreeMap<String, TermEntry>,
    /// Per-language sublayers, keyed by BCP 47 tag (applied in subtag-walk
    /// order, so `fr` applies before `fr-CA` when `lang: fr-CA`).
    pub sublayers: BTreeMap<String, TermLayer>,
}

/// The resolved term table for one document render.
///
/// Produced by [`resolve_language`]; consumed by AST transforms (via the
/// accessors here) and by templates (via the `quarto.language` metadata
/// subtree, see the `LanguageResolveStage`).
#[derive(Debug, Clone)]
pub struct LanguageTerms {
    lang: String,
    terms: BTreeMap<String, TermEntry>,
}

impl LanguageTerms {
    /// The BCP 47 tag the table was resolved for (default `"en"`).
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// The display string for a term key, if defined.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.terms.get(key).map(|t| t.value.as_str())
    }

    /// The full entry (value + source) for a term key, if defined.
    pub fn entry(&self, key: &str) -> Option<&TermEntry> {
        self.terms.get(key)
    }

    /// The crossref display title for a reference type (`fig`, `tbl`, …):
    /// the value of `crossref-<type>-title`.
    pub fn crossref_title(&self, ref_type: &str) -> Option<&str> {
        self.get(&format!("crossref-{ref_type}-title"))
    }

    /// The crossref reference prefix for a reference type: the value of
    /// `crossref-<type>-prefix`, falling back to `crossref-<type>-title`
    /// when no explicit prefix is defined (Quarto 1 semantics).
    pub fn crossref_prefix(&self, ref_type: &str) -> Option<&str> {
        self.get(&format!("crossref-{ref_type}-prefix"))
            .or_else(|| self.crossref_title(ref_type))
    }

    /// Iterate over all resolved terms in key-sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TermEntry)> {
        self.terms.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// The set of known term keys: the keys of the embedded base catalog
/// (`_language.yml`).
fn catalog_keys() -> &'static std::collections::BTreeSet<String> {
    static CATALOG: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let content = embedded_language_file(BASE_LANGUAGE_FILE)
            .expect("base language file is embedded at compile time");
        parse_term_file(content, BASE_LANGUAGE_FILE)
            .expect("embedded base language file parses (integrity-tested)")
            .terms
            .into_keys()
            .collect()
    })
}

/// Whether `key` is a legal term key: in the base catalog, or matching the
/// `crossref-<type>-title` / `crossref-<type>-prefix` patterns (crossref
/// types are user-extensible, so those keys are accepted for any type).
pub fn is_known_term_key(key: &str) -> bool {
    catalog_keys().contains(key)
        || (key.starts_with("crossref-") && (key.ends_with("-title") || key.ends_with("-prefix")))
}

/// The BCP 47 subtag prefix chain for a tag, most general first:
/// `"pt-BR"` → `["pt", "pt-BR"]`.
fn subtag_prefixes(lang: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    let mut current = String::new();
    for subtag in lang.split('-') {
        if !current.is_empty() {
            current.push('-');
        }
        current.push_str(subtag);
        prefixes.push(current.clone());
    }
    prefixes
}

/// Resolves the term table for `lang`.
///
/// Layer order (lowest to highest precedence):
///
/// 1. the embedded base catalog (`_language.yml`, English defaults);
/// 2. the embedded `_language-<prefix>.yml` files along the subtag walk of
///    `lang`, most general first (`pt` before `pt-BR`; missing intermediate
///    files are skipped — upstream ships `sr-Latn` with no `sr`);
/// 3. each layer in `extra_layers` in slice order (callers pass the
///    project-root `_language.yml` layer before the user `language:`
///    metadata layer). Within one layer, plain keys apply first, then
///    matching per-language sublayers in subtag-walk order — so a
///    lang-restricted subkey always beats a plain key from the same source.
///
/// Unknown-key warnings are emitted when *layers are built* (see
/// [`structured_layer_from_config`]), not here: the resolved table keeps
/// unknown keys so custom templates can reference user-defined terms.
pub fn resolve_language(lang: &str, extra_layers: &[StructuredTermLayer]) -> LanguageTerms {
    let mut terms: BTreeMap<String, TermEntry> = BTreeMap::new();

    // 1. Embedded base catalog.
    if let Some(content) = embedded_language_file(BASE_LANGUAGE_FILE)
        && let Ok(layer) = parse_term_file(content, BASE_LANGUAGE_FILE) {
            terms.extend(layer.terms);
        }

    // 2. Embedded per-language files along the subtag walk.
    for prefix in subtag_prefixes(lang) {
        let filename = format!("_language-{prefix}.yml");
        if let Some(content) = embedded_language_file(&filename) {
            // Embedded files are integrity-tested; a parse failure here is a
            // build defect, not a user error.
            let layer = parse_term_file(content, &filename)
                .expect("embedded language file parses (integrity-tested)");
            terms.extend(layer.terms);
        }
    }

    // 3. Extra layers: plain keys, then lang-matching sublayers.
    for layer in extra_layers {
        for (key, entry) in &layer.terms {
            terms.insert(key.clone(), entry.clone());
        }
        for prefix in subtag_prefixes(lang) {
            if let Some(sublayer) = layer.sublayers.get(&prefix) {
                for (key, entry) in &sublayer.terms {
                    terms.insert(key.clone(), entry.clone());
                }
            }
        }
    }

    LanguageTerms {
        lang: lang.to_string(),
        terms,
    }
}

/// Builds a [`StructuredTermLayer`] from user-facing `language:` config
/// (inline metadata or a custom YAML file parsed to a `ConfigValue`).
///
/// - Scalar / inline values become unconditional term overrides. Values are
///   flattened to plain text (`as_plain_text`), so both literal strings
///   (project config) and markdown-interpreted strings (document metadata)
///   work.
/// - Map values become per-language sublayers keyed by the entry key
///   (a BCP 47 tag such as `en`, `fr`, `fr-CA`).
/// - Term keys that are neither in the catalog nor `crossref-*-title/-prefix`
///   shaped produce a **warning** diagnostic (pointing at the key), but are
///   kept: custom templates may reference user-defined terms.
/// - Values that cannot be read as text produce a warning and are skipped.
pub fn structured_layer_from_config(
    config: &ConfigValue,
    diagnostics: &mut DiagnosticCollector,
) -> StructuredTermLayer {
    let mut layer = StructuredTermLayer::default();
    let ConfigValueKind::Map(entries) = &config.value else {
        diagnostics.add(
            quarto_error_reporting::DiagnosticMessageBuilder::warning(
                "`language` must be a map of term keys to strings (or a path to a YAML file)",
            )
            .with_location(config.source_info.clone())
            .build(),
        );
        return layer;
    };
    for entry in entries {
        if let ConfigValueKind::Map(sub_entries) = &entry.value.value {
            // Per-language sublayer: every value inside is a term.
            let mut sublayer = TermLayer::default();
            for sub_entry in sub_entries {
                if let Some(term) =
                    term_entry_from_config(&sub_entry.value, &sub_entry.key, diagnostics)
                {
                    warn_if_unknown_key(&sub_entry.key, &sub_entry.key_source, diagnostics);
                    sublayer.terms.insert(sub_entry.key.clone(), term);
                }
            }
            layer.sublayers.insert(entry.key.clone(), sublayer);
        } else if let Some(term) = term_entry_from_config(&entry.value, &entry.key, diagnostics) {
            warn_if_unknown_key(&entry.key, &entry.key_source, diagnostics);
            layer.terms.insert(entry.key.clone(), term);
        }
    }
    layer
}

/// Reads one term value from config; warns and returns `None` when the value
/// is not usable as display text.
fn term_entry_from_config(
    value: &ConfigValue,
    key: &str,
    diagnostics: &mut DiagnosticCollector,
) -> Option<TermEntry> {
    // quarto-yaml parses `key: ""` as Null (bd-gutochbq); a Null term is an
    // empty display string either way.
    let text = if matches!(value.value, ConfigValueKind::Scalar(Yaml::Null)) {
        Some(String::new())
    } else {
        value.as_plain_text()
    };
    match text {
        Some(text) => Some(TermEntry {
            value: text,
            source: value.source_info.clone(),
        }),
        None => {
            diagnostics.add(
                quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                    "language term `{key}` must be a string; ignoring this entry"
                ))
                .with_location(value.source_info.clone())
                .build(),
            );
            None
        }
    }
}

/// Warns when a term key is neither in the catalog nor crossref-shaped.
fn warn_if_unknown_key(key: &str, key_source: &SourceInfo, diagnostics: &mut DiagnosticCollector) {
    if !is_known_term_key(key) {
        diagnostics.add(
            quarto_error_reporting::DiagnosticMessageBuilder::warning(format!(
                "unknown language term key `{key}`"
            ))
            .add_note(
                "the key is kept and can be referenced from templates as \
                 `$quarto.language.<key>$`, but no built-in output uses it",
            )
            .add_hint("Is the key name misspelled? See _language.yml for the known keys?")
            .with_location(key_source.clone())
            .build(),
        );
    }
}

/// Parses user-facing language YAML content (a custom translation file, e.g.
/// `language: custom.yml`) into a [`StructuredTermLayer`].
///
/// Accepts both documented Quarto 1 forms: a flat term map, and a map of
/// per-language subkeys. Non-string term values are an error.
pub fn parse_language_file(
    content: &str,
    filename: &str,
    diagnostics: &mut DiagnosticCollector,
) -> Result<StructuredTermLayer, TermFileError> {
    let yaml = quarto_yaml::parse_file(content, filename).map_err(|e| TermFileError::Yaml {
        filename: filename.to_string(),
        message: e.to_string(),
    })?;
    let mut parse_diags = DiagnosticCollector::new();
    let config = pampa::pandoc::yaml_to_config_value(
        yaml,
        InterpretationContext::ProjectConfig,
        &mut parse_diags,
    );
    let ConfigValueKind::Map(entries) = &config.value else {
        return Err(TermFileError::NotAMap {
            filename: filename.to_string(),
        });
    };
    // Strict pass: unlike inline metadata (lenient, warning-based), a file
    // with non-string term values is malformed.
    for entry in entries {
        match &entry.value.value {
            ConfigValueKind::Map(sub_entries) => {
                for sub_entry in sub_entries {
                    if !is_stringish(&sub_entry.value) {
                        return Err(TermFileError::NonStringValue {
                            filename: filename.to_string(),
                            key: format!("{}.{}", entry.key, sub_entry.key),
                        });
                    }
                }
            }
            v if is_stringish_kind(v) => {}
            _ => {
                return Err(TermFileError::NonStringValue {
                    filename: filename.to_string(),
                    key: entry.key.clone(),
                });
            }
        }
    }
    Ok(structured_layer_from_config(&config, diagnostics))
}

fn is_stringish(value: &ConfigValue) -> bool {
    is_stringish_kind(&value.value)
}

fn is_stringish_kind(kind: &ConfigValueKind) -> bool {
    matches!(
        kind,
        ConfigValueKind::Scalar(Yaml::String(_) | Yaml::Null) |
ConfigValueKind::PandocInlines(_)
    )
}

/// Extracts a flat [`TermLayer`] from a parsed `ConfigValue` map.
fn term_layer_from_config(
    config: &ConfigValue,
    filename: &str,
) -> Result<TermLayer, TermFileError> {
    let ConfigValueKind::Map(entries) = &config.value else {
        return Err(TermFileError::NotAMap {
            filename: filename.to_string(),
        });
    };
    let mut layer = TermLayer::default();
    for entry in entries {
        // quarto-yaml parses `key: ""` as Null (bd-gutochbq); a Null term is
        // an empty display string either way.
        let value = if matches!(entry.value.value, ConfigValueKind::Scalar(Yaml::Null)) {
            ""
        } else {
            match entry.value.as_str() {
                Some(v) => v,
                None => {
                    return Err(TermFileError::NonStringValue {
                        filename: filename.to_string(),
                        key: entry.key.clone(),
                    });
                }
            }
        };
        layer.terms.insert(
            entry.key.clone(),
            TermEntry {
                value: value.to_string(),
                source: entry.value.source_info.clone(),
            },
        );
    }
    Ok(layer)
}
