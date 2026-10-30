/*
 * tests/integration/toc_js_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for the TOC's client side
 * (bd-toc-scrollspy-js-41e624d8): the scroll-tracking script shipped as
 * `js:quarto-toc`, the `toc-expand` attribute, the first-link `active`,
 * and `toc-component:` declaring a bound component over `nav#TOC` —
 * through the real `ProjectPipeline`.
 */

//! Same harness shape as `headroom_pipeline.rs`: write a small website
//! fixture to a temp dir, drive it through the real `ProjectPipeline`,
//! inspect the rendered HTML and the `site_libs/` payloads on disk. The
//! unit tests check each seam (the predicate, the render attributes, the
//! synthesized declaration); only a real render proves they meet: the
//! `<script>` tag in the page, the file on disk, the nav attribute in
//! the template output, the manifest entry the components transform
//! writes from the synthesized declaration.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// The shipped file must be byte-identical to the resource.
const QUARTO_TOC_JS: &str = include_str!("../../../../resources/js/quarto-toc/quarto-toc.js");

fn canonical(path: &std::path::Path) -> PathBuf {
    quarto_system_runtime::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Render a fixture project; return (site_root, [(href, html)]).
fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<(String, String)>) {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    assert!(
        !project.is_single_file,
        "test expected a multi-file project"
    );

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
    std::mem::forget(temp); // keep files alive for inspection

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
            (href, read(&out.output_path))
        })
        .collect();
    (site_root, outputs)
}

fn find_html<'a>(outputs: &'a [(String, String)], href: &str) -> &'a str {
    &outputs
        .iter()
        .find(|(h, _)| h == href)
        .unwrap_or_else(|| {
            panic!(
                "no output for href '{}'; got: {:?}",
                href,
                outputs.iter().map(|(h, _)| h).collect::<Vec<_>>()
            )
        })
        .1
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> &'a str {
    let a = s
        .find(start)
        .unwrap_or_else(|| panic!("missing {start:?} in:\n{s}"));
    let b = match s[a..].find(end) {
        Some(i) => a + i + end.len(),
        None => panic!("missing {end:?}"),
    };
    &s[a..b]
}

const SECTIONS: &str = "\n## Introduction\n\nText.\n\n### Why\n\nMore.\n\n## Methods\n\nText.\n";

/// A website with a TOC on every page; `index_front_matter` is spliced
/// into the index page's front matter (after `title`).
fn toc_site(project_dir: &std::path::Path, index_front_matter: &str) {
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\
         website:\n  title: Site\n\
         format:\n  html:\n    toc: true\n",
    );
    write(
        &project_dir.join("index.qmd"),
        &format!("---\ntitle: Home\n{index_front_matter}---\n{SECTIONS}"),
    );
    write(
        &project_dir.join("about.qmd"),
        &format!("---\ntitle: About\ntoc: false\n---\n{SECTIONS}"),
    );
}

const TOC_TSX: &str = r#"import type { QuartoComponentProps, QuartoTocProps } from "@quarto/components";

export default function Toc({ props, children }: QuartoComponentProps<QuartoTocProps>) {
  return (
    <div className="my-toc" data-entries={props.entries.length}>
      {children}
    </div>
  );
}
"#;

/// The default: a page with a TOC gets the script tag and the file; a
/// `toc: false` page on the same site gets no tag.
#[test]
fn toc_page_ships_the_script_and_marks_the_first_link_active() {
    let (site_root, outputs) = render_project(|dir| toc_site(dir, ""));
    let index = find_html(&outputs, "index.html");

    assert!(
        index.contains("<script src=\"site_libs/quarto-toc/quarto-toc.js\"></script>"),
        "index must link the TOC script; got:\n{index}"
    );
    let nav = between(index, "<nav id=\"TOC\"", "</nav>");
    assert!(
        nav.contains("<a href=\"#introduction\" class=\"nav-link active\" data-scroll-target=\"#introduction\">"),
        "first link active at render time: {nav}"
    );
    assert_eq!(
        nav.matches("nav-link active").count(),
        1,
        "exactly one active link: {nav}"
    );
    assert!(
        !nav.contains("data-toc-expanded"),
        "no toc-expand, no attribute: {nav}"
    );
    assert!(
        !nav.contains("data-quarto-component"),
        "no toc-component, no host attribute: {nav}"
    );

    let file = site_root.join("site_libs/quarto-toc/quarto-toc.js");
    assert!(file.exists(), "missing {}", file.display());
    assert_eq!(
        read(&file).replace("\r\n", "\n"),
        QUARTO_TOC_JS.replace("\r\n", "\n"),
        "shipped file must be the resource, byte for byte"
    );

    let about = find_html(&outputs, "about.html");
    assert!(
        !about.contains("quarto-toc.js"),
        "a toc: false page must not link the script; got:\n{about}"
    );
    assert!(!about.contains("<nav id=\"TOC\""), "{about}");
}

