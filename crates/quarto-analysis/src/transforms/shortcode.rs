//! Meta shortcode resolution transform.
//!
//! This transform resolves `{{< meta key >}}` shortcodes in the AST,
//! replacing them with the corresponding values from the document metadata.

use super::{AnalysisTransform, Result};
use crate::AnalysisContext;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;

/// Transform that resolves `{{< meta key >}}` shortcodes.
///
/// This transform walks the AST looking for shortcode nodes with name "meta",
/// and replaces them with the corresponding metadata value. It supports
/// dot notation for nested keys (e.g., `{{< meta author.name >}}`).
///
/// # Example
///
/// Given metadata:
/// ```yaml
/// title: "My Document"
/// author:
///   name: "Alice"
/// ```
///
/// The shortcode `{{< meta author.name >}}` would be replaced with "Alice".
///
/// # Error Handling
///
/// If a metadata key is not found, the transform:
/// - Reports a diagnostic warning via the context
/// - Replaces the shortcode with an error indicator (e.g., `?meta:key`)
pub struct MetaShortcodeTransform;

impl AnalysisTransform for MetaShortcodeTransform {
    fn name(&self) -> &str {
        "meta-shortcode"
    }

    fn transform(&self, pandoc: &mut Pandoc, ctx: &mut dyn AnalysisContext) -> Result<()> {
        // Resolve shortcodes in blocks, passing metadata from the Pandoc AST
        resolve_blocks(&mut pandoc.blocks, &pandoc.meta, ctx);
        Ok(())
    }
}

use quarto_pandoc_types::block::Block;

/// Resolve shortcodes in a list of blocks.
fn resolve_blocks(blocks: &mut [Block], metadata: &ConfigValue, ctx: &mut dyn AnalysisContext) {
    for block in blocks {
        resolve_block(block, metadata, ctx);
    }
}

/// Resolve shortcodes in a single block.
fn resolve_block(block: &mut Block, metadata: &ConfigValue, ctx: &mut dyn AnalysisContext) {
    match block {
        Block::Paragraph(para) => {
            resolve_inlines(&mut para.content, metadata, ctx);
        }
        Block::Plain(plain) => {
            resolve_inlines(&mut plain.content, metadata, ctx);
        }
        Block::Header(header) => {
            resolve_inlines(&mut header.content, metadata, ctx);
        }
        Block::BlockQuote(bq) => {
            resolve_blocks(&mut bq.content, metadata, ctx);
        }
        Block::Div(div) => {
            resolve_blocks(&mut div.content, metadata, ctx);
        }
        Block::BulletList(list) => {
            for item in &mut list.content {
                resolve_blocks(item, metadata, ctx);
            }
        }
        Block::OrderedList(list) => {
            for item in &mut list.content {
                resolve_blocks(item, metadata, ctx);
            }
        }
        Block::DefinitionList(list) => {
            for (term, definitions) in &mut list.content {
                resolve_inlines(term, metadata, ctx);
                for def in definitions {
                    resolve_blocks(def, metadata, ctx);
                }
            }
        }
        Block::Figure(fig) => {
            resolve_blocks(&mut fig.content, metadata, ctx);
            // Caption.short is Option<Inlines>, Caption.long is Option<Blocks>
            if let Some(short) = &mut fig.caption.short {
                resolve_inlines(short, metadata, ctx);
            }
            if let Some(long) = &mut fig.caption.long {
                resolve_blocks(long, metadata, ctx);
            }
        }
        Block::Table(table) => {
            // Caption.short is Option<Inlines>, Caption.long is Option<Blocks>
            if let Some(short) = &mut table.caption.short {
                resolve_inlines(short, metadata, ctx);
            }
            if let Some(long) = &mut table.caption.long {
                resolve_blocks(long, metadata, ctx);
            }
            // Table cells contain blocks
            for row in &mut table.head.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, metadata, ctx);
                }
            }
            for body in &mut table.bodies {
                for row in &mut body.head {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content, metadata, ctx);
                    }
                }
                for row in &mut body.body {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.content, metadata, ctx);
                    }
                }
            }
            // foot is TableFoot, not Option<TableFoot>
            for row in &mut table.foot.rows {
                for cell in &mut row.cells {
                    resolve_blocks(&mut cell.content, metadata, ctx);
                }
            }
        }
        Block::LineBlock(lb) => {
            for line in &mut lb.content {
                resolve_inlines(line, metadata, ctx);
            }
        }
        Block::Custom(custom) => {
            use quarto_pandoc_types::custom::Slot;
            for slot in custom.slots.values_mut() {
                match slot {
                    Slot::Blocks(blocks) => resolve_blocks(blocks, metadata, ctx),
                    Slot::Inlines(inlines) => resolve_inlines(inlines, metadata, ctx),
                    Slot::Block(block) => resolve_block(block, metadata, ctx),
                    Slot::Inline(inline) => {
                        // Single boxed inline - wrap in a vec to process
                        let mut inlines = vec![(**inline).clone()];
                        resolve_inlines(&mut inlines, metadata, ctx);
                        if let Some(resolved) = inlines.into_iter().next() {
                            **inline = resolved;
                        }
                    }
                }
            }
        }
        // Blocks that don't contain inlines
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => {}
    }
}

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::inline::{Inline, Str, Strong};
use quarto_pandoc_types::shortcode::ShortcodeArg;
use quarto_source_map::SourceInfo;

