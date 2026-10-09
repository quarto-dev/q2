/*
 * tests/integration/metadata_layers.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The metadata layer collector (FR14) and the merge stage built on it.
 */

//! Layer-order and provenance contract of `quarto_core::metadata_layers`,
//! and a no-behaviour-change guard for `MetadataMergeStage`.

use std::path::Path;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::get_config::{ProseMode, config_value_to_json, merge_document_metadata};
use quarto_core::metadata_layers::{
    DocumentLayer, LayerInputs, LayerKind, collect_metadata_layers,
};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::{Value, json};
use tempfile::TempDir;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Merge `doc` for `format` the way a render does and project it to JSON.
fn merged_json(root: &Path, doc: &Path, format: &str) -> Value {
    merged_json_with_profile(root, doc, format, None)
}

fn merged_json_with_profile(
    root: &Path,
    doc: &Path,
    format: &str,
    selection: Option<&[String]>,
) -> Value {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover_with_profile(root, runtime.as_ref(), selection).unwrap();
    let format = Format::from_format_string(format).unwrap();
    let source = std::fs::read(doc).unwrap();
    let (meta, ctx) = pollster::block_on(merge_document_metadata(
        runtime.clone(),
        &project,
        &format,
        &DocumentInfo::from_path(doc),
        &source,
    ))
    .unwrap();
    config_value_to_json(&meta, ProseMode::Value, &ctx)
}

/// A project with every layer the merge stage reads from disk: an extension's
/// `contributes.metadata` and format metadata, `_quarto.yml`, a profile-less
/// `_quarto.yml.local`, two nested `_metadata.yml` files and front matter.
/// Each layer sets a `probe-*` key of its own, and `winner` in all of them.
fn every_layer_fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    write(
        &root.join("_extensions/acme/_extension.yml"),
        "title: Acme\nversion: 1.0.0\ncontributes:\n  metadata:\n    probe-ext-meta: e\n    winner: ext-meta\n    project:\n      pre-render: x.ts\n  formats:\n    html:\n      probe-ext-format: f\n      winner: ext-format\n",
    );
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nprobe-project: p\nwinner: project\nformat:\n  html:\n    probe-project-html: ph\n",
    );
    write(
        &root.join("_quarto.yml.local"),
        "probe-local: l\nwinner: local\n",
    );
    write(
        &root.join("chapters/_metadata.yml"),
        "probe-dir1: d1\nwinner: dir1\nformat:\n  html:\n    probe-dir1-html: d1h\n",
    );
    write(
        &root.join("chapters/intro/_metadata.yml"),
        "probe-dir2: d2\nwinner: dir2\n",
    );
    let doc = root.join("chapters/intro/doc.qmd");
    write(
        &doc,
        "---\nprobe-doc: doc\nwinner: doc\nformat:\n  html:\n    probe-doc-fmt: df\n---\n\nBody\n",
    );
    (tmp, root, doc)
}

/// What the merge stage produced for `every_layer_fixture` before the layer
/// collection was factored out (computed against the pre-refactor stage), so
/// the refactor is observed to change nothing.
#[test]
fn merge_stage_output_is_unchanged_by_the_collector() {
    let (_tmp, root, doc) = every_layer_fixture();
    let got = merged_json(&root, &doc, "acme-html");
    let expected = json!({
        "probe-ext-meta": "e",
        "probe-ext-format": "f",
        "probe-project": "p",
        "probe-project-html": "ph",
        "probe-local": "l",
        "probe-dir1": "d1",
        "probe-dir1-html": "d1h",
        "probe-dir2": "d2",
        "probe-doc": "doc",
        "probe-doc-fmt": "df",
        "winner": "doc",
        "format": "acme-html",
    });
    // Subset check on the probe keys: the merged map also carries defaults
    // and extension metadata that are not the subject here.
    for (key, want) in expected.as_object().unwrap() {
        assert_eq!(got.get(key), Some(want), "key {key} in {got}");
    }
}

// ── layer sources: a probe key set in each layer ────────────────────

