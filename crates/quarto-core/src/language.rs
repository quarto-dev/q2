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
