/*
 * revealjs/toc.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The table-of-contents slide for `format: revealjs` with `toc: true`.
 */

//! The reveal.js table-of-contents slide.
//!
//! Quarto 1 adds a horizontal `<section id="TOC">` slide right after the
//! title slide (`share/formats/revealjs/pandoc/toc-slide.html`), filled by
//! Pandoc's TOC builder with reveal hash links:
//!
//! ```html
//! <section id="TOC">
//! <nav role="doc-toc">
//! <h2 id="toc-title">Table of contents</h2>
//! <ul>
//! <li><a href="#/intro" id="/toc-intro">Intro</a></li>
//! </ul>
//! </nav>
//! </section>
//! ```
//!
//! q2 has no second TOC builder for this. `TocGenerateTransform` builds
//! `navigation.toc` with `pampa::toc::generate_toc` for every format; for
//! revealjs it feeds that builder [`slide_headings`], because the slide tree
//! has already moved each slide heading's id onto its `<section>`.
//! [`RevealTocTransform`] then renders the list in Quarto 1's shape and
//! inserts the slide.

use hashlink::LinkedHashMap;
use pampa::toc::{NavigationToc, TocEntry};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Header, RawBlock};
use quarto_pandoc_types::inline::{Inline, Inlines, Space, Span, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::is_feature_disabled;
use crate::transforms::{html_escape, render_toc_label};

/// The id of the title slide `RevealSlidesTransform` builds. It is not a
/// TOC entry.
const TITLE_SLIDE_ID: &str = "title-slide";

/// The TOC slide's id (Quarto 1: `$idprefix$TOC`).
const TOC_SLIDE_ID: &str = "TOC";

fn is_section(div: &Div) -> bool {
    div.attr.1.iter().any(|c| c == "section")
}

/// The heading that titles a slide section: its first block, or for a
/// stack (a `#` section holding vertical slides), the first block of its
/// first vertical slide.
fn section_heading(div: &Div) -> Option<&Header> {
    match div.content.first()? {
        Block::Header(h) => Some(h),
        Block::Div(inner) if is_section(inner) => match inner.content.first()? {
            Block::Header(h) => Some(h),
            _ => None,
        },
        _ => None,
    }
}

/// A heading's inlines with its section number in front, as Pandoc's
/// reveal TOC shows it under `number-sections`
/// (`<span class="header-section-number">1</span> Section A`).
///
/// The crossref index stashes the number as a `number` attribute in the
/// Crossref phase, but the visible span is only added to the heading in
/// Finalization, after the TOC is built; so it is added here from the
/// attribute.
fn numbered_title(h: &Header) -> Inlines {
    let Some(number) = h.attr.2.get("number") else {
        return h.content.clone();
    };
    let si = h.source_info.clone();
    let mut out: Inlines = vec![
        Inline::Span(Box::new(Span {
            attr: (
                String::new(),
                vec!["header-section-number".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content: vec![Inline::Str(Str {
                text: number.clone(),
                source_info: si.clone(),
            })],
            source_info: si.clone(),
            attr_source: AttrSourceInfo::empty(),
        })),
        Inline::Space(Space { source_info: si }),
    ];
    out.extend(h.content.iter().cloned());
    out
}

/// Flatten the reveal slide tree back into the heading list a TOC is built
/// from.
///
/// `build_reveal_slides` moves each slide heading's id (and classes) onto
/// its `<section>` and leaves the heading id-less. This puts them back
/// together: each slide section with an id yields a `Header` carrying the
/// section's id and classes (so `.unlisted` and `.unnumbered` still apply)
/// and the heading's level and inlines. Headings inside a slide (below the
/// slide level) keep their own ids and pass through. The title slide and
/// the TOC slide are skipped.
pub(crate) fn slide_headings(blocks: &[Block]) -> Vec<Block> {
    let mut out = Vec::new();
    collect_slide_headings(blocks, &mut out);
    out
}

fn collect_slide_headings(blocks: &[Block], out: &mut Vec<Block>) {
    for block in blocks {
        match block {
            Block::Div(div) if is_section(div) => {
                let id = div.attr.0.as_str();
                if id == TITLE_SLIDE_ID || id == TOC_SLIDE_ID {
                    continue;
                }
                if !id.is_empty()
                    && let Some(h) = section_heading(div)
                {
                    let classes = div
                        .attr
                        .1
                        .iter()
                        .filter(|c| *c != "section")
                        .cloned()
                        .collect();
                    out.push(Block::Header(Box::new(Header {
                        level: h.level,
                        attr: (id.to_string(), classes, LinkedHashMap::new()).into(),
                        content: numbered_title(h),
                        source_info: h.source_info.clone(),
                        attr_source: AttrSourceInfo::empty(),
                    })));
                }
                collect_slide_headings(&div.content, out);
            }
            // A heading inside a slide. Id-less ones (the slide's own
            // heading, whose id moved to the section) are dropped by
            // `generate_toc`.
            Block::Header(h) if !h.attr.0.is_empty() => {
                out.push(Block::Header(Box::new(Header {
                    content: numbered_title(h),
                    ..(**h).clone()
                })));
            }
            _ => {}
        }
    }
}

/// Render TOC entries as Pandoc's reveal.js writer does: plain nested
/// lists whose links use reveal's hash routing.
pub(crate) fn render_reveal_toc_list(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut html = String::from("<ul>\n");
    for entry in entries {
        let id = html_escape(&entry.id);
        html.push_str(&format!(
            "<li><a href=\"#/{id}\" id=\"/toc-{id}\">{}</a>",
            render_toc_label(&entry.title)
        ));
        if !entry.children.is_empty() {
            html.push('\n');
            html.push_str(&render_reveal_toc_list(&entry.children));
        }
        html.push_str("</li>\n");
    }
    html.push_str("</ul>");
    html
}

/// The `<nav>` inside the TOC slide.
fn render_toc_nav(toc: &NavigationToc) -> String {
    let mut html = String::from("<nav role=\"doc-toc\">\n");
    if let Some(title) = &toc.title {
        html.push_str(&format!(
            "<h2 id=\"toc-title\">{}</h2>\n",
            render_toc_label(title)
        ));
    }
    html.push_str(&render_reveal_toc_list(&toc.entries));
    html.push_str("\n</nav>");
    html
}

/// Insert `slide` after the title slide, or first when there is none.
fn insert_toc_slide(blocks: &mut Vec<Block>, slide: Block) {
    let at = match blocks.first() {
        Some(Block::Div(d)) if d.attr.0 == TITLE_SLIDE_ID => 1,
        _ => 0,
    };
    blocks.insert(at, slide);
}

/// Adds the TOC slide to a revealjs deck with `toc: true`.
///
/// Runs in the Navigation phase after `TocGenerateTransform`, which has
/// already honored `toc`, `toc-depth` and `toc-title` (with its localized
/// default) and written `navigation.toc`.
pub struct RevealTocTransform;

impl RevealTocTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RevealTocTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for RevealTocTransform {
    fn name(&self) -> &str {
        "reveal-toc"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        if is_feature_disabled(&ast.meta, "toc") {
            return Ok(());
        }
        let Some(toc) = ast
            .meta
            .get_path(&["navigation", "toc"])
            .and_then(NavigationToc::from_config_value)
        else {
            return Ok(());
        };
        if toc.entries.is_empty() {
            return Ok(());
        }
        let source_info = SourceInfo::generated(By::revealjs());
        let slide = Block::Div(Box::new(Div {
            attr: (
                TOC_SLIDE_ID.to_string(),
                vec!["section".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content: vec![Block::RawBlock(Box::new(RawBlock {
                format: "html".to_string(),
                text: render_toc_nav(&toc),
                source_info: source_info.clone(),
            }))],
            source_info,
            attr_source: AttrSourceInfo::empty(),
        }));
        insert_toc_slide(&mut ast.blocks, slide);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::revealjs::build_reveal_slides;
    use pampa::toc::{TocConfig, generate_toc, plain_inlines};

    fn si() -> SourceInfo {
        SourceInfo::generated(By::revealjs())
    }

    fn header(level: usize, id: &str, classes: &[&str], text: &str) -> Block {
        Block::Header(Box::new(Header {
            level,
            attr: (
                id.to_string(),
                classes.iter().map(|c| c.to_string()).collect(),
                LinkedHashMap::new(),
            )
                .into(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: si(),
            })],
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    fn section(id: &str, content: Vec<Block>) -> Block {
        Block::Div(Box::new(Div {
            attr: (
                id.to_string(),
                vec!["section".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content,
            source_info: si(),
            attr_source: AttrSourceInfo::empty(),
        }))
    }

    /// `(level, id, classes)` of each flattened heading.
    fn shape(blocks: &[Block]) -> Vec<(usize, String, Vec<String>)> {
        blocks
            .iter()
            .map(|b| match b {
                Block::Header(h) => (h.level, h.attr.0.clone(), h.attr.1.clone()),
                other => panic!("expected Header, got {other:?}"),
            })
            .collect()
    }

    fn deck() -> Vec<Block> {
        build_reveal_slides(
            vec![
                header(1, "section-a", &[], "Section A"),
                header(2, "slide-one", &[], "Slide one"),
                header(2, "custom-id", &[], "Slide two"),
                header(3, "sub-heading", &[], "Sub heading"),
                header(1, "section-b", &["unnumbered"], "Section B"),
                header(2, "slide-three", &["unlisted"], "Slide three"),
            ],
            2,
        )
    }

    #[test]
    fn flattens_stacks_slides_and_sub_slide_headings() {
        assert_eq!(
            shape(&slide_headings(&deck())),
            vec![
                (1, "section-a".into(), vec![]),
                (2, "slide-one".into(), vec![]),
                (2, "custom-id".into(), vec![]),
                (3, "sub-heading".into(), vec![]),
                (1, "section-b".into(), vec!["unnumbered".into()]),
                (2, "slide-three".into(), vec!["unlisted".into()]),
            ]
        );
    }

    #[test]
    fn skips_the_title_and_toc_slides() {
        let blocks = vec![
            section(TITLE_SLIDE_ID, vec![header(1, "", &["title"], "Deck")]),
            section(TOC_SLIDE_ID, vec![]),
            section("a", vec![header(2, "", &[], "A")]),
        ];
        assert_eq!(
            shape(&slide_headings(&blocks)),
            vec![(2, "a".into(), vec![])]
        );
    }

    #[test]
    fn generate_toc_nests_and_drops_unlisted() {
        let config = TocConfig {
            depth: 3,
            title: Some(plain_inlines("Contents")),
        };
        let toc = generate_toc(&slide_headings(&deck()), &config);
        let html = render_reveal_toc_list(&toc.entries);
        assert_eq!(
            html,
            "<ul>\n\
             <li><a href=\"#/section-a\" id=\"/toc-section-a\">Section A</a>\n\
             <ul>\n\
             <li><a href=\"#/slide-one\" id=\"/toc-slide-one\">Slide one</a></li>\n\
             <li><a href=\"#/custom-id\" id=\"/toc-custom-id\">Slide two</a>\n\
             <ul>\n\
             <li><a href=\"#/sub-heading\" id=\"/toc-sub-heading\">Sub heading</a></li>\n\
             </ul></li>\n\
             </ul></li>\n\
             <li><a href=\"#/section-b\" id=\"/toc-section-b\">Section B</a></li>\n\
             </ul>"
        );
    }

    #[test]
    fn stashed_section_numbers_lead_the_label() {
        let mut numbered = header(1, "a", &[], "Section A");
        if let Block::Header(h) = &mut numbered {
            h.attr.2.insert("number".to_string(), "1".to_string());
        }
        let toc = generate_toc(
            &slide_headings(&build_reveal_slides(vec![numbered], 2)),
            &TocConfig {
                depth: 3,
                title: None,
            },
        );
        assert!(
            render_reveal_toc_list(&toc.entries).contains(">1</span> Section A</a>"),
            "{:?}",
            toc.entries
        );
    }

    #[test]
    fn link_ids_are_escaped() {
        let toc = generate_toc(
            &[header(2, "a\"b", &[], "Q")],
            &TocConfig {
                depth: 3,
                title: None,
            },
        );
        assert!(render_reveal_toc_list(&toc.entries).contains("href=\"#/a&quot;b\""));
    }

    #[test]
    fn toc_slide_goes_after_the_title_slide_or_first() {
        let id_at = |blocks: &[Block], i: usize| match &blocks[i] {
            Block::Div(d) => d.attr.0.clone(),
            _ => panic!(),
        };
        let mut with_title = vec![section(TITLE_SLIDE_ID, vec![]), section("a", vec![])];
        insert_toc_slide(&mut with_title, section(TOC_SLIDE_ID, vec![]));
        assert_eq!(id_at(&with_title, 1), TOC_SLIDE_ID);

        let mut without = vec![section("a", vec![])];
        insert_toc_slide(&mut without, section(TOC_SLIDE_ID, vec![]));
        assert_eq!(id_at(&without, 0), TOC_SLIDE_ID);
    }
}