/// The `probe` key's winning layer and value, from the collector, and the
/// merged value the stage produces for the same project (they must agree).
fn probe_winner(
    files: &[(&str, &str)],
    selection: Option<&[&str]>,
) -> (LayerKind, Option<std::path::PathBuf>, String, String) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    for (name, body) in files {
        write(&root.join(name), body);
    }
    let doc = root.join("a/b/doc.qmd");
    if !files.iter().any(|(name, _)| *name == "a/b/doc.qmd") {
        write(&doc, "# Body\n");
    }
    let owned: Option<Vec<String>> = selection.map(|s| s.iter().map(|p| p.to_string()).collect());
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project =
        ProjectContext::discover_with_profile(&root, runtime.as_ref(), owned.as_deref()).unwrap();
    let content = std::fs::read_to_string(&doc).unwrap();
    let layers = collect_metadata_layers(&LayerInputs {
        project: &project,
        document_path: &doc,
        document_dir: doc.parent().unwrap().to_path_buf(),
        document: DocumentLayer::Content(&content),
        extensions: None,
        target_format: None,
        is_book: false,
        runtime: runtime.as_ref(),
    });
    let winner = layers
        .iter()
        .rev()
        .find(|l| l.config.get("probe").is_some())
        .expect("some layer sets probe");
    let layer_value = winner.config.get("probe").unwrap().as_plain_text().unwrap();
    let merged = merged_json_with_profile(&root, &doc, "html", owned.as_deref());
    let merged_value = merged["probe"].as_str().unwrap().to_string();
    let source = winner
        .source
        .as_ref()
        .map(|p| p.strip_prefix(&root).unwrap_or(p).to_path_buf());
    (winner.kind, source, layer_value, merged_value)
}

const PROJECT_YML: &str = "project:\n  type: default\n";

#[test]
fn probe_in_the_project() {
    let yml = format!("{PROJECT_YML}probe: project\n");
    let (kind, source, layer, merged) = probe_winner(&[("_quarto.yml", &yml)], None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "project", "project")
    );
    assert_eq!(source.as_deref(), Some(Path::new("_quarto.yml")));
}

#[test]
fn project_profile_overlay_by_cli_selection_is_part_of_the_project_layer() {
    let yml = format!("{PROJECT_YML}probe: base\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("_quarto-prod.yml", "probe: prod\n"),
    ];
    let (kind, _, layer, merged) = probe_winner(&files, Some(&["prod"]));
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "prod", "prod")
    );
    // Without the selection the base value stands.
    let (_, _, layer, merged) = probe_winner(&files, None);
    assert_eq!((layer.as_str(), merged.as_str()), ("base", "base"));
}

#[test]
fn project_profile_overlay_by_profile_default_in_quarto_yml() {
    let yml = format!("{PROJECT_YML}probe: base\nprofile:\n  default: prod\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("_quarto-prod.yml", "probe: prod\n"),
    ];
    let (kind, _, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "prod", "prod")
    );
}

#[test]
fn project_profile_overlay_by_profile_default_in_quarto_yml_local() {
    let yml = format!("{PROJECT_YML}probe: base\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("_quarto.yml.local", "profile:\n  default: prod\n"),
        ("_quarto-prod.yml", "probe: prod\n"),
    ];
    let (kind, _, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "prod", "prod")
    );
}

#[test]
fn quarto_yml_local_values_are_part_of_the_project_layer() {
    let yml = format!("{PROJECT_YML}probe: base\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("_quarto.yml.local", "probe: local\n"),
    ];
    let (kind, _, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "local", "local")
    );
}

/// `QUARTO_PROFILE` defined in `_environment` (the dotenv bootstrap) selects
/// the overlay in-process. The real environment variable is covered through
/// the `q2` binary by `get_config_env_var_selects_overlay` in the `quarto`
/// crate's `project_profile_cli` tests (no `set_var` here).
#[test]
fn project_profile_overlay_by_environment_file() {
    let yml = format!("{PROJECT_YML}probe: base\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("_environment", "QUARTO_PROFILE=prod\n"),
        ("_quarto-prod.yml", "probe: prod\n"),
    ];
    let (kind, _, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Project, "prod", "prod")
    );
}

#[test]
fn probe_in_nested_metadata_files_deepest_wins() {
    let yml = format!("{PROJECT_YML}probe: project\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("a/_metadata.yml", "probe: dir-a\n"),
        ("a/b/_metadata.yml", "probe: dir-b\n"),
    ];
    let (kind, source, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Directory, "dir-b", "dir-b")
    );
    assert!(source.unwrap().ends_with("a/b/_metadata.yml"));
    // The shallower file alone.
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("a/_metadata.yml", "probe: dir-a\n"),
    ];
    let (kind, source, _, merged) = probe_winner(&files, None);
    assert_eq!((kind, merged.as_str()), (LayerKind::Directory, "dir-a"));
    assert!(source.unwrap().ends_with("a/_metadata.yml"));
}

#[test]
fn probe_in_front_matter_beats_directory_and_project() {
    let yml = format!("{PROJECT_YML}probe: project\n");
    let files = [
        ("_quarto.yml", yml.as_str()),
        ("a/b/_metadata.yml", "probe: dir-b\n"),
        ("a/b/doc.qmd", "---\nprobe: doc\n---\n\nBody\n"),
    ];
    let (kind, source, layer, merged) = probe_winner(&files, None);
    assert_eq!(
        (kind, layer.as_str(), merged.as_str()),
        (LayerKind::Document, "doc", "doc")
    );
    assert!(source.unwrap().ends_with("a/b/doc.qmd"));
}
