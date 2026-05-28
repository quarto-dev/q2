//! Integration tests for the `quarto trace` CLI surface.
//!
//! These tests bypass the CLI binary and invoke the `list_value` / `show_value`
//! helpers directly against a fixture `.quarto/trace/<doc>/latest.json`.

use std::fs;
use std::path::PathBuf;

use quarto_trace::{
    RenderInfo, StageErrorInfo, StageStatus, TraceDocument, TraceEntry, write::write_trace,
};
use serde_json::json;

// Pull the binary crate's module directly. `quarto` is a binary crate whose
// entry point is `crates/quarto/src/main.rs`, and Cargo exposes integration
// tests linked against `main.rs`. We can therefore reference modules via
// `quarto::commands::trace` from within the binary's own integration tests.
//
// Cargo doesn't expose `bin` crate modules to integration tests by default, so
// we re-declare the pieces we need here. This is a deliberately surgical
// duplication that keeps the test self-contained.
//
// If this becomes onerous, we can extract `commands::trace` into its own crate
// and depend on it here.
#[allow(dead_code)] // execute_* functions only used by the real CLI, not these tests
#[path = "../../src/commands/trace.rs"]
mod trace_cmd;

fn unique_trace_root(label: &str) -> PathBuf {
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir()
        .join(format!("quarto-trace-cli-{}-{}-{}", label, pid, ts))
        .join(".quarto")
        .join("trace");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir trace root");
    dir
}

fn write_fixture_trace(root: &std::path::Path, stem: &str) -> PathBuf {
    let doc_dir = root.join(stem);
    fs::create_dir_all(&doc_dir).unwrap();
    let path = doc_dir.join("latest.json");

    let render = RenderInfo {
        input_path: Some(format!("{}.qmd", stem)),
        output_path: Some(format!("{}.html", stem)),
        format_target: Some("html".into()),
        started_at_unix_ms: Some(1_799_200_496_000.0),
        git_hash: Some("deadbee".into()),
        total_duration_ms: Some(42.0),
    };
    let mut doc = TraceDocument::new(render);
    doc.pipeline.push(TraceEntry {
        stage: "__input".into(),
        index: 0,
        data_kind: Some("LoadedSource".into()),
        data: Some(json!({"path": format!("{}.qmd", stem)})),
        duration_ms: None,
        status: StageStatus::Ok,
        error: None,
    });
    doc.pipeline.push(TraceEntry {
        stage: "parse".into(),
        index: 0,
        data_kind: Some("DocumentAst".into()),
        data: Some(json!({"blocks": []})),
        duration_ms: Some(1.0),
        status: StageStatus::Ok,
        error: None,
    });
    doc.pipeline.push(TraceEntry {
        stage: "engine-execution".into(),
        index: 1,
        data_kind: None,
        data: None,
        duration_ms: Some(5.0),
        status: StageStatus::Error,
        error: Some(StageErrorInfo {
            message: "kernel died".into(),
        }),
    });

    write_trace(&doc, &path).unwrap();
    path
}

#[test]
fn test_trace_list_returns_discovered_traces() {
    let root = unique_trace_root("list");
    write_fixture_trace(&root, "doc-a");
    write_fixture_trace(&root, "doc-b");

    let value = trace_cmd::list_value(&trace_cmd::TraceListArgs {
        trace_dir: Some(root.clone()),
    })
    .unwrap();

    assert_eq!(
        value["trace_dir"],
        serde_json::Value::String(root.display().to_string())
    );
    let traces = value["traces"].as_array().unwrap();
    assert_eq!(traces.len(), 2);
    let mut stems: Vec<_> = traces.iter().map(|v| v["doc"].as_str().unwrap()).collect();
    stems.sort();
    assert_eq!(stems, vec!["doc-a", "doc-b"]);
}

#[test]
fn test_trace_list_empty_dir_returns_empty_array() {
    let root = unique_trace_root("empty");
    let value = trace_cmd::list_value(&trace_cmd::TraceListArgs {
        trace_dir: Some(root),
    })
    .unwrap();
    assert!(value["traces"].as_array().unwrap().is_empty());
}

#[test]
fn test_trace_show_full_document() {
    let root = unique_trace_root("show");
    write_fixture_trace(&root, "only-doc");

    let value = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: None,
    })
    .unwrap();

    // Writer stamps newly-written fixtures with the current
    // SCHEMA_VERSION (2 as of bd-5qnj). Older traces with
    // schema_version: 1 still parse via the reader's backwards-compat
    // path, exercised in quarto-trace's `test_v2_reader_handles_v1_input`.
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["render"]["format_target"], "html");
    assert_eq!(value["pipeline"].as_array().unwrap().len(), 3);
}

#[test]
fn test_trace_show_single_stage() {
    let root = unique_trace_root("show-stage");
    write_fixture_trace(&root, "doc");

    let value = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: Some("parse".into()),
    })
    .unwrap();

    assert_eq!(value["stage"], "parse");
    assert_eq!(value["status"], "ok");
    assert!(value["data"].is_object());
}

#[test]
fn test_trace_show_errored_stage() {
    let root = unique_trace_root("show-err");
    write_fixture_trace(&root, "doc");

    let value = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: Some("engine-execution".into()),
    })
    .unwrap();

    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["message"], "kernel died");
}

#[test]
fn test_trace_show_requires_doc_when_ambiguous() {
    let root = unique_trace_root("ambiguous");
    write_fixture_trace(&root, "doc-a");
    write_fixture_trace(&root, "doc-b");

    let err = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: None,
    })
    .unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Multiple traces"), "unexpected: {}", msg);
}

#[test]
fn test_trace_show_unknown_stage_errors() {
    let root = unique_trace_root("unknown-stage");
    write_fixture_trace(&root, "doc");

    let err = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: Some("not-a-stage".into()),
    })
    .unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("No stage"), "unexpected: {}", msg);
}

#[test]
fn test_trace_show_empty_root_errors() {
    let root = unique_trace_root("no-traces");
    let err = trace_cmd::show_value(&trace_cmd::TraceShowArgs {
        trace_dir: Some(root),
        doc: None,
        stage: None,
    })
    .unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("No traces"), "unexpected: {}", msg);
}
