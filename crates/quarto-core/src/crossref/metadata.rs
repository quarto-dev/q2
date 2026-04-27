/*
 * crossref/metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Extract crossref-relevant information from merged document metadata.
 */

//! Pull crossref configuration out of merged document metadata.
//!
//! Two things live under the top-level `crossref` key that the pre-engine
//! sugaring stage cares about:
//!
//! 1. `crossref.custom` — a list of user-defined categories. We map this
//!    verbatim to the Q1 schema (see `crossref/custom.lua` in
//!    `external-sources/quarto-cli`): `{ key: <prefix>, reference-prefix:
//!    <display-name>, ... }`. Fields other than `key` and `reference-prefix`
//!    are ignored here — they are consumed by back-end renderers later.
//!
//! 2. `crossref.ids` — a list of identifier strings promised by `output:
//!    asis` cells (see design plan D6). Each entry is matched against the
//!    already-built [`RefTypeRegistry`] to determine its ref-type; unknown
//!    prefixes are recorded as [`RefTypeSource::Promised`] so the resolver
//!    can still classify `@<prefix>-..` citations even if the category
//!    wasn't declared (the indexer will still emit a diagnostic when it
//!    sees the realized id without a declaration).
//!
//! Errors collected here are non-fatal: a malformed `crossref.custom` entry
//! produces a diagnostic and is skipped, rather than failing the whole
//! render. This mirrors Q1's lenient behavior on metadata-shape issues while
//! keeping the resulting registry/promised-id list usable downstream.

use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;

use super::index::{PromisedId, PromisedIdSource};
use super::registry::{RefTypeRegistry, RegistryError};

/// Result of reading the `crossref.*` section of document metadata.
#[derive(Debug, Default)]
pub struct CrossrefMetadata {
    /// Promised ids from `crossref.ids`.
    pub promised_ids: Vec<PromisedId>,

    /// Non-fatal problems encountered while extracting. Each is something a
    /// caller should surface as a diagnostic.
    pub errors: Vec<MetadataError>,
}

/// Non-fatal problems encountered when reading crossref metadata.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MetadataError {
    /// `crossref.custom` had a scalar shape where a list was expected.
    #[error("`crossref.custom` must be a list of entries; got a different shape")]
    CustomNotAList { source_info: SourceInfo },

    /// A `crossref.custom` entry was not a map.
    #[error("`crossref.custom` entry must be a map")]
    CustomEntryNotMap { source_info: SourceInfo },

    /// A `crossref.custom` entry was missing `key` or `reference-prefix`.
    #[error(
        "`crossref.custom` entry missing required `{missing}` (both `key` and `reference-prefix` are required)"
    )]
    CustomEntryMissingField {
        missing: &'static str,
        source_info: SourceInfo,
    },

    /// A `crossref.custom` entry clashed with an existing registry entry.
    #[error(transparent)]
    RegistryClash(#[from] RegistryError),

    /// `crossref.ids` had an unexpected shape.
    #[error("`crossref.ids` must be a list of identifier strings")]
    IdsNotAList { source_info: SourceInfo },

    /// A `crossref.ids` entry was not a string.
    #[error("`crossref.ids` entry must be a string identifier")]
    IdEntryNotString { source_info: SourceInfo },

    /// A `crossref.ids` entry didn't look like `<prefix>-<rest>`.
    #[error("`crossref.ids` entry `{id}` is not of the form `<prefix>-<rest>`")]
    IdEntryMalformed { id: String, source_info: SourceInfo },
}

/// Extend `registry` from `meta.crossref.custom`, and return a
/// [`CrossrefMetadata`] containing promised ids (from `meta.crossref.ids`)
/// plus any non-fatal errors.
///
/// The caller is expected to convert the errors into diagnostics. `registry`
/// is mutated in place for `crossref.custom`; promised-id prefixes are *not*
/// registered here so the caller can decide the order (Phase 0 contract:
/// metadata extension first, then promised-id extension). Use
/// [`RefTypeRegistry::extend_from_promised`] on the returned
/// `promised_ids` as the second step.
pub fn read(meta: &ConfigValue, registry: &mut RefTypeRegistry) -> CrossrefMetadata {
    let Some(crossref) = meta.get("crossref") else {
        return CrossrefMetadata::default();
    };

    let mut out = CrossrefMetadata::default();

    if let Some(custom) = crossref.get("custom") {
        read_custom(custom, registry, &mut out);
    }

    if let Some(ids) = crossref.get("ids") {
        read_ids(ids, registry, &mut out);
    }

    out
}

