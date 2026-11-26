/*
 * transforms/cell_renderings.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Choose between a cell's light and dark outputs (`renderings: [light, dark]`).
//!
//! A cell with `#| renderings: [light, dark]` draws its output twice, and the
//! presenter forwards the option onto the `.cell` div as
//! `renderings='["light","dark"]'`. Each name in the list labels the
//! `cell-output-display` div at the same position. This is the native port of
//! Quarto 1's `quarto-post/cell-renderings.lua`:
//!
//! - **HTML and dashboard** keep both outputs and add `light-content` /
//!   `dark-content`, so the page's light/dark toggle shows only one
//!   (`_bootstrap-rules.scss`).
//! - **Reveal.js** keeps only the output for the document's `brand-mode`
//!   (`light` when unset), as Quarto 1 does for Reveal.js and Typst.
//! - Without both a `light` and a `dark` output, the cell keeps the one it
//!   has.
//!
//! As in Quarto 1, the cell keeps the blocks before its first
//! `cell-output-display` (the echoed source, stream output), followed by the
//! chosen outputs. A cell whose output count does not match the list is left
//! alone.
//!
//! Typst is not handled here: it goes through Pandoc, whose Lua filter chain
//! runs the original `cell-renderings.lua`. The transform is HTML-family
//! presentation and is excluded for every Pandoc-writer profile.
//!
//! WASM-safe (pure AST manipulation).

use std::collections::HashMap;

use quarto_pandoc_types::block::{Block, Div};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::ast_walk::for_each_block_list_mut;
use crate::format::PipelineProfile;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

const CELL: &str = "cell";
const CELL_OUTPUT_DISPLAY: &str = "cell-output-display";
const RENDERINGS: &str = "renderings";

/// What to do with a cell that has both a light and a dark output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// Keep both, marked `light-content` / `dark-content` (HTML, dashboard).
    MarkBoth,
    /// Keep only the output for the document's `brand-mode` (Reveal.js).
    /// `None` is a `brand-mode` that is neither `light` nor `dark`: Quarto 1
    /// keeps neither output then.
    Keep(Option<&'static str>),
}

/// See module docs.
pub struct CellRenderingsTransform;

impl CellRenderingsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CellRenderingsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CellRenderingsTransform {
    fn name(&self) -> &str {
        "cell-renderings"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let choice = match ctx.pipeline_profile {
            PipelineProfile::HtmlRender
            | PipelineProfile::HtmlPreview
            | PipelineProfile::DashboardRender
            | PipelineProfile::DashboardPreview => Choice::MarkBoth,
            PipelineProfile::RevealjsRender | PipelineProfile::RevealjsPreview => {
                let mode = ast.meta.get("brand-mode").and_then(|v| v.as_plain_text());
                Choice::Keep(match mode.as_deref() {
                    None | Some("light") => Some("light"),
                    Some("dark") => Some("dark"),
                    Some(_) => None,
                })
            }
            PipelineProfile::Pandoc(_) => return Ok(()),
        };
        for_each_block_list_mut(&mut ast.blocks, &mut |list| {
            for block in list.iter_mut() {
                if let Block::Div(div) = block {
                    choose_renderings(div, choice);
                }
            }
        });
        Ok(())
    }
}

fn has_class(div: &Div, class: &str) -> bool {
    div.attr.1.iter().any(|c| c == class)
}

/// The rendering names in a `renderings` attribute: a JSON array, or one bare
/// name (Quarto 1's `jsonDecodeArray`). `None` for anything else.
fn rendering_names(value: &str) -> Option<Vec<String>> {
    let value = value.trim();
    let names = if value.starts_with('[') {
        serde_json::from_str::<Vec<String>>(value).ok()?
    } else if value.starts_with('{') || value.is_empty() {
        return None;
    } else {
        vec![value.to_string()]
    };
    (!names.is_empty()).then_some(names)
}

/// Rewrite one `.cell` div that carries `renderings` (see module docs).
fn choose_renderings(cell: &mut Div, choice: Choice) {
    if !has_class(cell, CELL) {
        return;
    }
    let Some(names) = cell.attr.2.get(RENDERINGS).and_then(|v| rendering_names(v)) else {
        return;
    };
    let is_output = |b: &Block| matches!(b, Block::Div(d) if has_class(d, CELL_OUTPUT_DISPLAY));
    let Some(first) = cell.content.iter().position(is_output) else {
        return;
    };
    if cell.content.iter().filter(|b| is_output(b)).count() != names.len() {
        return;
    }

    let mut content = std::mem::take(&mut cell.content);
    let tail = content.split_off(first);
    let mut outputs: HashMap<String, Block> = HashMap::new();
    // A repeated name keeps its last output, as in Quarto 1.
    for (name, output) in names.into_iter().zip(tail.into_iter().filter(is_output)) {
        outputs.insert(name, output);
    }
    let light = outputs.remove("light");
    let dark = outputs.remove("dark");
    match (light, dark, choice) {
        (Some(mut light), Some(mut dark), Choice::MarkBoth) => {
            add_class(&mut light, "light-content");
            add_class(&mut dark, "dark-content");
            content.push(light);
            content.push(dark);
        }
        (Some(light), Some(dark), Choice::Keep(mode)) => match mode {
            Some("light") => content.push(light),
            Some("dark") => content.push(dark),
            _ => {}
        },
        (light, dark, _) => content.extend(light.or(dark)),
    }
    cell.content = content;
}

fn add_class(block: &mut Block, class: &str) {
    if let Block::Div(div) = block {
        div.attr.1.push(class.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;

    fn parse(qmd: &str) -> Pandoc {
        let (ast, _ctx, _diags) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            "doc.qmd",
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("parse");
        ast
    }

    async fn run(qmd: &str, profile: PipelineProfile) -> Pandoc {
        let mut ast = parse(qmd);
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let format = Format::html();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.pipeline_profile = profile;
        CellRenderingsTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        ast
    }

    /// The cell's children, each as `(classes, text)`: a Div's classes and the
    /// text of its first paragraph; other blocks as `([], "")`.
    fn cell_children(ast: &Pandoc) -> Vec<(Vec<String>, String)> {
        let Some(Block::Div(cell)) = ast.blocks.first() else {
            panic!("expected the cell div first, got {:?}", ast.blocks.first());
        };
        cell.content
            .iter()
            .map(|b| match b {
                Block::Div(d) => {
                    let text = match d.content.first() {
                        Some(Block::Paragraph(p)) => {
                            quarto_pandoc_types::config_value::inlines_to_plain_text(&p.content)
                        }
                        _ => String::new(),
                    };
                    (d.attr.1.clone(), text)
                }
                _ => (Vec::new(), String::new()),
            })
            .collect()
    }

    fn cell(front_matter: &str, renderings: &str) -> String {
        format!(
            "{front_matter}::: {{.cell renderings='{renderings}'}}\n\n\
             ```python\nplot()\n```\n\n\
             ::: {{.cell-output-display}}\n\nfirst\n\n:::\n\n\
             ::: {{.cell-output-display}}\n\nsecond\n\n:::\n\n\
             :::\n"
        )
    }

    fn classes(c: &[&str]) -> Vec<String> {
        c.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn html_keeps_both_outputs_with_mode_classes() {
        let ast = run(
            &cell("", r#"["light","dark"]"#),
            PipelineProfile::HtmlRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 3, "{children:?}");
        assert_eq!(
            children[1],
            (
                classes(&[CELL_OUTPUT_DISPLAY, "light-content"]),
                "first".into()
            )
        );
        assert_eq!(
            children[2],
            (
                classes(&[CELL_OUTPUT_DISPLAY, "dark-content"]),
                "second".into()
            )
        );
    }

    #[tokio::test]
    async fn html_follows_the_order_of_the_names() {
        let ast = run(
            &cell("", r#"["dark","light"]"#),
            PipelineProfile::HtmlRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(
            children[1],
            (
                classes(&[CELL_OUTPUT_DISPLAY, "light-content"]),
                "second".into()
            )
        );
        assert_eq!(
            children[2],
            (
                classes(&[CELL_OUTPUT_DISPLAY, "dark-content"]),
                "first".into()
            )
        );
    }

    #[tokio::test]
    async fn dashboard_marks_both_outputs() {
        let ast = run(
            &cell("", r#"["light","dark"]"#),
            PipelineProfile::DashboardRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 3);
        assert!(children[1].0.contains(&"light-content".to_string()));
        assert!(children[2].0.contains(&"dark-content".to_string()));
    }

    #[tokio::test]
    async fn revealjs_keeps_the_light_output_by_default() {
        let ast = run(
            &cell("", r#"["light","dark"]"#),
            PipelineProfile::RevealjsRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 2, "{children:?}");
        assert_eq!(
            children[1],
            (classes(&[CELL_OUTPUT_DISPLAY]), "first".into())
        );
    }

    #[tokio::test]
    async fn revealjs_keeps_the_output_for_brand_mode_dark() {
        let ast = run(
            &cell("---\nbrand-mode: dark\n---\n\n", r#"["light","dark"]"#),
            PipelineProfile::RevealjsRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 2, "{children:?}");
        assert_eq!(
            children[1],
            (classes(&[CELL_OUTPUT_DISPLAY]), "second".into())
        );
    }

    #[tokio::test]
    async fn a_mismatched_output_count_leaves_the_cell_alone() {
        let ast = run(
            &cell("", r#"["light","dark","extra"]"#),
            PipelineProfile::HtmlRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 3);
        assert_eq!(children[1].0, classes(&[CELL_OUTPUT_DISPLAY]));
        assert_eq!(children[2].0, classes(&[CELL_OUTPUT_DISPLAY]));
    }

    #[tokio::test]
    async fn without_a_dark_output_the_light_one_is_kept() {
        let ast = run(
            &cell("", r#"["light","other"]"#),
            PipelineProfile::HtmlRender,
        )
        .await;
        let children = cell_children(&ast);
        assert_eq!(children.len(), 2, "{children:?}");
        assert_eq!(
            children[1],
            (classes(&[CELL_OUTPUT_DISPLAY]), "first".into())
        );
    }

    #[tokio::test]
    async fn pandoc_profiles_are_untouched() {
        let ast = run(
            &cell("", r#"["light","dark"]"#),
            PipelineProfile::Pandoc("docx".into()),
        )
        .await;
        assert_eq!(cell_children(&ast).len(), 3);
    }

    #[test]
    fn rendering_names_accepts_an_array_or_a_bare_name() {
        assert_eq!(
            rendering_names(r#"["light","dark"]"#),
            Some(vec!["light".to_string(), "dark".to_string()])
        );
        assert_eq!(rendering_names("light"), Some(vec!["light".to_string()]));
        assert_eq!(rendering_names(r#"{"a":1}"#), None);
        assert_eq!(rendering_names("[]"), None);
        assert_eq!(rendering_names(""), None);
    }
}