/// Resolve shortcodes in a list of inlines.
fn resolve_inlines(
    inlines: &mut Vec<Inline>,
    metadata: &ConfigValue,
    ctx: &mut dyn AnalysisContext,
) {
    let mut i = 0;
    while i < inlines.len() {
        match &inlines[i] {
            Inline::Shortcode(shortcode) if shortcode.name == "meta" => {
                // Get the key from the first positional argument
                let key = shortcode.positional_args.first().and_then(|arg| match arg {
                    ShortcodeArg::String(s) => Some(s.clone()),
                    _ => None,
                });

                let source_info = shortcode.source_info.clone();

                let replacement = if let Some(key) = key {
                    // Look up the metadata value using get_nested
                    if let Some(value) = metadata.get_nested(&key) {
                        // Convert to plain text
                        if let Some(text) = value.as_plain_text() {
                            vec![Inline::Str(Str {
                                text,
                                source_info: SourceInfo::default(),
                            })]
                        } else {
                            // Value exists but can't be converted to text
                            let diag = DiagnosticMessageBuilder::warning("Invalid metadata type")
                                .problem(format!(
                                    "Metadata key `{}` exists but cannot be converted to text",
                                    key
                                ))
                                .with_location(source_info)
                                .build();
                            ctx.add_diagnostic(diag);

                            vec![Inline::Strong(Strong {
                                content: vec![Inline::Str(Str {
                                    text: format!("?meta:{}", key),
                                    source_info: SourceInfo::default(),
                                })],
                                source_info: SourceInfo::default(),
                            })]
                        }
                    } else {
                        // Key not found
                        let diag = DiagnosticMessageBuilder::warning("Unknown metadata key")
                            .problem(format!("Metadata key `{}` not found in document", key))
                            .add_hint("Check that the key exists in your YAML frontmatter")
                            .with_location(source_info)
                            .build();
                        ctx.add_diagnostic(diag);

                        vec![Inline::Strong(Strong {
                            content: vec![Inline::Str(Str {
                                text: format!("?meta:{}", key),
                                source_info: SourceInfo::default(),
                            })],
                            source_info: SourceInfo::default(),
                        })]
                    }
                } else {
                    // No key provided
                    let diag = DiagnosticMessageBuilder::warning("Missing shortcode argument")
                        .problem("The `meta` shortcode requires a metadata key")
                        .add_hint("Use `{{< meta key >}}` where `key` is a metadata field name")
                        .with_location(source_info)
                        .build();
                    ctx.add_diagnostic(diag);

                    vec![Inline::Strong(Strong {
                        content: vec![Inline::Str(Str {
                            text: "?meta".to_string(),
                            source_info: SourceInfo::default(),
                        })],
                        source_info: SourceInfo::default(),
                    })]
                };

                // Replace the shortcode with the resolved content
                let replacement_len = replacement.len();
                inlines.splice(i..=i, replacement);
                i += replacement_len;
            }
            // Recursively process inlines that contain other inlines
            Inline::Emph(emph) => {
                let mut content = emph.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Emph(e) = &mut inlines[i] {
                    e.content = content;
                }
                i += 1;
            }
            Inline::Strong(strong) => {
                let mut content = strong.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Strong(s) = &mut inlines[i] {
                    s.content = content;
                }
                i += 1;
            }
            Inline::Strikeout(s) => {
                let mut content = s.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Strikeout(st) = &mut inlines[i] {
                    st.content = content;
                }
                i += 1;
            }
            Inline::Superscript(s) => {
                let mut content = s.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Superscript(sp) = &mut inlines[i] {
                    sp.content = content;
                }
                i += 1;
            }
            Inline::Subscript(s) => {
                let mut content = s.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Subscript(sb) = &mut inlines[i] {
                    sb.content = content;
                }
                i += 1;
            }
            Inline::SmallCaps(s) => {
                let mut content = s.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::SmallCaps(sc) = &mut inlines[i] {
                    sc.content = content;
                }
                i += 1;
            }
            Inline::Quoted(q) => {
                let mut content = q.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Quoted(qt) = &mut inlines[i] {
                    qt.content = content;
                }
                i += 1;
            }
            Inline::Link(link) => {
                let mut content = link.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Link(l) = &mut inlines[i] {
                    l.content = content;
                }
                i += 1;
            }
            Inline::Span(span) => {
                let mut content = span.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Span(sp) = &mut inlines[i] {
                    sp.content = content;
                }
                i += 1;
            }
            Inline::Cite(cite) => {
                let mut content = cite.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Cite(c) = &mut inlines[i] {
                    c.content = content;
                }
                i += 1;
            }
            Inline::Note(note) => {
                let mut content = note.content.clone();
                resolve_blocks(&mut content, metadata, ctx);
                if let Inline::Note(n) = &mut inlines[i] {
                    n.content = content;
                }
                i += 1;
            }
            Inline::Image(img) => {
                let mut content = img.content.clone();
                resolve_inlines(&mut content, metadata, ctx);
                if let Inline::Image(im) = &mut inlines[i] {
                    im.content = content;
                }
                i += 1;
            }
            // Inlines that don't contain other inlines or shortcodes
            _ => {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentAnalysisContext;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Header, Paragraph};
    use quarto_pandoc_types::config_value::{
        ConfigMapEntry, ConfigValue, ConfigValueKind, MergeOp,
    };
    use quarto_pandoc_types::shortcode::Shortcode;
    use yaml_rust2::Yaml;

    fn make_meta_shortcode(key: &str) -> Inline {
        Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "meta".to_string(),
            positional_args: vec![ShortcodeArg::String(key.to_string())],
            keyword_args: Default::default(),
            source_info: SourceInfo::default(),
        })
    }

    fn make_metadata(entries: Vec<(&str, &str)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: ConfigValue {
                    value: ConfigValueKind::Scalar(Yaml::String(v.to_string())),
                    source_info: SourceInfo::default(),
                    merge_op: MergeOp::Concat,
                },
            })
            .collect();

        ConfigValue {
            value: ConfigValueKind::Map(map_entries),
            source_info: SourceInfo::default(),
            merge_op: MergeOp::Concat,
        }
    }

    #[test]
    fn test_resolve_meta_shortcode_in_header() {
        let metadata = make_metadata(vec![("title", "My Document")]);
        let source_context = quarto_source_map::SourceContext::default();
        let mut ctx = DocumentAnalysisContext::new(source_context);

        let mut pandoc = Pandoc {
            meta: metadata,
            blocks: vec![Block::Header(Header {
                level: 1,
                attr: Default::default(),
                content: vec![make_meta_shortcode("title")],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            })],
            ..Default::default()
        };

        let transform = MetaShortcodeTransform;
        transform.transform(&mut pandoc, &mut ctx).unwrap();

        // Check that the shortcode was resolved
        if let Block::Header(header) = &pandoc.blocks[0] {
            assert_eq!(header.content.len(), 1);
            if let Inline::Str(s) = &header.content[0] {
                assert_eq!(s.text, "My Document");
            } else {
                panic!("Expected Str inline, got {:?}", header.content[0]);
            }
        } else {
            panic!("Expected Header block");
        }

        // No diagnostics for successful resolution
        assert!(ctx.diagnostics().is_empty());
    }

    #[test]
    fn test_resolve_missing_key() {
        let metadata = make_metadata(vec![("title", "My Document")]);
        let source_context = quarto_source_map::SourceContext::default();
        let mut ctx = DocumentAnalysisContext::new(source_context);

        let mut pandoc = Pandoc {
            meta: metadata,
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![make_meta_shortcode("nonexistent")],
                source_info: SourceInfo::default(),
            })],
            ..Default::default()
        };

        let transform = MetaShortcodeTransform;
        transform.transform(&mut pandoc, &mut ctx).unwrap();

        // Check that an error placeholder was inserted
        if let Block::Paragraph(para) = &pandoc.blocks[0] {
            assert_eq!(para.content.len(), 1);
            if let Inline::Strong(strong) = &para.content[0] {
                if let Inline::Str(s) = &strong.content[0] {
                    assert_eq!(s.text, "?meta:nonexistent");
                } else {
                    panic!("Expected Str inline in Strong");
                }
            } else {
                panic!("Expected Strong inline, got {:?}", para.content[0]);
            }
        } else {
            panic!("Expected Paragraph block");
        }

        // Should have a diagnostic
        assert_eq!(ctx.diagnostics().len(), 1);
        assert!(ctx.diagnostics()[0].title.contains("Unknown metadata key"));
    }

    #[test]
    fn test_resolve_mixed_content() {
        let metadata = make_metadata(vec![("author", "Alice")]);
        let source_context = quarto_source_map::SourceContext::default();
        let mut ctx = DocumentAnalysisContext::new(source_context);

        let mut pandoc = Pandoc {
            meta: metadata,
            blocks: vec![Block::Header(Header {
                level: 2,
                attr: Default::default(),
                content: vec![
                    Inline::Str(Str {
                        text: "Written by ".to_string(),
                        source_info: SourceInfo::default(),
                    }),
                    make_meta_shortcode("author"),
                ],
                source_info: SourceInfo::default(),
                attr_source: AttrSourceInfo::empty(),
            })],
            ..Default::default()
        };

        let transform = MetaShortcodeTransform;
        transform.transform(&mut pandoc, &mut ctx).unwrap();

        // Check the header content
        if let Block::Header(header) = &pandoc.blocks[0] {
            assert_eq!(header.content.len(), 2);
            if let Inline::Str(s1) = &header.content[0] {
                assert_eq!(s1.text, "Written by ");
            }
            if let Inline::Str(s2) = &header.content[1] {
                assert_eq!(s2.text, "Alice");
            }
        } else {
            panic!("Expected Header block");
        }
    }
}
