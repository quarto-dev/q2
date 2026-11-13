/*
 * tests/integration/fig_alt.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-ykjwiqcc / GH #877: `fig-alt` (and a bare `alt` key-value) reach the
 * image's alt text in every output: HTML/revealjs `alt=`, docx `descr`,
 * typst `image(alt:)`, and the q2-preview AST.
 */

//! End-to-end tests for `image-alt-normalize`.
//!
//! One fixture, one image per case, each with its own `src` so an
//! assertion can name the case it is about. The per-node rules are
//! unit-tested in `crates/quarto-core/src/transforms/image_alt_normalize.rs`;
//! these prove the transform is wired into every pipeline and that each
//! writer reads the canonical shape it produces.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::pipeline::render_qmd_to_preview_ast;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Cases A–J of the GH #877 reproduction (A–E from the issue, F–J added
/// in the plan). The `src` letter names the case.
const FIXTURE: &str = r#"---
title: fig-alt forms
---

## A fig-alt on a bare image

![](a.png){fig-alt="Alt A"}

## B fig-alt with a caption

![Caption B](b.png){fig-alt="Alt B"}

## C caption only

![Alt C](c.png)

## D fig-alt on an inline image

Text ![](d.png){fig-alt="Alt D"} text.

## E fig-alt with a class

![](e.png){.mb-4 fig-alt="Alt E"}

## F crossref figure with fig-alt

![Caption F](f.png){#fig-f fig-alt="Alt F"}

## G id, no caption

![](g.png){#fig-g fig-alt="Alt G"}

## H alt key-value

![](h.png){alt="KV H"}

## I empty fig-alt

![Cap I](i.png){fig-alt=""}

## J div float with fig-alt

::: {#fig-j fig-alt="Alt J"}
![](j.png)

Caption J
:::
"#;

const CASES: &[&str] = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];

/// A minimal valid 1x1 PNG, so Pandoc's writers embed every image.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

fn runtime() -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::new())
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A temp project holding `source` as `doc.qmd` plus every case's image.
fn project(source: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    write(&dir.join("doc.qmd"), source.as_bytes());
    for case in CASES {
        write(&dir.join(format!("{case}.png")), ONE_PIXEL_PNG);
    }
    (temp, dir)
}

fn render_native(source: &str, format: &str, extra: &[(&str, &str)]) -> String {
    let (_temp, dir) = project(source);
    for (name, body) in extra {
        write(&dir.join(name), body.as_bytes());
    }
    let result = render_to_file(
        &dir.join("doc.qmd"),
        format,
        &RenderToFileOptions::default(),
        runtime(),
    )
    .expect("single-doc render");
    std::fs::read_to_string(&result.output_path).expect("read rendered output")
}

/// The cases whose alt text differs from any caption (C's alt *is* its
/// caption, I's is empty). Their alt text must never become visible
/// content — e.g. a bare image turned into a figure captioned with its
/// alt, which is what the vendored Q1 filters do to `Para [Image "alt"]`.
const DISTINCT_ALTS: &[&str] = &[
    "Alt A", "Alt B", "Alt D", "Alt E", "Alt F", "Alt G", "KV H", "Alt J",
];

/// Every occurrence of each distinct alt text in `output` is inside
/// `slot` (e.g. `alt="{}"`), i.e. none is rendered as text.
fn assert_alt_only_in_slot(output: &str, slot: &str, label: &str) {
    for alt in DISTINCT_ALTS {
        let anywhere = output.matches(alt).count();
        let in_slot = output.matches(&slot.replace("{}", alt)).count();
        assert_eq!(
            anywhere, in_slot,
            "{label}: {alt:?} appears {anywhere} time(s) but only {in_slot} as `{slot}`; \
             alt text leaked into visible content in:\n{output}"
        );
    }
}

// ── HTML / revealjs ───────────────────────────────────────────────────────

/// Every body `<img …>` tag, keyed by case letter (`src="<case>.png"`).
fn img_tags_by_case(html: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = html;
    while let Some(start) = rest.find("<img ") {
        let after = &rest[start..];
        let end = after.find('>').expect("unterminated <img>");
        let tag = &after[..=end];
        for case in CASES {
            if tag.contains(&format!("src=\"{case}.png\"")) {
                out.insert(case.to_string(), tag.to_string());
            }
        }
        rest = &after[end..];
    }
    out
}

/// Every value of the ` alt="…"` attribute in `tag` (an attribute, not
/// the tail of `data-fig-alt`).
fn alt_values(tag: &str) -> Vec<String> {
    tag.match_indices(" alt=\"")
        .map(|(i, m)| {
            let value = &tag[i + m.len()..];
            value[..value.find('"').unwrap()].to_string()
        })
        .collect()
}

/// The expected `alt` per case, for HTML-family output.
fn expected_html_alt(case: &str) -> Option<&'static str> {
    match case {
        "a" => Some("Alt A"),
        "b" => Some("Alt B"),
        "c" => Some("Alt C"),
        "d" => Some("Alt D"),
        "e" => Some("Alt E"),
        "f" => Some("Alt F"),
        "g" => Some("Alt G"),
        "h" => Some("KV H"),
        "i" => Some(""),
        "j" => Some("Alt J"),
        _ => unreachable!(),
    }
}

