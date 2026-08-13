/*
 * appendix.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Transform that consolidates appendix content into a single appendix container.
 */

//! Appendix structure transform for HTML rendering.
//!
//! This transform collects various appendix-related content and consolidates it
//! into a single appendix container at the end of the document. It runs in the
//! **finalization phase** of the pipeline, after FootnotesTransform and CiteprocTransform.
//!
//! ## Input
//!
//! - Div blocks with class `appendix` (user-defined appendix sections)
//! - Footnotes section (from FootnotesTransform, id="footnotes")
//! - Bibliography (from CiteprocTransform when implemented, id="refs")
//! - License/copyright/citation metadata
//!
//! ## Output
//!
//! A consolidated appendix container at end of document:
//!
//! ```html
//! <div id="quarto-appendix" class="default">
//!   <!-- User appendix sections -->
//!   <!-- Bibliography (if present and not margin) -->
//!   <!-- Footnotes (if present and not margin) -->
//!   <!-- License section (if metadata present) -->
//!   <!-- Copyright section (if metadata present) -->
//!   <!-- Citation section (if metadata present) -->
//! </div>
//! ```
//!
//! ## Configuration
//!
//! - `appendix-style`: Controls appendix processing
//!   - `default` (default): Standard appendix processing
//!   - `plain`: Minimal appendix styling
//!   - `none`: Disable appendix processing
//!
//! - `reference-location`: If `margin`, footnotes are NOT moved into appendix
//! - `citation-location`: If `margin`, bibliography is NOT moved into appendix

use hashlink::LinkedHashMap;
use quarto_pandoc_types::Blocks;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, Div, Header, Paragraph};
use quarto_pandoc_types::inline::{Inline, Link, Space, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};
use smallvec::smallvec;

use quarto_pandoc_types::ConfigValue;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::{AppendixStyle, ReferenceLocation};

/// Transform that consolidates appendix content into a single container.
///
/// This transform is part of the **finalization phase** and runs late in the
/// pipeline, after FootnotesTransform and CiteprocTransform have created their
/// respective sections.
pub struct AppendixStructureTransform;

impl AppendixStructureTransform {
    /// Create a new appendix structure transform.
    pub fn new() -> Self {
        Self
    }

    /// Get the appendix-style configuration.
    fn get_appendix_style(meta: &ConfigValue) -> AppendixStyle {
        meta.get("appendix-style")
            .map(|v| {
                if let Some(b) = v.as_bool() {
                    AppendixStyle::from_bool(b)
                } else if let Some(s) = v.as_plain_text() {
                    // `as_plain_text` (not `as_str`): a bare front-matter string
                    // is stored as `ConfigValueKind::PandocInlines`, for which
                    // `as_str` returns `None`. (bd-y89ihf0i)
                    AppendixStyle::from_str(&s)
                } else {
                    AppendixStyle::default()
                }
            })
            .unwrap_or_default()
    }

    /// Get the reference-location configuration.
    fn get_reference_location(meta: &ConfigValue) -> ReferenceLocation {
        // Use `as_plain_text` (not `as_str`): front-matter string values are
        // stored as `ConfigValueKind::PandocInlines` in document-metadata
        // context, for which `as_str` returns `None`. (bd-9ez3ngt1)
        meta.get("reference-location")
            .and_then(|v| v.as_plain_text())
            .map(|s| ReferenceLocation::from_str(&s))
            .unwrap_or_default()
    }

    /// Check if this is a book format (appendix processing is skipped for books).
    fn is_book_format(meta: &ConfigValue) -> bool {
        meta.get("book").and_then(|v| v.as_bool()).unwrap_or(false)
    }
}

