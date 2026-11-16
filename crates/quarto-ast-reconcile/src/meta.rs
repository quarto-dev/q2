/*
 * meta.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Three-way reconciliation of document metadata across an engine run.
 * bd-8svy3p89; plan: claude-notes/plans/2026-11-10-engine-metadata-round-trip.md.
 */

//! Three-way metadata reconciliation.
//!
//! An engine receives the document as qmd, and its output is read back.
//! Blocks survive that trip close to unchanged, so a two-way diff of the
//! original blocks against the output's finds what the engine did.
//! Metadata does not survive it: the front matter is re-read with
//! document rules, so a project string comes back as markdown, a marked
//! path loses its kind, and `_a_b` becomes emphasis, whether or not the
//! engine touched the value.
//!
//! So the output's metadata is compared not with the original but with a
//! parse of the exact text the engine received (the *baseline*). Baseline
//! and output went through the same writer and reader, so where they
//! agree the engine changed nothing and the original value is kept, with
//! its kind and spans; where they differ the engine's value is taken.

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue, ConfigValueKind};

use crate::hash::compute_meta_hash_fresh;

/// Reconcile the metadata of an engine run.
///
/// - `original`: the metadata before the engine ran (the real values).
/// - `baseline`: the metadata of a parse of the text the engine received.
/// - `executed`: the metadata of the parse of the engine's output.
///
/// Recursively from the root:
///
/// - baseline and executed structurally equal (source locations ignored)
///   → `original`;
/// - all three maps → key by key, in `original`'s key order: a key in
///   all three recurses; a key the engine removed (in baseline, not in
///   executed) is dropped; a key the writer never emitted (in original,
///   not in baseline) keeps `original`'s value. Keys only the output has
///   (added by the engine) follow, in its order;
/// - all three arrays of one length → item by item;
/// - otherwise → `executed`.
pub fn reconcile_meta(
    original: ConfigValue,
    baseline: &ConfigValue,
    executed: ConfigValue,
) -> ConfigValue {
    if compute_meta_hash_fresh(baseline) == compute_meta_hash_fresh(&executed) {
        return original;
    }
    match (original.value, &baseline.value, executed.value) {
        (
            ConfigValueKind::Map(original_entries),
            ConfigValueKind::Map(baseline_entries),
            ConfigValueKind::Map(executed_entries),
        ) => ConfigValue {
            value: ConfigValueKind::Map(reconcile_entries(
                original_entries,
                baseline_entries,
                executed_entries,
            )),
            source_info: original.source_info,
            merge_op: original.merge_op,
        },
        (
            ConfigValueKind::Array(original_items),
            ConfigValueKind::Array(baseline_items),
            ConfigValueKind::Array(executed_items),
        ) if original_items.len() == baseline_items.len()
            && baseline_items.len() == executed_items.len() =>
        {
            ConfigValue {
                value: ConfigValueKind::Array(
                    original_items
                        .into_iter()
                        .zip(baseline_items)
                        .zip(executed_items)
                        .map(|((o, b), e)| reconcile_meta(o, b, e))
                        .collect(),
                ),
                source_info: original.source_info,
                merge_op: original.merge_op,
            }
        }
        (_, _, executed_value) => ConfigValue {
            value: executed_value,
            source_info: executed.source_info,
            merge_op: executed.merge_op,
        },
    }
}

