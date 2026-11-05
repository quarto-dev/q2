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
//! the link to that section can be copied. It has three parts, and this
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
//! `PipelineProfile::HtmlRender` and the `html` format only. The preview
//! renders the AST through React and excludes this transform; revealjs has
//! its own heading chrome; a theme-less page is the unsupported unthemed
//! path above.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope};
use crate::format::{FormatIdentifier, PipelineProfile, is_minimal_html};
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
        if ctx.pipeline_profile != PipelineProfile::HtmlRender
            || ctx.format.identifier != FormatIdentifier::Html
            || is_minimal_html(&ast.meta)
            || !anchors_enabled(&ast.meta)
        {
            return Ok(());
        }

        mark_blocks(&mut ast.blocks);

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

/// Walk every container that can hold a heading, figure or table wrapper
/// (the same set `MermaidRenderTransform` descends into).
fn mark_blocks(blocks: &mut Blocks) {
    for block in blocks.iter_mut() {
        match block {
            Block::Header(h) if (2..=6).contains(&h.level) => mark(&mut h.attr.1),
            Block::Div(div) => {
                if is_anchorable_div(&div.attr.0, &div.attr.1) {
                    mark(&mut div.attr.1);
                }
                mark_blocks(&mut div.content);
            }
            Block::BlockQuote(bq) => mark_blocks(&mut bq.content),
            Block::Figure(fig) => mark_blocks(&mut fig.content),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    mark_blocks(item);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    mark_blocks(item);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        mark_blocks(def);
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

    #[test]
    fn vendored_runtime_is_anchorjs_5() {
        let head = std::str::from_utf8(&ANCHOR_JS[..200]).unwrap();
        assert!(head.contains("AnchorJS - v5.0.0"), "{head}");
    }
}
