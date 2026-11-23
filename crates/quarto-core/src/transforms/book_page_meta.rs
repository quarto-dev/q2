/*
 * book_page_meta.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: the `<title>` and `<meta name="description">` of a book
 * chapter page.
 */

//! Page-level metadata for the pages of a book.
//!
//! [`BookPageMetaTransform`] puts `book.description` in
//! `<meta name="description">` on every page that has no description of
//! its own, as Quarto 1 does. (A chapter page's `<title>` comes from its
//! title block: see [`super::BookChapterTitleTransform`].)
//!
//! [`BookHomeMetaTransform`] gives the home page the book's title block.
//!
//! Both are no-ops outside a book project.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::ProjectKind;
use crate::project::book::BookRenderItemKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

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

        Ok(())
    }
}

/// AST transform: the title block of a book's home page.
///
/// Quarto 1 opens the home page (`index.*`) with the book's own title
/// block: `book.title`, and `book.subtitle`, `book.author`, `book.date`,
/// `book.description` and `book.abstract` where set. The page's own first
/// heading ("Preface") stays in the body below it (bd-tqeeqx1c).
///
/// The book title always replaces the page's `title`. The other keys
/// fill in only what the page does not set itself. Runs before
/// `MetadataNormalizeTransform`, so the copied values are normalized and
/// formatted like any document's. A no-op on every other page.
pub struct BookHomeMetaTransform;

impl BookHomeMetaTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BookHomeMetaTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// `book.*` keys the home page's title block shows, besides `title`.
const BOOK_HOME_KEYS: &[&str] = &["subtitle", "author", "date", "description", "abstract"];

#[async_trait::async_trait(?Send)]
impl AstTransform for BookHomeMetaTransform {
    fn name(&self) -> &str {
        "book-home-meta"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let is_home = ctx.project.book_render_items.as_ref().is_some_and(|items| {
            crate::project::book::render_item::render_item_for_input(
                &ctx.project.dir,
                items,
                &ctx.document.input,
            )
            .is_some_and(|item| item.kind == BookRenderItemKind::Index)
        });
        if !is_home {
            return Ok(());
        }
        let Some(book) = ast.meta.get("book").cloned() else {
            return Ok(());
        };
        if let Some(title) = book.get("title") {
            ast.meta.insert_path(&["title"], title.clone());
        }
        for key in BOOK_HOME_KEYS {
            if ast.meta.get(key).is_none()
                && let Some(value) = book.get(key)
            {
                ast.meta.insert_path(&[key], value.clone());
            }
        }
        Ok(())
    }
}