/// `toc-expand` lands on the top-level list as Q1's values.
#[test]
fn toc_expand_marks_the_top_level_list() {
    let (_, outputs) = render_project(|dir| toc_site(dir, "toc-expand: false\n"));
    let index = find_html(&outputs, "index.html");
    let nav = between(index, "<nav id=\"TOC\"", "</nav>");
    assert!(
        nav.contains("<ul data-toc-expanded=\"-1\">"),
        "toc-expand: false is -1: {nav}"
    );
    assert_eq!(nav.matches("data-toc-expanded").count(), 1, "{nav}");
    assert!(
        index.contains("quarto-toc.js"),
        "the script still ships: {index}"
    );
}

/// `toc-component:` binds the nav instead of shipping the script: the
/// nav carries the host attribute, the synthesized declaration reaches
/// the manifest with the structured TOC as props, and the `.tsx` module
/// is transpiled under `site_libs/`.
#[test]
fn toc_component_binds_the_nav_and_omits_the_script() {
    let (site_root, outputs) = render_project(|dir| {
        toc_site(dir, "toc-component: toc.tsx\n");
        write(&dir.join("toc.tsx"), TOC_TSX);
    });
    let index = find_html(&outputs, "index.html");

    assert!(
        !index.contains("quarto-toc.js"),
        "the default script must not ship with a toc-component; got:\n{index}"
    );
    assert!(
        index.contains(
            "<nav id=\"TOC\" role=\"doc-toc\" class=\"toc-active\" data-quarto-component=\"quarto-toc\">"
        ),
        "nav must carry the host attribute; got:\n{index}"
    );

    let head = between(index, "<head>", "</head>");
    let manifest = between(
        head,
        "<script type=\"application/json\" id=\"quarto-components-manifest\">",
        "</script>",
    );
    assert!(
        manifest.contains("\"quarto-toc\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"site_libs/quarto-components/toc.js\",\"props\":{"),
        "manifest entry: {manifest}"
    );
    for needle in [
        "\"entries\":[{",
        "\"id\":\"introduction\"",
        "\"title\":\"Introduction\"",
        "\"level\":2",
        "\"children\":[{\"children\":[],\"id\":\"why\"",
        "\"expanded\":null",
        "\"location\":\"right\"",
        "\"title\":\"On this page\"",
    ] {
        assert!(manifest.contains(needle), "{needle} in {manifest}");
    }
    assert!(
        head.contains("<script type=\"importmap\">"),
        "React import map ships: {head}"
    );
    assert!(
        site_root
            .join("site_libs/quarto-components/toc.js")
            .exists(),
        "the .tsx module is transpiled under site_libs"
    );
    assert!(
        site_root.join("site_libs/quarto-react/react.js").exists(),
        "vendored React ships"
    );

    // The other page has no TOC: no binding, no component assets linked.
    let about = find_html(&outputs, "about.html");
    assert!(
        !about.contains("quarto-components-manifest"),
        "toc-component is inert without a rendered TOC; got:\n{about}"
    );
}

/// `toc-expand` reaches the component as `props.expanded`.
#[test]
fn toc_component_props_carry_toc_expand() {
    let (_, outputs) = render_project(|dir| {
        toc_site(dir, "toc-component: toc.tsx\ntoc-expand: true\n");
        write(&dir.join("toc.tsx"), TOC_TSX);
    });
    let index = find_html(&outputs, "index.html");
    assert!(index.contains("\"expanded\":99"), "{index}");
}

/// The website-left regime builds the nav in Rust (`toc_block_html`),
/// not from the template partial: the attribute must land there too.
#[test]
fn toc_component_attribute_survives_the_sidebar_twin() {
    let (_, outputs) = render_project(|dir| {
        toc_site(dir, "toc-component: toc.tsx\ntoc-location: left\n");
        write(&dir.join("toc.tsx"), TOC_TSX);
    });
    let index = find_html(&outputs, "index.html");
    let sidebar = between(index, "<nav id=\"quarto-sidebar\"", "</nav>\n</nav>");
    assert!(
        sidebar.contains("data-quarto-component=\"quarto-toc\""),
        "the merged TOC must carry the host attribute; got:\n{sidebar}"
    );
    assert!(index.contains("\"location\":\"left\""), "{index}");
    assert!(!index.contains("quarto-toc.js"), "{index}");
}
