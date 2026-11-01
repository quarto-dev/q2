/*
 * tests/integration/table_bootstrap_classes_preview.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-hfghrqvh: Bootstrap table shorthands (`: cap {.striped .sm}`) reach
 * the q2-preview AST as `table-*` classes.
 */

//! The render path is covered end to end by the smoke-all fixture
//! `crates/quarto/tests/smoke-all/markdown/table-caption-bootstrap-classes.qmd`.
//! q2-preview is the case that fixture can't see: it skips
//! `crossref-render`, so a `tbl` float reaches the React renderer as a
//! `FloatRefTarget` custom node, and both table transforms have to work on
//! that shape. These pin the AST the SPA actually receives.

use std::sync::Arc;

use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::Value;

fn preview_table_classes(content: &str) -> Vec<Vec<String>> {
    let project = ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: std::path::PathBuf::from("/project"),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = quarto_core::format::Format::from_format_string("q2-preview")
        .expect("pseudo-format resolves");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let ast_json = pollster::block_on(render_qmd_to_preview_ast(
        content.as_bytes(),
        "test.qmd",
        &mut ctx,
        runtime,
        None,
        vec![].into(),
    ))
    .expect("preview pipeline render")
    .ast_json;
    let ast: Value = serde_json::from_str(&ast_json).expect("preview AST is JSON");
    let mut out = Vec::new();
    collect_table_classes(&ast, &mut out);
    out
}

/// Every `Table` node's class list, in document order, wherever it sits
/// (custom-node slots included).
fn collect_table_classes(value: &Value, out: &mut Vec<Vec<String>>) {
    match value {
        Value::Object(map) => {
            if map.get("t").and_then(Value::as_str) == Some("Table")
                && let Some(classes) = map.get("c").and_then(|c| c[0][1].as_array())
            {
                out.push(
                    classes
                        .iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect(),
                );
            }
            map.values().for_each(|v| collect_table_classes(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_table_classes(v, out)),
        _ => {}
    }
}

const TABLE: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

#[test]
fn caption_shorthands_are_prefixed_in_preview() {
    let classes = preview_table_classes(&format!("{TABLE}\n: Cap {{.striped .hover .my-class}}\n"));
    assert_eq!(
        classes,
        vec![vec![
            "table-striped",
            "table-hover",
            "my-class",
            "caption-top",
            "table"
        ]]
    );
}

#[test]
fn labelled_table_float_classes_reach_the_table_in_preview() {
    let classes = preview_table_classes(&format!("{TABLE}\n: Cap {{#tbl-one .bordered .sm}}\n"));
    assert_eq!(
        classes,
        vec![vec![
            "table-bordered",
            "table-sm",
            "small",
            "caption-top",
            "table"
        ]]
    );
}

#[test]
fn div_form_table_float_classes_reach_the_table_in_preview() {
    let classes =
        preview_table_classes(&format!("::: {{#tbl-div .striped}}\n\n{TABLE}\nCap\n:::\n"));
    assert_eq!(classes, vec![vec!["table-striped", "caption-top", "table"]]);
}
