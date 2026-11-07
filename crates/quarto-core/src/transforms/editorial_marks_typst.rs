/*
 * editorial_marks_typst.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Editorial marks to Typst (GH #1102).
//!
//! Rewrites q2's editorial marks (`[++ ]`, `[-- ]`, `[!! ]`, `[>> ]` and their `:::` block
//! forms) into calls to Typst functions defined in the Typst template's
//! `definitions.typ`:
//!
//! | mark       | inline form                  | block form                         |
//! |------------|------------------------------|------------------------------------|
//! | insertion  | `#quarto-insert(..)[..];`    | `#quarto-insert-block(..)[..]`     |
//! | deletion   | `#quarto-delete(..)[..];`    | `#quarto-delete-block(..)[..]`     |
//! | highlight  | `#quarto-highlight(..)[..];` | `#quarto-highlight-block(..)[..]`  |
//! | comment    | `#quarto-edit-comment(..)[..];` | `#quarto-edit-comment-block(..)[..]` |
//!
//! Every function takes the mark's `author` and `date` as named arguments (`none` when the
//! mark has neither) and its content as the body. How a mark *looks* is decided only by
//! those functions, so a user restyles the marks by redefining them (a `#let` of the same
//! name in a raw-Typst `header-includes`, which the template splices in after the
//! definitions), with no Lua filter. (`include-in-header` would be the natural spelling, but
//! it doesn't reach Pandoc formats yet: bd-7avt1ogu.)
//!
//! Without this transform pandoc's typst writer prints a mark span's content as ordinary
//! text, so deletions and reviewer comments read as body copy in the PDF.
//!
//! ## Shape
//!
//! A mark span becomes `RawInline("#quarto-insert(..)[")`, its (converted) content, then
//! `RawInline("];")`. The trailing `;` ends the embedded Typst expression: without it, text
//! that starts with `(` or `[` right after the mark would be parsed as more arguments to the
//! call. Pandoc escapes those characters after its own calls, but it cannot know to after a
//! raw `]`. A mark div becomes a `RawBlock` open, its blocks, and a `RawBlock("]")`; a block
//! ends at a line break, so it needs no `;`.
//!
//! Marks nest naturally (a highlight inside a deletion is a call inside a call), so unlike
//! the OOXML export there is no folding of comments into messages and no commented-range
//! special case: an I4 commented range renders as its text followed by its comments. A
//! mark's id and any attributes other than `author`/`date` are dropped.
//!
//! The transform is a no-op for every format but `typst`. Like
//! [`crate::transforms::EditorialMarksOoxmlTransform`], it runs before pandoc, in the
//! Finalization phase, and is a bucket-B3 service (`pipeline::BUCKETS`).

use std::sync::Arc;

use hashlink::LinkedHashMap;
use quarto_pandoc_types::block::{Block, RawBlock};
use quarto_pandoc_types::inline::{Inline, Inlines, RawInline};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::ast_walk::{
    ListVisitor, block_children_mut, config_value_lists_mut, inline_children_mut,
};
use crate::attribution::AttributionData;
use crate::editorial_marks::{MarkKind, stamp_attribution};
use crate::format::FormatIdentifier;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// Rewrites editorial marks for Typst. See the module documentation.
#[derive(Debug, Default)]
pub struct EditorialMarksTypstTransform;

impl EditorialMarksTypstTransform {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for EditorialMarksTypstTransform {
    fn name(&self) -> &str {
        "editorial-marks-typst"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.format.identifier != FormatIdentifier::Typst {
            return Ok(());
        }
        convert_document(ast, ctx.attribution_data.clone());
        Ok(())
    }
}

/// Rewrite every editorial mark in `ast` (body and metadata).
fn convert_document(ast: &mut Pandoc, attribution: Option<Arc<AttributionData>>) {
    let mut conv = Converter { attribution };
    config_value_lists_mut(&mut ast.meta, &mut conv);
    conv.blocks(&mut ast.blocks);
}

/// The Typst function a mark calls (the names `definitions.typ` defines).
fn function_name(kind: MarkKind, block: bool) -> &'static str {
    match (kind, block) {
        (MarkKind::Insert, false) => "quarto-insert",
        (MarkKind::Delete, false) => "quarto-delete",
        (MarkKind::Highlight, false) => "quarto-highlight",
        (MarkKind::Comment, false) => "quarto-edit-comment",
        (MarkKind::Insert, true) => "quarto-insert-block",
        (MarkKind::Delete, true) => "quarto-delete-block",
        (MarkKind::Highlight, true) => "quarto-highlight-block",
        (MarkKind::Comment, true) => "quarto-edit-comment-block",
    }
}

/// A Typst string literal holding `text`.
fn typst_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `#name(author: .., date: ..)[`: the call up to and including the body's opening bracket.
/// An absent or empty attribute is left out, so the function's default (`none`) applies.
fn open_call(kind: MarkKind, block: bool, attrs: &LinkedHashMap<String, String>) -> String {
    let args: Vec<String> = ["author", "date"]
        .into_iter()
        .filter_map(|key| {
            attrs
                .get(key)
                .filter(|v| !v.is_empty())
                .map(|v| format!("{key}: {}", typst_string(v)))
        })
        .collect();
    format!("#{}({})[", function_name(kind, block), args.join(", "))
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::unknown())
}

fn raw_inline(text: String) -> Inline {
    Inline::RawInline(RawInline {
        format: "typst".to_string(),
        text,
        source_info: generated(),
    })
}

fn raw_block(text: String) -> Block {
    Block::RawBlock(RawBlock {
        format: "typst".to_string(),
        text,
        source_info: generated(),
    })
}

struct Converter {
    attribution: Option<Arc<AttributionData>>,
}

impl Converter {
    /// Fill in the blamed author and date where the mark didn't set them. A highlight isn't
    /// an authored change, so it isn't stamped (as in the OOXML export).
    fn stamp(&self, kind: MarkKind, attrs: &mut LinkedHashMap<String, String>, at: &SourceInfo) {
        if kind != MarkKind::Highlight {
            stamp_attribution(self.attribution.as_deref(), attrs, at);
        }
    }
}

impl ListVisitor for Converter {
    fn inlines(&mut self, list: &mut Inlines) {
        let mut out: Inlines = Vec::with_capacity(list.len());
        for inline in std::mem::take(list) {
            match inline {
                Inline::Span(mut span) => match MarkKind::from_first_class(&span.attr.1) {
                    Some(kind) => {
                        self.stamp(kind, &mut span.attr.2, &span.source_info);
                        self.inlines(&mut span.content);
                        out.push(raw_inline(open_call(kind, false, &span.attr.2)));
                        out.extend(span.content);
                        out.push(raw_inline("];".to_string()));
                    }
                    None => {
                        self.inlines(&mut span.content);
                        out.push(Inline::Span(span));
                    }
                },
                mut other => {
                    inline_children_mut(&mut other, self);
                    out.push(other);
                }
            }
        }
        *list = out;
    }

    fn blocks(&mut self, list: &mut Vec<Block>) {
        let mut out: Vec<Block> = Vec::with_capacity(list.len());
        for block in std::mem::take(list) {
            match block {
                Block::Div(mut div) => match MarkKind::from_first_class(&div.attr.1) {
                    Some(kind) => {
                        self.stamp(kind, &mut div.attr.2, &div.source_info);
                        self.blocks(&mut div.content);
                        out.push(raw_block(open_call(kind, true, &div.attr.2)));
                        out.extend(div.content);
                        out.push(raw_block("]".to_string()));
                    }
                    None => {
                        self.blocks(&mut div.content);
                        out.push(Block::Div(div));
                    }
                },
                mut other => {
                    block_children_mut(&mut other, self);
                    out.push(other);
                }
            }
        }
        *list = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribution::{AttributionDataBuilder, Identity};
    use pampa::readers::qmd;
    use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};
    use quarto_pandoc_types::custom::{CustomNode, Slot};

    fn parse(qmd_text: &str) -> Pandoc {
        let mut sink = Vec::<u8>::new();
        let (ast, _ctx, _warnings) = qmd::read(
            qmd_text.as_bytes(),
            false,
            "test.qmd",
            &mut sink,
            true,
            None,
        )
        .expect("parse qmd");
        ast
    }

    fn convert(qmd_text: &str) -> String {
        let mut ast = parse(qmd_text);
        convert_document(&mut ast, None);
        show_blocks(&ast.blocks)
    }

    // ----- a terse structural printer ---------------------------------------------------

    fn show_inlines(list: &[Inline]) -> String {
        list.iter().map(show_inline).collect::<String>()
    }

    fn show_inline(i: &Inline) -> String {
        match i {
            Inline::Str(s) => s.text.clone(),
            Inline::Space(_) => "_".into(),
            Inline::SoftBreak(_) => "~".into(),
            Inline::Emph(e) => format!("Emph[{}]", show_inlines(&e.content)),
            Inline::Strong(e) => format!("Strong[{}]", show_inlines(&e.content)),
            Inline::Span(s) => format!(
                "Span(.{})[{}]",
                s.attr.1.join(" ."),
                show_inlines(&s.content)
            ),
            Inline::RawInline(r) => format!("{{{}}}", r.text),
            Inline::Note(n) => format!("Note[{}]", show_blocks(&n.content)),
            Inline::Custom(c) => format!("Custom:{}", c.type_name),
            _ => "?".into(),
        }
    }

    fn show_blocks(list: &[Block]) -> String {
        list.iter().map(show_block).collect::<Vec<_>>().join(" ")
    }

    fn show_block(b: &Block) -> String {
        match b {
            Block::Paragraph(p) => format!("Para[{}]", show_inlines(&p.content)),
            Block::Plain(p) => format!("Plain[{}]", show_inlines(&p.content)),
            Block::Header(h) => format!("H{}[{}]", h.level, show_inlines(&h.content)),
            Block::RawBlock(r) => format!("{{{}}}", r.text),
            Block::Div(d) => format!("Div[{}]", show_blocks(&d.content)),
            Block::BulletList(l) => format!(
                "Bullets[{}]",
                l.content
                    .iter()
                    .map(|item| show_blocks(item))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            Block::Custom(c) => {
                let mut slots: Vec<String> = Vec::new();
                for (name, slot) in &c.slots {
                    let shown = match slot {
                        Slot::Inlines(i) => show_inlines(i),
                        Slot::Blocks(b) => show_blocks(b),
                        Slot::Block(b) => show_block(b),
                        _ => "?".into(),
                    };
                    slots.push(format!("{name}={shown}"));
                }
                format!("Custom:{}[{}]", c.type_name, slots.join(";"))
            }
            _ => "?".into(),
        }
    }

    // ----- inline marks ------------------------------------------------------------------

    #[test]
    fn each_inline_mark_calls_its_function() {
        assert_eq!(
            convert("[++ a] [-- b] [!! c] [>> d]\n"),
            "Para[{#quarto-insert()[}a{];}_{#quarto-delete()[}b{];}_\
             {#quarto-highlight()[}c{];}_{#quarto-edit-comment()[}d{];}]"
        );
    }

    #[test]
    fn the_issue_example_converts_every_mark() {
        // GH #1102: none of these may reach the typst writer as a plain span.
        let shown = convert(
            "[++ Deze zin is toegevoegd.] [-- Deze zin is verwijderd.] [>> Klopt dit getal?]\n",
        );
        assert!(!shown.contains("Span("), "{shown}");
        assert!(shown.contains("{#quarto-insert()[}Deze_zin_is_toegevoegd.{];}"));
        assert!(shown.contains("{#quarto-delete()[}Deze_zin_is_verwijderd.{];}"));
        assert!(shown.contains("{#quarto-edit-comment()[}Klopt_dit_getal?{];}"));
    }

    #[test]
    fn author_and_date_become_named_arguments() {
        assert_eq!(
            convert("[-- gone]{author=\"Ann\" date=\"2026-11-07\"}\n"),
            "Para[{#quarto-delete(author: \"Ann\", date: \"2026-11-07\")[}gone{];}]"
        );
    }

    #[test]
    fn attribute_values_are_escaped_as_typst_strings() {
        assert_eq!(typst_string(r#"a "q" \ b"#), r#""a \"q\" \\ b""#);
        assert_eq!(typst_string("line\nbreak\ttab"), r#""line\nbreak\ttab""#);
        let mut attrs = LinkedHashMap::new();
        attrs.insert("author".to_string(), "O\"Neil".to_string());
        attrs.insert("date".to_string(), String::new());
        attrs.insert("other".to_string(), "dropped".to_string());
        assert_eq!(
            open_call(MarkKind::Comment, false, &attrs),
            "#quarto-edit-comment(author: \"O\\\"Neil\")["
        );
    }

    #[test]
    fn nested_marks_nest_their_calls() {
        assert_eq!(
            convert("[-- old [!! hot] text]\n"),
            "Para[{#quarto-delete()[}old_{#quarto-highlight()[}hot{];}_text{];}]"
        );
    }

    #[test]
    fn marks_inside_other_inlines_and_plain_spans_are_converted() {
        assert_eq!(
            convert("*[++ a]* [[-- b]]{.keep}\n"),
            "Para[Emph[{#quarto-insert()[}a{];}]_Span(.keep)[{#quarto-delete()[}b{];}]]"
        );
    }

    #[test]
    fn an_unmarked_document_is_untouched() {
        assert_eq!(
            convert("Just *text* [x]{.y}\n"),
            "Para[Just_Emph[text]_Span(.y)[x]]"
        );
    }

    // ----- block marks -------------------------------------------------------------------

    #[test]
    fn each_block_mark_calls_its_block_function() {
        for (marker, name) in [
            ("++", "quarto-insert-block"),
            ("--", "quarto-delete-block"),
            ("!!", "quarto-highlight-block"),
            (">>", "quarto-edit-comment-block"),
        ] {
            assert_eq!(
                convert(&format!("::: {marker}\n\nOne.\n\n- item\n\n:::\n")),
                format!("{{#{name}()[}} Para[One.] Bullets[Plain[item]] {{]}}"),
                "{marker}"
            );
        }
    }

    #[test]
    fn a_block_mark_passes_its_attributes_and_converts_inner_marks() {
        assert_eq!(
            convert("::: -- {author=\"Bo\"}\n\nSee [++ this].\n\n:::\n"),
            "{#quarto-delete-block(author: \"Bo\")[} \
             Para[See_{#quarto-insert()[}this{];}.] {]}"
        );
    }

    #[test]
    fn a_plain_div_keeps_its_wrapper() {
        assert_eq!(
            convert("::: {.note}\n\n[++ a]\n\n:::\n"),
            "Div[Para[{#quarto-insert()[}a{];}]]"
        );
    }

    // ----- metadata and custom nodes ---------------------------------------------------

    #[test]
    fn marks_in_the_title_and_in_custom_node_slots_are_converted() {
        let mut ast = parse("[++ new] title\n\n[-- gone]\n\n::: ++\n\nOne.\n\n:::\n");
        let title = match &ast.blocks[0] {
            Block::Paragraph(p) => p.content.clone(),
            _ => panic!("a paragraph"),
        };
        let inline_para = ast.blocks[1].clone();
        let div = ast.blocks[2].clone();
        ast.meta.insert_path(
            &["title"],
            ConfigValue::new_inlines(title.clone(), SourceInfo::for_test()),
        );
        let node = CustomNode::new("Callout", Default::default(), SourceInfo::for_test())
            .with_slot("title", Slot::Inlines(title))
            .with_slot("content", Slot::Blocks(vec![inline_para]))
            .with_slot("one", Slot::Block(Box::new(div)));
        ast.blocks = vec![Block::Custom(node)];
        convert_document(&mut ast, None);

        assert_eq!(
            show_blocks(&ast.blocks),
            "Custom:Callout[title={#quarto-insert()[}new{];}_title;\
             content=Para[{#quarto-delete()[}gone{];}];\
             one=Div[{#quarto-insert-block()[} Para[One.] {]}]]"
        );
        match &ast.meta.get("title").expect("title").value {
            ConfigValueKind::PandocInlines(i) => {
                assert_eq!(show_inlines(i), "{#quarto-insert()[}new{];}_title");
            }
            other => panic!("inlines, got {other:?}"),
        }
    }

    // ----- attribution -------------------------------------------------------------------

    fn alice_wrote(text: &str) -> Arc<AttributionData> {
        let mut b = AttributionDataBuilder::new();
        b.set_identity(
            "alice@example.com",
            Identity {
                display_name: "Alice".to_string(),
                color: "#000000".to_string(),
            },
        );
        b.push_run(0, text.len(), "alice@example.com", 1_700_000_000);
        Arc::new(b.build())
    }

    #[test]
    fn changes_get_the_blamed_author_and_date_but_highlights_do_not() {
        let text = "[++ x] [!! y]\n";
        let mut ast = parse(text);
        convert_document(&mut ast, Some(alice_wrote(text)));
        assert_eq!(
            show_blocks(&ast.blocks),
            "Para[{#quarto-insert(author: \"Alice\", date: \"2023-11-14T22:13:20Z\")[}x{];}_\
             {#quarto-highlight()[}y{];}]"
        );
    }

    #[test]
    fn an_explicit_author_wins_over_the_blamed_one() {
        let text = "[>> c]{author=\"Me\"}\n";
        let mut ast = parse(text);
        convert_document(&mut ast, Some(alice_wrote(text)));
        assert_eq!(
            show_blocks(&ast.blocks),
            "Para[{#quarto-edit-comment(author: \"Me\", date: \"2023-11-14T22:13:20Z\")[}c{];}]"
        );
    }

    // ----- gating through the transform ------------------------------------------------

    async fn run_transform(format: crate::format::Format, qmd_text: &str) -> String {
        use crate::project::{DocumentInfo, ProjectContext};
        use crate::render::BinaryDependencies;
        let mut ast = parse(qmd_text);
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/test.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let doc = DocumentInfo::from_path("/project/test.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        EditorialMarksTypstTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        show_blocks(&ast.blocks)
    }

    #[tokio::test]
    async fn the_transform_acts_for_typst_only() {
        use crate::format::Format;
        let q = "[++ x]\n";
        assert_eq!(
            run_transform(Format::from_format_string("typst").unwrap(), q).await,
            "Para[{#quarto-insert()[}x{];}]"
        );
        for other in [Format::html(), Format::pdf(), Format::docx()] {
            assert_eq!(
                run_transform(other, q).await,
                "Para[Span(.quarto-insert)[x]]"
            );
        }
    }
}
