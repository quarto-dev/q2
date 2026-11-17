/*
 * tests/integration/image_dimensions_pipeline.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Integration tests for bd-html-image-dims-t5ogxjxd: image `width` /
 * `height` reach the HTML as valid markup — `N`/`Npx` as an integer
 * attribute, everything else in `style`.
 */

//! End-to-end tests for the `image-dimensions` pass.
//!
//! The per-value rules are unit-tested in
//! `crates/quarto-core/src/transforms/image_dimensions.rs`. These prove
//! the pass is wired into the pipeline the `q2` binary runs, for each
//! HTML-family format, and that it sits where the design says: after
//! `responsive-image` (which reads the unlifted `height`) and before
//! Lua post filters (which see the lifted form).

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Render `qmd` to `format`, with `extra` files beside it, and return
/// the emitted HTML.
fn render_with(qmd: &str, format: &str, extra: &[(&str, &str)]) -> String {
    let temp = TempDir::new().unwrap();
    for (name, contents) in extra {
        write_file(&temp.path().join(name), contents);
    }
    for name in ["a.png", "b.png"] {
        write_file(&temp.path().join(name), "not-really-a-png");
    }
    let qmd_path = temp.path().join("doc.qmd");
    write_file(&qmd_path, qmd);
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&qmd_path, format, &RenderToFileOptions::default(), runtime)
        .expect("render");
    std::fs::read_to_string(&result.output_path).expect("read output")
}

/// The `<img …>` tag whose `src` is `src`.
fn img_tag(html: &str, src: &str) -> String {
    let needle = format!("src=\"{src}\"");
    let mut search = html;
    while let Some(start) = search.find("<img ") {
        let after = &search[start..];
        let end = after.find('>').expect("malformed <img>: no closing '>'");
        let tag = &after[..=end];
        if tag.contains(&needle) {
            return tag.to_string();
        }
        search = &after[end..];
    }
    panic!("no <img> with {needle} in:\n{html}");
}

/// The two images from the field report.
const REPORT_DOC: &str = "---\ntitle: T\n---\n\n\
     ![](a.png){width=60%}\n\n\
     ![](b.png){width=750px}\n";

fn assert_report_case_lifted(html: &str, format: &str) {
    let a = img_tag(html, "a.png");
    assert!(
        a.contains("style=\"width:60%\""),
        "{format}: a percentage must move into style; got {a}"
    );
    assert!(
        !a.contains(" width="),
        "{format}: no width attribute may remain; got {a}"
    );
    let b = img_tag(html, "b.png");
    assert!(
        b.contains("width=\"750\""),
        "{format}: px must become a bare integer attribute; got {b}"
    );
    assert!(!b.contains("750px"), "{format}: got {b}");
}

/// The bug as reported, for plain HTML.
#[test]
fn html_lifts_percent_and_strips_px() {
    let html = render_with(REPORT_DOC, "html", &[]);
    assert_report_case_lifted(&html, "html");
}

/// revealjs is HTML-family too.
#[test]
fn revealjs_lifts_percent_and_strips_px() {
    let html = render_with(REPORT_DOC, "revealjs", &[]);
    assert_report_case_lifted(&html, "revealjs");
}

/// …and so are dashboards.
#[test]
fn dashboard_lifts_percent_and_strips_px() {
    let html = render_with(REPORT_DOC, "dashboard", &[]);
    assert_report_case_lifted(&html, "dashboard");
}

/// Values Pandoc cannot parse move into `style` as written instead of
/// being dropped, and lifted declarations follow the author's `style`.
#[test]
fn unparseable_values_and_existing_style_are_kept() {
    let html = render_with(
        "---\ntitle: T\n---\n\n\
         ![](a.png){width=50vw style=\"border: 1px solid;\"}\n\n\
         ![](b.png){width=\"calc(100% - 2em)\" height=3in}\n",
        "html",
        &[],
    );
    let a = img_tag(&html, "a.png");
    assert!(
        a.contains("style=\"border: 1px solid;width:50vw\""),
        "got {a}"
    );
    let b = img_tag(&html, "b.png");
    assert!(
        b.contains("style=\"width:calc(100% - 2em);height:3in\""),
        "got {b}"
    );
}

/// `responsive-image` skips an image with an explicit height. It must
/// still see that height after this pass moved it into `style`, so the
/// lifting has to run after it.
#[test]
fn lifted_height_still_opts_out_of_img_fluid() {
    let html = render_with(
        "---\ntitle: T\n---\n\n![](a.png){height=50%}\n\n![](b.png){width=50%}\n",
        "html",
        &[],
    );
    let a = img_tag(&html, "a.png");
    assert!(a.contains("style=\"height:50%\""), "got {a}");
    assert!(
        !a.contains("img-fluid"),
        "height-sized image got tagged: {a}"
    );
    let b = img_tag(&html, "b.png");
    assert!(
        b.contains("img-fluid"),
        "width-sized image must be tagged: {b}"
    );
}

/// Records what the filter saw as classes on the image.
const WITNESS_LUA: &str = r#"
function Image(el)
  if el.attributes["width"] == "60%" then
    el.classes:insert("saw-raw-width")
  end
  if el.attributes["style"] == "width:60%" then
    el.classes:insert("saw-lifted-style")
  end
  return el
end
"#;

/// A post filter (after the `quarto` sentinel) sees the lifted form; a
/// pre filter sees what the author wrote.
#[test]
fn post_filters_see_lifted_form_and_pre_filters_see_raw() {
    let doc = |filters: &str| {
        format!("---\ntitle: T\nfilters:\n{filters}---\n\n![](a.png){{width=60%}}\n")
    };
    let post = render_with(
        &doc("  - quarto\n  - witness.lua\n"),
        "html",
        &[("witness.lua", WITNESS_LUA)],
    );
    let tag = img_tag(&post, "a.png");
    assert!(tag.contains("saw-lifted-style"), "post filter: {tag}");
    assert!(!tag.contains("saw-raw-width"), "post filter: {tag}");

    let pre = render_with(
        &doc("  - witness.lua\n"),
        "html",
        &[("witness.lua", WITNESS_LUA)],
    );
    let tag = img_tag(&pre, "a.png");
    assert!(tag.contains("saw-raw-width"), "pre filter: {tag}");
    assert!(!tag.contains("saw-lifted-style"), "pre filter: {tag}");
}

const RESTYLE_LUA: &str = r#"
function Image(el)
  el.attributes["style"] = "width:25%"
  return el
end
"#;

/// A post filter can rewrite the lifted value, and the writer prints
/// what the filter left.
#[test]
fn post_filter_can_rewrite_lifted_style() {
    let html = render_with(
        "---\ntitle: T\nfilters:\n  - quarto\n  - restyle.lua\n---\n\n![](a.png){width=60%}\n",
        "html",
        &[("restyle.lua", RESTYLE_LUA)],
    );
    let tag = img_tag(&html, "a.png");
    assert!(tag.contains("style=\"width:25%\""), "got {tag}");
}
