/*
 * inline_span_investigation.rs
 *
 * Investigation tests for Phase 5 (inline splicing) of the incremental writer.
 * These tests examine inline-level source spans to determine how the
 * incremental writer can splice individual inlines within blocks.
 *
 * Key questions:
 * 1. Do inline source spans within indentation boundaries include or exclude line prefixes?
 * 2. What are the gaps between consecutive inline spans (e.g., emphasis delimiters)?
 * 3. Can we splice child inlines inside container inlines without touching delimiters?
 *
 * See: claude-notes/plans/2026-02-10-inline-splicing.md
 * Beads issue: bd-1hwd
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::{Block, Inline};
use pampa::readers;
use quarto_source_map::SourceInfo;

// =============================================================================
// Helpers
// =============================================================================

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    result.expect("Failed to parse QMD").0
}

/// Extract source span (start, end) from an Inline's source_info.
fn inline_span(inline: &Inline) -> (usize, usize) {
    let si = inline_source_info(inline);
    (si.start_offset(), si.end_offset())
}

fn inline_source_info(inline: &Inline) -> &SourceInfo {
    match inline {
        Inline::Str(s) => &s.source_info,
        Inline::Emph(e) => &e.source_info,
        Inline::Strong(s) => &s.source_info,
        Inline::Underline(u) => &u.source_info,
        Inline::Strikeout(s) => &s.source_info,
        Inline::Superscript(s) => &s.source_info,
        Inline::Subscript(s) => &s.source_info,
        Inline::SmallCaps(s) => &s.source_info,
        Inline::Quoted(q) => &q.source_info,
        Inline::Cite(c) => &c.source_info,
        Inline::Code(c) => &c.source_info,
        Inline::Space(s) => &s.source_info,
        Inline::SoftBreak(s) => &s.source_info,
        Inline::LineBreak(l) => &l.source_info,
        Inline::Math(m) => &m.source_info,
        Inline::RawInline(r) => &r.source_info,
        Inline::Link(l) => &l.source_info,
        Inline::Image(i) => &i.source_info,
        Inline::Note(n) => &n.source_info,
        Inline::Span(s) => &s.source_info,
        Inline::Shortcode(sc) => &sc.source_info,
        Inline::NoteReference(nr) => &nr.source_info,
        // Attr is a special case — AttrSourceInfo doesn't have a single span.
        // Use a dummy; Attr inlines are rare and won't appear in our tests.
        Inline::Attr(_, _) => {
            static DUMMY: std::sync::LazyLock<SourceInfo> =
                std::sync::LazyLock::new(SourceInfo::default);
            &DUMMY
        }
        Inline::Insert(i) => &i.source_info,
        Inline::Delete(d) => &d.source_info,
        Inline::Highlight(h) => &h.source_info,
        Inline::EditComment(e) => &e.source_info,
        Inline::Custom(c) => &c.source_info,
    }
}

fn inline_type_name(inline: &Inline) -> &'static str {
    match inline {
        Inline::Str(_) => "Str",
        Inline::Emph(_) => "Emph",
        Inline::Strong(_) => "Strong",
        Inline::Underline(_) => "Underline",
        Inline::Strikeout(_) => "Strikeout",
        Inline::Superscript(_) => "Superscript",
        Inline::Subscript(_) => "Subscript",
        Inline::SmallCaps(_) => "SmallCaps",
        Inline::Quoted(_) => "Quoted",
        Inline::Cite(_) => "Cite",
        Inline::Code(_) => "Code",
        Inline::Space(_) => "Space",
        Inline::SoftBreak(_) => "SoftBreak",
        Inline::LineBreak(_) => "LineBreak",
        Inline::Math(_) => "Math",
        Inline::RawInline(_) => "RawInline",
        Inline::Link(_) => "Link",
        Inline::Image(_) => "Image",
        Inline::Note(_) => "Note",
        Inline::Span(_) => "Span",
        Inline::Shortcode(_) => "Shortcode",
        Inline::NoteReference(_) => "NoteReference",
        Inline::Attr(_, _) => "Attr",
        Inline::Insert(_) => "Insert",
        Inline::Delete(_) => "Delete",
        Inline::Highlight(_) => "Highlight",
        Inline::EditComment(_) => "EditComment",
        Inline::Custom(_) => "Custom",
    }
}

/// Get the text content of a Str inline.
fn str_text(inline: &Inline) -> &str {
    match inline {
        Inline::Str(s) => &s.text,
        _ => panic!("Expected Str, got {:?}", inline_type_name(inline)),
    }
}

/// Get the content of a container inline (Emph, Strong, etc.)
fn inline_children(inline: &Inline) -> &[Inline] {
    match inline {
        Inline::Emph(e) => &e.content,
        Inline::Strong(s) => &s.content,
        Inline::Underline(u) => &u.content,
        Inline::Strikeout(s) => &s.content,
        Inline::Superscript(s) => &s.content,
        Inline::Subscript(s) => &s.content,
        Inline::SmallCaps(s) => &s.content,
        Inline::Link(l) => &l.content,
        Inline::Image(i) => &i.content,
        Inline::Span(s) => &s.content,
        Inline::Insert(i) => &i.content,
        Inline::Delete(d) => &d.content,
        Inline::Highlight(h) => &h.content,
        _ => panic!("Not a container inline: {:?}", inline_type_name(inline)),
    }
}

/// Print the inlines of a block for diagnostic purposes.
/// Returns Vec of (type_name, start, end, source_text) for assertions.
fn describe_inlines(input: &str, inlines: &[Inline]) -> Vec<(String, usize, usize, String)> {
    inlines
        .iter()
        .map(|inline| {
            let (start, end) = inline_span(inline);
            let text = if start <= end && end <= input.len() {
                input[start..end].to_string()
            } else {
                format!("<out of range: [{start}, {end}) in {} bytes>", input.len())
            };
            (inline_type_name(inline).to_string(), start, end, text)
        })
        .collect()
}

/// Describe gaps between consecutive inline spans.
fn describe_inline_gaps(input: &str, inlines: &[Inline]) -> Vec<(usize, usize, String)> {
    let spans: Vec<(usize, usize)> = inlines.iter().map(|i| inline_span(i)).collect();
    let mut gaps = Vec::new();
    for i in 0..spans.len().saturating_sub(1) {
        let gap_start = spans[i].1;
        let gap_end = spans[i + 1].0;
        if gap_start <= gap_end && gap_end <= input.len() {
            gaps.push((gap_start, gap_end, input[gap_start..gap_end].to_string()));
        } else {
            gaps.push((
                gap_start,
                gap_end,
                format!("INVALID: [{gap_start}, {gap_end})"),
            ));
        }
    }
    gaps
}

/// Extract the inlines from the first block (expected to be Paragraph or Plain).
fn first_block_inlines(doc: &pampa::pandoc::Pandoc) -> &[Inline] {
    match &doc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        Block::Header(h) => &h.content,
        other => panic!(
            "Expected Paragraph/Plain/Header as first block, got {:?}",
            std::mem::discriminant(other)
        ),
    }
}

// =============================================================================
// Investigation 1: Inline source spans in simple paragraphs
// =============================================================================

#[test]
fn inline_spans_simple_paragraph() {
    let input = "Hello world today.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Simple Paragraph Inline Spans ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Expect: Str("Hello"), Space, Str("world"), Space, Str("today.")
    // (reader merges adjacent Str nodes but Space separates them)
    assert!(
        descs.len() >= 3,
        "Expected at least 3 inlines, got {}",
        descs.len()
    );

    // First inline should be Str containing "Hello"
    assert_eq!(descs[0].0, "Str");
    assert_eq!(descs[0].3, "Hello");
}

#[test]
fn inline_spans_paragraph_with_softbreak() {
    let input = "Hello\nworld\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Paragraph with SoftBreak ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Expect: Str("Hello"), SoftBreak, Str("world")
    // Key question: what does the SoftBreak span cover?
    let softbreak = descs.iter().find(|d| d.0 == "SoftBreak");
    assert!(
        softbreak.is_some(),
        "Expected a SoftBreak inline, got types: {:?}",
        descs.iter().map(|d| &d.0).collect::<Vec<_>>()
    );
    let sb = softbreak.unwrap();
    eprintln!("  SoftBreak span: [{}, {}) = {:?}", sb.1, sb.2, sb.3);
}

// =============================================================================
// Investigation 1a: Inline spans inside BlockQuote
// =============================================================================

#[test]
fn inline_spans_in_blockquote_single_line() {
    let input = "> Hello world.\n";
    let doc = parse_qmd(input);

    // Navigate: BlockQuote → inner Paragraph → inlines
    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let inner_para_inlines = match &bq.content[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain inside BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, inner_para_inlines);

    eprintln!("=== Inline Spans in BlockQuote (single line) ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Key question: Does Str("Hello") span include the "> " prefix?
    // Expected: Str("Hello") at [2, 7) — does NOT include "> "
    assert_eq!(descs[0].0, "Str");
    // The span text should be "Hello", not "> Hello"
    assert_eq!(
        str_text(&inner_para_inlines[0]),
        "Hello",
        "First inline text content should be 'Hello'"
    );
}

#[test]
fn inline_spans_in_blockquote_multiline() {
    // This is THE critical test for Investigation 1.
    // Two-line paragraph inside a block quote.
    let input = "> Hello\n> world\n";
    let doc = parse_qmd(input);

    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let inner_para_inlines = match &bq.content[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain inside BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, inner_para_inlines);
    let gaps = describe_inline_gaps(input, inner_para_inlines);

    eprintln!("=== Inline Spans in BlockQuote (multi-line) ===");
    eprintln!("  Input: {:?}", input);
    eprintln!("  Input bytes: {:?}", input.as_bytes());
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Expected structure: [Str("Hello"), SoftBreak, Str("world")]
    // KEY QUESTION: Where is the "> " prefix on line 2?
    //
    // Hypothesis A: SoftBreak spans the "\n" only, and there's a gap for "> "
    //   Str("Hello"): [2, 7)    SoftBreak: [7, 8)    gap: [8, 10)=">" + " "    Str("world"): [10, 15)
    //
    // Hypothesis B: SoftBreak spans "\n> " (includes the prefix)
    //   Str("Hello"): [2, 7)    SoftBreak: [7, 10)   Str("world"): [10, 15)
    //
    // Hypothesis C: SoftBreak spans just "\n", Str("world") spans "> world"
    //   (unlikely — Str should contain just the text content)
    //
    // We'll document whichever is the case.

    // Find the SoftBreak and its surrounding context
    let sb_idx = descs.iter().position(|d| d.0 == "SoftBreak");
    if let Some(idx) = sb_idx {
        let sb = &descs[idx];
        eprintln!("\n  === KEY FINDING ===");
        eprintln!("  SoftBreak: [{}, {}) = {:?}", sb.1, sb.2, sb.3);
        if idx > 0 {
            let prev = &descs[idx - 1];
            eprintln!("  Previous ({})  end: {}", prev.0, prev.2);
        }
        if idx + 1 < descs.len() {
            let next = &descs[idx + 1];
            eprintln!("  Next ({}) start: {}", next.0, next.1);
        }
        if idx < gaps.len() {
            let gap_after = &gaps[idx];
            eprintln!(
                "  Gap after SoftBreak: [{}, {}) = {:?}",
                gap_after.0, gap_after.1, gap_after.2
            );
        }
    }
}

#[test]
fn inline_spans_in_blockquote_three_lines() {
    let input = "> Hello\n> beautiful\n> world\n";
    let doc = parse_qmd(input);

    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let inner_para_inlines = match &bq.content[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain inside BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, inner_para_inlines);
    let gaps = describe_inline_gaps(input, inner_para_inlines);

    eprintln!("=== Inline Spans in BlockQuote (three lines) ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Investigation 1b: Inline spans inside BulletList
// =============================================================================

#[test]
fn inline_spans_in_bulletlist_single_line() {
    let input = "* Hello world.\n";
    let doc = parse_qmd(input);

    let bl = match &doc.blocks[0] {
        Block::BulletList(bl) => bl,
        other => panic!(
            "Expected BulletList, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let item_inlines = match &bl.content[0][0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain in list item, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, item_inlines);

    eprintln!("=== Inline Spans in BulletList (single line) ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
}

#[test]
fn inline_spans_in_bulletlist_multiline() {
    // Multi-line content within a single list item
    let input = "* Hello\n  world\n";
    let doc = parse_qmd(input);

    let bl = match &doc.blocks[0] {
        Block::BulletList(bl) => bl,
        other => panic!(
            "Expected BulletList, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let item_inlines = match &bl.content[0][0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain in list item, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, item_inlines);
    let gaps = describe_inline_gaps(input, item_inlines);

    eprintln!("=== Inline Spans in BulletList (multi-line) ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // KEY: Where is the "  " continuation indent on line 2?
    // Is it in the SoftBreak span, in a gap, or in the next Str span?
}

// =============================================================================
// Investigation 1c: Inline spans inside OrderedList
// =============================================================================

#[test]
fn inline_spans_in_orderedlist_multiline() {
    let input = "1. Hello\n   world\n";
    let doc = parse_qmd(input);

    let ol = match &doc.blocks[0] {
        Block::OrderedList(ol) => ol,
        other => panic!(
            "Expected OrderedList, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let item_inlines = match &ol.content[0][0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain in list item, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, item_inlines);
    let gaps = describe_inline_gaps(input, item_inlines);

    eprintln!("=== Inline Spans in OrderedList (multi-line) ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Investigation 1d: Inline spans inside nested blockquote > list
// =============================================================================

#[test]
fn inline_spans_in_nested_blockquote_list() {
    let input = "> * Hello\n>   world\n";
    let doc = parse_qmd(input);

    eprintln!("=== Inline Spans in Nested BlockQuote > BulletList ===");
    eprintln!("  Input: {:?}", input);

    // Navigate: BlockQuote → BulletList → item → Paragraph/Plain → inlines
    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    eprintln!("  BlockQuote inner blocks: {}", bq.content.len());
    for (i, block) in bq.content.iter().enumerate() {
        eprintln!("    block {}: {:?}", i, std::mem::discriminant(block));
    }

    let bl = match &bq.content[0] {
        Block::BulletList(bl) => bl,
        other => {
            eprintln!(
                "  Inner block is not BulletList: {:?}",
                std::mem::discriminant(other)
            );
            return; // Document what we find instead of panicking
        }
    };

    let item_inlines = match &bl.content[0][0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => {
            eprintln!(
                "  List item content is not Paragraph/Plain: {:?}",
                std::mem::discriminant(other)
            );
            return;
        }
    };

    let descs = describe_inlines(input, item_inlines);
    let gaps = describe_inline_gaps(input, item_inlines);

    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Investigation 2: Inline span coverage and gaps — emphasis
// =============================================================================

#[test]
fn inline_spans_emphasis_simple() {
    let input = "*Hello* world\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Emphasis Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Expected: Emph([Str("Hello")]), Space, Str("world")
    assert_eq!(descs[0].0, "Emph", "First inline should be Emph");

    // KEY: Does the Emph span include the *...* delimiters?
    eprintln!("\n  === KEY FINDING: Emph span ===");
    eprintln!(
        "  Emph span: [{}, {}) = {:?}",
        descs[0].1, descs[0].2, descs[0].3
    );

    // Now look at the child of Emph
    let emph_children = inline_children(&inlines[0]);
    let child_descs = describe_inlines(input, emph_children);
    eprintln!("  Emph children:");
    for (typ, start, end, text) in &child_descs {
        eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
    }

    // KEY: Is there a gap between the Emph's start and the child Str's start?
    // That gap would be the opening * delimiter.
    if !child_descs.is_empty() {
        let emph_start = descs[0].1;
        let child_start = child_descs[0].1;
        let child_end = child_descs.last().unwrap().2;
        let emph_end = descs[0].2;
        eprintln!(
            "  Opening delimiter gap: [{}, {}) = {:?}",
            emph_start,
            child_start,
            &input[emph_start..child_start]
        );
        eprintln!(
            "  Closing delimiter gap: [{}, {}) = {:?}",
            child_end,
            emph_end,
            &input[child_end..emph_end]
        );
    }
}

#[test]
fn inline_spans_strong_simple() {
    let input = "**Hello** world\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);

    eprintln!("=== Strong Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    assert_eq!(descs[0].0, "Strong", "First inline should be Strong");

    let strong_children = inline_children(&inlines[0]);
    let child_descs = describe_inlines(input, strong_children);
    eprintln!("  Strong children:");
    for (typ, start, end, text) in &child_descs {
        eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
    }

    if !child_descs.is_empty() {
        let strong_start = descs[0].1;
        let child_start = child_descs[0].1;
        let child_end = child_descs.last().unwrap().2;
        let strong_end = descs[0].2;
        eprintln!(
            "  Opening delimiter gap: [{}, {}) = {:?}",
            strong_start,
            child_start,
            &input[strong_start..child_start]
        );
        eprintln!(
            "  Closing delimiter gap: [{}, {}) = {:?}",
            child_end,
            strong_end,
            &input[child_end..strong_end]
        );
    }
}

#[test]
fn inline_spans_nested_emphasis() {
    // Nested: strong containing emphasis
    let input = "**_Hello_** world\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);

    eprintln!("=== Nested Emphasis Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Navigate into Strong → Emph → Str
    if descs[0].0 == "Strong" {
        let strong_children = inline_children(&inlines[0]);
        let sc_descs = describe_inlines(input, strong_children);
        eprintln!("  Strong children:");
        for (typ, start, end, text) in &sc_descs {
            eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
        }

        if !strong_children.is_empty() && matches!(strong_children[0], Inline::Emph(_)) {
            let emph_children = inline_children(&strong_children[0]);
            let ec_descs = describe_inlines(input, emph_children);
            eprintln!("  Emph children (inside Strong):");
            for (typ, start, end, text) in &ec_descs {
                eprintln!("      {typ}: [{start}, {end}) = {:?}", text);
            }
        }
    }
}

// =============================================================================
// Investigation 2b: Inline span coverage — links
// =============================================================================

#[test]
fn inline_spans_link() {
    let input = "Click [here](https://example.com) please.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Link Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Find the Link inline
    let link_desc = descs.iter().find(|d| d.0 == "Link");
    if let Some(ld) = link_desc {
        eprintln!("\n  === KEY FINDING: Link span ===");
        eprintln!("  Link span: [{}, {}) = {:?}", ld.1, ld.2, ld.3);

        let link_idx = descs.iter().position(|d| d.0 == "Link").unwrap();
        let link_children = inline_children(&inlines[link_idx]);
        let child_descs = describe_inlines(input, link_children);
        eprintln!("  Link children:");
        for (typ, start, end, text) in &child_descs {
            eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
        }
    }
}

// =============================================================================
// Investigation 2c: Inline span coverage — inline code
// =============================================================================

#[test]
fn inline_spans_code() {
    let input = "Use `code` here.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);

    eprintln!("=== Code Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Does the Code span include the backtick delimiters?
    let code_desc = descs.iter().find(|d| d.0 == "Code");
    if let Some(cd) = code_desc {
        eprintln!("\n  === KEY FINDING: Code span ===");
        eprintln!("  Code span: [{}, {}) = {:?}", cd.1, cd.2, cd.3);
    }
}

// =============================================================================
// Investigation 2d: Inline span coverage — multiple container inlines
// =============================================================================

#[test]
fn inline_spans_mixed_containers() {
    let input = "*Hello* and **world** today.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Mixed Container Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Check total coverage: do inline spans + gaps tile the paragraph content?
    let _para_inlines = inlines;
    let total_inline_bytes: usize = descs.iter().map(|d| d.2 - d.1).sum();
    let total_gap_bytes: usize = gaps.iter().map(|g| g.1 - g.0).sum();
    let first_start = descs.first().map(|d| d.1).unwrap_or(0);
    let last_end = descs.last().map(|d| d.2).unwrap_or(0);
    eprintln!(
        "\n  Coverage: {} inline bytes + {} gap bytes = {} total, span range [{}, {})",
        total_inline_bytes,
        total_gap_bytes,
        total_inline_bytes + total_gap_bytes,
        first_start,
        last_end
    );
}

// =============================================================================
// Investigation 3: Container inline delimiter handling
// =============================================================================

#[test]
fn inline_spans_emph_delimiter_gaps() {
    // Can we splice the child Str inside Emph while preserving the * delimiters?
    let input = "*Hello* rest.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);

    eprintln!("=== Emph Delimiter Gap Analysis ===");
    eprintln!("  Input: {:?}", input);

    let emph = &inlines[0];
    let (emph_start, emph_end) = inline_span(emph);
    let emph_text = &input[emph_start..emph_end];
    eprintln!("  Emph: [{}, {}) = {:?}", emph_start, emph_end, emph_text);

    let children = inline_children(emph);
    assert!(!children.is_empty(), "Emph should have children");

    let (child_start, child_end) = inline_span(&children[0]);
    let child_text = &input[child_start..child_end];
    eprintln!(
        "  Child Str: [{}, {}) = {:?}",
        child_start, child_end, child_text
    );

    let opening = &input[emph_start..child_start];
    let closing = &input[child_end..emph_end];
    eprintln!(
        "  Opening delimiter: {:?} ({} bytes)",
        opening,
        opening.len()
    );
    eprintln!(
        "  Closing delimiter: {:?} ({} bytes)",
        closing,
        closing.len()
    );

    eprintln!("\n  === SPLICE FEASIBILITY ===");
    eprintln!(
        "  If we replace child span [{}, {}) with new text,",
        child_start, child_end
    );
    eprintln!(
        "  the delimiters {:?} and {:?} are preserved.",
        opening, closing
    );
    eprintln!("  This means: YES, we can splice the child without touching delimiters");
    eprintln!("  (assuming delimiter gaps are non-empty and correct).");

    // The actual assertion: opening delimiter should be "*" and closing should be "*"
    // (We'll check this once we know the actual span layout)
}

#[test]
fn inline_spans_strong_delimiter_gaps() {
    let input = "**Hello** rest.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let strong = &inlines[0];
    let (strong_start, strong_end) = inline_span(strong);
    let children = inline_children(strong);
    let (child_start, child_end) = inline_span(&children[0]);

    let opening = &input[strong_start..child_start];
    let closing = &input[child_end..strong_end];

    eprintln!("=== Strong Delimiter Gap Analysis ===");
    eprintln!(
        "  Strong: [{}, {}) = {:?}",
        strong_start,
        strong_end,
        &input[strong_start..strong_end]
    );
    eprintln!(
        "  Child:  [{}, {}) = {:?}",
        child_start,
        child_end,
        &input[child_start..child_end]
    );
    eprintln!("  Opening: {:?}", opening);
    eprintln!("  Closing: {:?}", closing);
}

#[test]
fn inline_spans_link_delimiter_gaps() {
    let input = "[Hello](https://example.com) rest.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let link = &inlines[0];
    let (link_start, link_end) = inline_span(link);
    let link_text = &input[link_start..link_end];

    eprintln!("=== Link Delimiter Gap Analysis ===");
    eprintln!("  Link: [{}, {}) = {:?}", link_start, link_end, link_text);

    let children = inline_children(link);
    if !children.is_empty() {
        let (child_start, child_end) = inline_span(&children[0]);
        let child_text = &input[child_start..child_end];
        eprintln!(
            "  Child: [{}, {}) = {:?}",
            child_start, child_end, child_text
        );

        let opening = &input[link_start..child_start];
        let closing = &input[child_end..link_end];
        eprintln!("  Opening: {:?}", opening);
        eprintln!("  Closing (includes URL part): {:?}", closing);
    }

    let child_descs = describe_inlines(input, children);
    for (typ, start, end, text) in &child_descs {
        eprintln!("  Child {typ}: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Investigation 3b: Nested container delimiter handling
// =============================================================================

#[test]
fn inline_spans_nested_container_delimiters() {
    // Strong > Emph > Str — three levels of nesting
    let input = "**_Hello_** rest.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);

    eprintln!("=== Nested Container Delimiter Analysis ===");
    eprintln!("  Input: {:?}", input);

    // Level 0: Strong
    let strong = &inlines[0];
    let (s_start, s_end) = inline_span(strong);
    eprintln!(
        "  Strong: [{}, {}) = {:?}",
        s_start,
        s_end,
        &input[s_start..s_end]
    );

    // Level 1: Emph (child of Strong)
    let strong_children = inline_children(strong);
    if !strong_children.is_empty() && matches!(strong_children[0], Inline::Emph(_)) {
        let emph = &strong_children[0];
        let (e_start, e_end) = inline_span(emph);
        eprintln!(
            "  Emph:   [{}, {}) = {:?}",
            e_start,
            e_end,
            &input[e_start..e_end]
        );

        // Level 2: Str (child of Emph)
        let emph_children = inline_children(emph);
        if !emph_children.is_empty() {
            let str_node = &emph_children[0];
            let (t_start, t_end) = inline_span(str_node);
            eprintln!(
                "  Str:    [{}, {}) = {:?}",
                t_start,
                t_end,
                &input[t_start..t_end]
            );

            // Delimiter analysis at each level
            eprintln!("\n  Delimiter analysis:");
            eprintln!(
                "  Strong opening: [{}, {}) = {:?}",
                s_start,
                e_start,
                &input[s_start..e_start]
            );
            eprintln!(
                "  Emph opening:   [{}, {}) = {:?}",
                e_start,
                t_start,
                &input[e_start..t_start]
            );
            eprintln!(
                "  Emph closing:   [{}, {}) = {:?}",
                t_end,
                e_end,
                &input[t_end..e_end]
            );
            eprintln!(
                "  Strong closing: [{}, {}) = {:?}",
                e_end,
                s_end,
                &input[e_end..s_end]
            );

            eprintln!("\n  === NESTED SPLICE FEASIBILITY ===");
            eprintln!(
                "  To change 'Hello' to 'World', we replace [{}, {})",
                t_start, t_end
            );
            eprintln!("  All delimiter bytes are preserved in gaps.");
        }
    }
}

// =============================================================================
// Investigation: LineBreak span analysis
// =============================================================================

#[test]
fn inline_spans_linebreak() {
    // LineBreak is written as "\" + newline in QMD
    let input = "Hello\\\nworld\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== LineBreak Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    let lb = descs.iter().find(|d| d.0 == "LineBreak");
    if let Some(lb) = lb {
        eprintln!("\n  === KEY FINDING: LineBreak span ===");
        eprintln!("  LineBreak: [{}, {}) = {:?}", lb.1, lb.2, lb.3);
        eprintln!("  Does it include the backslash? {}", lb.3.contains('\\'));
    }
}

#[test]
fn inline_spans_linebreak_in_blockquote() {
    let input = "> Hello\\\n> world\n";
    let doc = parse_qmd(input);

    let bq = match &doc.blocks[0] {
        Block::BlockQuote(bq) => bq,
        other => panic!(
            "Expected BlockQuote, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    let inner_inlines = match &bq.content[0] {
        Block::Paragraph(p) => &p.content,
        Block::Plain(p) => &p.content,
        other => panic!(
            "Expected Paragraph/Plain, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let descs = describe_inlines(input, inner_inlines);
    let gaps = describe_inline_gaps(input, inner_inlines);

    eprintln!("=== LineBreak in BlockQuote ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Investigation: Space inline span analysis
// =============================================================================

#[test]
fn inline_spans_space() {
    let input = "Hello world today.\n";
    let doc = parse_qmd(input);

    let inlines = first_block_inlines(&doc);
    let descs = describe_inlines(input, inlines);
    let gaps = describe_inline_gaps(input, inlines);

    eprintln!("=== Space Inline Spans ===");
    eprintln!("  Input: {:?}", input);
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Check: does Space span cover the actual space character?
    let space = descs.iter().find(|d| d.0 == "Space");
    if let Some(sp) = space {
        eprintln!("\n  === KEY FINDING: Space span ===");
        eprintln!("  Space: [{}, {}) = {:?}", sp.1, sp.2, sp.3);
        eprintln!("  Space span length: {} bytes", sp.2 - sp.1);
    }
}
