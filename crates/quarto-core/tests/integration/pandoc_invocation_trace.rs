/*
 * tests/integration/pandoc_invocation_trace.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `PandocWriteStage` records the exact pandoc request in the pipeline
 * trace, so the vendored Lua filter stage can be replayed in isolation.
 */

//! A `trace: true` docx render must leave a `PandocInvocation` aux entry
//! carrying the AST pandoc received, the decoded filter params, and argv.

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

#[test]
fn traced_docx_render_records_pandoc_invocation() {
    let temp = TempDir::new().unwrap();
    let project_dir = temp.path().canonicalize().unwrap();
    let input_path = project_dir.join("f.qmd");
    std::fs::write(
        &input_path,
        "---\ntitle: F\ntrace: true\n---\n\nHello trace body.\n",
    )
    .unwrap();

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    render_document_to_file(
        &input_path,
        "docx",
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .expect("traced docx render should succeed");

    let trace_path = project_dir
        .join(".quarto")
        .join("trace")
        .join("f")
        .join("latest.json.gz");
    let trace = quarto_trace::read::read_trace(&trace_path).expect("trace should be readable");

    let entry = trace
        .pipeline
        .iter()
        .find(|e| e.data_kind.as_deref() == Some("PandocInvocation"))
        .expect("trace should contain a PandocInvocation aux entry");
    let data = entry.data.as_ref().expect("entry carries data");

    // The AST is rehydrated from the dedup map and is the pandoc-JSON the
    // child received.
    let ast = &data["ast"];
    assert!(ast["blocks"].is_array(), "ast should be Pandoc JSON: {ast}");
    assert!(
        serde_json::to_string(ast).unwrap().contains("Hello"),
        "ast should contain the body text"
    );

    // Params are decoded JSON, not the base64 blob. `active-filters` is a
    // param the Lua chain always reads.
    assert!(
        data["params"].is_object(),
        "params should be a JSON object: {}",
        data["params"]
    );
    assert!(data["params"].get("active-filters").is_some());

    assert_eq!(data["to_format"], "docx");
    let argv: Vec<&str> = data["argv"]
        .as_array()
        .expect("argv array")
        .iter()
        .filter_map(|a| a.as_str())
        .collect();
    assert!(argv.windows(2).any(|w| w == ["-f", "json"]));
    assert!(argv.contains(&"-L"));
}