fn read_custom(custom: &ConfigValue, registry: &mut RefTypeRegistry, out: &mut CrossrefMetadata) {
    let Some(entries) = custom.as_array() else {
        out.errors.push(MetadataError::CustomNotAList {
            source_info: custom.source_info.clone(),
        });
        return;
    };

    for entry in entries {
        if entry.as_map_entries().is_none() {
            out.errors.push(MetadataError::CustomEntryNotMap {
                source_info: entry.source_info.clone(),
            });
            continue;
        }

        // `as_plain_text` so YAML values parsed as `PandocInlines` (when
        // the document is parsed through pampa) still read as the string
        // the user wrote. `as_str` handles only Scalar-String / Path /
        // Glob / Expr, which misses the common case.
        let key = entry.get("key").and_then(|v| v.as_plain_text());
        let ref_prefix = entry
            .get("reference-prefix")
            .and_then(|v| v.as_plain_text());

        let ref_type = match key {
            Some(s) => s,
            None => {
                out.errors.push(MetadataError::CustomEntryMissingField {
                    missing: "key",
                    source_info: entry.source_info.clone(),
                });
                continue;
            }
        };
        let kind = match ref_prefix {
            Some(s) => s,
            None => {
                out.errors.push(MetadataError::CustomEntryMissingField {
                    missing: "reference-prefix",
                    source_info: entry.source_info.clone(),
                });
                continue;
            }
        };

        let src = entry.source_info.clone();
        if let Err(e) = registry.register_custom(ref_type, kind, Some(src)) {
            out.errors.push(MetadataError::RegistryClash(e));
        }
    }
}

