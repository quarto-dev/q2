/*
 * tests/integration/anchor_sections_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for heading anchor links (bd-5kf2dnw4, GH #1058):
 * the `anchored` class, the AnchorJS script and its init, and the
 * `anchor-sections` opt-out — through the real `ProjectPipeline`.
 */

//! Same harness shape as `toc_js_pipeline.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// The shipped file must be byte-identical to the resource.
const ANCHOR_JS: &[u8] = include_bytes!("../../../../resources/js/anchor/anchor.min.js");

fn canonical(path: &std::path::Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<(String, String)>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    std::mem::forget(temp);

    let site_root = project_dir.join("_site");
    let outputs = summary
        .outputs
        .iter()
        .map(|out| {
            let href = out
                .output_path
                .strip_prefix(&site_root)
                .unwrap_or(&out.output_path)
                .to_string_lossy()
                .replace('\\', "/");
            (href, std::fs::read_to_string(&out.output_path).unwrap())
        })
        .collect();
    (site_root, outputs)
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| panic!("no output for {href}"))
        .1
}

const BODY: &str = "\n## Events\n\nText.[^1]\n\n### Detail\n\nMore.\n\n###### Deep\n\nx\n\n\
    | a | b |\n|---|---|\n| 1 | 2 |\n\n: A table {#tbl-t}\n\n[^1]: A note.\n";

fn site(dir: &std::path::Path) {
    write(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nwebsite:\n  title: Site\n",
    );
    write(
        &dir.join("index.qmd"),
        &format!("---\ntitle: Home\n---\n{BODY}"),
    );
    write(
        &dir.join("off.qmd"),
        &format!("---\ntitle: Off\nanchor-sections: false\n---\n{BODY}"),
    );
}

#[test]
fn headings_get_anchored_and_the_runtime_ships() {
    let (site_root, outputs) = render_project(site);
    let index = find_html(&outputs, "index.html");

    for h in [
        "<h2 class=\"anchored\"",
        "<h3 class=\"anchored\"",
        "<h6 class=\"anchored\"",
    ] {
        assert!(index.contains(h), "missing {h} in:\n{index}");
    }
    assert!(
        !index.contains("<h1 class=\"anchored"),
        "the title h1 is never anchored:\n{index}"
    );
    assert!(
        index.contains("src=\"site_libs/anchor/anchor.min.js\""),
        "script tag missing:\n{index}"
    );
    assert!(index.contains("anchorJS.add('.anchored')"), "init missing");
    assert!(
        !index.contains("anchored anchored"),
        "class must not be doubled (appendix heading):\n{index}"
    );

    let shipped = std::fs::read(site_root.join("site_libs/anchor/anchor.min.js")).unwrap();
    assert_eq!(shipped, ANCHOR_JS, "shipped file must be the resource");
}

#[test]
fn table_float_wrapper_is_anchored() {
    let (_, outputs) = render_project(site);
    let index = find_html(&outputs, "index.html");
    let at = index.find("id=\"tbl-t\"").expect("tbl-t wrapper");
    let open = index[..at].rfind("<div").unwrap();
    let close = at + index[at..].find('>').unwrap();
    let tag = &index[open..close];
    assert!(tag.contains("anchored"), "tbl wrapper not anchored: {tag}");
}

#[test]
fn anchor_sections_false_turns_everything_off() {
    let (_, outputs) = render_project(site);
    let off = find_html(&outputs, "off.html");
    // The appendix heading is built with `anchored` regardless (as in Q1);
    // nothing else may be marked.
    assert_eq!(
        off.matches("anchored").count(),
        1,
        "only the appendix heading carries the class:\n{off}"
    );
    assert!(!off.contains("anchor.min.js"), "no script");
    assert!(!off.contains("AnchorJS"), "no init");
}