impl Default for AppendixStructureTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for AppendixStructureTransform {
    fn name(&self) -> &str {
        "appendix-structure"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        let meta = &ast.meta;
        let appendix_style = Self::get_appendix_style(meta);
        let reference_location = Self::get_reference_location(meta);

        // Skip appendix processing if disabled or book format
        if !appendix_style.is_enabled() || Self::is_book_format(meta) {
            return Ok(());
        }

        // Collect appendix sections
        let mut appendix_sections: Blocks = Vec::new();

        // 1. Collect user-defined appendix sections (Divs with class "appendix")
        let user_appendices = extract_appendix_divs(&mut ast.blocks);
        appendix_sections.extend(user_appendices);

        // 2. Collect bibliography (if not margin mode)
        // For now, look for Div with id="refs" - CiteprocTransform will create this later
        if reference_location != ReferenceLocation::Margin
            && let Some(bibliography) = extract_bibliography(&mut ast.blocks)
        {
            appendix_sections.push(wrap_bibliography(bibliography, &ast.meta));
        }

        // 3. Collect footnotes section (if not margin mode)
        if reference_location != ReferenceLocation::Margin
            && let Some(footnotes) = extract_footnotes(&mut ast.blocks)
        {
            appendix_sections.push(wrap_footnotes(footnotes, &ast.meta));
        }

        // 4. Create metadata-driven sections
        let meta = &ast.meta;

        // License section
        if let Some(license_section) = create_license_section(meta) {
            appendix_sections.push(license_section);
        }

        // Copyright section
        if let Some(copyright_section) = create_copyright_section(meta) {
            appendix_sections.push(copyright_section);
        }

        // Citation section
        if let Some(citation_section) = create_citation_section(meta) {
            appendix_sections.push(citation_section);
        }

        // Only create appendix container if we have content
        if !appendix_sections.is_empty() {
            let appendix_class = appendix_style.as_str().to_string();
            let appendix_container = create_appendix_container(appendix_sections, &appendix_class);
            ast.blocks.push(appendix_container);
        }

        Ok(())
    }
}

/// Extract Div blocks with class "appendix" from the document.
fn extract_appendix_divs(blocks: &mut Vec<Block>) -> Blocks {
    let mut appendix_divs = Vec::new();

    blocks.retain(|block| {
        if let Block::Div(div) = block
            && div.attr.1.contains(&"appendix".to_string())
        {
            appendix_divs.push(block.clone());
            return false; // Remove from original position
        }
        true
    });

    appendix_divs
}

/// Extract the bibliography block (Div with id="refs" or class="references").
fn extract_bibliography(blocks: &mut Vec<Block>) -> Option<Block> {
    let mut bibliography = None;

    blocks.retain(|block| {
        if let Block::Div(div) = block {
            // Check for id="refs" or class="references"
            if div.attr.0 == "refs" || div.attr.1.contains(&"references".to_string()) {
                bibliography = Some(block.clone());
                return false; // Remove from original position
            }
        }
        true
    });

    bibliography
}

/// Extract the footnotes section (Div with id="footnotes").
fn extract_footnotes(blocks: &mut Vec<Block>) -> Option<Block> {
    let mut footnotes = None;

    blocks.retain(|block| {
        if let Block::Div(div) = block
            && div.attr.0 == "footnotes"
        {
            footnotes = Some(block.clone());
            return false; // Remove from original position
        }
        true
    });

    footnotes
}

/// The localized title for an appendix section.
///
/// Precedence follows `toc_generate.rs` (decided under bd-llhlzd7p): the
/// resolved `section-title-*` term when [`LanguageResolveStage`] has run,
/// otherwise the English literal. `LanguageTerms::from_meta` returns `None`
/// when the stage has not run — the case in stage-less unit tests — which is
/// what the literal tail is for.
///
/// There is deliberately **no** per-document override tier here: unlike
/// `toc-title`, Quarto 1 exposes no `footnotes-title`-style option for these
/// sections, so metadata is not consulted beyond the language table.
///
/// [`LanguageResolveStage`]: crate::stage::stages::language_resolve
fn appendix_title(meta: &ConfigValue, term_key: &str, english: &str) -> String {
    crate::language::LanguageTerms::from_meta(meta)
        .and_then(|terms| terms.get(term_key).map(str::to_string))
        // A term explicitly set to null round-trips as an empty string; an
        // untitled section is never what the reader wants, so fall through.
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| english.to_string())
}

