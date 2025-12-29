/*
 * resource_collector.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that collects resource dependencies from the AST.
 */

//! Resource collection transform.
//!
//! This transform walks the AST and collects resource dependencies:
//! - Image files referenced in the document
//! - Other embedded resources
//!
//! Resources are stored in the ArtifactStore for later processing.

use std::path::{Path, PathBuf};

use quarto_pandoc_types::Slot;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::artifact::Artifact;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that collects resource dependencies from the AST.
///
/// This walks through all blocks and inlines, identifying external resources
/// that need to be available for the rendered output (e.g., images).
///
/// Resources are stored in the ArtifactStore with the key prefix `resource:`.
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

impl AstTransform for ResourceCollectorTransform {
    fn name(&self) -> &str {
        "resource-collector"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let base_dir = ctx.document.input.parent().unwrap_or(Path::new("."));
        let mut collector = ResourceVisitor::new(base_dir);

        // Walk the AST and collect resources
        for block in &ast.blocks {
            collector.visit_block(block);
        }

        // Store collected resources in the artifact store
        for (i, resource) in collector.resources.iter().enumerate() {
            let key = format!("resource:image:{}", i);
            ctx.artifacts.store(
                key,
                Artifact::from_path(resource.clone(), "application/octet-stream"),
            );
        }

        tracing::debug!(
            "Collected {} resource(s) from document",
            collector.resources.len()
        );

        Ok(())
    }
}

/// Visitor that collects resources from the AST.
struct ResourceVisitor<'a> {
    base_dir: &'a Path,
    resources: Vec<PathBuf>,
}

impl<'a> ResourceVisitor<'a> {
    fn new(base_dir: &'a Path) -> Self {
        Self {
            base_dir,
            resources: Vec::new(),
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
                // Collect image resource - target is (url, title) tuple
                self.collect_resource(&img.target.0);
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
            | Inline::Attr(_, _) => {}
        }
    }

    fn collect_resource(&mut self, url: &str) {
        // Skip external URLs
        if url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("data:")
            || url.starts_with("//")
        {
            return;
        }

        // Resolve relative path
        let path = if url.starts_with('/') {
            PathBuf::from(url)
        } else {
            self.base_dir.join(url)
        };

        // Add to resources if not already present
        if !self.resources.contains(&path) {
            self.resources.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::attr::{AttrSourceInfo, TargetSourceInfo};
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::{Image, Inline, Str};
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectContext};
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
            config: None,
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    #[test]
    fn test_collects_local_images() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
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

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Check that the image was collected
        assert!(ctx.artifacts.get("resource:image:0").is_some());
    }

    #[test]
    fn test_skips_external_urls() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
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

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should not collect external URLs
        assert!(ctx.artifacts.get("resource:image:0").is_none());
    }

    #[test]
    fn test_skips_data_urls() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::meta::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: dummy_source_info(),
            },
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

        let transform = ResourceCollectorTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        // Should not collect data URLs
        assert!(ctx.artifacts.get("resource:image:0").is_none());
    }

    #[test]
    fn test_transform_name() {
        let transform = ResourceCollectorTransform::new();
        assert_eq!(transform.name(), "resource-collector");
    }
}
