/*
 * resource_collector.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that collects resource dependencies from the AST.
 */

//! Resource collection transform.
//!
//! This transform walks the AST and collects user-authored resource
//! files (images, etc.) that the rendered document references and
//! that must be copied from the source tree to the output tree so
//! the rendered HTML can resolve its `<img src="...">` URLs.
//!
//! Each discovered resource is recorded as a
//! `(source_path, destination_path)` pair on
//! [`RenderContext::resource_copies`]. The render orchestrator
//! drains those pairs into the per-render
//! [`crate::output_sink::OutputSink`] before flush, so every
//! destructive copy goes through the sink's `allowed_roots` and
//! `src != dest` checks. When source and destination canonicalize
//! to the same path (the common single-doc case where the output
//! dir equals the input dir), the sink silently skips the copy —
//! the file is already where the HTML expects it.
//!
//! Pre-bd-cfl67 this transform stored an empty artifact whose
//! `path` field carried the *source* path; the writer then opened
//! that source path with truncating write semantics and zeroed the
//! user's file. The artifact-store route is gone; the
//! [`crate::artifact::Artifact`] contract now requires relative
//! paths, and this producer no longer touches the artifact store
//! at all.

use std::path::Path;

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Transform that collects user-authored resource dependencies
/// (images, etc.) referenced from the AST and records them as
/// copy intents on [`RenderContext::resource_copies`].
///
/// Walks through all blocks and inlines, identifying external
/// resources that need to land in the output tree for the rendered
/// HTML to function. The destination is the
/// [`crate::resource_resolver::ResourceResolverContext::page_dir`]
/// joined with the URL as written in the source — so a
/// `![](figs/diagram.png)` inside `docs/index.qmd` becomes a copy
/// from `<input_dir>/docs/figs/diagram.png` to
/// `<output_dir>/docs/figs/diagram.png`.
pub struct ResourceCollectorTransform;

impl ResourceCollectorTransform {
    /// Create a new resource collector transform.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ResourceCollectorTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ResourceCollectorTransform {
    fn name(&self) -> &str {
        "resource-collector"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let input_dir = ctx.document.input.parent().unwrap_or(Path::new("."));

        // Without a resolver, we have no destination to compute.
        // This happens in some unit-test scaffolding paths; let the
        // walk be a no-op rather than guess a destination.
        let Some(resolver) = ctx.resource_resolver.as_ref() else {
            tracing::debug!("resource-collector: no resolver attached to context; skipping");
            return Ok(());
        };
        // In VFS-root mode (WASM hub-client preview) the synthetic
        // `page_output` lives at the root of the VFS regardless of
        // the source qmd's depth, so `page_dir().join("../hero.png")`
        // escapes the VFS root. The hub-client's parent-side asset
        // walker reads bytes directly from the VFS source path
        // (matching the bd-3gtn-era "skip empty content" behavior the
        // old artifact-store route degraded to), so the producer
        // doesn't need to emit copies at all in that mode.
        if resolver.is_vfs_root_mode() {
            tracing::debug!(
                "resource-collector: vfs_root mode — asset walker handles VFS reads directly; skipping copy intents"
            );
            return Ok(());
        }
        let page_dir = resolver.page_dir().to_path_buf();

        let mut collector = ResourceVisitor::new(
            input_dir,
            &page_dir,
            &ctx.project.dir,
            &ctx.project.output_dir,
        );
        for block in &ast.blocks {
            collector.visit_block(block);
        }

        let count = collector.copies.len();
        ctx.resource_copies.extend(collector.copies);

        tracing::debug!("Collected {} resource copy intent(s) from document", count);
        Ok(())
    }
}

/// Visitor that collects user-authored resources from the AST.
///
/// `input_dir` is the directory containing the source qmd —
/// relative URLs in the document resolve against it (the
/// source-side anchor). `output_dir` is the directory containing
/// the page's rendered HTML — the *same* relative URL must land
/// there in the output tree (the destination-side anchor).
struct ResourceVisitor<'a> {
    input_dir: &'a Path,
    output_dir: &'a Path,
    /// Source-side anchor for site-root-relative (`/...`) URLs — the
    /// project root (decision 4 of
    /// bd-root-relative-paths-design-fc5pvkcv).
    root_input_dir: &'a Path,
    /// Destination-side anchor for site-root-relative URLs — the
    /// project output root.
    root_output_dir: &'a Path,
    /// Copy intents in discovery order, each carrying the source span
    /// of the reference that produced it. The producer dedupes by URL
    /// — see [`Self::collect_resource`].
    copies: Vec<crate::render::ResourceCopyIntent>,
    /// Set of URLs already added, for dedup within a single page.
    seen_urls: std::collections::HashSet<String>,
}

