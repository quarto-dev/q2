/*
 * book_cover_image.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Book-project cover image for the multi-file HTML home page.
 */

//! Insert a book project's cover image into the index page's AST.
//!
//! Port of the AST-visible half of Q1's book cover flow
//! (`book-render.ts` prepends a `![](cover.png "Title"){.quarto-cover-image
//! .nolightbox}` paragraph on the index page; `bookHtmlPostprocessor`
//! then DOM-moves that `<p>` into the first `<section>` after its
//! header). q2 has no DOM postprocessor — the insertion happens here,
//! before `SectionizeTransform` wraps the flat blocks, so the paragraph
//! lands inside the first section right after its header with no fixup.
//!
//! Sources, in Q1's precedence: the page's own `cover-image` metadata,
//! then `book.cover-image` from the project config. Paths follow the
//! repo's path-resolution contract (declared-file-relative; a leading
//! `/` means the project root), re-expressed document-relative (or
//! site-root-relative with a leading `/`) as the Image target so the
//! ordinary `ResourceCollectorTransform` copies the file into the
//! output tree — no special-cased copy.

use hashlink::LinkedHashMap;
use std::path::{Path, PathBuf};

use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
use quarto_pandoc_types::block::{Block, Paragraph};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::inline::{Image, Inline, split_string_to_inlines};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::config_paths::{PathCtx, resolve_config_path};
use crate::project::ProjectKind;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

pub struct BookCoverImageTransform;

impl BookCoverImageTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BookCoverImageTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// A resolved cover request: the absolute source path and the
/// accessible-name override, with where each was declared.
struct CoverRequest {
    /// Absolute path of the image file in the source tree.
    src: PathBuf,
    alt: Option<String>,
}

impl CoverRequest {
    /// Q1's precedence: the page's `cover-image` metadata (resolved
    /// against the document's directory), then `book.cover-image`
    /// (resolved against the project directory).
    fn find(
        ast_meta: &ConfigValue,
        project_meta: Option<&ConfigValue>,
        path_ctx: &PathCtx<'_>,
    ) -> Option<CoverRequest> {
        if let Some(value) = ast_meta.get("cover-image")
            && value.as_plain_text().is_some_and(|s| !s.is_empty())
        {
            return Some(CoverRequest {
                src: resolve_config_path(value, path_ctx)?.file()?,
                alt: config_plain_text(ast_meta, "cover-image-alt"),
            });
        }
        let book = project_meta.and_then(|m| m.get("book"))?;
        let value = book.get("cover-image")?;
        if !value.as_plain_text().is_some_and(|s| !s.is_empty()) {
            return None;
        }
        // `book.cover-image` is `_quarto.yml`-only: the project root is
        // its anchor and, for a span the context cannot place, its
        // fallback (never the index page's directory).
        let project_ctx = PathCtx {
            document_dir: path_ctx.project_dir,
            ..*path_ctx
        };
        Some(CoverRequest {
            src: resolve_config_path(value, &project_ctx)?.file()?,
            alt: config_plain_text(book, "cover-image-alt"),
        })
    }
}

fn config_plain_text(meta: &ConfigValue, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_plain_text())
        .filter(|s| !s.is_empty())
}

/// Express the cover's source path as a URL the renderer resolves the
/// same way: document-relative when the file lives under the document's
/// directory, otherwise site-root-relative (`/…`, which the project
/// root anchors).
fn cover_target(src: &Path, doc_dir: &Path, project_dir: &Path) -> Option<String> {
    src.strip_prefix(doc_dir)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| {
            src.strip_prefix(project_dir)
                .ok()
                .map(|p| format!("/{}", p.to_string_lossy().into_owned()))
        })
}

#[async_trait::async_trait(?Send)]
impl AstTransform for BookCoverImageTransform {
    fn name(&self) -> &str {
        "book-cover-image"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> crate::Result<()> {
        // Books only, index page only — Q1's `isBookIndexPage` gate.
        if ctx.project.project_kind() != ProjectKind::Book {
            return Ok(());
        }
        let is_index = ctx
            .document
            .input
            .file_name()
            .is_some_and(|f| f.to_string_lossy().starts_with("index."));
        if !is_index {
            return Ok(());
        }
        let Some(doc_dir) = ctx.document.input.parent() else {
            return Ok(());
        };
        let project_meta = ctx.project.config.metadata.as_ref();
        // Config-authored paths resolve relative to the declaring file's
        // directory; a leading `/` means the project root
        // (`crate::config_paths`; claude-notes/designs/path-resolution-model.md).
        let path_ctx =
            PathCtx::for_document(ctx.source_context, &ctx.project.dir, &ctx.document.input);
        let Some(request) = CoverRequest::find(&ast.meta, project_meta, &path_ctx) else {
            return Ok(());
        };
        let Some(target) = cover_target(&request.src, doc_dir, &ctx.project.dir) else {
            return Ok(());
        };

        // Q1 appends the doc title as the img's title attribute; omit
        // entirely when there is none (Q1 emits an empty one). Q1 sources
        // it via `withBookTitleMetadata` — the page's own title, falling
        // back to the book's (the project title is not merged into
        // per-page meta in q2's multi-file path, so read it explicitly).
        let title = config_plain_text(&ast.meta, "title")
            .or_else(|| {
                project_meta
                    .and_then(|m| m.get("book"))
                    .and_then(|b| b.get("title"))
                    .and_then(|v| v.as_plain_text())
            })
            .unwrap_or_default();
        // The alt text goes straight into the alt inlines, the canonical
        // form `image-alt-normalize` would otherwise make of Q1's
        // `fig-alt` (bd-ykjwiqcc). Q1 writes `fig-alt` here and folds it
        // in its HTML post-filter; this transform runs before the
        // normalizer, but building the canonical shape directly keeps the
        // cover independent of that ordering.
        let content = request
            .alt
            .as_deref()
            .map(split_string_to_inlines)
            .unwrap_or_default();
        let image = Inline::Image(Box::new(Image {
            attr: (
                String::new(),
                vec!["quarto-cover-image".to_string(), "nolightbox".to_string()],
                LinkedHashMap::new(),
            )
                .into(),
            content,
            target: (target, title),
            source_info: SourceInfo::generated(By::programmatic_config()),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }));
        let paragraph = Block::Paragraph(Paragraph {
            content: vec![image],
            source_info: SourceInfo::generated(By::programmatic_config()),
        });

        // After the first top-level header (so SectionizeTransform wraps
        // it inside the first section, Q1's post-fixup shape); at the
        // very start when the page has no header.
        let insert_at = ast
            .blocks
            .iter()
            .position(|b| matches!(b, Block::Header(_)))
            .map_or(0, |i| i + 1);
        ast.blocks.insert(insert_at, paragraph);
        Ok(())
    }
}