/// Split a title into canonical Pandoc inlines.
///
/// Titles are `Str`/`Space` alternations rather than one `Str` carrying
/// embedded spaces, which is what the AST means by a run of text. Every
/// English appendix title happens to be a single word, so this only shows up
/// once localized — `section-title-copyright` is "Derechos de autor" in
/// Spanish and "Droits d'auteur" in French.
fn title_inlines(title: &str, source_info: &SourceInfo) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for (i, word) in title.split_whitespace().enumerate() {
        if i > 0 {
            inlines.push(Inline::Space(Space {
                source_info: source_info.clone(),
            }));
        }
        inlines.push(Inline::Str(Str {
            text: word.to_string(),
            source_info: source_info.clone(),
        }));
    }
    inlines
}

/// Build the level-2 heading that titles an appendix section.
///
/// Mirrors Quarto 1's `prependHeading` + `headingClasses`
/// (`format-html-appendix.ts:98`): no id — the heading is not linkable and
/// does not enter the TOC — and the classes `anchored quarto-appendix-heading`.
///
/// `quarto-appendix-heading` drives real styling that q2 already ships
/// (`resources/scss/bootstrap/_bootstrap-rules.scss`). `anchored` is inert
/// here: in Quarto 1 it is a selector hook AnchorJS reads to inject heading
/// anchor links, and q2 ships neither that runtime nor any rule matching the
/// class. It is emitted anyway so the appendix is already correct when
/// heading anchors land — see bd-5kf2dnw4, which must skip headings that
/// already carry the class (pushing onto a `Vec<String>` is not idempotent
/// the way Quarto 1's `classList.add` is).
fn appendix_heading(meta: &ConfigValue, term_key: &str, english: &str) -> Block {
    let source_info = SourceInfo::Generated {
        by: By::appendix(),
        from: smallvec![],
    };
    Block::Header(Header {
        level: 2,
        attr: (
            String::new(),
            vec![
                "anchored".to_string(),
                "quarto-appendix-heading".to_string(),
            ],
            LinkedHashMap::new(),
        ),
        content: title_inlines(&appendix_title(meta, term_key, english), &source_info),
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Title the footnotes section in place.
///
/// The section built by `FootnotesTransform` is already a `.section` Div with
/// the right id and `doc-endnotes` role, so the heading is prepended into it
/// rather than nesting a second section around it.
///
/// The leading `<hr>` is dropped: Quarto 1's `prependHeading` removes the
/// rule when it inserts the heading (`format-html-shared.ts:405-409`), the
/// heading taking over as the section separator. The strip lives here, not in
/// `create_footnotes_section`, because the rule must survive when appendix
/// processing is off and no heading is ever added (bd-v9zs83zj).
fn wrap_footnotes(footnotes: Block, meta: &ConfigValue) -> Block {
    let mut div = match footnotes {
        Block::Div(div) => div,
        // `extract_footnotes` only ever yields a Div; nothing to title otherwise.
        other => return other,
    };

    if let Some(pos) = div
        .content
        .iter()
        .position(|b| matches!(b, Block::HorizontalRule(_)))
    {
        div.content.remove(pos);
    }

    div.content.insert(
        0,
        appendix_heading(meta, "section-title-footnotes", "Footnotes"),
    );
    Block::Div(div)
}

/// Wrap bibliography in a section with appropriate attributes.
fn wrap_bibliography(bibliography: Block, meta: &ConfigValue) -> Block {
    let source_info = SourceInfo::Generated {
        by: By::appendix(),
        from: smallvec![],
    };

    // Create header for the bibliography section
    let header = appendix_heading(meta, "section-title-references", "References");

    Block::Div(Div {
        attr: (
            "quarto-bibliography".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::from_iter([("role".to_string(), "doc-bibliography".to_string())]),
        ),
        content: vec![header, bibliography],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create the appendix container div.
fn create_appendix_container(sections: Blocks, style_class: &str) -> Block {
    Block::Div(Div {
        attr: (
            "quarto-appendix".to_string(),
            vec![style_class.to_string()],
            LinkedHashMap::new(),
        ),
        content: sections,
        source_info: SourceInfo::Generated {
            by: By::appendix(),
            from: smallvec![],
        },
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Create license section from metadata.
fn create_license_section(meta: &ConfigValue) -> Option<Block> {
    let license = meta.get("license")?;

    // License can be a string (e.g., "CC BY") or an object with more details.
    // `as_plain_text` (not `as_str`): bare front-matter strings are stored as
    // `ConfigValueKind::PandocInlines`, for which `as_str` returns `None`,
    // silently dropping the section. (bd-y89ihf0i)
    let license_text = if let Some(s) = license.as_plain_text() {
        s
    } else {
        // Try to get "text" or "type" field
        license
            .get("text")
            .or_else(|| license.get("type"))
            .and_then(|v| v.as_plain_text())?
    };

    let source_info = SourceInfo::Generated {
        by: By::appendix(),
        from: smallvec![],
    };

    let header = appendix_heading(meta, "section-title-reuse", "Reuse");

    let content = Block::Paragraph(Paragraph {
        content: vec![Inline::Str(Str {
            text: license_text,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-reuse".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

/// Create copyright section from metadata.
fn create_copyright_section(meta: &ConfigValue) -> Option<Block> {
    let copyright = meta.get("copyright")?;

    // Copyright can be a string or an object. `as_plain_text` (not `as_str`):
    // bare front-matter strings are stored as `ConfigValueKind::PandocInlines`,
    // for which `as_str` returns `None`, silently dropping the section.
    // (bd-y89ihf0i)
    let copyright_text = if let Some(s) = copyright.as_plain_text() {
        s
    } else {
        // Try to get "holder" or "statement" field
        copyright
            .get("statement")
            .or_else(|| copyright.get("holder"))
            .and_then(|v| v.as_plain_text())?
    };

    let source_info = SourceInfo::Generated {
        by: By::appendix(),
        from: smallvec![],
    };

    let header = appendix_heading(meta, "section-title-copyright", "Copyright");

    let content = Block::Paragraph(Paragraph {
        content: vec![Inline::Str(Str {
            text: copyright_text,
            source_info: source_info.clone(),
        })],
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-copyright".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

/// Create citation section from metadata.
fn create_citation_section(meta: &ConfigValue) -> Option<Block> {
    let citation = meta.get("citation")?;

    // Citation metadata typically includes how to cite this document
    // It can have various formats - for now, look for a "url" or create a simple reference.
    // `as_plain_text` (not `as_str`): a bare front-matter URL string is stored
    // as `ConfigValueKind::PandocInlines`, for which `as_str` returns `None`.
    // (bd-y89ihf0i)
    let citation_url = citation.get("url").and_then(|v| v.as_plain_text());

    let source_info = SourceInfo::Generated {
        by: By::appendix(),
        from: smallvec![],
    };

    let header = appendix_heading(meta, "section-title-citation", "Citation");

    // Create citation content based on what's available
    let content_inlines = if let Some(url) = citation_url {
        vec![
            Inline::Str(Str {
                text: "For attribution, please cite this work as: ".to_string(),
                source_info: source_info.clone(),
            }),
            Inline::Link(Link {
                attr: (String::new(), Vec::new(), LinkedHashMap::new()),
                content: vec![Inline::Str(Str {
                    text: url.clone(),
                    source_info: source_info.clone(),
                })],
                target: (url.clone(), String::new()),
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
                target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
            }),
        ]
    } else {
        // If no URL, just note that citation info is available
        vec![Inline::Str(Str {
            text: "Please cite this work appropriately.".to_string(),
            source_info: source_info.clone(),
        })]
    };

    let content = Block::Paragraph(Paragraph {
        content: content_inlines,
        source_info: source_info.clone(),
    });

    Some(Block::Div(Div {
        attr: (
            "quarto-citation".to_string(),
            vec!["section".to_string()],
            LinkedHashMap::new(),
        ),
        content: vec![header, content],
        source_info,
        attr_source: AttrSourceInfo::empty(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::Plain;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
    use quarto_source_map::{FileId, Location, Range};

    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;

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
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
        }
    }

    fn make_str(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn make_appendix_div(id: &str, content: &str) -> Block {
        Block::Div(Div {
            attr: (
                id.to_string(),
                vec!["appendix".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![make_str(content)],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// Metadata carrying a resolved `quarto.language` table, shaped exactly as
    /// `LanguageResolveStage` injects it. Transforms read it back through
    /// `LanguageTerms::from_meta`.
    fn make_meta_with_lang(lang: &str) -> ConfigValue {
        let terms = crate::language::resolve_language(lang, &[]);
        make_meta(vec![
            meta_entry(
                "lang",
                ConfigValue::new_string(lang.to_string(), dummy_source_info()),
            ),
            meta_entry(
                "quarto",
                make_meta(vec![meta_entry("language", terms.to_config_value())]),
            ),
        ])
    }

    /// A footnotes section shaped like the one `create_footnotes_section`
    /// actually emits: an `<hr>` followed by the note list. The plain
    /// `make_footnotes_section` helper omits the rule, which is precisely the
    /// element the appendix transform has to strip (bd-v9zs83zj).
    fn make_footnotes_section_with_rule() -> Block {
        Block::Div(Div {
            attr: (
                "footnotes".to_string(),
                vec!["footnotes".to_string(), "section".to_string()],
                LinkedHashMap::from_iter([("role".to_string(), "doc-endnotes".to_string())]),
            ),
            content: vec![
                Block::HorizontalRule(quarto_pandoc_types::block::HorizontalRule {
                    source_info: dummy_source_info(),
                }),
                Block::Plain(Plain {
                    content: vec![make_str("Footnote content")],
                    source_info: dummy_source_info(),
                }),
            ],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    /// The `Header` an appendix section leads with.
    fn section_heading(block: &Block) -> &Header {
        let Block::Div(div) = block else {
            panic!("expected a section Div, got {block:?}");
        };
        match div.content.first() {
            Some(Block::Header(h)) => h,
            other => panic!("expected a leading Header, got {other:?}"),
        }
    }

    /// Flattened text of a heading's inlines. Titles are `Str`/`Space`
    /// alternations, so `Space` has to render as a space rather than be
    /// skipped — otherwise "Derechos de autor" reads as "Derechosdeautor".
    fn heading_text(header: &Header) -> String {
        header
            .content
            .iter()
            .map(|inline| match inline {
                Inline::Str(s) => s.text.as_str(),
                Inline::Space(_) => " ",
                other => panic!("unexpected inline in an appendix heading: {other:?}"),
            })
            .collect()
    }

    /// The sections inside the `#quarto-appendix` container, which the
    /// transform appends as the document's last block.
    fn appendix_sections(ast: &Pandoc) -> &[Block] {
        let Some(Block::Div(div)) = ast.blocks.last() else {
            panic!("expected a trailing appendix Div");
        };
        assert_eq!(div.attr.0, "quarto-appendix");
        &div.content
    }

    /// Run the transform over `ast` with a throwaway HTML render context.
    async fn run_transform(ast: &mut Pandoc) {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        AppendixStructureTransform::new()
            .transform(ast, &mut ctx)
            .await
            .unwrap();
    }

    fn make_footnotes_section() -> Block {
        Block::Div(Div {
            attr: (
                "footnotes".to_string(),
                vec!["footnotes".to_string(), "section".to_string()],
                LinkedHashMap::from_iter([("role".to_string(), "doc-endnotes".to_string())]),
            ),
            content: vec![Block::Plain(Plain {
                content: vec![make_str("Footnote content")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn meta_entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    fn make_meta(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue::new_map(entries, dummy_source_info())
    }

    fn make_bibliography() -> Block {
        Block::Div(Div {
            attr: (
                "refs".to_string(),
                vec!["references".to_string()],
                LinkedHashMap::new(),
            ),
            content: vec![Block::Plain(Plain {
                content: vec![make_str("Bibliography entries")],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    #[tokio::test]
    async fn test_transform_name() {
        let transform = AppendixStructureTransform::new();
        assert_eq!(transform.name(), "appendix-structure");
    }

    #[tokio::test]
    async fn test_no_appendix_content() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Regular content")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // No appendix should be created if no appendix content
        assert_eq!(ast.blocks.len(), 1);
        assert!(matches!(ast.blocks[0], Block::Paragraph(_)));
    }

    #[tokio::test]
    async fn test_user_appendix_sections() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "Appendix A content"),
                make_appendix_div("appendix-b", "Appendix B content"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have main content + appendix container
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            // Should contain both appendix sections
            assert_eq!(div.content.len(), 2);
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[tokio::test]
    async fn test_footnotes_moved_to_appendix() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_footnotes_section(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have main content + appendix container (footnotes moved into it)
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container contains footnotes
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            // First item should be the footnotes section
            if let Block::Div(footnotes) = &div.content[0] {
                assert_eq!(footnotes.attr.0, "footnotes");
            } else {
                panic!("Expected footnotes Div in appendix");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[tokio::test]
    async fn test_bibliography_moved_to_appendix() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_bibliography(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have main content + appendix container
        assert_eq!(ast.blocks.len(), 2);

        // Check appendix container contains wrapped bibliography
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            // First item should be the wrapped bibliography section
            if let Block::Div(bib_section) = &div.content[0] {
                assert_eq!(bib_section.attr.0, "quarto-bibliography");
            } else {
                panic!("Expected bibliography section Div in appendix");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[tokio::test]
    async fn test_appendix_section_ordering() {
        let mut ast = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                // Add in wrong order to test ordering
                make_footnotes_section(),
                make_bibliography(),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Check ordering: user appendix → bibliography → footnotes
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.content.len(), 3);

            // 1. User appendix
            if let Block::Div(user) = &div.content[0] {
                assert!(user.attr.1.contains(&"appendix".to_string()));
            } else {
                panic!("First item should be user appendix");
            }

            // 2. Bibliography
            if let Block::Div(bib) = &div.content[1] {
                assert_eq!(bib.attr.0, "quarto-bibliography");
            } else {
                panic!("Second item should be bibliography");
            }

            // 3. Footnotes
            if let Block::Div(footnotes) = &div.content[2] {
                assert_eq!(footnotes.attr.0, "footnotes");
            } else {
                panic!("Third item should be footnotes");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[tokio::test]
    async fn test_appendix_style_none_skips_processing() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "appendix-style",
                ConfigValue::new_string("none", dummy_source_info()),
            )]),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Blocks should be unchanged - appendix div stays in place
        assert_eq!(ast.blocks.len(), 2);
        // The appendix div should still be there, not moved
        if let Block::Div(div) = &ast.blocks[1] {
            assert!(div.attr.1.contains(&"appendix".to_string()));
            assert_ne!(div.attr.0, "quarto-appendix"); // NOT the container
        }
    }

    #[tokio::test]
    async fn test_margin_mode_footnotes_not_moved() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "reference-location",
                ConfigValue::new_string("margin", dummy_source_info()),
            )]),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_footnotes_section(),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Footnotes should stay in place, no appendix created
        assert_eq!(ast.blocks.len(), 2);
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "footnotes");
        } else {
            panic!("Footnotes should remain in place");
        }
    }

    #[tokio::test]
    async fn test_license_metadata_creates_section() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "license",
                ConfigValue::new_string("CC BY 4.0", dummy_source_info()),
            )]),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_str("Main content")],
                source_info: dummy_source_info(),
            })],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Should have main content + appendix with license section
        assert_eq!(ast.blocks.len(), 2);

        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert_eq!(div.content.len(), 1);

            if let Block::Div(license) = &div.content[0] {
                assert_eq!(license.attr.0, "quarto-reuse");
            } else {
                panic!("Expected license section");
            }
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[tokio::test]
    async fn test_appendix_style_plain() {
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "appendix-style",
                ConfigValue::new_string("plain", dummy_source_info()),
            )]),
            blocks: vec![
                Block::Paragraph(Paragraph {
                    content: vec![make_str("Main content")],
                    source_info: dummy_source_info(),
                }),
                make_appendix_div("appendix-a", "User appendix"),
            ],
        };

        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

        let transform = AppendixStructureTransform::new();
        transform.transform(&mut ast, &mut ctx).await.unwrap();

        // Check that appendix container has "plain" class
        if let Block::Div(div) = &ast.blocks[1] {
            assert_eq!(div.attr.0, "quarto-appendix");
            assert!(div.attr.1.contains(&"plain".to_string()));
        } else {
            panic!("Expected appendix Div");
        }
    }

    #[test]
    fn test_create_appendix_container_has_generated_provenance() {
        // Plan 6: the synthesized appendix container Div carries
        // Generated { by: appendix(), from: [] }.
        let block = create_appendix_container(vec![], "default");
        let Block::Div(div) = &block else {
            panic!("Expected Div");
        };
        match &div.source_info {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.kind, "appendix");
                assert!(from.is_empty());
            }
            other => panic!("Expected Generated, got {:?}", other),
        }
    }

    // ---------------------------------------------------------------
    // Appendix section headings (bd-v9zs83zj)
    //
    // Quarto 1 titles every appendix section from a `section-title-*`
    // language term and tags the heading `anchored quarto-appendix-heading`
    // (`format-html-appendix.ts:98`). The footnotes section additionally
    // loses its `<hr>`, because Q1's `prependHeading` removes the rule when
    // it inserts the heading (`format-html-shared.ts:405-409`).
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_footnotes_section_gets_heading() {
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let heading = section_heading(&appendix_sections(&ast)[0]);
        assert_eq!(heading_text(heading), "Footnotes");
        assert_eq!(heading.level, 2);
    }

    #[tokio::test]
    async fn test_appendix_heading_has_q1_classes_and_no_id() {
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let (id, classes, attrs) = &section_heading(&appendix_sections(&ast)[0]).attr;
        // Q1's `prependHeading` sets no id: the heading is not linkable and
        // does not enter the TOC.
        assert_eq!(id, "");
        assert_eq!(classes, &["anchored", "quarto-appendix-heading"]);
        assert!(attrs.is_empty());
    }

    #[tokio::test]
    async fn test_footnotes_rule_removed_when_titled() {
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let Block::Div(footnotes) = &appendix_sections(&ast)[0] else {
            panic!("expected the footnotes Div");
        };
        assert!(
            !footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::HorizontalRule(_))),
            "the heading replaces the rule as the separator; got {:?}",
            footnotes.content
        );
        // The note content itself must survive the strip.
        assert!(
            footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::Plain(_))),
            "footnote content was dropped along with the rule"
        );
    }

    #[tokio::test]
    async fn test_footnotes_rule_kept_when_appendix_disabled() {
        // With no appendix to title the section, the rule is the only
        // separator the reader gets — so it must stay. Q1 agrees: it only
        // titles footnotes from inside `processDocumentAppendix`.
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "appendix-style",
                ConfigValue::new_string("none".to_string(), dummy_source_info()),
            )]),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let Some(Block::Div(footnotes)) = ast.blocks.last() else {
            panic!("expected the footnotes Div to stay in place");
        };
        assert_eq!(footnotes.attr.0, "footnotes");
        assert!(
            footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::HorizontalRule(_))),
            "the rule must survive when the appendix never titles the section"
        );
        assert!(
            !footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::Header(_))),
            "no heading may be added when appendix processing is off"
        );
    }

    #[tokio::test]
    async fn test_footnotes_rule_kept_for_book_format() {
        // Books skip appendix processing entirely, so — as with
        // `appendix-style: none` — nothing titles the section and the rule
        // stays.
        let mut ast = Pandoc {
            meta: make_meta(vec![meta_entry(
                "book",
                ConfigValue::new_bool(true, dummy_source_info()),
            )]),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let Some(Block::Div(footnotes)) = ast.blocks.last() else {
            panic!("expected the footnotes Div to stay in place");
        };
        assert_eq!(footnotes.attr.0, "footnotes");
        assert!(
            footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::HorizontalRule(_))),
            "the rule must survive in book format"
        );
        assert!(
            !footnotes
                .content
                .iter()
                .any(|b| matches!(b, Block::Header(_))),
            "no heading may be added in book format"
        );
    }

    #[tokio::test]
    async fn test_footnotes_heading_english_fallback_without_language_stage() {
        // `LanguageTerms::from_meta` returns None when the resolve stage has
        // not run (as in these stage-less unit tests). The English literal is
        // the documented fallback tier, per `toc_generate.rs`.
        let mut ast = Pandoc {
            meta: ConfigValue::default(),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        assert_eq!(
            heading_text(section_heading(&appendix_sections(&ast)[0])),
            "Footnotes"
        );
    }

    #[tokio::test]
    async fn test_footnotes_heading_localized() {
        let mut ast = Pandoc {
            meta: make_meta_with_lang("es"),
            blocks: vec![make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        assert_eq!(
            heading_text(section_heading(&appendix_sections(&ast)[0])),
            "Notas"
        );
    }

    #[tokio::test]
    async fn test_all_appendix_headings_localized() {
        // Every section the transform can emit takes its title from the
        // matching `section-title-*` term, not an English literal.
        let mut meta_entries = match &make_meta_with_lang("es").value {
            ConfigValueKind::Map(entries) => entries.clone(),
            _ => unreachable!("make_meta_with_lang builds a map"),
        };
        meta_entries.push(meta_entry(
            "license",
            ConfigValue::new_string("CC BY 4.0".to_string(), dummy_source_info()),
        ));
        meta_entries.push(meta_entry(
            "copyright",
            ConfigValue::new_string("Posit, PBC".to_string(), dummy_source_info()),
        ));
        meta_entries.push(meta_entry(
            "citation",
            make_meta(vec![meta_entry(
                "url",
                ConfigValue::new_string("https://example.com".to_string(), dummy_source_info()),
            )]),
        ));

        let mut ast = Pandoc {
            meta: make_meta(meta_entries),
            blocks: vec![make_bibliography(), make_footnotes_section_with_rule()],
        };
        run_transform(&mut ast).await;

        let titles: Vec<String> = appendix_sections(&ast)
            .iter()
            .map(|s| heading_text(section_heading(s)))
            .collect();
        assert_eq!(
            titles,
            vec![
                "Referencias",       // section-title-references
                "Notas",             // section-title-footnotes
                "Reutilización",     // section-title-reuse
                "Derechos de autor", // section-title-copyright
                "Cómo citar",        // section-title-citation
            ]
        );
    }
}