impl<'a> ResourceVisitor<'a> {
    fn new(
        input_dir: &'a Path,
        output_dir: &'a Path,
        root_input_dir: &'a Path,
        root_output_dir: &'a Path,
    ) -> Self {
        Self {
            input_dir,
            output_dir,
            root_input_dir,
            root_output_dir,
            copies: Vec::new(),
            seen_urls: std::collections::HashSet::new(),
        }
    }

    fn visit_block(&mut self, block: &Block) {
        match block {
            Block::Paragraph(p) => {
                for inline in &p.content {
                    self.visit_inline(inline);
                }
            }
            Block::Plain(p) => {
                for inline in &p.content {
                    self.visit_inline(inline);
                }
            }
            Block::BlockQuote(bq) => {
                for block in &bq.content {
                    self.visit_block(block);
                }
            }
            Block::OrderedList(ol) => {
                for item in &ol.content {
                    for block in item {
                        self.visit_block(block);
                    }
                }
            }
            Block::BulletList(bl) => {
                for item in &bl.content {
                    for block in item {
                        self.visit_block(block);
                    }
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &dl.content {
                    for inline in term {
                        self.visit_inline(inline);
                    }
                    for def in defs {
                        for block in def {
                            self.visit_block(block);
                        }
                    }
                }
            }
            Block::Header(h) => {
                for inline in &h.content {
                    self.visit_inline(inline);
                }
            }
            Block::Div(d) => {
                for block in &d.content {
                    self.visit_block(block);
                }
            }
            Block::Figure(f) => {
                for block in &f.content {
                    self.visit_block(block);
                }
            }
            Block::Table(t) => {
                // Visit table caption
                if let Some(short) = &t.caption.short {
                    for inline in short {
                        self.visit_inline(inline);
                    }
                }
                if let Some(long) = &t.caption.long {
                    for block in long {
                        self.visit_block(block);
                    }
                }
                // Visit table cells
                for row in t.head.rows.iter().chain(t.foot.rows.iter()) {
                    for cell in &row.cells {
                        for block in &cell.content {
                            self.visit_block(block);
                        }
                    }
                }
                for body in &t.bodies {
                    for row in &body.body {
                        for cell in &row.cells {
                            for block in &cell.content {
                                self.visit_block(block);
                            }
                        }
                    }
                }
            }
            Block::LineBlock(lb) => {
                for line in &lb.content {
                    for inline in line {
                        self.visit_inline(inline);
                    }
                }
            }
            Block::Custom(c) => {
                // Visit custom node slots
                for (_name, slot) in &c.slots {
                    match slot {
                        Slot::Block(block) => {
                            self.visit_block(block);
                        }
                        Slot::Blocks(blocks) => {
                            for block in blocks {
                                self.visit_block(block);
                            }
                        }
                        Slot::Inline(inline) => {
                            self.visit_inline(inline);
                        }
                        Slot::Inlines(inlines) => {
                            for inline in inlines {
                                self.visit_inline(inline);
                            }
                        }
                    }
                }
            }
            // These don't contain nested content
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_)
            | Block::CaptionBlock(_) => {}
        }
    }

