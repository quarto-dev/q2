/*
 * transforms/anchor_sections.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Hover anchor links on headings, figures and tables (Quarto 1's
 * `anchor-sections`).
 */

//! Hover anchor links on headings, figures and tables (bd-5kf2dnw4,
//! GH #1058).
//!
//! Quarto 1 shows a link icon when the reader hovers a section heading, so
//! the link to that section can be copied. It has four parts, and this
//! transform supplies all of them for Bootstrap-themed HTML pages:
//!
//! 1. **Mark.** Add the class `anchored` to every `h2`–`h6`, every
//!    `.quarto-figure[id]` and every `div[id^=tbl-]` (Q1
//!    `format-html.ts:828-848`), except anything carrying `.no-anchor`. The
//!    class is purely a selector hook: there is no `.anchored` CSS rule.
//!    `h1` is never marked, and the TOC title (`#toc-title`) is emitted by
//!    the template rather than found in the AST, so it never meets this
//!    walk.
//! 2. **Ship the runtime.** AnchorJS (`resources/js/anchor/anchor.min.js`,
//!    byte-identical to Q1's copy) as the Project-scoped `js:anchor`
//!    artifact; `ApplyTemplateStage` turns it into a `<script src>` in the
//!    head, as Q1 does.
//! 3. **Initialise it.** A small inline script in `after-body` calls
//!    `new AnchorJS().add('.anchored')` on `DOMContentLoaded`, with the
//!    options Q1 uses (`placement: 'right'`, default icon).
//! 4. **Name the target** (bd-d5gss9pz). sectionize moves a heading's id
//!    onto its `<section>`, as Pandoc's `--section-divs` does. AnchorJS
//!    reads the anchor from the heading's `id`, then its
//!    `data-anchor-id`; with neither, it urlifies the heading text and
//!    de-duplicates it against the ids already on the page. So `## Overview`
//!    linked to `#overview-1`, a fragment that exists only after AnchorJS
//!    runs and that disagrees with the TOC. Q1's Bootstrap postprocessor
//!    (`format-html-bootstrap.ts:534-544`) copies each section's id back
//!    onto its heading as `data-anchor-id`; so does this transform. Q1
//!    takes the first `h2`…`h6` of each `section[class^="level"]` in
//!    document order, letting nested sections overwrite, which leaves each
//!    section's own heading carrying its id. sectionize puts that heading
//!    first, so here only a section's leading `Header` (levels 2–6) is
//!    written, with no overwrite. Like Q1, this ignores `anchor-sections:
//!    false` and `.no-anchor` (the attribute is inert without AnchorJS);
//!    unlike Q1, an empty section id writes nothing rather than `""`.
//!    Also unlike Q1, any `section`-class Div qualifies, not only
//!    `level*` ones: Q1's `[class^="level"]` misses appendix sections such
//!    as `section#footnotes.footnotes`, so its Footnotes heading anchors
//!    to an invented `#footnotes-1`. Here it anchors to `#footnotes`.
//!    `quarto-toc.js` also reads the attribute, to reveal a heading's link
//!    when its TOC entry is clicked.
//!
//! The `.anchorjs-link` styling lives in
//! `resources/scss/bootstrap/_bootstrap-rules.scss`.
//!
//! ## The `anchor-sections` option
//!
//! `anchor-sections: false` turns the feature off; any other value (or none)
//! leaves it on. That is what Q1 does for every themed page
//! (`options.anchors = metadata["anchor-sections"] !== false`). Q1 also
//! reads a string value as a custom icon glyph, but only on the unthemed
//! (`minimal`) path, where the feature is off unless asked for; on the
//! default-on path the string is overwritten by `true`. q2 does not support
//! the unthemed path yet, so a string simply means "on" here.
//!
//! ## Idempotence
//!
//! Q1's `classList.add` is idempotent; pushing onto a `Vec<String>` is not.
//! `AppendixStructureTransform` already builds its headings with `anchored`
//! (Q1's `headingClasses`), so this transform skips anything that has the
//! class. It therefore has to run *after* the appendix transform (those
//! headings do not exist earlier) and after `CrossrefRenderTransform` (the
//! figure and table wrappers do not exist earlier either).
//!
//! ## Scope
//!
//! Bootstrap-themed `HtmlRender` and `HtmlPreview` pipelines. Both get the
//! `anchored` class (the preview's `<main>` DOM must match the render's);
//! only the render ships the script and init. Revealjs has its own heading
//! chrome; a theme-less page is the unsupported unthemed path above.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope};
use crate::format::{PipelineProfile, is_minimal_html};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// The vendored AnchorJS 5.0.0. Version contract: `resources/js/README.md`
/// § `anchor/`.
const ANCHOR_JS: &[u8] = include_bytes!("../../../../resources/js/anchor/anchor.min.js");