fn assert_html_alts(html: &str, label: &str) {
    let tags = img_tags_by_case(html);
    for case in CASES {
        let tag = tags
            .get(*case)
            .unwrap_or_else(|| panic!("{label}: no <img> for case {case} in:\n{html}"));
        assert!(
            !tag.contains("data-fig-alt") && !tag.contains("data-alt"),
            "{label} case {case}: fig-alt must not be parked in a data- attribute: {tag}"
        );
        let alts = alt_values(tag);
        let expected: Vec<String> = expected_html_alt(case)
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            alts, expected,
            "{label} case {case}: wrong alt attribute(s): {tag}"
        );
    }
}

#[test]
fn html_alt_text_comes_from_fig_alt() {
    let html = render_native(FIXTURE, "html", &[]);
    assert_html_alts(&html, "html");
    assert_alt_only_in_slot(&html, "alt=\"{}\"", "html");
}

#[test]
fn revealjs_alt_text_comes_from_fig_alt() {
    assert_html_alts(&render_native(FIXTURE, "revealjs", &[]), "revealjs");
}

// ── Ordering: user filters on either side of the transform ────────────────

/// Decision 3 of the plan: the transform runs inside the Quarto
/// transforms, so a user pre-filter sees the authored `fig-alt` and a
/// user post-filter sees the alt inlines instead.
#[test]
fn pre_filters_see_fig_alt_and_post_filters_see_alt_inlines() {
    let pre = r#"
function Image(img)
  img.attributes["seen-pre"] = img.attributes["fig-alt"] or "none"
  return img
end
"#;
    let post = r#"
function Image(img)
  img.attributes["seen-post"] =
    (img.attributes["fig-alt"] or "none") .. "|" .. pandoc.utils.stringify(img.caption)
  return img
end
"#;
    let source = "---\ntitle: T\nfilters:\n  - pre.lua\n  - quarto\n  - post.lua\n---\n\n\
                  ![Caption B](b.png){fig-alt=\"Alt B\"}\n";
    let html = render_native(source, "html", &[("pre.lua", pre), ("post.lua", post)]);
    let tag = img_tags_by_case(&html).remove("b").expect("case b <img>");
    assert!(
        tag.contains("data-seen-pre=\"Alt B\""),
        "a pre-filter must see the authored fig-alt: {tag}"
    );
    assert!(
        tag.contains("data-seen-post=\"none|Alt B\""),
        "a post-filter must see fig-alt folded into the alt inlines: {tag}"
    );
}

// ── docx ──────────────────────────────────────────────────────────────────

fn zip_member_text(bytes: &[u8], member: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a ZIP archive");
    let mut file = zip.by_name(member).expect("member exists");
    let mut text = String::new();
    file.read_to_string(&mut text).expect("UTF-8 member");
    text
}

