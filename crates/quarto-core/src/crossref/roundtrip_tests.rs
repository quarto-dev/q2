/*
 * crossref/roundtrip_tests.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * QMD serialize/parse round-trip guard for synthetic crossref scaffolds.
 */

//! QMD serialize/parse round-trip tests for synthetic crossref scaffolds.
//!
//! The engine execution stage serializes the whole pre-engine AST to QMD,
//! runs the engine, and reconciles the post-engine parsed AST against the
//! pre-engine one (`crates/quarto-core/src/stage/stages/engine_execution.rs`).
//! For this to hold together with pre-engine crossref sugaring, the synthetic
//! `Div(#fig-..) > CodeBlock` scaffolds introduced by `PreEngineSugaringStage`
//! in Phase 1.1 must survive serialize → parse round-tripping **structurally
//! unchanged** — i.e. the same block shape, identifiers, and class lists come
//! back out. Source locations legitimately change (synthetic vs. parsed) and
//! are *not* part of the contract.
//!
//! These tests exist to pin that invariant before the Phase 1.1 implementation
//! is built on top of it. If pampa's writer or parser ever regresses in a way
//! that breaks the scaffold round-trip, these tests fail loudly rather than
//! allowing the pre-engine sugar to produce ASTs that reconciliation silently
//! mishandles.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::attr::{Attr, AttrSourceInfo};
use quarto_pandoc_types::block::{Block, CodeBlock, Div};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{FileId, SourceInfo};

fn si() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

fn empty_attr_source() -> AttrSourceInfo {
    AttrSourceInfo::empty()
}

fn attr_with_id(id: &str) -> Attr {
    (id.to_string(), Vec::new(), LinkedHashMap::new())
}

fn attr_with_class(class: &str) -> Attr {
    (String::new(), vec![class.to_string()], LinkedHashMap::new())
}

/// Build a synthetic `Div(#<ident>) > CodeBlock(class=<lang>, text=<body>)`.
fn synthetic_div_over_code_block(ident: &str, lang: &str, body: &str) -> Pandoc {
    let code_block = Block::CodeBlock(CodeBlock {
        attr: attr_with_class(lang),
        text: body.to_string(),
        source_info: si(),
        attr_source: empty_attr_source(),
    });
    let div = Block::Div(Div {
        attr: attr_with_id(ident),
        content: vec![code_block],
        source_info: si(),
        attr_source: empty_attr_source(),
    });
    Pandoc {
        meta: quarto_pandoc_types::ConfigValue::default(),
        blocks: vec![div],
    }
}

/// Structural equality shapes for comparing original and round-tripped blocks.
///
/// Keep this intentionally narrow — it's the public shape contract of a
/// synthetic crossref scaffold. If the round-trip starts adding new fields
/// or rearranging them, tests fail and we revisit Phase 1.1 assumptions.
#[derive(Debug, PartialEq, Eq)]
enum Shape {
    Div {
        id: String,
        classes: Vec<String>,
        children: Vec<Shape>,
    },
    CodeBlock {
        id: String,
        classes: Vec<String>,
        text: String,
    },
    Other(&'static str),
}

fn shape_of(block: &Block) -> Shape {
    match block {
        Block::Div(div) => Shape::Div {
            id: div.attr.0.clone(),
            classes: div.attr.1.clone(),
            children: div.content.iter().map(shape_of).collect(),
        },
        Block::CodeBlock(cb) => Shape::CodeBlock {
            id: cb.attr.0.clone(),
            classes: cb.attr.1.clone(),
            // The qmd reader strips the trailing newline that fenced code
            // produces; the writer round-trips bodies *up to* that trailing
            // newline. Normalize here so the shape comparison tracks the
            // structurally relevant content, not fence terminator whitespace.
            text: cb.text.trim_end_matches('\n').to_string(),
        },
        Block::Plain(_) => Shape::Other("Plain"),
        Block::Paragraph(_) => Shape::Other("Paragraph"),
        Block::Header(_) => Shape::Other("Header"),
        Block::BlockQuote(_) => Shape::Other("BlockQuote"),
        Block::RawBlock(_) => Shape::Other("RawBlock"),
        Block::HorizontalRule(_) => Shape::Other("HorizontalRule"),
        Block::LineBlock(_) => Shape::Other("LineBlock"),
        Block::OrderedList(_) => Shape::Other("OrderedList"),
        Block::BulletList(_) => Shape::Other("BulletList"),
        Block::DefinitionList(_) => Shape::Other("DefinitionList"),
        Block::Table(_) => Shape::Other("Table"),
        Block::Figure(_) => Shape::Other("Figure"),
        Block::BlockMetadata(_) => Shape::Other("BlockMetadata"),
        Block::NoteDefinitionPara(_) => Shape::Other("NoteDefinitionPara"),
        Block::NoteDefinitionFencedBlock(_) => Shape::Other("NoteDefinitionFencedBlock"),
        Block::CaptionBlock(_) => Shape::Other("CaptionBlock"),
        Block::Custom(c) => Shape::Other(match c.type_name.as_str() {
            "Callout" => "Custom:Callout",
            "FloatRefTarget" => "Custom:FloatRefTarget",
            _ => "Custom:other",
        }),
    }
}

fn round_trip(ast: &Pandoc) -> Pandoc {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(ast, &mut buf).expect("qmd writer should succeed");
    let qmd = String::from_utf8(buf).expect("qmd output is utf-8");
    let (parsed, _ctx, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<synthetic>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd reader should succeed");
    parsed
}

#[test]
fn synthetic_fig_div_over_code_block_roundtrips() {
    let input = synthetic_div_over_code_block("fig-1", "python", "print(\"hello\")\n");
    let parsed = round_trip(&input);

    let input_shapes: Vec<Shape> = input.blocks.iter().map(shape_of).collect();
    let parsed_shapes: Vec<Shape> = parsed.blocks.iter().map(shape_of).collect();

    assert_eq!(
        parsed_shapes, input_shapes,
        "synthetic Div(#fig-1) > CodeBlock must round-trip through QMD unchanged"
    );
}

#[test]
fn synthetic_tbl_div_over_code_block_roundtrips() {
    // Same invariant, different ref-type prefix — tbl- should behave the same.
    let input = synthetic_div_over_code_block("tbl-summary", "r", "summary(df)\n");
    let parsed = round_trip(&input);
    assert_eq!(
        parsed.blocks.iter().map(shape_of).collect::<Vec<_>>(),
        input.blocks.iter().map(shape_of).collect::<Vec<_>>(),
    );
}

#[test]
fn roundtrip_preserves_executable_code_block_languages() {
    // Regression guard: make sure the language class on the inner CodeBlock
    // isn't dropped or rewritten during round-trip. Engines that classify code
    // blocks by class would otherwise stop firing after pre-engine sugaring.
    let input = synthetic_div_over_code_block("fig-1", "python", "x = 1\n");
    let parsed = round_trip(&input);
    let inner_classes = match &parsed.blocks[0] {
        Block::Div(div) => match div.content.first().expect("div has a child") {
            Block::CodeBlock(cb) => cb.attr.1.clone(),
            other => panic!("expected CodeBlock, got {:?}", other),
        },
        other => panic!("expected Div, got {:?}", other),
    };
    assert_eq!(inner_classes, vec!["python".to_string()]);
}