fn read_ids(ids: &ConfigValue, registry: &RefTypeRegistry, out: &mut CrossrefMetadata) {
    let Some(entries) = ids.as_array() else {
        out.errors.push(MetadataError::IdsNotAList {
            source_info: ids.source_info.clone(),
        });
        return;
    };

    for entry in entries {
        let Some(id) = entry.as_plain_text() else {
            out.errors.push(MetadataError::IdEntryNotString {
                source_info: entry.source_info.clone(),
            });
            continue;
        };

        let Some((prefix, _)) = id.split_once('-') else {
            out.errors.push(MetadataError::IdEntryMalformed {
                id: id.clone(),
                source_info: entry.source_info.clone(),
            });
            continue;
        };

        let prefix = prefix.to_string();
        let _ = registry; // retained for symmetry; currently unused.
        out.promised_ids.push(PromisedId {
            identifier: id,
            ref_type: prefix,
            source_info: entry.source_info.clone(),
            source: PromisedIdSource::DocumentMetadata,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};
    use quarto_source_map::{FileId, SourceInfo};
    use yaml_rust2::Yaml;

    fn si() -> SourceInfo {
        SourceInfo::original(FileId(0), 0, 0)
    }

    fn scalar(s: &str) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Scalar(Yaml::String(s.to_string())),
            source_info: si(),
            merge_op: MergeOp::default(),
        }
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Map(
                entries
                    .into_iter()
                    .map(|(k, v)| ConfigMapEntry {
                        key: k.to_string(),
                        key_source: si(),
                        value: v,
                    })
                    .collect(),
            ),
            source_info: si(),
            merge_op: MergeOp::default(),
        }
    }

    fn array(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue {
            value: ConfigValueKind::Array(items),
            source_info: si(),
            merge_op: MergeOp::default(),
        }
    }

    fn meta_with_crossref(crossref: ConfigValue) -> ConfigValue {
        map(vec![("crossref", crossref)])
    }

    #[test]
    fn empty_metadata_yields_empty_result() {
        let mut reg = RefTypeRegistry::builtin();
        let meta = map(vec![]);
        let out = read(&meta, &mut reg);
        assert!(out.promised_ids.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn no_crossref_key_yields_empty_result() {
        let mut reg = RefTypeRegistry::builtin();
        let meta = map(vec![("title", scalar("Hello"))]);
        let out = read(&meta, &mut reg);
        assert!(out.promised_ids.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn custom_entries_extend_registry() {
        let mut reg = RefTypeRegistry::builtin();
        let custom_entry = map(vec![
            ("key", scalar("dia")),
            ("reference-prefix", scalar("Diagram")),
        ]);
        let meta = meta_with_crossref(map(vec![("custom", array(vec![custom_entry]))]));

        let out = read(&meta, &mut reg);
        assert!(out.errors.is_empty(), "unexpected errors: {:?}", out.errors);
        let def = reg.classify_cite_id("dia-one").expect("dia registered");
        assert_eq!(def.kind, "Diagram");
    }

    #[test]
    fn custom_missing_key_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let custom_entry = map(vec![("reference-prefix", scalar("NoKey"))]);
        let meta = meta_with_crossref(map(vec![("custom", array(vec![custom_entry]))]));

        let out = read(&meta, &mut reg);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(
            out.errors[0],
            MetadataError::CustomEntryMissingField { missing: "key", .. }
        ));
    }

    #[test]
    fn custom_missing_reference_prefix_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let custom_entry = map(vec![("key", scalar("nope"))]);
        let meta = meta_with_crossref(map(vec![("custom", array(vec![custom_entry]))]));
        let out = read(&meta, &mut reg);
        assert!(matches!(
            out.errors[0],
            MetadataError::CustomEntryMissingField {
                missing: "reference-prefix",
                ..
            }
        ));
        // And the registry wasn't polluted.
        assert!(!reg.contains("nope"));
    }

    #[test]
    fn custom_clashing_with_builtin_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let custom_entry = map(vec![
            ("key", scalar("fig")),
            ("reference-prefix", scalar("Shadow")),
        ]);
        let meta = meta_with_crossref(map(vec![("custom", array(vec![custom_entry]))]));
        let out = read(&meta, &mut reg);
        assert!(matches!(out.errors[0], MetadataError::RegistryClash(_)));
        // built-in preserved
        assert_eq!(reg.classify_cite_id("fig-x").unwrap().kind, "Figure");
    }

    #[test]
    fn custom_not_a_list_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        // custom is a scalar — not a list.
        let meta = meta_with_crossref(map(vec![("custom", scalar("oops"))]));
        let out = read(&meta, &mut reg);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(
            out.errors[0],
            MetadataError::CustomNotAList { .. }
        ));
    }

    #[test]
    fn custom_entry_not_a_map_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let meta = meta_with_crossref(map(vec![("custom", array(vec![scalar("just a string")]))]));
        let out = read(&meta, &mut reg);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(
            out.errors[0],
            MetadataError::CustomEntryNotMap { .. }
        ));
    }

    #[test]
    fn ids_entries_are_lifted_to_promised() {
        let mut reg = RefTypeRegistry::builtin();
        let ids = array(vec![scalar("tbl-dynamic"), scalar("fig-later")]);
        let meta = meta_with_crossref(map(vec![("ids", ids)]));
        let out = read(&meta, &mut reg);
        assert!(out.errors.is_empty());
        assert_eq!(out.promised_ids.len(), 2);
        assert_eq!(out.promised_ids[0].identifier, "tbl-dynamic");
        assert_eq!(out.promised_ids[0].ref_type, "tbl");
        assert_eq!(out.promised_ids[1].ref_type, "fig");
        assert!(matches!(
            out.promised_ids[0].source,
            PromisedIdSource::DocumentMetadata
        ));
    }

    #[test]
    fn ids_malformed_entry_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let ids = array(vec![scalar("no-dashes-works"), scalar("nodashes")]);
        let meta = meta_with_crossref(map(vec![("ids", ids)]));
        let out = read(&meta, &mut reg);
        // "no-dashes-works" has a dash so it's accepted with ref_type="no"
        // "nodashes" has no dash so it's malformed.
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(
            out.errors[0],
            MetadataError::IdEntryMalformed { .. }
        ));
        assert_eq!(out.promised_ids.len(), 1);
        assert_eq!(out.promised_ids[0].identifier, "no-dashes-works");
    }

    #[test]
    fn ids_entry_not_string_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let ids = array(vec![map(vec![])]);
        let meta = meta_with_crossref(map(vec![("ids", ids)]));
        let out = read(&meta, &mut reg);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(
            out.errors[0],
            MetadataError::IdEntryNotString { .. }
        ));
    }

    #[test]
    fn ids_not_a_list_emits_error() {
        let mut reg = RefTypeRegistry::builtin();
        let meta = meta_with_crossref(map(vec![("ids", scalar("oops"))]));
        let out = read(&meta, &mut reg);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(out.errors[0], MetadataError::IdsNotAList { .. }));
    }

    #[test]
    fn custom_and_ids_compose() {
        let mut reg = RefTypeRegistry::builtin();
        let custom = array(vec![map(vec![
            ("key", scalar("dia")),
            ("reference-prefix", scalar("Diagram")),
        ])]);
        let ids = array(vec![scalar("dia-one"), scalar("unknown-two")]);
        let meta = meta_with_crossref(map(vec![("custom", custom), ("ids", ids)]));
        let out = read(&meta, &mut reg);
        assert!(out.errors.is_empty());
        assert!(reg.contains("dia"));
        assert_eq!(out.promised_ids.len(), 2);
        assert_eq!(out.promised_ids[1].ref_type, "unknown");
        // Caller would now call reg.extend_from_promised(&out.promised_ids)
        // to register "unknown" as Promised. We only record intent here.
    }
}
