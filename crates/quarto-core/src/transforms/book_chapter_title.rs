/*
 * book_chapter_title.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: a book chapter's title, in the page's title block.
 */

//! Move a book chapter's title into the title block, as Quarto 1 does.
//!
//! An author titles a chapter with a first heading (`# Methods
//! {#sec-methods}`) or with front matter (`title: "Methods"`). Quarto 1
//! gives both the same page: the title is the page's `h1.title`, with the
//! chapter number in front (`2<nbsp> Methods`, or `Appendix A — Extra`),
//! and the body starts with the chapter's text
//! (`markdownWithExtractedHeading`, `pandoc-partition.ts`; decorated by
//! `formatChapterTitle`, `book-chapters.ts`).
//!
//! This transform does that for an HTML book page:
//!
//! 1. With no front-matter `title`, the document's first heading (of any
//!    level, wherever it sits among the top-level blocks) is removed from
//!    the body and becomes the title. Its inline formatting is kept. Its
//!    id moves onto a `span.quarto-section-identifier` in the title, so
//!    `page.html#sec-methods` still lands on the chapter.
//! 2. The title, from either source, is decorated with the chapter's
//!    number from the book's render items and written back to `title`.
//!    `MetadataNormalizeTransform` and `WebsiteTitlePrefixTransform`,
//!    which run after this, turn it into the page's `<title>`.
//! 3. [`LiftedChapterTitle`] is left on the context. With the chapter
//!    heading gone from the body, `CrossrefIndexTransform` uses it to
//!    start the section counter inside the chapter and to register the
//!    chapter's `@sec-` id.
//!
//! The home page is left alone: its title block is the book's
//! ([`super::BookHomeMetaTransform`]) and its own heading stays in the
//! body. A no-op outside a book project and for a page that is not one of
//! the book's files.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::inline::{Inline, Inlines, Space, Span, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::book::config::chapter_label_prefix;
use crate::project::book::render_item::render_item_for_input;
use crate::project::book::{BookRenderItem, BookRenderItemKind};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// What [`BookChapterTitleTransform`] took out of a chapter's body, for
/// the transforms that used to read it from the chapter heading.
#[derive(Debug, Clone)]
pub struct LiftedChapterTitle {
    /// The chapter heading's id, when the title came from a heading.
    pub id: Option<String>,
    /// The undecorated title inlines (a crossref's caption).
    pub content: Inlines,
    /// Where the title was written.
    pub source_info: SourceInfo,
}

/// AST transform: see the module docs.
pub struct BookChapterTitleTransform;

impl BookChapterTitleTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BookChapterTitleTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for BookChapterTitleTransform {
    fn name(&self) -> &str {
        "book-chapter-title"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ctx.format.identifier.is_html_based() {
            return Ok(());
        }
        let Some(item) = ctx.project.book_render_items.as_ref().and_then(|items| {
            render_item_for_input(&ctx.project.dir, items, &ctx.document.input)
                .filter(|item| item.kind != BookRenderItemKind::Index)
                .cloned()
        }) else {
            return Ok(());
        };

        let (title, id, source_info) = match ast.meta.get("title") {
            Some(title) => (title_inlines(title), None, title.source_info.clone()),
            None => {
                // Q1 takes the first heading of any level, wherever it is.
                let Some(pos) = ast
                    .blocks
                    .iter()
                    .position(|block| matches!(block, Block::Header(_)))
                else {
                    return Ok(());
                };
                let Block::Header(header) = ast.blocks.remove(pos) else {
                    unreachable!("position() matched a header");
                };
                let id = Some(header.attr.0.clone()).filter(|id| !id.is_empty());
                (header.content, id, header.source_info)
            }
        };

        let decorated = decorate(&item, title.clone(), id.as_deref(), &source_info);
        ast.meta.insert_path(
            &["title"],
            ConfigValue::new_inlines(decorated, source_info.clone()),
        );
        ctx.lifted_chapter_title = Some(LiftedChapterTitle {
            id,
            content: title,
            source_info,
        });
        Ok(())
    }
}