/// The map arm of [`reconcile_meta`].
fn reconcile_entries(
    original: Vec<ConfigMapEntry>,
    baseline: &[ConfigMapEntry],
    executed: Vec<ConfigMapEntry>,
) -> Vec<ConfigMapEntry> {
    let in_baseline = |key: &str| baseline.iter().find(|e| e.key == key);
    let mut executed: Vec<Option<ConfigMapEntry>> = executed.into_iter().map(Some).collect();
    let mut take_executed = |key: &str| {
        executed
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|e| e.key == key))
            .and_then(Option::take)
    };

    let mut result = Vec::with_capacity(original.len());
    let mut emitted: Vec<String> = Vec::new();
    for entry in original {
        let Some(baseline_entry) = in_baseline(&entry.key) else {
            // The writer never emitted it, so the engine cannot have
            // seen or changed it.
            result.push(entry);
            continue;
        };
        emitted.push(entry.key.clone());
        // Absent from the output: the engine removed it.
        if let Some(executed_entry) = take_executed(&entry.key) {
            result.push(ConfigMapEntry {
                value: reconcile_meta(entry.value, &baseline_entry.value, executed_entry.value),
                ..entry
            });
        }
    }
    // Keys the output has that neither the original nor the baseline
    // matched: added by the engine. A key in the baseline but not the
    // original (synthesized by the writer) cannot be told apart from an
    // engine addition and is taken too.
    result.extend(
        executed
            .into_iter()
            .flatten()
            .filter(|e| !emitted.contains(&e.key)),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, SourceInfo};
    use yaml_rust2::Yaml;

    /// Spans in three different files, so a result's provenance shows
    /// which input it came from.
    const ORIGINAL: FileId = FileId(1);
    const BASELINE: FileId = FileId(2);
    const EXECUTED: FileId = FileId(3);

    fn at(file: FileId) -> SourceInfo {
        SourceInfo::original(file, 0, 1)
    }

    fn string(s: &str, file: FileId) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), at(file))
    }

    fn path(s: &str, file: FileId) -> ConfigValue {
        ConfigValue::new_path(s.into(), at(file))
    }

    fn map(entries: Vec<(&str, ConfigValue)>, file: FileId) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(key, value)| ConfigMapEntry {
                    key: key.into(),
                    key_source: at(file),
                    value,
                })
                .collect(),
            at(file),
        )
    }

    fn array(items: Vec<ConfigValue>, file: FileId) -> ConfigValue {
        ConfigValue::new_array(items, at(file))
    }

    fn file_of(value: &ConfigValue) -> Option<FileId> {
        value.source_info.root_file_id()
    }

    fn keys(value: &ConfigValue) -> Vec<&str> {
        value
            .as_map_entries()
            .unwrap()
            .iter()
            .map(|e| e.key.as_str())
            .collect()
    }

    #[test]
    fn unchanged_value_keeps_the_original() {
        // The round-trip turned the marked path into a plain string in
        // both parses; the engine changed nothing.
        let result = reconcile_meta(
            path("_extensions/a/_b.scss", ORIGINAL),
            &string("extensions/a/b.scss", BASELINE),
            string("extensions/a/b.scss", EXECUTED),
        );
        assert!(matches!(&result.value, ConfigValueKind::Path(p) if p == "_extensions/a/_b.scss"));
        assert_eq!(file_of(&result), Some(ORIGINAL));
    }

    #[test]
    fn changed_value_takes_the_engines() {
        let result = reconcile_meta(
            string("`{python} x`", ORIGINAL),
            &string("`{python} x`", BASELINE),
            string("42", EXECUTED),
        );
        assert_eq!(result.as_str(), Some("42"));
        assert_eq!(file_of(&result), Some(EXECUTED));
    }

    #[test]
    fn map_keeps_unchanged_siblings_of_a_changed_leaf() {
        let result = reconcile_meta(
            map(
                vec![
                    ("title", string("T", ORIGINAL)),
                    ("css", path("s.css", ORIGINAL)),
                ],
                ORIGINAL,
            ),
            &map(
                vec![
                    ("title", string("T", BASELINE)),
                    ("css", string("s.css", BASELINE)),
                ],
                BASELINE,
            ),
            map(
                vec![
                    ("title", string("New", EXECUTED)),
                    ("css", string("s.css", EXECUTED)),
                ],
                EXECUTED,
            ),
        );
        assert_eq!(file_of(&result), Some(ORIGINAL), "the map itself is kept");
        assert_eq!(result.get("title").and_then(|v| v.as_str()), Some("New"));
        let css = result.get("css").unwrap();
        assert!(matches!(&css.value, ConfigValueKind::Path(p) if p == "s.css"));
        assert_eq!(file_of(css), Some(ORIGINAL));
    }

    #[test]
    fn nested_maps_recurse() {
        let result = reconcile_meta(
            map(
                vec![(
                    "format",
                    map(
                        vec![("a", string("1", ORIGINAL)), ("b", string("2", ORIGINAL))],
                        ORIGINAL,
                    ),
                )],
                ORIGINAL,
            ),
            &map(
                vec![(
                    "format",
                    map(
                        vec![("a", string("1", BASELINE)), ("b", string("2", BASELINE))],
                        BASELINE,
                    ),
                )],
                BASELINE,
            ),
            map(
                vec![(
                    "format",
                    map(
                        vec![("a", string("1", EXECUTED)), ("b", string("3", EXECUTED))],
                        EXECUTED,
                    ),
                )],
                EXECUTED,
            ),
        );
        let format = result.get("format").unwrap();
        assert_eq!(file_of(format.get("a").unwrap()), Some(ORIGINAL));
        assert_eq!(format.get("b").and_then(|v| v.as_str()), Some("3"));
    }

    #[test]
    fn key_added_by_the_engine_is_taken() {
        let result = reconcile_meta(
            map(vec![("a", string("1", ORIGINAL))], ORIGINAL),
            &map(vec![("a", string("1", BASELINE))], BASELINE),
            map(
                vec![("a", string("1", EXECUTED)), ("new", string("x", EXECUTED))],
                EXECUTED,
            ),
        );
        assert_eq!(keys(&result), ["a", "new"]);
        assert_eq!(file_of(result.get("a").unwrap()), Some(ORIGINAL));
        assert_eq!(result.get("new").and_then(|v| v.as_str()), Some("x"));
    }

    #[test]
    fn key_removed_by_the_engine_is_dropped() {
        let result = reconcile_meta(
            map(
                vec![
                    ("a", string("1", ORIGINAL)),
                    ("gone", string("x", ORIGINAL)),
                ],
                ORIGINAL,
            ),
            &map(
                vec![
                    ("a", string("1", BASELINE)),
                    ("gone", string("x", BASELINE)),
                ],
                BASELINE,
            ),
            map(vec![("a", string("1", EXECUTED))], EXECUTED),
        );
        assert_eq!(keys(&result), ["a"]);
        assert_eq!(file_of(result.get("a").unwrap()), Some(ORIGINAL));
    }

    #[test]
    fn key_the_writer_never_emitted_keeps_the_original() {
        let result = reconcile_meta(
            map(
                vec![
                    ("a", string("1", ORIGINAL)),
                    ("unwritten", string("x", ORIGINAL)),
                ],
                ORIGINAL,
            ),
            &map(vec![("a", string("1", BASELINE))], BASELINE),
            map(vec![("a", string("2", EXECUTED))], EXECUTED),
        );
        assert_eq!(keys(&result), ["a", "unwritten"]);
        assert_eq!(result.get("a").and_then(|v| v.as_str()), Some("2"));
        assert_eq!(file_of(result.get("unwritten").unwrap()), Some(ORIGINAL));
    }

    #[test]
    fn equal_length_arrays_reconcile_item_by_item() {
        let result = reconcile_meta(
            array(
                vec![path("_a/_b.scss", ORIGINAL), string("x", ORIGINAL)],
                ORIGINAL,
            ),
            &array(
                vec![string("a/b.scss", BASELINE), string("x", BASELINE)],
                BASELINE,
            ),
            array(
                vec![string("a/b.scss", EXECUTED), string("y", EXECUTED)],
                EXECUTED,
            ),
        );
        let items = result.as_array().unwrap();
        assert!(matches!(&items[0].value, ConfigValueKind::Path(p) if p == "_a/_b.scss"));
        assert_eq!(items[1].as_str(), Some("y"));
    }

    #[test]
    fn arrays_of_different_length_take_the_engines() {
        let result = reconcile_meta(
            array(vec![string("x", ORIGINAL)], ORIGINAL),
            &array(vec![string("x", BASELINE)], BASELINE),
            array(vec![string("x", EXECUTED), string("y", EXECUTED)], EXECUTED),
        );
        assert_eq!(file_of(&result), Some(EXECUTED));
        assert_eq!(result.as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_change_of_shape_takes_the_engines() {
        let result = reconcile_meta(
            map(vec![("a", string("1", ORIGINAL))], ORIGINAL),
            &map(vec![("a", string("1", BASELINE))], BASELINE),
            string("flat", EXECUTED),
        );
        assert_eq!(result.as_str(), Some("flat"));
    }

    #[test]
    fn original_shape_differing_from_the_baseline_takes_the_engines_on_change() {
        // The writer emitted a map where the original held something
        // else; with no correspondence to recurse through, a change
        // takes the engine's value.
        let result = reconcile_meta(
            string("opaque", ORIGINAL),
            &map(vec![("a", string("1", BASELINE))], BASELINE),
            map(vec![("a", string("2", EXECUTED))], EXECUTED),
        );
        assert_eq!(file_of(&result), Some(EXECUTED));
    }
}
