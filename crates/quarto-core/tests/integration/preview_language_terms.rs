/*
 * tests/integration/preview_language_terms.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-21n09dwj: the q2-preview AST carries the resolved language terms at
 * `meta.quarto.language`, which `PreviewDocument.tsx` reads for the
 * skip-to-content label (render computes the same label from the same
 * table in `template.rs`).
 */

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn preview_meta(source: &str) -> serde_json::Value {
    let project = ProjectContext {
        dir: PathBuf::from("/project"),
        config: quarto_core::project::ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/test.qmd")],
        output_dir: PathBuf::from("/project"),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/project/test.qmd");
    let format = Format::from_format_string("q2-preview").expect("pseudo-format");
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let output = pollster::block_on(render_qmd_to_preview_ast(
        source.as_bytes(),
        "test.qmd",
        &mut ctx,
        runtime,
        None,
        vec![].into(),
    ))
    .expect("preview render");
    let ast: serde_json::Value = serde_json::from_str(&output.ast_json).expect("AST JSON");
    ast["meta"].clone()
}

/// The plain text of a `MetaString` / `MetaInlines` JSON value.
fn meta_text(v: &serde_json::Value) -> String {
    match v["t"].as_str() {
        Some("MetaString") => v["c"].as_str().unwrap().to_string(),
        Some("MetaInlines") => v["c"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| match i["t"].as_str() {
                Some("Str") => i["c"].as_str().unwrap(),
                Some("Space" | "SoftBreak") => " ",
                _ => "",
            })
            .collect(),
        other => panic!("not a string-like meta value: {other:?} in {v}"),
    }
}

fn skip_to_content_term(meta: &serde_json::Value) -> String {
    let term = &meta["quarto"]["c"]["language"]["c"]["skip-to-content"];
    assert!(
        !term.is_null(),
        "no quarto.language.skip-to-content in preview meta: {meta}"
    );
    meta_text(term)
}

#[test]
fn preview_ast_carries_skip_to_content_term() {
    let meta = preview_meta("---\ntitle: T\n---\n\nBody.\n");
    assert_eq!(skip_to_content_term(&meta), "Skip to main content");
}

#[test]
fn preview_ast_skip_to_content_term_is_localized() {
    let meta = preview_meta("---\ntitle: T\nlang: fr\n---\n\nBody.\n");
    assert_eq!(skip_to_content_term(&meta), "Aller au contenu principal");
}
