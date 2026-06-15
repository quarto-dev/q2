/*
 * title_block.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that adds a title header from metadata if not present.
 */

//! Title block transform.
//!
//! This transform ensures the document has a visible title by:
//! 1. Checking if there's an existing level-1 header in the document
//! 2. If not, prepending a level-1 header from the `title` metadata
//!
//! ## Template Mode Behavior
//!
//! The transform behavior depends on which HTML template mode is being used:
//!
//! - **Minimal mode** (`minimal: true` or `theme: none/pandoc`):
//!   The transform adds an h1 header to the AST body if title metadata exists.
//!   This is necessary because the minimal template renders `$body$` directly.
//!
//! - **Full mode** (default):
//!   The transform does NOT add an h1 header because the full template generates
//!   a structured `<header id="title-block-header">` from metadata variables.
//!   Adding an h1 here would result in duplicate titles.
//!
//! This is a simplified version of Quarto's title block handling for
//! prototyping purposes.

use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
use quarto_pandoc_types::block::{Block, Header};
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{By, SourceInfo};
use smallvec::smallvec;

use crate::Result;
use crate::format::is_minimal_html;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Transform that adds a title header from metadata if not present.
///
/// If the document has no level-1 header but has a `title` in metadata,
/// this transform prepends a level-1 header with the title text.
///
/// ## Mode-Aware Behavior
///
/// This transform only adds the h1 header in **minimal mode** (`minimal: true`,
/// `theme: none`, or `theme: pandoc`). In **full mode** (default), the template
/// generates a structured title block header, so we don't add an h1 here.
pub struct TitleBlockTransform;

impl TitleBlockTransform {
    /// Create a new title block transform.
    pub fn new() -> Self {
        Self
    }

    /// Check if we should add an h1 header based on template mode.
    ///
    /// Returns true only for minimal mode where the template doesn't
    /// generate a title block.
    fn should_add_h1(meta: &ConfigValue, is_html: bool) -> bool {
        // For HTML formats, check if using minimal template
        // (where we need to add h1 to the body)
        if is_html {
            is_minimal_html(meta)
        } else {
            // For non-HTML formats (PDF, DOCX, etc.), always add the h1
            // since there's no template-based title block
            true
        }
    }
}

impl Default for TitleBlockTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TitleBlockTransform {
    fn name(&self) -> &str {
        "title-block"
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // In full template mode (default for HTML), the template generates
        // the title block header. Skip adding h1 to avoid duplication.
        if !Self::should_add_h1(&ast.meta, ctx.format.is_html()) {
            return Ok(());
        }

        // Check if there's already a level-1 header
        if has_level1_header(&ast.blocks) {
            return Ok(());
        }

        // Try to get title from metadata
        if let Some(title_inlines) = extract_title_inlines(&ast.meta) {
            // Create a level-1 header with the title, preserving inline
            // markup (code spans, emphasis, …).
            let header = create_title_header(title_inlines);
            ast.blocks.insert(0, header);
        }

        Ok(())
    }
}

/// Check if the document has any level-1 header.
fn has_level1_header(blocks: &[Block]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, Block::Header(h) if h.level == 1))
}

/// Extract the document title from metadata as Pandoc inlines, preserving
/// inline markup (code spans, emphasis, …).
///
/// Returns `None` when there is no `title` or it carries no renderable
/// content. A bare string scalar becomes a single `Str`; Pandoc inlines are
/// used verbatim; Pandoc blocks are flattened to their inline content (a
/// title is always rendered as inline content in an `<h1>`).
fn extract_title_inlines(meta: &ConfigValue) -> Option<Vec<Inline>> {
    let ConfigValueKind::Map(entries) = &meta.value else {
        return None;
    };

    let title_entry = entries.iter().find(|e| e.key == "title")?;
    value_to_inlines(&title_entry.value)
}

/// Convert a metadata value to title inlines.
fn value_to_inlines(meta: &ConfigValue) -> Option<Vec<Inline>> {
    // A bare string scalar becomes a single Str. (The placeholder
    // source_info is overwritten with the synthetic title-block provenance
    // by `create_title_header`.)
    if let Some(s) = meta.as_str() {
        return Some(vec![Inline::Str(Str {
            text: s.to_string(),
            source_info: meta.source_info.clone(),
        })]);
    }
    match &meta.value {
        ConfigValueKind::PandocInlines(content) => Some(content.clone()),
        ConfigValueKind::PandocBlocks(content) => Some(blocks_to_inlines(content)),
        _ => None,
    }
}

/// Flatten the inline content of `Plain`/`Paragraph` blocks. A title given
/// as block-level metadata still renders as inline content inside the `<h1>`.
fn blocks_to_inlines(blocks: &[Block]) -> Vec<Inline> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Plain(p) => out.extend(p.content.iter().cloned()),
            Block::Paragraph(p) => out.extend(p.content.iter().cloned()),
            _ => {}
        }
    }
    out
}

