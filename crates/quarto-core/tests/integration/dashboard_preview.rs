//! The dashboard live preview (`q2-dashboard`, plan
//! `claude-notes/plans/2026-11-03-dashboard-format-port.md`, Phase 8): the
//! preview pipeline runs the same lowering as the render and leaves view
//! nodes (`dashboard/view.rs`) for the React preview, with the navbar
//! metadata the React shell reads.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const DOC: &[u8] = b"---
title: Preview Dash
format:
  dashboard:
    nav-buttons: [twitter]
---

# Overview

## {.sidebar}

Sidebar text.

## Column

::: {.card title=\"A\"}
Card A.
:::

# Details

## Row {.tabset}

::: {.card title=\"T1\"}
One.
:::

::: {.card title=\"T2\"}
Two.
:::
";

fn render_preview(content: &[u8]) -> serde_json::Value {
    let project = ProjectContext {
        dir: PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: PathBuf::from("/project"),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::from_format_string("q2-dashboard").expect("pseudo-format");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let temp = TempDir::new().unwrap();
    let runtime: Arc<dyn SystemRuntime> =
        Arc::new(NativeRuntime::with_cache_dir(temp.path().to_path_buf()));
    let out = pollster::block_on(render_qmd_to_preview_ast(
        content,
        "test.qmd",
        &mut ctx,
        runtime,
        None,
        vec![],
    ))
    .expect("q2-dashboard preview render");
    serde_json::from_str(&out.ast_json).expect("ast json")
}

/// Every custom node\'s type anywhere in the JSON. On the wire a custom
/// node is a wrapper Div carrying `data-custom-type`.
fn custom_types(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Array(a) => {
            if let [serde_json::Value::String(k), serde_json::Value::String(t)] = a.as_slice()
                && k == "data-custom-type"
            {
                out.push(t.clone());
            }
            a.iter().for_each(|x| custom_types(x, out));
        }
        serde_json::Value::Object(m) => m.values().for_each(|x| custom_types(x, out)),
        _ => {}
    }
}

#[test]
fn preview_ast_carries_view_nodes_and_navbar_metadata() {
    let ast = render_preview(DOC);
    let mut types = Vec::new();
    custom_types(&ast["blocks"], &mut types);
    let count = |t: &str| types.iter().filter(|x| *x == t).count();
    assert_eq!(count("DashboardPageView"), 2, "{types:?}");
    assert_eq!(count("DashboardSidebarLayout"), 1, "{types:?}");
    assert_eq!(count("DashboardCardView"), 2, "{types:?}");
    assert_eq!(count("DashboardTabNav"), 1, "{types:?}");
    assert_eq!(count("DashboardTabPane"), 2, "{types:?}");
    // The structure nodes are all lowered.
    for t in [
        "DashboardPage",
        "DashboardContainer",
        "DashboardCard",
        "DashboardPanel",
    ] {
        assert_eq!(count(t), 0, "{t} left in the preview AST");
    }
    let text = ast["blocks"].to_string();
    assert!(!text.contains("<script") && !text.contains("<aside"));

    // The navbar's metadata, from format-scoped options too.
    let meta = ast["meta"].to_string();
    assert!(meta.contains("nav-buttons"), "nav-buttons missing: {meta}");
    assert!(meta.contains("twitter.com/intent/tweet"), "{meta}");
    assert!(meta.contains("\"pages\""), "pages missing: {meta}");
}
