/*
 * link_rewrite.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Body-content link rewriting transform.
//!
//! Walks the AST body, finds every [`Inline::Link`], and rewrites
//! its `target.0` URL to a page-relative URL when it points at
//! another project document. Phase 6 of the website-projects epic.
//!
//! See `claude-notes/plans/2026-04-24-websites-phase-6.md` for the
//! design (especially Decisions 1, 2, 6, 7).
//!
//! ## What it rewrites
//!
//! - Internal `.qmd` references resolved through
//!   [`ProjectIndex`](crate::project::index::ProjectIndex), with
//!   `..` / `.` / leading `/` normalization handled by
//!   [`resolve_doc_relative_href`](super::navigation_href::resolve_doc_relative_href).
//! - Query strings and fragment anchors are preserved across the
//!   rewrite (`other.qmd?x=1#sec` → `other.html?x=1#sec`).
//!
//! ## What it leaves alone
//!
//! - External URLs (`http:`, `https:`, `mailto:`, `tel:`, `ftp:`,
//!   `//host/...`).
//! - Fragment-only anchors (`#section`).
//! - `Image::target.0` — images point at static resources, not
//!   project documents (Q1 doesn't rewrite them either).
//! - Body links inside a document rendered without a
//!   `ProjectIndex` (standalone single-doc render). The transform
//!   short-circuits and the AST is untouched.
//!
//! ## Pipeline placement
//!
//! Runs at the start of the Finalization Phase, after all
//! Navigation Phase Render transforms but before
//! `AppendixStructureTransform`. By this point most semantic
//! transforms (callouts, crossrefs, shortcodes) have already
//! resolved their custom nodes, so the AST is mostly plain Pandoc
//! nodes. The walk recurses through `Inline::Custom` slots
//! defensively, in case any custom nodes remain.

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Inlines};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::project::index::ProjectIndex;
use crate::render::RenderContext;
use crate::resource_resolver::ResourceResolverContext;
use crate::transform::AstTransform;
use crate::transforms::navigation_active::page_relative_source;
use crate::transforms::navigation_href::resolve_doc_relative_href;

/// Body-content link rewriter (Phase 6).
pub struct LinkRewriteTransform;

impl LinkRewriteTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinkRewriteTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LinkRewriteTransform {
    fn name(&self) -> &str {
        "link-rewrite"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Standalone render: no project context, nothing to rewrite.
        // Body hrefs pass through verbatim. See Decision 7.
        let Some(index) = ctx.project_index.as_deref() else {
            return Ok(());
        };

        let resolver = ctx.resource_resolver.as_ref();
        let source = page_relative_source(ctx);

        // Move diagnostics into a local buffer so the helper can
        // push without a borrow cycle on `ctx`.
        let mut local_diags = std::mem::take(&mut ctx.diagnostics);
        let mut rewriter = LinkRewriter {
            source: &source,
            index,
            resolver,
            diagnostics: &mut local_diags,
        };
        for block in &mut ast.blocks {
            rewriter.visit_block(block);
        }
        ctx.diagnostics = local_diags;

        Ok(())
    }
}

struct LinkRewriter<'a> {
    source: &'a str,
    index: &'a ProjectIndex,
    resolver: Option<&'a ResourceResolverContext>,
    diagnostics: &'a mut Vec<quarto_error_reporting::DiagnosticMessage>,
}

impl<'a> LinkRewriter<'a> {
    fn visit_block(&mut self, block: &mut Block) {
        match block {
            Block::Plain(p) => self.visit_inlines(&mut p.content),
            Block::Paragraph(p) => self.visit_inlines(&mut p.content),
            Block::LineBlock(lb) => {
                for line in lb.content.iter_mut() {
                    self.visit_inlines(line);
                }
            }
            Block::BlockQuote(bq) => {
                for b in bq.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::OrderedList(ol) => {
                for item in ol.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::BulletList(bl) => {
                for item in bl.content.iter_mut() {
                    for b in item.iter_mut() {
                        self.visit_block(b);
                    }
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in dl.content.iter_mut() {
                    self.visit_inlines(term);
                    for def in defs.iter_mut() {
                        for b in def.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
            }
            Block::Header(h) => self.visit_inlines(&mut h.content),
            Block::Div(d) => {
                for b in d.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Figure(f) => {
                for b in f.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Block::Table(t) => {
                if let Some(short) = t.caption.short.as_mut() {
                    self.visit_inlines(short);
                }
                if let Some(long) = t.caption.long.as_mut() {
                    for b in long.iter_mut() {
                        self.visit_block(b);
                    }
                }
                for row in t.head.rows.iter_mut().chain(t.foot.rows.iter_mut()) {
                    for cell in row.cells.iter_mut() {
                        for b in cell.content.iter_mut() {
                            self.visit_block(b);
                        }
                    }
                }
                for body in t.bodies.iter_mut() {
                    for row in body.body.iter_mut() {
                        for cell in row.cells.iter_mut() {
                            for b in cell.content.iter_mut() {
                                self.visit_block(b);
                            }
                        }
                    }
                }
            }
            Block::CaptionBlock(cb) => self.visit_inlines(&mut cb.content),
            Block::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            // Variants without nested rewritable content.
            Block::CodeBlock(_)
            | Block::RawBlock(_)
            | Block::HorizontalRule(_)
            | Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_) => {}
        }
    }

    fn visit_inlines(&mut self, inlines: &mut Inlines) {
        for inline in inlines.iter_mut() {
            self.visit_inline(inline);
        }
    }

    fn visit_inline(&mut self, inline: &mut Inline) {
        match inline {
            Inline::Link(link) => {
                // Recurse into link content first — a link's inner
                // text could itself contain rewritable inlines
                // (uncommon, but possible after filter passes).
                self.visit_inlines(&mut link.content);
                let new_url = resolve_doc_relative_href(
                    &link.target.0,
                    self.source,
                    self.resolver,
                    Some(self.index),
                    Some("Body link"),
                    self.diagnostics,
                );
                link.target.0 = new_url;
            }
            Inline::Image(img) => {
                // Walk image content (alt-text inlines) but leave
                // `img.target.0` (the image URL) alone — images
                // point at static resources, not project docs.
                self.visit_inlines(&mut img.content);
            }
            Inline::Emph(e) => self.visit_inlines(&mut e.content),
            Inline::Underline(u) => self.visit_inlines(&mut u.content),
            Inline::Strong(s) => self.visit_inlines(&mut s.content),
            Inline::Strikeout(s) => self.visit_inlines(&mut s.content),
            Inline::Superscript(s) => self.visit_inlines(&mut s.content),
            Inline::Subscript(s) => self.visit_inlines(&mut s.content),
            Inline::SmallCaps(s) => self.visit_inlines(&mut s.content),
            Inline::Quoted(q) => self.visit_inlines(&mut q.content),
            Inline::Note(n) => {
                for b in n.content.iter_mut() {
                    self.visit_block(b);
                }
            }
            Inline::Span(s) => self.visit_inlines(&mut s.content),
            Inline::Insert(i) => self.visit_inlines(&mut i.content),
            Inline::Delete(d) => self.visit_inlines(&mut d.content),
            Inline::Highlight(h) => self.visit_inlines(&mut h.content),
            Inline::Custom(c) => {
                for (_name, slot) in c.slots.iter_mut() {
                    self.visit_slot(slot);
                }
            }
            // No-op variants: leaves with no rewritable nested content.
            Inline::Str(_)
            | Inline::Cite(_)
            | Inline::Code(_)
            | Inline::Space(_)
            | Inline::SoftBreak(_)
            | Inline::LineBreak(_)
            | Inline::Math(_)
            | Inline::RawInline(_)
            | Inline::Shortcode(_)
            | Inline::NoteReference(_)
            | Inline::Attr(_)
            | Inline::EditComment(_) => {}
        }
    }

    fn visit_slot(&mut self, slot: &mut Slot) {
        match slot {
            Slot::Block(b) => self.visit_block(b),
            Slot::Blocks(bs) => {
                for b in bs.iter_mut() {
                    self.visit_block(b);
                }
            }
            Slot::Inline(i) => self.visit_inline(i),
            Slot::Inlines(is) => self.visit_inlines(is),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::{DOCUMENT_PROFILE_VERSION, DocumentProfile};
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use pampa::toc::TocEntry;
    use quarto_pandoc_types::attr::{Attr, AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::{Block, BulletList, Div, Paragraph};
    use quarto_pandoc_types::config_value::ConfigValue;
    use quarto_pandoc_types::custom::CustomNode;
    use quarto_pandoc_types::inline::{Emph, Image, Inline, Link, Str};
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_pandoc_types::{ConfigMapEntry, Slot};
    use quarto_source_map::SourceInfo;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_profile(source: &str, output_href: &str) -> DocumentProfile {
        DocumentProfile {
            profile_version: DOCUMENT_PROFILE_VERSION,
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some("T".to_string()),
            subtitle: None,
            description: None,
            authors: Vec::new(),
            date: None,
            categories: Vec::new(),
            keywords: Vec::new(),
            image: None,
            draft: false,
            order: None,
            outline: Vec::<TocEntry>::new(),
        }
    }

    fn make_project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/index.qmd")],
            output_dir: PathBuf::from("/project/_site"),
        }
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(Vec::<ConfigMapEntry>::new(), SourceInfo::default())
    }

    fn link_inline(url: &str, text: &str) -> Inline {
        Inline::Link(Link {
            attr: Attr::default(),
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::default(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn image_inline(url: &str, alt: &str) -> Inline {
        Inline::Image(Image {
            attr: Attr::default(),
            content: vec![Inline::Str(Str {
                text: alt.to_string(),
                source_info: SourceInfo::default(),
            })],
            target: (url.to_string(), String::new()),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }

    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::default(),
        })
    }

    /// Run the Link rewrite transform on a synthetic AST.
    ///
    /// `source_doc` is the project-relative path of the doc being
    /// rendered (e.g. `"docs/api.qmd"`).
    /// `output_href` is the target output path used by the resolver
    /// (e.g. `"docs/api.html"`).
    /// `with_index` decides whether to attach a `ProjectIndex` —
    /// `false` exercises the standalone-render no-op branch.
    async fn run(
        blocks: Vec<Block>,
        source_doc: &str,
        output_href: &str,
        index_profiles: Vec<DocumentProfile>,
        with_index: bool,
    ) -> (Vec<Block>, Vec<quarto_error_reporting::DiagnosticMessage>) {
        let project = make_project();
        let input_path = format!("/project/{}", source_doc);
        let doc = DocumentInfo::from_path(&input_path);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if with_index {
            ctx = ctx.with_project_index(Arc::new(ProjectIndex::new(index_profiles)));
        }
        // Wire a website-flavored resolver pinned at the given page.
        let page_output = format!("/project/_site/{}", output_href);
        let stem = std::path::Path::new(output_href)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ctx.resource_resolver = Some(ResourceResolverContext::website(
            "/project/_site",
            page_output,
            "site_libs",
            stem,
        ));
        let mut ast = Pandoc {
            meta: empty_meta(),
            blocks,
        };
        LinkRewriteTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        (ast.blocks, ctx.diagnostics)
    }

    fn first_link_url(blocks: &[Block]) -> &str {
        for b in blocks {
            if let Block::Paragraph(p) = b {
                for i in &p.content {
                    if let Inline::Link(l) = i {
                        return &l.target.0;
                    }
                }
            }
        }
        panic!("no link found");
    }

    /// Plan test 29: standalone render (no `project_index`) is a
    /// no-op; every body link survives unchanged.
    #[tokio::test]
    async fn link_rewrite_skips_when_no_index() {
        let blocks = vec![para(vec![link_inline("about.qmd", "About")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], false).await;
        assert_eq!(first_link_url(&out), "about.qmd");
        assert!(diags.is_empty());
    }

    /// Plan test 30: walking through a Paragraph, multiple Inline
    /// links rewrite.
    #[tokio::test]
    async fn link_rewrite_walks_paragraph_inlines() {
        let blocks = vec![para(vec![
            link_inline("about.qmd", "About"),
            Inline::Str(Str {
                text: " or ".into(),
                source_info: SourceInfo::default(),
            }),
            link_inline("docs/api.qmd", "API"),
        ])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![
                make_profile("about.qmd", "about.html"),
                make_profile("docs/api.qmd", "docs/api.html"),
            ],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        let urls: Vec<&str> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Link(l) => Some(l.target.0.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(urls, vec!["about.html", "docs/api.html"]);
    }

    /// Plan test 31: link nested inside `Emph`.
    #[tokio::test]
    async fn link_rewrite_walks_nested_emph_link() {
        let emph = Inline::Emph(Emph {
            content: vec![link_inline("about.qmd", "About")],
            source_info: SourceInfo::default(),
        });
        let blocks = vec![para(vec![emph])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        let inner = match &p.content[0] {
            Inline::Emph(e) => &e.content,
            _ => unreachable!(),
        };
        match &inner[0] {
            Inline::Link(l) => assert_eq!(l.target.0, "about.html"),
            _ => panic!("expected Link"),
        }
    }

    /// Plan test 32: link inside a `Div`.
    #[tokio::test]
    async fn link_rewrite_walks_div_blocks() {
        let div = Block::Div(Div {
            attr: Attr::default(),
            content: vec![para(vec![link_inline("about.qmd", "About")])],
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        });
        let (out, _) = run(
            vec![div],
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let inner_blocks = match &out[0] {
            Block::Div(d) => &d.content,
            _ => unreachable!(),
        };
        assert_eq!(first_link_url(inner_blocks), "about.html");
    }

    /// Plan test 33: link inside a bullet list item.
    #[tokio::test]
    async fn link_rewrite_walks_lists() {
        let bl = Block::BulletList(BulletList {
            content: vec![vec![para(vec![link_inline("about.qmd", "About")])]],
            source_info: SourceInfo::default(),
        });
        let (out, _) = run(
            vec![bl],
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let item_blocks = match &out[0] {
            Block::BulletList(b) => &b.content[0],
            _ => unreachable!(),
        };
        assert_eq!(first_link_url(item_blocks), "about.html");
    }

    /// Plan test 34: link inside a `Custom` node's `Inlines` slot.
    #[tokio::test]
    async fn link_rewrite_walks_custom_node_slots() {
        // Build a minimal CustomNode with one Inlines slot
        // containing the link. We don't need a registered custom-
        // node type for the walker — it iterates `slots` generically.
        let mut custom = CustomNode::new("test:wrapper", Attr::default(), SourceInfo::default());
        let link = link_inline("about.qmd", "About");
        custom
            .slots
            .insert("content".to_string(), Slot::Inlines(vec![link]));
        let blocks = vec![Block::Custom(custom)];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let custom_out = match &out[0] {
            Block::Custom(c) => c,
            _ => unreachable!(),
        };
        let slot = custom_out.slots.get("content").unwrap();
        let inlines = match slot {
            Slot::Inlines(is) => is,
            _ => unreachable!(),
        };
        match &inlines[0] {
            Inline::Link(l) => assert_eq!(l.target.0, "about.html"),
            _ => panic!("expected Link"),
        }
    }

    /// Plan test 35: external URL passes through.
    #[tokio::test]
    async fn link_rewrite_external_pass_through() {
        let blocks = vec![para(vec![link_inline("https://example.com", "X")])];
        let (out, diags) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        assert_eq!(first_link_url(&out), "https://example.com");
        assert!(diags.is_empty());
    }

    /// Plan test 36: fragment-only anchor passes through.
    #[tokio::test]
    async fn link_rewrite_fragment_pass_through() {
        let blocks = vec![para(vec![link_inline("#section", "X")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], true).await;
        assert_eq!(first_link_url(&out), "#section");
        assert!(diags.is_empty());
    }

    /// Plan test 37: image URLs are not rewritten, but a Link
    /// inside the image's alt-content is.
    #[tokio::test]
    async fn link_rewrite_image_url_unchanged() {
        // Image whose target.0 looks like a project doc — must NOT
        // be rewritten. We don't try the alt-content link here
        // (uncommon shape), just the image URL preservation contract.
        let blocks = vec![para(vec![image_inline("about.qmd", "alt text")])];
        let (out, _) = run(
            blocks,
            "index.qmd",
            "index.html",
            vec![make_profile("about.qmd", "about.html")],
            true,
        )
        .await;
        let p = match &out[0] {
            Block::Paragraph(p) => p,
            _ => unreachable!(),
        };
        match &p.content[0] {
            Inline::Image(i) => assert_eq!(i.target.0, "about.qmd"),
            _ => panic!("expected Image"),
        }
    }

    /// Plan test 38: diagnostic uses the "Body link" source label.
    #[tokio::test]
    async fn link_rewrite_diagnostic_uses_body_link_label() {
        let blocks = vec![para(vec![link_inline("missing.qmd", "X")])];
        let (out, diags) = run(blocks, "index.qmd", "index.html", vec![], true).await;
        assert_eq!(first_link_url(&out), "missing.qmd");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.starts_with("Body link"));
        assert!(diags[0].title.contains("missing.qmd"));
    }
}