/// A front-matter `title` as inlines: the parsed markdown when there is
/// some, else its plain text.
fn title_inlines(title: &ConfigValue) -> Inlines {
    match &title.value {
        ConfigValueKind::PandocInlines(inlines) => inlines.clone(),
        _ => vec![Inline::Str(Str {
            text: title.as_plain_text().unwrap_or_default(),
            source_info: title.source_info.clone(),
        })],
    }
}

/// The title as the page shows it (Q1's `formatChapterTitle`):
/// `<span.chapter-number>N</span><nbsp> <span.chapter-title>T</span>` for
/// a numbered chapter, `Appendix A — T` for a numbered appendix, the bare
/// title otherwise; wrapped in `span#id.quarto-section-identifier` when
/// the title came from a heading with an id.
fn decorate(
    item: &BookRenderItem,
    title: Inlines,
    id: Option<&str>,
    source_info: &SourceInfo,
) -> Inlines {
    let text = |text: &str| {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: source_info.clone(),
        })
    };
    let space = || {
        Inline::Space(Space {
            source_info: source_info.clone(),
        })
    };
    let span = |id: &str, class: &str, content: Inlines| {
        Inline::Span(Box::new(Span {
            attr: (
                id.to_string(),
                vec![class.to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content,
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        }))
    };

    let decorated: Inlines = match chapter_label_prefix(item) {
        Some(letter) if item.kind == BookRenderItemKind::Appendix => {
            let mut inlines = vec![
                text("Appendix"),
                space(),
                text(&letter),
                space(),
                text("—"),
                space(),
            ];
            inlines.extend(title);
            inlines
        }
        Some(number) => vec![
            span("", "chapter-number", vec![text(&number)]),
            text("\u{a0}"),
            space(),
            span("", "chapter-title", title),
        ],
        None => title,
    };
    match id {
        Some(id) => vec![span(id, "quarto-section-identifier", decorated)],
        None => decorated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: BookRenderItemKind, number: Option<u32>) -> BookRenderItem {
        BookRenderItem {
            kind,
            depth: 0,
            text: None,
            file: Some("ch.qmd".into()),
            number,
            heading_title: None,
        }
    }

    fn plain(inlines: &Inlines) -> String {
        let cv = ConfigValue::new_inlines(inlines.clone(), SourceInfo::for_test());
        cv.as_plain_text().unwrap_or_default()
    }

    fn title(text: &str) -> Inlines {
        vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })]
    }

    #[test]
    fn numbered_chapter_title_has_number_and_title_spans() {
        let out = decorate(
            &item(BookRenderItemKind::Chapter, Some(2)),
            title("Methods"),
            None,
            &SourceInfo::for_test(),
        );
        assert_eq!(plain(&out), "2\u{a0} Methods");
        let Inline::Span(number) = &out[0] else {
            panic!("first inline must be the chapter-number span: {out:?}");
        };
        assert_eq!(number.attr.1, vec!["chapter-number".to_string()]);
    }

    #[test]
    fn appendix_title_has_its_letter() {
        let out = decorate(
            &item(BookRenderItemKind::Appendix, Some(1)),
            title("Extra"),
            None,
            &SourceInfo::for_test(),
        );
        assert_eq!(plain(&out), "Appendix A — Extra");
    }

    #[test]
    fn unnumbered_title_is_bare_and_a_heading_id_wraps_it() {
        let out = decorate(
            &item(BookRenderItemKind::Chapter, None),
            title("References"),
            Some("sec-refs"),
            &SourceInfo::for_test(),
        );
        assert_eq!(plain(&out), "References");
        let Inline::Span(wrapper) = &out[0] else {
            panic!("an id must wrap the title in a span: {out:?}");
        };
        assert_eq!(wrapper.attr.0, "sec-refs");
        assert_eq!(
            wrapper.attr.1,
            vec!["quarto-section-identifier".to_string()]
        );
    }
}