/// Create a level-1 header block from the given title inlines.
///
/// The synthesized Header carries `Generated { by: title_block(), from: [] }`
/// provenance, which is atomic per `By::is_atomic_kind`. This is the boundary
/// the consumers respect: the preview's edit-back gate marks the whole
/// subtree read-only (it threads a no-op `setLocalAst` down from the Header),
/// and the incremental writer re-serializes the whole block because a
/// `Generated` node reports no preimage. We stamp the *entire* inline subtree
/// with the same provenance so the synthetic h1 is uniformly
/// "generated by title-block" — the title's real bytes live in the
/// front-matter metadata, not in the body where this h1 is injected.
fn create_title_header(content: Vec<Inline>) -> Block {
    let source_info = SourceInfo::Generated {
        by: By::title_block(),
        from: smallvec![],
    };
    let mut content = content;
    for inline in &mut content {
        stamp_generated(inline, &source_info);
    }
    Block::Header(Header {
        level: 1,
        attr: empty_attr(),
        content,
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Recursively stamp `provenance` over an inline subtree.
fn stamp_generated(inline: &mut Inline, provenance: &SourceInfo) {
    *inline.source_info_mut() = provenance.clone();
    if let Some(children) = child_inlines_mut(inline) {
        for child in children {
            stamp_generated(child, provenance);
        }
    }
}

/// Mutable access to an inline's child inlines, for container variants.
///
/// `Note` (whose children are blocks) and leaf variants return `None` — a
/// footnote in a title is not a meaningful case, and stamping the `Note`
/// node itself suffices for the atomic boundary.
fn child_inlines_mut(inline: &mut Inline) -> Option<&mut quarto_pandoc_types::inline::Inlines> {
    match inline {
        Inline::Emph(e) => Some(&mut e.content),
        Inline::Underline(u) => Some(&mut u.content),
        Inline::Strong(s) => Some(&mut s.content),
        Inline::Strikeout(s) => Some(&mut s.content),
        Inline::Superscript(s) => Some(&mut s.content),
        Inline::Subscript(s) => Some(&mut s.content),
        Inline::SmallCaps(s) => Some(&mut s.content),
        Inline::Quoted(q) => Some(&mut q.content),
        Inline::Cite(c) => Some(&mut c.content),
        Inline::Link(l) => Some(&mut l.content),
        Inline::Image(i) => Some(&mut i.content),
        Inline::Span(s) => Some(&mut s.content),
        Inline::Insert(i) => Some(&mut i.content),
        Inline::Delete(d) => Some(&mut d.content),
        Inline::Highlight(h) => Some(&mut h.content),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::block::Paragraph;
    use quarto_pandoc_types::inline::Code;
    use quarto_source_map::{FileId, Location, Range};
    use std::path::PathBuf;

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
        }
    }

    fn meta_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    // === Minimal mode tests (h1 SHOULD be added) ===

    #[tokio::test]
    async fn test_minimal_mode_adds_title_header_when_missing() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    meta_entry(
                        "title",
                        ConfigValue::new_string("My Document", dummy_source_info()),
                    ),
                    meta_entry("minimal", ConfigValue::new_bool(true, dummy_source_info())),
                ],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should now have 2 blocks: header + paragraph
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the title header
        match &ast.blocks[0] {
            Block::Header(h) => {
                assert_eq!(h.level, 1);
                match &h.content[0] {
                    Inline::Str(s) => assert_eq!(s.text, "My Document"),
                    _ => panic!("Expected Str inline"),
                }
            }
            _ => panic!("Expected Header block"),
        }
    }

    /// Minimal-mode title with inline Markdown (a code span) must keep its
    /// inline structure in the injected `<h1>` — the `Code` inline must
    /// survive rather than being flattened to a single `Str`. (bd-5706gcrq)
    #[tokio::test]
    async fn test_minimal_mode_preserves_inline_markup_in_title() {
        // title: Branding with `_brand.yml`
        let title_inlines = vec![
            Inline::Str(Str {
                text: "Branding with ".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::Code(Code {
                attr: empty_attr(),
                text: "_brand.yml".to_string(),
                source_info: dummy_source_info(),
                attr_source: AttrSourceInfo::empty(),
            }),
        ];
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    meta_entry(
                        "title",
                        ConfigValue::new_inlines(title_inlines, dummy_source_info()),
                    ),
                    meta_entry("minimal", ConfigValue::new_bool(true, dummy_source_info())),
                ],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        let Block::Header(h) = &ast.blocks[0] else {
            panic!("Expected Header block");
        };
        assert_eq!(h.level, 1);
        assert_eq!(h.content.len(), 2, "title inline structure must be kept");
        match &h.content[0] {
            Inline::Str(s) => assert_eq!(s.text, "Branding with "),
            other => panic!("Expected leading Str, got {other:?}"),
        }
        match &h.content[1] {
            Inline::Code(c) => assert_eq!(c.text, "_brand.yml"),
            other => panic!("Expected Code inline to survive, got {other:?}"),
        }
        // The whole injected subtree must carry title-block Generated
        // provenance (the atomic boundary that gates edit-back and forces
        // full re-serialization in the incremental writer).
        assert!(
            matches!(&h.source_info, SourceInfo::Generated { by, .. } if by.is_kind("title-block")),
            "header must be generated-by-title-block"
        );
        for child in &h.content {
            assert!(
                matches!(child.source_info(), SourceInfo::Generated { by, .. } if by.is_kind("title-block")),
                "title inline {child:?} must carry title-block provenance"
            );
        }
    }

    #[tokio::test]
    async fn test_minimal_mode_does_not_add_when_h1_exists() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    meta_entry(
                        "title",
                        ConfigValue::new_string("My Document", dummy_source_info()),
                    ),
                    meta_entry("minimal", ConfigValue::new_bool(true, dummy_source_info())),
                ],
                dummy_source_info(),
            ),
            blocks: vec![
                Block::Header(Header {
                    level: 1,
                    attr: empty_attr(),
                    content: vec![Inline::Str(Str {
                        text: "Existing Title".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                    attr_source: AttrSourceInfo::empty(),
                }),
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Str(Str {
                        text: "Content".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    source_info: dummy_source_info(),
                }),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should still have 2 blocks (no new header added)
        assert_eq!(ast.blocks.len(), 2);

        // First block should be the existing header
        match &ast.blocks[0] {
            Block::Header(h) => match &h.content[0] {
                Inline::Str(s) => assert_eq!(s.text, "Existing Title"),
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Header block"),
        }
    }

    #[tokio::test]
    async fn test_minimal_mode_does_nothing_without_title_metadata() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![meta_entry(
                    "minimal",
                    ConfigValue::new_bool(true, dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should still have 1 block (no header added)
        assert_eq!(ast.blocks.len(), 1);
    }

    // === Full mode tests (h1 should NOT be added, template handles it) ===

    #[tokio::test]
    async fn test_full_mode_does_not_add_title_header() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![meta_entry(
                    "title",
                    ConfigValue::new_string("My Document", dummy_source_info()),
                )],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        // Default HTML format is full mode (no minimal/theme in ast.meta)
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should still have 1 block - no header added in full mode
        // because the template will render the title block
        assert_eq!(ast.blocks.len(), 1);

        // The only block should be the paragraph
        match &ast.blocks[0] {
            Block::Paragraph(_) => {}
            _ => panic!("Expected Paragraph block, no Header"),
        }
    }

    #[tokio::test]
    async fn test_full_mode_theme_cosmo_does_not_add_header() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    meta_entry(
                        "title",
                        ConfigValue::new_string("My Document", dummy_source_info()),
                    ),
                    meta_entry(
                        "theme",
                        ConfigValue::new_string("cosmo", dummy_source_info()),
                    ),
                ],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // No header added - Bootstrap theme = full mode
        assert_eq!(ast.blocks.len(), 1);
    }

    #[tokio::test]
    async fn test_theme_none_adds_header() {
        let mut ast = Pandoc {
            meta: ConfigValue::new_map(
                vec![
                    meta_entry(
                        "title",
                        ConfigValue::new_string("My Document", dummy_source_info()),
                    ),
                    meta_entry(
                        "theme",
                        ConfigValue::new_string("none", dummy_source_info()),
                    ),
                ],
                dummy_source_info(),
            ),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = TitleBlockTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Header should be added in minimal mode
        assert_eq!(ast.blocks.len(), 2);
        assert!(matches!(&ast.blocks[0], Block::Header(_)));
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = TitleBlockTransform::new();
        assert_eq!(transform.name(), "title-block");
    }

    #[test]
    fn test_create_title_header_has_generated_provenance() {
        // Plan 6: the synthesized h1 + inner Str both carry
        // Generated { by: title_block(), from: [] }.
        let block = create_title_header(vec![Inline::Str(Str {
            text: "My Title".to_string(),
            source_info: dummy_source_info(),
        })]);
        let Block::Header(header) = &block else {
            panic!("Expected Header");
        };
        match &header.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "title-block");
                assert!(from.is_empty());
            }
            other => panic!("Expected Generated, got {:?}", other),
        }
        // Inner Str carries the same shape.
        let Inline::Str(s) = &header.content[0] else {
            panic!("Expected Str inside header");
        };
        match &s.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "title-block");
                assert!(from.is_empty());
            }
            other => panic!("Expected Generated, got {:?}", other),
        }
    }
}