/// `descr` of every `<wp:docPr …>` in document order (one per image;
/// `pic:cNvPr` carries the file name instead and is skipped).
fn docx_descriptions(document_xml: &str) -> Vec<String> {
    document_xml
        .match_indices("<wp:docPr ")
        .map(|(i, _)| {
            let tag = &document_xml[i..i + document_xml[i..].find('>').unwrap()];
            let start = tag.find("descr=\"").map(|j| j + "descr=\"".len());
            start
                .map(|s| tag[s..s + tag[s..].find('"').unwrap()].to_string())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn docx_descr_comes_from_fig_alt() {
    let (_temp, dir) = project(FIXTURE);
    let result = render_document_to_file(
        &dir.join("doc.qmd"),
        "docx",
        &RenderToFileOptions::default(),
        None,
        runtime(),
        None,
        None,
        None,
    )
    .expect("docx render");
    let bytes = std::fs::read(&result.output_path).unwrap();
    let document_xml = zip_member_text(&bytes, "word/document.xml");
    assert_alt_only_in_slot(&document_xml, "descr=\"{}\"", "docx");
    let descr = docx_descriptions(&document_xml);
    // Pandoc's docx writer does not de-duplicate a caption-valued alt
    // (case C), and Quarto 1's docx output matches this list exactly.
    assert_eq!(
        descr,
        vec![
            "Alt A", "Alt B", "Alt C", "Alt D", "Alt E", "Alt F", "Alt G", "KV H", "", "Alt J"
        ]
    );
}

// ── typst ─────────────────────────────────────────────────────────────────

/// The `alt:` argument of each case's `image("<case>.png", …)` call.
fn typst_alts(typ: &str) -> BTreeMap<String, Option<String>> {
    let mut out = BTreeMap::new();
    for case in CASES {
        let needle = format!("image(\"{case}.png\"");
        let Some(i) = typ.find(&needle) else { continue };
        let call = &typ[i..i + typ[i..].find(')').unwrap()];
        let alt = call.find("alt: \"").map(|j| {
            let v = &call[j + "alt: \"".len()..];
            v[..v.find('"').unwrap()].to_string()
        });
        out.insert(case.to_string(), alt);
    }
    out
}

/// Typst matched Quarto 1 before the fix (the vendored `typst.lua` reads
/// `fig-alt` itself); this pins that folding `fig-alt` into the alt
/// inlines keeps it so, and that the div float's `fig-alt` (J) arrives.
#[test]
fn typst_image_alt_comes_from_fig_alt() {
    let (_temp, dir) = project(FIXTURE);
    let output_path = dir.join("doc.typ");
    render_document_to_file(
        &dir.join("doc.qmd"),
        "typst",
        &RenderToFileOptions {
            output_path: Some(output_path.clone()),
            ..Default::default()
        },
        None,
        runtime(),
        None,
        None,
        None,
    )
    .expect("typst render");
    let typ = std::fs::read_to_string(&output_path).unwrap();
    assert_alt_only_in_slot(&typ, "alt: \"{}\"", "typst");
    let alts = typst_alts(&typ);
    let s = |v: &str| Some(v.to_string());
    let expected: BTreeMap<String, Option<String>> = [
        ("a", s("Alt A")),
        ("b", s("Alt B")),
        ("c", None),
        ("d", s("Alt D")),
        ("e", s("Alt E")),
        ("f", s("Alt F")),
        ("g", s("Alt G")),
        ("h", s("KV H")),
        ("i", None),
        ("j", s("Alt J")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(alts, expected, "typst image() alt arguments in:\n{typ}");
}

// ── q2-preview AST ────────────────────────────────────────────────────────

/// Every `Image` node in a Pandoc-JSON value: (src, alt plain text, kv keys).
fn json_images(v: &serde_json::Value, out: &mut Vec<(String, String, Vec<String>)>) {
    match v {
        serde_json::Value::Object(map) => {
            if map.get("t").and_then(|t| t.as_str()) == Some("Image") {
                let c = &map["c"];
                let keys = c[0][2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|kv| kv[0].as_str().unwrap().to_string())
                    .collect();
                let mut alt = String::new();
                json_plain_text(&c[1], &mut alt);
                out.push((c[2][0].as_str().unwrap().to_string(), alt, keys));
            }
            for value in map.values() {
                json_images(value, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| json_images(i, out)),
        _ => {}
    }
}

fn json_plain_text(v: &serde_json::Value, out: &mut String) {
    for inline in v.as_array().unwrap() {
        match inline["t"].as_str() {
            Some("Str") => out.push_str(inline["c"].as_str().unwrap()),
            Some("Space" | "SoftBreak") => out.push(' '),
            _ => {}
        }
    }
}

#[test]
fn preview_ast_carries_fig_alt_as_alt_inlines() {
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
    let output = pollster::block_on(render_qmd_to_preview_ast(
        FIXTURE.as_bytes(),
        "test.qmd",
        &mut ctx,
        runtime(),
        None,
        vec![].into(),
    ))
    .expect("preview render");
    let ast: serde_json::Value = serde_json::from_str(&output.ast_json).expect("AST JSON");
    let mut images = Vec::new();
    json_images(&ast, &mut images);
    let by_src: BTreeMap<String, (String, Vec<String>)> = images
        .into_iter()
        .map(|(src, alt, keys)| (src, (alt, keys)))
        .collect();
    for case in CASES {
        let (alt, keys) = by_src
            .get(&format!("{case}.png"))
            .unwrap_or_else(|| panic!("no Image for case {case}"));
        assert_eq!(
            Some(alt.as_str()),
            expected_html_alt(case),
            "preview case {case}: alt inlines"
        );
        assert!(
            !keys.iter().any(|k| k == "fig-alt" || k == "alt"),
            "preview case {case}: alt keys must be consumed, got {keys:?}"
        );
    }
}

// ── D5: a figcaption that only repeats the alt text ───────────────────────

/// The `<figcaption …>` open tag following the `<img>` for `src`.
fn figcaption_after(html: &str, src: &str) -> String {
    let img = html
        .find(&format!("src=\"{src}\""))
        .unwrap_or_else(|| panic!("no <img src={src:?}> in:\n{html}"));
    let start = img
        + html[img..]
            .find("<figcaption")
            .unwrap_or_else(|| panic!("no figcaption after {src}"));
    html[start..start + html[start..].find('>').unwrap() + 1].to_string()
}

/// Pandoc's rule (plan D5): the alt is kept and a figcaption that repeats
/// it is hidden from assistive tech, so the text is announced once. A
/// crossref figcaption carries its "Figure 1:" prefix, so it never repeats
/// the alt and the figure number stays announced.
fn assert_figcaption_visibility(html: &str, label: &str) {
    for (src, hidden) in [("c.png", true), ("b.png", false), ("k.png", false)] {
        let tag = figcaption_after(html, src);
        assert_eq!(
            tag.contains("aria-hidden=\"true\""),
            hidden,
            "{label} {src}: figcaption {tag}"
        );
    }
    let tags = img_tags_by_case(html);
    assert_eq!(
        alt_values(&tags["c"]),
        vec!["Alt C".to_string()],
        "{label}: C keeps its alt"
    );
}

const D5_FIXTURE: &str = "---\ntitle: T\n---\n\n\
                          ![Alt C](c.png)\n\n\
                          ![Caption B](b.png){fig-alt=\"Alt B\"}\n\n\
                          ![Cap K](k.png){#fig-k}\n";

#[test]
fn html_hides_a_figcaption_that_repeats_the_alt() {
    let html = render_native(D5_FIXTURE, "html", &[]);
    assert_figcaption_visibility(&html, "html");
}

#[test]
fn revealjs_hides_a_figcaption_that_repeats_the_alt() {
    let html = render_native(D5_FIXTURE, "revealjs", &[]);
    assert_figcaption_visibility(&html, "revealjs");
}