    fn visit_inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Image(img) => {
                // Collect image resource - target is (url, title) tuple.
                // Prefer the URL's own span (underlines just the path);
                // fall back to the whole-image span when absent.
                let origin = img
                    .target_source
                    .url
                    .clone()
                    .unwrap_or_else(|| img.source_info.clone());
                self.collect_resource(&img.target.0, origin);
            }
            Inline::Link(link) => {
                // Visit link content
                for inline in &link.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Emph(e) => {
                for inline in &e.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Underline(u) => {
                for inline in &u.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Strong(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Strikeout(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Superscript(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Subscript(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::SmallCaps(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Quoted(q) => {
                for inline in &q.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Cite(c) => {
                for inline in &c.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Span(s) => {
                for inline in &s.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Note(n) => {
                for block in &n.content {
                    self.visit_block(block);
                }
            }
            Inline::Insert(i) => {
                for inline in &i.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Delete(d) => {
                for inline in &d.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Highlight(h) => {
                for inline in &h.content {
                    self.visit_inline(inline);
                }
            }
            Inline::EditComment(e) => {
                for inline in &e.content {
                    self.visit_inline(inline);
                }
            }
            Inline::Custom(c) => {
                // Visit custom node slots
                for (_name, slot) in &c.slots {
                    match slot {
                        Slot::Block(block) => {
                            self.visit_block(block);
                        }
                        Slot::Blocks(blocks) => {
                            for block in blocks {
                                self.visit_block(block);
                            }
                        }
                        Slot::Inline(inline) => {
                            self.visit_inline(inline);
                        }
                        Slot::Inlines(inlines) => {
                            for inline in inlines {
                                self.visit_inline(inline);
                            }
                        }
                    }
                }
            }
            // These don't contain nested content or resources
            Inline::Str(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Code(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_) => {}
        }
    }

    fn collect_resource(&mut self, url: &str, origin: quarto_source_map::SourceInfo) {
        // Skip external URLs and inlined data URIs — nothing on
        // disk to copy.
        if url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("data:")
            || url.starts_with("//")
        {
            return;
        }
        if !self.seen_urls.insert(url.to_string()) {
            return;
        }

        // A leading `/` means site-root-relative (decision 4 of
        // bd-root-relative-paths-design-fc5pvkcv): anchor at the
        // project root / output root instead of the page's dirs.
        // Anchoring at the root also keeps this safe — `..` popping in
        // URL space cannot climb above the join base, so `/etc/passwd`
        // probes `<project>/etc/passwd`, never the filesystem root.
        // (In the pipeline, `LinkRewriteTransform` has usually already
        // rebased such URLs to page-relative by the time this runs;
        // this arm keeps the collector correct on its own, and gives
        // `collect_referenced_asset_urls` — whose anchors are empty —
        // the doc-dir-relative form the preview asset sync needs.)
        let (src, dest) = if let Some(stripped) = url.strip_prefix('/') {
            if stripped.is_empty() {
                return;
            }
            (
                self.root_input_dir.join(stripped),
                self.root_output_dir.join(stripped),
            )
        } else {
            (self.input_dir.join(url), self.output_dir.join(url))
        };
        self.copies
            .push(crate::render::ResourceCopyIntent { src, dest, origin });
    }
}

/// Collect the relative URLs of on-disk assets a document references
/// (currently `Image` targets), in document order and deduplicated.
///
/// External (`http://`, `https://`, `//`, `data:`) URLs are skipped — the
/// returned paths are all resolvable against the source directory.
/// Site-root-relative (`/...`) URLs come back stripped of the leading slash:
/// in single-file mode the site root *is* the document's directory
/// (decision 4 of bd-root-relative-paths-design-fc5pvkcv), so the caller's
/// source-dir join lands at the right file. This reuses the same
/// AST traversal as [`ResourceCollectorTransform`] (so nested images — in
/// lists, `Div`s, figures, tables — are found) but without the copy-intent /
/// resolver machinery.
///
/// `q2 preview`'s single-file mode uses this to sync exactly the assets a deck
/// references into the VFS, without walking the deck's directory (which the
/// `bd-tnm3k` safety property forbids). See bd-kpuweafo.
pub fn collect_referenced_asset_urls(blocks: &[Block]) -> Vec<String> {
    // Empty anchors (page-relative and site-root-relative alike):
    // `collect_resource` does the external filtering and dedup we want, and
    // stores `Path::new("").join(url)` (== the relative URL) as the copy
    // source. We read those back as the raw URLs.
    let anchor = Path::new("");
    let mut visitor = ResourceVisitor::new(anchor, anchor, anchor, anchor);
    for block in blocks {
        visitor.visit_block(block);
    }
    visitor
        .copies
        .into_iter()
        .map(|intent| intent.src.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::{Image, Inline, Str};
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::{BinaryDependencies, RenderContext};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),

            ..Default::default()
        }
    }

    /// Build a website-style resolver pointing the page at
    /// `<project>/_site/doc.html`, so the output dir is distinct
    /// from the input dir and we can observe the destination path
    /// the collector computes.
    fn website_resolver_for_doc() -> crate::resource_resolver::ResourceResolverContext {
        crate::resource_resolver::ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/doc.html",
            "site_libs",
            "doc",
        )
    }

    /// Build an `Image` inline with the given target URL.
    fn image(url: &str) -> Inline {
        Inline::Image(Image {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            content: vec![],
            target: (url.to_string(), String::new()),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(inlines: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content: inlines,
            source_info: dummy_source_info(),
        })
    }

    /// bd-kpuweafo: `collect_referenced_asset_urls` returns document-relative
    /// image URLs (including nested ones), in order, deduped; external URLs
    /// are dropped. The single-file preview uses this to sync exactly the
    /// assets a deck references into the VFS.
    ///
    /// Leading-`/` URLs are site-root-relative (decision 4 of
    /// bd-root-relative-paths-design-fc5pvkcv); in single-file mode the
    /// site root is the document's own directory, so they come back
    /// stripped of the slash and the caller's source-dir join lands at
    /// the right file. (Before that decision they were dropped as
    /// "filesystem-absolute".)
    #[test]
    fn collect_referenced_asset_urls_returns_relative_images_only() {
        let blocks = vec![
            para(vec![image("./sibling-image.png")]),
            // Nested in a bullet list — must still be found.
            Block::BulletList(quarto_pandoc_types::block::BulletList {
                content: vec![vec![para(vec![image("sub/diagram.svg")])]],
                source_info: dummy_source_info(),
            }),
            // External / root-relative / duplicate.
            para(vec![
                image("https://example.com/remote.png"),
                image("/hero.png"),
                image("data:image/png;base64,AAAA"),
                image("./sibling-image.png"),
            ]),
        ];

        let urls = collect_referenced_asset_urls(&blocks);
        assert_eq!(
            urls,
            vec![
                "./sibling-image.png".to_string(),
                "sub/diagram.svg".to_string(),
                "hero.png".to_string(),
            ],
            "relative image URLs in order, deduped; external dropped; \
             leading-/ mapped to doc-dir-relative"
        );
    }

    /// R5 (bd-cfl67): the collector pushes a `(src, dest)` pair
    /// into `ctx.resource_copies`, where `src` is the source-tree
    /// path the qmd points at and `dest` is the matching position
    /// under the page's output dir. **It no longer stores anything
    /// in `ctx.artifacts`** — the artifact-store route was the
    /// source-truncation footgun.
    #[tokio::test]
    async fn test_collects_local_images() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Image(Image {
                    attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                    content: vec![Inline::Str(Str {
                        text: "alt text".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    target: ("images/photo.png".to_string(), String::new()),
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(website_resolver_for_doc());

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        assert_eq!(ctx.resource_copies.len(), 1);
        assert_eq!(
            ctx.resource_copies[0].src,
            PathBuf::from("/project/images/photo.png"),
            "src under input_dir",
        );
        assert_eq!(
            ctx.resource_copies[0].dest,
            PathBuf::from("/project/_site/images/photo.png"),
            "dest under page_dir, same relative position",
        );
        // The artifact store is untouched — the contract is that
        // user resources flow through `resource_copies`, not
        // through `artifacts`.
        assert!(ctx.artifacts.is_empty());
    }

    /// Duplicate URLs in the same document are deduped — we emit
    /// exactly one copy intent per unique URL.
    #[tokio::test]
    async fn test_dedupes_repeated_urls() {
        let image = |url: &str| {
            Inline::Image(Image {
                attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                content: vec![],
                target: (url.to_string(), String::new()),
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            })
        };

        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![image("photo.png"), image("photo.png")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(website_resolver_for_doc());

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        assert_eq!(ctx.resource_copies.len(), 1);
    }

    /// Decision 4 (bd-root-relative-paths-design-fc5pvkcv): a leading
    /// `/` in a resource URL means site-root-relative. The collector
    /// anchors such URLs at the project root (source side) and the
    /// output root (destination side) instead of skipping them — a
    /// page two levels deep referencing `/images/x.svg` copies
    /// `<project>/images/x.svg` → `<output>/images/x.svg`.
    #[tokio::test]
    async fn test_collects_root_absolute_anchored_at_project_root() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![para(vec![image("/images/x.svg")])],
        };

        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/deep/deeper/doc.qmd")],
            output_dir: PathBuf::from("/project/_site"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/deep/deeper/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(crate::resource_resolver::ResourceResolverContext::website(
            "/project/_site",
            "/project/_site/deep/deeper/doc.html",
            "site_libs",
            "doc",
        ));

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        assert_eq!(ctx.resource_copies.len(), 1);
        assert_eq!(
            ctx.resource_copies[0].src,
            PathBuf::from("/project/images/x.svg"),
            "src anchored at the project root, not the doc dir",
        );
        assert_eq!(
            ctx.resource_copies[0].dest,
            PathBuf::from("/project/_site/images/x.svg"),
            "dest anchored at the output root, not the page dir",
        );
    }

    #[tokio::test]
    async fn test_skips_external_urls() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Image(Image {
                    attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                    content: vec![],
                    target: ("https://example.com/image.png".to_string(), String::new()),
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(website_resolver_for_doc());

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // No external URL → no copy intent.
        assert!(ctx.resource_copies.is_empty());
    }

    #[tokio::test]
    async fn test_skips_data_urls() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Image(Image {
                    attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
                    content: vec![],
                    target: ("data:image/png;base64,abc123".to_string(), String::new()),
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                    target_source: TargetSourceInfo::empty(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.resource_resolver = Some(website_resolver_for_doc());

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // No data URL → no copy intent.
        assert!(ctx.resource_copies.is_empty());
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = ResourceCollectorTransform::new();
        assert_eq!(transform.name(), "resource-collector");
    }
}
