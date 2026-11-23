/*
 * book_page_meta.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: the `<title>` and `<meta name="description">` of a book
 * chapter page.
 */

//! Page-level metadata for a chapter of a multi-file HTML book.
//!
//! Quarto 1 titles a chapter page `N  Chapter Title – Book Title`
//! (`Appendix A — Chapter Title – Book Title` for an appendix), and puts
//! `book.description` in `<meta name="description">` on every page. A
//! chapter usually has no `title:` in its front matter (its title is its
//! first heading), so without this transform
//! [`super::WebsiteTitlePrefixTransform`] finds no page title and every
//! chapter page is titled with the book title alone.
//!
//! This transform sets `pagetitle` and `description-meta` only. It runs
//! before `WebsiteTitlePrefixTransform`, which leaves a `pagetitle` that
//! differs from `title` alone. The book's home page keeps the book title.
//! A no-op outside a book project.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::ProjectKind;
use crate::project::book::BookRenderItem;
use crate::project::book::BookRenderItemKind;
use crate::project::book::config::chapter_label_prefix;
use crate::project::website_config::website_title;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Separator between the chapter title and the book title (as
/// `WebsiteTitlePrefixTransform`).
const TITLE_SEPARATOR: &str = " – ";

/// AST transform: see the module docs.
pub struct BookPageMetaTransform;

impl BookPageMetaTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BookPageMetaTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for BookPageMetaTransform {
    fn name(&self) -> &str {
        "book-page-meta"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.project.project_kind() != ProjectKind::Book {
            return Ok(());
        }
        let source_info = ast.meta.source_info.clone();

        // `book.description` describes every page that has no description
        // of its own.
        if ast.meta.get("description").is_none()
            && ast.meta.get("description-meta").is_none()
            && let Some(description) = ast
                .meta
                .get_path(&["book", "description"])
                .and_then(|v| v.as_plain_text())
                .filter(|d| !d.trim().is_empty())
        {
            ast.meta.insert_path(
                &["description-meta"],
                ConfigValue::new_string(description, source_info.clone()),
            );
        }

        let Some(item) = book_item_for(ctx) else {
            return Ok(());
        };
        let title = ast.meta.get("title").and_then(|v| v.as_plain_text());
        // A `pagetitle` that differs from `title` is the author's own.
        let pagetitle = ast.meta.get("pagetitle").and_then(|v| v.as_plain_text());
        if pagetitle.is_some() && pagetitle != title {
            return Ok(());
        }
        let Some(site_title) = website_title(&ast.meta) else {
            return Ok(());
        };
        let Some(chapter_title) = title.or_else(|| item.heading_title.clone()) else {
            return Ok(());
        };
        let pagetitle = format!(
            "{}{TITLE_SEPARATOR}{site_title}",
            chapter_page_title(item, &chapter_title)
        );
        ast.meta.insert_path(
            &["pagetitle"],
            ConfigValue::new_string(pagetitle, source_info),
        );
        Ok(())
    }
}

/// The book item the current document renders, unless it is the book's
/// home page, which keeps the book title.
fn book_item_for<'a>(ctx: &'a RenderContext) -> Option<&'a BookRenderItem> {
    let items = ctx.project.book_render_items.as_ref()?;
    crate::project::book::render_item::render_item_for_input(
        &ctx.project.dir,
        items,
        &ctx.document.input,
    )
    .filter(|item| item.kind != BookRenderItemKind::Index)
}

/// A chapter's title as its page shows it: `N<nbsp> Title` for a numbered
/// chapter (Q1's `numberChapterHtmlNav` text), `Appendix A — Title` for a
/// numbered appendix, the bare title otherwise.
fn chapter_page_title(item: &BookRenderItem, title: &str) -> String {
    match chapter_label_prefix(item) {
        Some(letter) if item.kind == BookRenderItemKind::Appendix => {
            format!("Appendix {letter} — {title}")
        }
        Some(number) => format!("{number}\u{a0} {title}"),
        None => title.to_string(),
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

    #[test]
    fn numbered_chapter_title_has_its_number() {
        assert_eq!(
            chapter_page_title(&item(BookRenderItemKind::Chapter, Some(2)), "Methods"),
            "2\u{a0} Methods"
        );
    }

    #[test]
    fn appendix_title_has_its_letter() {
        assert_eq!(
            chapter_page_title(&item(BookRenderItemKind::Appendix, Some(1)), "Extra"),
            "Appendix A — Extra"
        );
    }

    #[test]
    fn unnumbered_chapter_title_is_bare() {
        assert_eq!(
            chapter_page_title(&item(BookRenderItemKind::Chapter, None), "References"),
            "References"
        );
    }
}
