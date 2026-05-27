//! Metadata-merge interaction tests for the `engine:` key (bd-5yff4).
//!
//! These pin down how a multi-engine `engine:` array composes across
//! config layers (project `_quarto.yml` → directory `_metadata.yml` →
//! document front matter) under the general merge rules in
//! `quarto-config`, and how the resolved value is then interpreted by
//! `detect_engine_sequence`. They are the executable form of the
//! "verified merge behavior" table in
//! `claude-notes/plans/2026-05-27-multi-engine-execution.md`.
//!
//! Key facts they lock in:
//! - Arrays merge by `!concat` (default): layers accumulate.
//! - `!prefer` replaces the array wholesale.
//! - The *kind* of the highest-priority layer that sets `engine:` wins:
//!   a scalar layer drops a lower array layer, and an array layer drops a
//!   lower scalar layer.
//! - Engines must be distinct; duplicates from `!concat` across layers
//!   are de-duplicated (first kept) with the drops reported.

use quarto_config::MergedConfig;
use quarto_core::engine::detect_engine_sequence;
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::{ConfigValue, MergeOp};
use quarto_source_map::SourceInfo;

fn s(x: &str) -> ConfigValue {
    ConfigValue::new_string(x, SourceInfo::default())
}

fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let info = SourceInfo::default();
    let map_entries: Vec<ConfigMapEntry> = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: info.clone(),
            value: v,
        })
        .collect();
    ConfigValue::new_map(map_entries, info)
}

fn arr(items: Vec<ConfigValue>) -> ConfigValue {
    ConfigValue::new_array(items, SourceInfo::default())
}

/// Merge layers (lowest priority first) via the production merge
/// machinery and return the flat `ConfigValue` that would be assigned to
/// `ast.meta`.
fn merge(layers: Vec<&ConfigValue>) -> ConfigValue {
    MergedConfig::new(layers)
        .materialize()
        .expect("materialize")
}

/// Resolve the engine names a merged metadata map yields.
fn engine_names(meta: &ConfigValue) -> Vec<String> {
    detect_engine_sequence(meta)
        .engines
        .into_iter()
        .map(|e| e.name)
        .collect()
}

#[test]
fn concat_default_accumulates_engine_arrays() {
    // project engine: [knitr] + doc engine: [mermaidjs] → [knitr, mermaidjs]
    let project = map(vec![("engine", arr(vec![s("knitr")]))]);
    let document = map(vec![("engine", arr(vec![s("mermaidjs")]))]);
    let merged = merge(vec![&project, &document]);
    assert_eq!(engine_names(&merged), vec!["knitr", "mermaidjs"]);
}

#[test]
fn concat_accumulates_across_three_layers_in_order() {
    let project = map(vec![("engine", arr(vec![s("knitr")]))]);
    let directory = map(vec![("engine", arr(vec![s("jupyter")]))]);
    let document = map(vec![("engine", arr(vec![s("mermaidjs")]))]);
    let merged = merge(vec![&project, &directory, &document]);
    assert_eq!(engine_names(&merged), vec!["knitr", "jupyter", "mermaidjs"]);
}

#[test]
fn prefer_replaces_engine_array() {
    // doc uses !prefer → its array replaces the project's.
    let project = map(vec![("engine", arr(vec![s("knitr"), s("jupyter")]))]);
    let document = map(vec![(
        "engine",
        arr(vec![s("mermaidjs")]).with_merge_op(MergeOp::Prefer),
    )]);
    let merged = merge(vec![&project, &document]);
    assert_eq!(engine_names(&merged), vec!["mermaidjs"]);
}

#[test]
fn scalar_doc_overrides_lower_array() {
    // Highest layer is a scalar → it wins; the lower array is dropped.
    let project = map(vec![("engine", arr(vec![s("knitr")]))]);
    let document = map(vec![("engine", s("jupyter"))]);
    let merged = merge(vec![&project, &document]);
    assert_eq!(engine_names(&merged), vec!["jupyter"]);
}

#[test]
fn array_doc_drops_lower_scalar() {
    // Highest layer is an array → array semantics apply, and the lower
    // *scalar* layer is not folded in (documented gotcha).
    let project = map(vec![("engine", s("jupyter"))]);
    let document = map(vec![("engine", arr(vec![s("knitr")]))]);
    let merged = merge(vec![&project, &document]);
    assert_eq!(engine_names(&merged), vec!["knitr"]);
}

#[test]
fn duplicate_engine_across_layers_is_deduped() {
    // The one shape that yields a duplicate: two array layers naming the
    // same engine. Dedup keeps the first; the drop is reported.
    let project = map(vec![("engine", arr(vec![s("jupyter")]))]);
    let document = map(vec![("engine", arr(vec![s("jupyter")]))]);
    let merged = merge(vec![&project, &document]);

    let seq = detect_engine_sequence(&merged);
    let names: Vec<_> = seq.engines.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["jupyter"]);
    assert_eq!(seq.dropped_duplicates, vec!["jupyter".to_string()]);
}

#[test]
fn single_scalar_engine_unchanged_by_merge() {
    // Back-compat: a lone scalar engine still resolves to one engine.
    let project = map(vec![("engine", s("knitr"))]);
    let document = map(vec![("title", s("Doc"))]);
    let merged = merge(vec![&project, &document]);
    assert_eq!(engine_names(&merged), vec!["knitr"]);
}