/// Artifact key. The `js:` prefix is deliberate: `ApplyTemplateStage`
/// collects it and emits the `<script src>` in the head, which runs before
/// the after-body init below.
const ANCHOR_KEY: &str = "js:anchor";

/// Destination under the project lib dir (`site_libs/anchor/anchor.min.js`).
const ANCHOR_REL_PATH: &str = "anchor/anchor.min.js";

/// The class AnchorJS is told to decorate.
const ANCHORED: &str = "anchored";

/// Opt-out class (Q1: `.no-anchor`).
const NO_ANCHOR: &str = "no-anchor";

/// The attribute AnchorJS reads a heading's anchor from when the heading
/// has no `id` (sectionize moved it to the `<section>`). The HTML writer
/// emits `data-*` keys verbatim.
const ANCHOR_ID_ATTR: &str = "data-anchor-id";

/// Sentinel making a re-run append the init only once.
const INIT_SENTINEL: &str = "<!-- quarto-anchor-sections -->";

/// Q1's init (`quarto-html-after-body.ejs:73-82`) with `icon` empty, which
/// is what Q1 renders for `anchors === true`.
const INIT_SCRIPT: &str = "<!-- quarto-anchor-sections -->\n\
<script>\n\
window.document.addEventListener(\"DOMContentLoaded\", function (event) {\n\
  const anchorJS = new window.AnchorJS();\n\
  anchorJS.options = {\n\
    placement: 'right',\n\
    icon: ''\n\
  };\n\
  anchorJS.add('.anchored');\n\
});\n\
</script>";

/// See module docs.
pub struct AnchorSectionsTransform;

