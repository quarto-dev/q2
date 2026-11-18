//! `analyze_in_project` (epic bd-3f6ijnw9, C3): one analysis run over a
//! real project yields the editor-facing analysis *and* the Pass-1 profile
//! and authored metadata, with project config and includes taken into
//! account.

use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_lsp_core::analyze_in_project;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn names(symbols: &[quarto_lsp_core::Symbol]) -> Vec<String> {
    let mut out = Vec::new();
    for s in symbols {
        out.push(s.name.clone());
        out.extend(names(&s.children));
    }
    out
}

#[test]
fn one_run_yields_symbols_profile_and_authored_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/_metadata.yml"), "description: from dir\n").unwrap();
    std::fs::write(root.join("sub/_child.qmd"), "## From the child\n").unwrap();
    let doc = root.join("sub/doc.qmd");
    // The text is the editor's, not the disk's: the file need not exist.
    let text = "---\ntitle: Buffer title\n---\n\n# Top\n\n{{< include _child.qmd >}}\n";

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let format = Format::from_format_string("html").unwrap();
    let result = pollster::block_on(analyze_in_project(
        runtime,
        &project,
        &format,
        &DocumentInfo::from_path(&doc),
        text,
    ));

    assert_eq!(names(&result.analysis.symbols), ["Top", "From the child"]);
    let profile = result.profile.expect("profile");
    assert_eq!(profile.title_text().as_deref(), Some("Buffer title"));
    assert_eq!(profile.source_path, std::path::PathBuf::from("sub/doc.qmd"));
    let meta = pampa::config_json::config_value_to_json(
        &result.authored_meta.expect("authored metadata"),
        pampa::config_json::ProseMode::Value,
        &result.ast_context.expect("ast context"),
    );
    assert_eq!(meta["description"], "from dir");
    assert!(meta.get("quarto").is_none(), "no derived keys: {meta}");
}

#[test]
fn a_parse_failure_has_diagnostics_and_no_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let doc = root.join("doc.qmd");
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let result = pollster::block_on(analyze_in_project(
        runtime,
        &project,
        &Format::from_format_string("html").unwrap(),
        &DocumentInfo::from_path(&doc),
        "Some text [>> half a comment\n",
    ));
    assert!(result.profile.is_none());
    assert!(!result.analysis.diagnostics.is_empty());
}