impl AnchorSectionsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AnchorSectionsTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// Is the feature on for this document? `anchor-sections: false` is the only
/// way off (Q1: `!== false`).
fn anchors_enabled(meta: &quarto_pandoc_types::ConfigValue) -> bool {
    meta.get("anchor-sections").and_then(|v| v.as_bool()) != Some(false)
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AnchorSectionsTransform {
    fn name(&self) -> &str {
        "anchor-sections"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let render = ctx.pipeline_profile == PipelineProfile::HtmlRender;
        let preview = ctx.pipeline_profile == PipelineProfile::HtmlPreview;
        if !(render || preview) || is_minimal_html(&ast.meta) {
            return Ok(());
        }

        // Q1 writes `data-anchor-id` on every Bootstrap page, before and
        // independent of the `anchor-sections` switch (module docs, item 4).
        walk_blocks(&mut ast.blocks, &mut anchor_section_heading);

        if !anchors_enabled(&ast.meta) {
            return Ok(());
        }

        walk_blocks(&mut ast.blocks, &mut mark_block);

        // The runtime is render-only: the preview has no `ApplyTemplateStage`
        // to turn the artifact into a `<script>`, and renders through React.
        // It still gets the class, so the preview's `<main>` DOM matches the
        // render's (the smoke-all parity tests compare them); the hover
        // link itself is bd-c0erkhqk.
        if render {
            ctx.artifacts.store(
                ANCHOR_KEY,
                Artifact::from_bytes(ANCHOR_JS.to_vec(), "text/javascript")
                    .with_path(ANCHOR_REL_PATH)
                    .with_scope(ArtifactScope::Project),
            );
            append_with_sentinel(
                &mut ast.meta,
                "after-body",
                INIT_SENTINEL,
                INIT_SCRIPT.to_string(),
            );
        }
        Ok(())
    }
}

/// Add `anchored` to `classes` unless it is already there or the element
/// opted out.
fn mark(classes: &mut Vec<String>) {
    if classes.iter().any(|c| c == NO_ANCHOR || c == ANCHORED) {
        return;
    }
    classes.push(ANCHORED.to_string());
}

fn is_anchorable_div(id: &str, classes: &[String]) -> bool {
    !id.is_empty() && (classes.iter().any(|c| c == "quarto-figure") || id.starts_with("tbl-"))
}

/// Mark one block (not its children) per item 1 of the module docs.
fn mark_block(block: &mut Block) {
    match block {
        Block::Header(h) if (2..=6).contains(&h.level) => mark(&mut h.attr.1),
        Block::Div(div) if is_anchorable_div(&div.attr.0, &div.attr.1) => mark(&mut div.attr.1),
        _ => {}
    }
}

/// Give a sectionize section's own heading the section's id as
/// `data-anchor-id` (item 4 of the module docs). sectionize puts that
/// heading first, so only the leading block is considered.
fn anchor_section_heading(block: &mut Block) {
    let Block::Div(div) = block else { return };
    let (id, classes, _) = &div.attr;
    if id.is_empty() || !classes.iter().any(|c| c == "section") {
        return;
    }
    let id = id.clone();
    if let Some(Block::Header(h)) = div.content.first_mut()
        && (2..=6).contains(&h.level)
    {
        h.attr.2.insert(ANCHOR_ID_ATTR.to_string(), id);
    }
}

/// Apply `f` to every block, pre-order, descending into every container
/// that can hold a heading, figure or table wrapper (the same set
/// `MermaidRenderTransform` descends into).
fn walk_blocks(blocks: &mut Blocks, f: &mut impl FnMut(&mut Block)) {
    for block in blocks.iter_mut() {
        f(block);
        match block {
            Block::Div(div) => walk_blocks(&mut div.content, f),
            Block::BlockQuote(bq) => walk_blocks(&mut bq.content, f),
            Block::Figure(fig) => walk_blocks(&mut fig.content, f),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks(item, f);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks(item, f);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        walk_blocks(def, f);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use hashlink::LinkedHashMap;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Div, Header};
    use quarto_source_map::SourceInfo;

    fn header(level: usize, id: &str, classes: &[&str]) -> Block {
        Block::Header(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|c| c.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content: vec![],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn div(id: &str, classes: &[&str], content: Blocks) -> Block {
        Block::Div(Div {
            attr: (
                id.to_string(),
                classes.iter().map(|c| c.to_string()).collect(),
                LinkedHashMap::new(),
            ),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn classes(b: &Block) -> Vec<String> {
        match b {
            Block::Header(h) => h.attr.1.clone(),
            Block::Div(d) => d.attr.1.clone(),
            other => panic!("unexpected block {other:?}"),
        }
    }

    fn meta(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let mut m = ConfigValue::new_map(vec![], SourceInfo::for_test());
        for (k, v) in entries {
            m.insert_path(&[k], v);
        }
        m
    }

    fn project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        }
    }

    /// Run the transform; returns the context's artifact keys.
    async fn run(ast: &mut Pandoc, format: Format) -> Vec<String> {
        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        AnchorSectionsTransform::new()
            .transform(ast, &mut ctx)
            .await
            .unwrap();
        ctx.artifacts
            .keys()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
    }

    fn after_body(meta: &ConfigValue) -> Vec<String> {
        meta.get_path(&["rendered", "includes", "after-body"])
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn sample_blocks() -> Blocks {
        vec![
            header(1, "title", &[]),
            header(2, "a", &[]),
            header(3, "b", &["no-anchor"]),
            header(6, "c", &["keep"]),
            div(
                "fig-x",
                &["quarto-float", "quarto-figure"],
                vec![header(4, "nested", &[])],
            ),
            div("tbl-x", &["quarto-float"], vec![]),
            div("other", &["callout"], vec![]),
            div("", &["quarto-figure"], vec![]),
        ]
    }

    #[tokio::test]
    async fn marks_h2_to_h6_figures_and_tables() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: sample_blocks(),
        };
        run(&mut ast, Format::html()).await;
        let b = &ast.blocks;
        assert!(classes(&b[0]).is_empty(), "h1 is never marked");
        assert_eq!(classes(&b[1]), ["anchored"]);
        assert_eq!(classes(&b[2]), ["no-anchor"], "opt-out wins");
        assert_eq!(classes(&b[3]), ["keep", "anchored"]);
        assert_eq!(
            classes(&b[4]),
            ["quarto-float", "quarto-figure", "anchored"]
        );
        let Block::Div(fig) = &b[4] else { panic!() };
        assert_eq!(classes(&fig.content[0]), ["anchored"], "descends into divs");
        assert_eq!(classes(&b[5]), ["quarto-float", "anchored"]);
        assert_eq!(classes(&b[6]), ["callout"], "plain id div untouched");
        assert_eq!(
            classes(&b[7]),
            ["quarto-figure"],
            "figure without id untouched"
        );
    }

    #[tokio::test]
    async fn already_anchored_heading_is_not_doubled() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![header(2, "", &["anchored", "quarto-appendix-heading"])],
        };
        run(&mut ast, Format::html()).await;
        assert_eq!(
            classes(&ast.blocks[0]),
            ["anchored", "quarto-appendix-heading"]
        );
        // and a second run changes nothing
        run(&mut ast, Format::html()).await;
        assert_eq!(
            classes(&ast.blocks[0]),
            ["anchored", "quarto-appendix-heading"]
        );
    }

    #[tokio::test]
    async fn ships_runtime_and_init_once() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: sample_blocks(),
        };
        let keys = run(&mut ast, Format::html()).await;
        assert!(keys.iter().any(|k| k == "js:anchor"), "{keys:?}");
        run(&mut ast, Format::html()).await;
        let init = after_body(&ast.meta);
        assert_eq!(init.len(), 1, "init appended once: {init:?}");
        assert!(init[0].contains("anchorJS.add('.anchored')"));
        assert!(init[0].contains("placement: 'right'"));
    }

    #[tokio::test]
    async fn anchor_sections_false_disables_everything() {
        let mut ast = Pandoc {
            meta: meta(vec![(
                "anchor-sections",
                ConfigValue::new_bool(false, SourceInfo::for_test()),
            )]),
            blocks: sample_blocks(),
        };
        let keys = run(&mut ast, Format::html()).await;
        assert!(classes(&ast.blocks[1]).is_empty());
        assert!(!keys.iter().any(|k| k == "js:anchor"));
        assert!(after_body(&ast.meta).is_empty());
    }

    #[tokio::test]
    async fn string_value_means_on() {
        let mut ast = Pandoc {
            meta: meta(vec![(
                "anchor-sections",
                ConfigValue::new_string("#", SourceInfo::for_test()),
            )]),
            blocks: sample_blocks(),
        };
        run(&mut ast, Format::html()).await;
        assert_eq!(classes(&ast.blocks[1]), ["anchored"]);
    }

    /// The preview gets the class (DOM parity with the render) but no
    /// runtime and no init.
    #[tokio::test]
    async fn preview_marks_classes_but_ships_nothing() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: sample_blocks(),
        };
        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let format = Format::html();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.pipeline_profile = PipelineProfile::HtmlPreview;
        AnchorSectionsTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        assert_eq!(classes(&ast.blocks[1]), ["anchored"]);
        assert_eq!(ctx.artifacts.keys().count(), 0);
        assert!(after_body(&ast.meta).is_empty());
    }

    #[tokio::test]
    async fn minimal_html_is_untouched() {
        let mut ast = Pandoc {
            meta: meta(vec![(
                "minimal",
                ConfigValue::new_bool(true, SourceInfo::for_test()),
            )]),
            blocks: sample_blocks(),
        };
        let keys = run(&mut ast, Format::html()).await;
        assert!(classes(&ast.blocks[1]).is_empty());
        assert!(!keys.iter().any(|k| k == "js:anchor"));
    }

    #[tokio::test]
    async fn non_html_formats_are_untouched() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: sample_blocks(),
        };
        let keys = run(&mut ast, Format::pdf()).await;
        assert!(classes(&ast.blocks[1]).is_empty());
        assert!(keys.is_empty());
    }

    /// A sectionize-shaped section Div: `#id .section .levelN`, its own
    /// (id-less) header first, then `rest`.
    fn section(id: &str, level: usize, header_classes: &[&str], rest: Blocks) -> Block {
        let mut content = vec![header(level, "", header_classes)];
        content.extend(rest);
        div(id, &["section", &format!("level{level}")], content)
    }

    fn anchor_id(b: &Block) -> Option<String> {
        match b {
            Block::Header(h) => h.attr.2.get(ANCHOR_ID_ATTR).cloned(),
            other => panic!("expected a header, got {other:?}"),
        }
    }

    /// The `i`-th block of section Div `b`.
    fn child(b: &Block, i: usize) -> &Block {
        let Block::Div(d) = b else {
            panic!("expected a div, got {b:?}")
        };
        &d.content[i]
    }

    /// Each section's own heading carries the section id as
    /// `data-anchor-id` (Q1 format-html-bootstrap.ts:534-544), so AnchorJS
    /// links to `#overview` instead of inventing `#overview-1`.
    #[tokio::test]
    async fn section_headings_get_data_anchor_id() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![section(
                "overview",
                2,
                &[],
                vec![section("details", 3, &[], vec![])],
            )],
        };
        run(&mut ast, Format::html()).await;
        let outer = &ast.blocks[0];
        assert_eq!(anchor_id(child(outer, 0)).as_deref(), Some("overview"));
        let inner = child(outer, 1);
        assert_eq!(anchor_id(child(inner, 0)).as_deref(), Some("details"));
    }

    /// A level-1 section's `h1` is not an AnchorJS target and gets
    /// nothing; its nested `h2` gets its own section's id, not the h1's.
    #[tokio::test]
    async fn h1_section_is_skipped_and_nested_h2_keeps_its_own_id() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![section("top", 1, &[], vec![section("sub", 2, &[], vec![])])],
        };
        run(&mut ast, Format::html()).await;
        let top = &ast.blocks[0];
        assert_eq!(anchor_id(child(top, 0)), None);
        assert_eq!(anchor_id(child(child(top, 1), 0)).as_deref(), Some("sub"));
    }

    /// Q1 writes the attribute on every Bootstrap page, whatever
    /// `anchor-sections` or `.no-anchor` say.
    #[tokio::test]
    async fn data_anchor_id_ignores_no_anchor_and_anchor_sections_false() {
        let mut ast = Pandoc {
            meta: meta(vec![(
                "anchor-sections",
                ConfigValue::new_bool(false, SourceInfo::for_test()),
            )]),
            blocks: vec![section("quiet", 2, &["no-anchor"], vec![])],
        };
        run(&mut ast, Format::html()).await;
        let h = child(&ast.blocks[0], 0);
        assert_eq!(anchor_id(h).as_deref(), Some("quiet"));
        assert_eq!(classes(h), ["no-anchor"], "but no anchored class");
    }

    #[tokio::test]
    async fn data_anchor_id_skips_empty_ids_and_is_idempotent() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![section("", 2, &[], vec![]), section("a", 2, &[], vec![])],
        };
        run(&mut ast, Format::html()).await;
        run(&mut ast, Format::html()).await;
        assert_eq!(anchor_id(child(&ast.blocks[0], 0)), None);
        let Block::Header(h) = child(&ast.blocks[1], 0) else {
            panic!()
        };
        assert_eq!(h.attr.2.len(), 1, "{:?}", h.attr.2);
        assert_eq!(h.attr.2.get(ANCHOR_ID_ATTR).map(String::as_str), Some("a"));
    }

    /// Only the section's leading block counts: a section that does not
    /// open with its heading (not sectionize-shaped) is left alone.
    #[tokio::test]
    async fn data_anchor_id_only_on_leading_header() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![div(
                "s",
                &["section", "level2"],
                vec![div("", &["callout"], vec![]), header(2, "", &[])],
            )],
        };
        run(&mut ast, Format::html()).await;
        assert_eq!(anchor_id(child(&ast.blocks[0], 1)), None);
    }

    #[tokio::test]
    async fn data_anchor_id_not_written_for_pdf_or_minimal() {
        let mut ast = Pandoc {
            meta: meta(vec![]),
            blocks: vec![section("a", 2, &[], vec![])],
        };
        run(&mut ast, Format::pdf()).await;
        assert_eq!(anchor_id(child(&ast.blocks[0], 0)), None);

        let mut ast = Pandoc {
            meta: meta(vec![(
                "minimal",
                ConfigValue::new_bool(true, SourceInfo::for_test()),
            )]),
            blocks: vec![section("a", 2, &[], vec![])],
        };
        run(&mut ast, Format::html()).await;
        assert_eq!(anchor_id(child(&ast.blocks[0], 0)), None);
    }

    #[test]
    fn vendored_runtime_is_anchorjs_5() {
        let head = std::str::from_utf8(&ANCHOR_JS[..200]).unwrap();
        assert!(head.contains("AnchorJS - v5.0.0"), "{head}");
    }
}
