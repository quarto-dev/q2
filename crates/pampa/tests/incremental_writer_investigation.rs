/*
 * incremental_writer_investigation.rs
 *
 * Investigation tests for the incremental writer design.
 * These tests examine source span behavior to determine how the incremental
 * writer should assemble output from verbatim spans and rewritten blocks.
 *
 * See: claude-notes/plans/2026-02-07-incremental-writer.md
 * Beads issue: bd-2t4o
 *
 * Copyright (c) 2025 Posit, PBC
 */

use pampa::pandoc::Block;
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

/// Extract the source span (start_offset, end_offset) from a Block's source_info.
fn block_span(block: &Block) -> (usize, usize) {
    let si = block_source_info(block);
    (si.start_offset(), si.end_offset())
}

/// Extract the SourceInfo from a Block.
fn block_source_info(block: &Block) -> &SourceInfo {
    match block {
        Block::Paragraph(p) => &p.source_info,
        Block::Header(h) => &h.source_info,
        Block::CodeBlock(cb) => &cb.source_info,
        Block::BlockQuote(bq) => &bq.source_info,
        Block::BulletList(bl) => &bl.source_info,
        Block::OrderedList(ol) => &ol.source_info,
        Block::Div(d) => &d.source_info,
        Block::HorizontalRule(hr) => &hr.source_info,
        Block::Table(t) => &t.source_info,
        Block::RawBlock(rb) => &rb.source_info,
        Block::Plain(p) => &p.source_info,
        Block::LineBlock(lb) => &lb.source_info,
        Block::DefinitionList(dl) => &dl.source_info,
        Block::Figure(f) => &f.source_info,
        Block::BlockMetadata(m) => &m.source_info,
        Block::NoteDefinitionPara(nd) => &nd.source_info,
        Block::NoteDefinitionFencedBlock(nd) => &nd.source_info,
        Block::CaptionBlock(cb) => &cb.source_info,
        Block::Custom(cn) => &cn.source_info,
    }
}

/// Extract the text that a block's source span covers in the original input.
#[allow(dead_code)]
fn block_text<'a>(input: &'a str, block: &Block) -> &'a str {
    let (start, end) = block_span(block);
    &input[start..end]
}

/// Print a diagnostic summary of all top-level block spans.
/// Returns a Vec of (block_type, start, end, covered_text) for assertions.
fn describe_blocks(input: &str, blocks: &[Block]) -> Vec<(String, usize, usize, String)> {
    blocks
        .iter()
        .map(|b| {
            let (start, end) = block_span(b);
            let block_type = match b {
                Block::Paragraph(_) => "Paragraph",
                Block::Header(_) => "Header",
                Block::CodeBlock(_) => "CodeBlock",
                Block::BlockQuote(_) => "BlockQuote",
                Block::BulletList(_) => "BulletList",
                Block::OrderedList(_) => "OrderedList",
                Block::Div(_) => "Div",
                Block::HorizontalRule(_) => "HorizontalRule",
                Block::Table(_) => "Table",
                Block::RawBlock(_) => "RawBlock",
                Block::Plain(_) => "Plain",
                Block::LineBlock(_) => "LineBlock",
                Block::DefinitionList(_) => "DefinitionList",
                Block::Figure(_) => "Figure",
                Block::BlockMetadata(_) => "BlockMetadata",
                Block::Custom(_) => "Custom",
                _ => "Other",
            };
            let text = &input[start..end];
            (block_type.to_string(), start, end, text.to_string())
        })
        .collect()
}

/// Describe the gap between consecutive block spans.
/// Returns Vec of (gap_start, gap_end, gap_text) for the spaces between blocks.
fn describe_gaps(input: &str, blocks: &[Block]) -> Vec<(usize, usize, String)> {
    let spans: Vec<(usize, usize)> = blocks.iter().map(|b| block_span(b)).collect();
    let mut gaps = Vec::new();
    for i in 0..spans.len() - 1 {
        let gap_start = spans[i].1;
        let gap_end = spans[i + 1].0;
        if gap_start < gap_end {
            gaps.push((gap_start, gap_end, input[gap_start..gap_end].to_string()));
        } else if gap_start == gap_end {
            gaps.push((gap_start, gap_end, String::new()));
        } else {
            // Overlapping spans — this would be surprising
            gaps.push((
                gap_start,
                gap_end,
                format!("OVERLAP: {} > {}", gap_start, gap_end),
            ));
        }
    }
    gaps
}

// =============================================================================
// Test: Sequential paragraphs
// =============================================================================

#[test]
fn span_sequential_paragraphs() {
    let input = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.\n";
    let doc = parse_qmd(input);

    assert_eq!(doc.blocks.len(), 3);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    // Document what each block's span covers
    eprintln!("=== Sequential Paragraphs ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Key assertions: verify we understand the span structure
    // Each paragraph should cover its text content
    assert!(
        descs[0].3.contains("First paragraph"),
        "First block should contain 'First paragraph', got: {:?}",
        descs[0].3
    );
    assert!(
        descs[1].3.contains("Second paragraph"),
        "Second block should contain 'Second paragraph', got: {:?}",
        descs[1].3
    );
    assert!(
        descs[2].3.contains("Third paragraph"),
        "Third block should contain 'Third paragraph', got: {:?}",
        descs[2].3
    );

    // Check coverage: do the spans + gaps cover the entire input?
    let first_start = descs[0].1;
    let last_end = descs.last().unwrap().2;
    eprintln!(
        "  Coverage: [{first_start}, {last_end}) out of [0, {})",
        input.len()
    );
}

// =============================================================================
// Test: Headers followed by paragraphs
// =============================================================================

#[test]
fn span_headers_and_paragraphs() {
    let input = "## First Header\n\nA paragraph.\n\n### Second Header\n\nAnother paragraph.\n";
    let doc = parse_qmd(input);

    assert_eq!(doc.blocks.len(), 4);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Headers and Paragraphs ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    assert_eq!(descs[0].0, "Header");
    assert_eq!(descs[1].0, "Paragraph");
    assert_eq!(descs[2].0, "Header");
    assert_eq!(descs[3].0, "Paragraph");
}

// =============================================================================
// Test: Fenced divs with inner content
// =============================================================================

#[test]
fn span_fenced_div() {
    let input = "Before.\n\n::: {.note}\n\nInner paragraph.\n\n:::\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Fenced Div ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Should have: Paragraph, Div, Paragraph
    assert_eq!(descs.len(), 3, "Expected 3 blocks, got: {:?}", descs);
    assert_eq!(descs[0].0, "Paragraph");
    assert_eq!(descs[1].0, "Div");
    assert_eq!(descs[2].0, "Paragraph");

    // The div span should include the ::: fences
    assert!(
        descs[1].3.contains(":::"),
        "Div span should include ::: fences, got: {:?}",
        descs[1].3
    );

    // Check the div's inner blocks
    if let Block::Div(div) = &doc.blocks[1] {
        assert!(!div.content.is_empty(), "Div should have inner content");
        let inner_descs = describe_blocks(input, &div.content);
        eprintln!("  Div inner blocks:");
        for (typ, start, end, text) in &inner_descs {
            eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
        }
    }
}

// =============================================================================
// Test: Block quotes (single level)
// =============================================================================

#[test]
fn span_block_quote_single() {
    let input = "Before.\n\n> Quoted paragraph.\n> Continued.\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Single Block Quote ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Should have: Paragraph, BlockQuote, Paragraph
    assert_eq!(descs[0].0, "Paragraph");
    assert_eq!(descs[1].0, "BlockQuote");
    assert_eq!(descs[2].0, "Paragraph");

    // Block quote span should include the > markers
    assert!(
        descs[1].3.contains(">"),
        "BlockQuote span should include > markers, got: {:?}",
        descs[1].3
    );

    // Check inner block of block quote
    if let Block::BlockQuote(bq) = &doc.blocks[1] {
        let inner_descs = describe_blocks(input, &bq.content);
        eprintln!("  BlockQuote inner blocks:");
        for (typ, start, end, text) in &inner_descs {
            eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
        }
        // Inner paragraph span — does it include the > prefix or not?
        // This is a KEY question for the incremental writer.
    }
}

// =============================================================================
// Test: Nested block quotes
// =============================================================================

#[test]
fn span_block_quote_nested() {
    let input = "> Outer.\n>\n> > Inner.\n> > Continued.\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Nested Block Quote ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Navigate into nested structure
    if let Some(Block::BlockQuote(outer)) = doc.blocks.first() {
        eprintln!("  Outer BlockQuote inner blocks:");
        for block in &outer.content {
            let (start, end) = block_span(block);
            let text = &input[start..end];
            eprintln!(
                "    {:?}: [{start}, {end}) = {:?}",
                std::mem::discriminant(block),
                text
            );

            if let Block::BlockQuote(inner) = block {
                eprintln!("    Inner BlockQuote inner blocks:");
                for inner_block in &inner.content {
                    let (start, end) = block_span(inner_block);
                    let text = &input[start..end];
                    eprintln!(
                        "      {:?}: [{start}, {end}) = {:?}",
                        std::mem::discriminant(inner_block),
                        text
                    );
                }
            }
        }
    }
}

// =============================================================================
// Test: Bullet list
// =============================================================================

#[test]
fn span_bullet_list() {
    let input = "Before.\n\n* First item\n* Second item\n* Third item\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Bullet List ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Should have: Paragraph, BulletList, Paragraph
    assert_eq!(descs[1].0, "BulletList");

    // BulletList span should include the * markers
    assert!(
        descs[1].3.contains("*"),
        "BulletList span should include * markers"
    );

    // Check list item inner blocks
    if let Block::BulletList(bl) = &doc.blocks[1] {
        eprintln!("  BulletList items ({} items):", bl.content.len());
        for (i, item) in bl.content.iter().enumerate() {
            eprintln!("    Item {i}:");
            for block in item {
                let (start, end) = block_span(block);
                let text = &input[start..end];
                eprintln!(
                    "      {:?}: [{start}, {end}) = {:?}",
                    std::mem::discriminant(block),
                    text
                );
            }
        }
    }
}

// =============================================================================
// Test: Bullet list with multi-line items
// =============================================================================

#[test]
fn span_bullet_list_multiline() {
    let input = "* First item\n  continued here.\n* Second item\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Bullet List (multi-line items) ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    if let Block::BulletList(bl) = &doc.blocks[0] {
        eprintln!("  BulletList items:");
        for (i, item) in bl.content.iter().enumerate() {
            eprintln!("    Item {i}:");
            for block in item {
                let (start, end) = block_span(block);
                let text = &input[start..end];
                eprintln!(
                    "      {:?}: [{start}, {end}) = {:?}",
                    std::mem::discriminant(block),
                    text
                );
            }
        }
    }
}

// =============================================================================
// Test: Ordered list
// =============================================================================

#[test]
fn span_ordered_list() {
    let input = "1. First\n2. Second\n3. Third\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Ordered List ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    assert_eq!(descs[0].0, "OrderedList");

    if let Block::OrderedList(ol) = &doc.blocks[0] {
        eprintln!("  OrderedList items ({} items):", ol.content.len());
        for (i, item) in ol.content.iter().enumerate() {
            eprintln!("    Item {i}:");
            for block in item {
                let (start, end) = block_span(block);
                let text = &input[start..end];
                eprintln!(
                    "      {:?}: [{start}, {end}) = {:?}",
                    std::mem::discriminant(block),
                    text
                );
            }
        }
    }
}

// =============================================================================
// Test: Code blocks
// =============================================================================

#[test]
fn span_code_block() {
    let input = "Before.\n\n```python\nprint('hello')\n```\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Code Block ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    assert_eq!(descs[1].0, "CodeBlock");

    // Code block span should include the ``` fences
    assert!(
        descs[1].3.contains("```"),
        "CodeBlock span should include ``` fences, got: {:?}",
        descs[1].3
    );
}

// =============================================================================
// Test: Pipe table
// =============================================================================

#[test]
fn span_pipe_table() {
    let input = "Before.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Pipe Table ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // Should have a Table block (pipe tables are parsed directly as Table)
    let table_block = descs.iter().find(|d| d.0 == "Table");
    assert!(
        table_block.is_some(),
        "Expected a Table block, got: {:?}",
        descs.iter().map(|d| &d.0).collect::<Vec<_>>()
    );
}

// =============================================================================
// Test: List-table (desugared from div)
// =============================================================================

#[test]
fn span_list_table() {
    let input = "\
Before.

::: {.list-table}

* * A
  * B

* * 1
  * 2

:::

After.
";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== List-Table ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }

    // After desugaring, the div should become a Table
    // The source span should still point to the original div text
    let table_block = descs.iter().find(|d| d.0 == "Table");
    assert!(
        table_block.is_some(),
        "Expected list-table div to be desugared into Table, got: {:?}",
        descs.iter().map(|d| &d.0).collect::<Vec<_>>()
    );

    // The Table's source span should cover the original ::: div text
    if let Some(tb) = table_block {
        assert!(
            tb.3.contains(":::"),
            "Table source span should cover original ::: div, got: {:?}",
            tb.3
        );
    }
}

// =============================================================================
// Test: YAML front matter
// =============================================================================

#[test]
fn span_yaml_front_matter() {
    let input = "---\ntitle: Hello\nauthor: World\n---\n\nA paragraph.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== YAML Front Matter ===");
    eprintln!("  Number of blocks: {}", doc.blocks.len());
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Front matter is parsed into doc.meta, not as a block.
    // Check if there are any blocks at all, and what spans they have.
    // The paragraph should follow the front matter.
    eprintln!("  Meta keys: {:?}", {
        if let quarto_pandoc_types::ConfigValueKind::Map(entries) = &doc.meta.value {
            entries.iter().map(|e| e.key.clone()).collect::<Vec<_>>()
        } else {
            vec![]
        }
    });
}

// =============================================================================
// Test: Inner metadata block (pampa extension)
// =============================================================================

#[test]
fn span_inner_metadata() {
    let input = "A paragraph.\n\n---\nkey: value\n---\n\nAnother paragraph.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Inner Metadata ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Test: Horizontal rule
// =============================================================================

#[test]
fn span_horizontal_rule() {
    let _input = "Before.\n\n---\n\nAfter.\n";
    // Note: --- at the start of a doc is YAML front matter, but between blocks
    // it could be a horizontal rule. Let's use *** to be unambiguous.
    let input2 = "Before.\n\n***\n\nAfter.\n";
    let doc = parse_qmd(input2);

    let descs = describe_blocks(input2, &doc.blocks);
    let gaps = describe_gaps(input2, &doc.blocks);

    eprintln!("=== Horizontal Rule ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (start, end, text) in &gaps {
        eprintln!("  gap: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Test: Shortcode nodes
// =============================================================================

#[test]
fn span_shortcode_in_paragraph() {
    let input = "Before {{< video https://example.com >}} after.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Shortcode in Paragraph ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // Navigate into the paragraph's inlines to find the shortcode
    if let Some(Block::Paragraph(para)) = doc.blocks.first() {
        eprintln!("  Paragraph inlines ({}):", para.content.len());
        for inline in &para.content {
            let (si, typ) = match inline {
                pampa::pandoc::Inline::Str(s) => (&s.source_info, "Str"),
                pampa::pandoc::Inline::Space(s) => (&s.source_info, "Space"),
                pampa::pandoc::Inline::Shortcode(sc) => (&sc.source_info, "Shortcode"),
                pampa::pandoc::Inline::SoftBreak(sb) => (&sb.source_info, "SoftBreak"),
                pampa::pandoc::Inline::Custom(cn) => (&cn.source_info, "Custom"),
                _ => continue,
            };
            let start = si.start_offset();
            let end = si.end_offset();
            let text = if start < input.len() && end <= input.len() {
                &input[start..end]
            } else {
                "<out of range>"
            };
            eprintln!("    {typ}: [{start}, {end}) = {:?}", text);
        }
    }
}

// =============================================================================
// Test: Standalone shortcode block
// =============================================================================

#[test]
fn span_shortcode_standalone() {
    let input = "Before.\n\n{{< include _partial.qmd >}}\n\nAfter.\n";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Standalone Shortcode ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
}

// =============================================================================
// Test: Complete coverage check — do block spans tile the input?
// =============================================================================

#[test]
fn span_coverage_simple_document() {
    let input = "\
## Title

First paragraph.

Second paragraph.

### Subtitle

Third paragraph.
";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);
    let gaps = describe_gaps(input, &doc.blocks);

    eprintln!("=== Coverage Check ===");
    eprintln!("  Input length: {}", input.len());
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }
    for (i, (start, end, text)) in gaps.iter().enumerate() {
        eprintln!("  gap {i}: [{start}, {end}) = {:?}", text);
    }

    // Compute: what fraction of the input is covered by block spans?
    let covered: usize = descs.iter().map(|d| d.2 - d.1).sum();
    let total = input.len();
    eprintln!(
        "  Coverage: {covered}/{total} bytes ({:.1}%)",
        100.0 * covered as f64 / total as f64
    );

    // Check for overlaps between consecutive blocks
    for i in 0..descs.len() - 1 {
        assert!(
            descs[i].2 <= descs[i + 1].1,
            "Block spans should not overlap: block {} ends at {}, block {} starts at {}",
            i,
            descs[i].2,
            i + 1,
            descs[i + 1].1
        );
    }
}

// =============================================================================
// Test: Block quote with multiple inner blocks
// =============================================================================

#[test]
fn span_block_quote_multiple_inner() {
    let input = "\
> First paragraph.
>
> Second paragraph.
>
> Third paragraph.

After.
";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Block Quote with Multiple Inner Blocks ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    if let Some(Block::BlockQuote(bq)) = doc.blocks.first() {
        eprintln!("  BlockQuote inner blocks ({}):", bq.content.len());
        for block in &bq.content {
            let (start, end) = block_span(block);
            let text = &input[start..end];
            eprintln!(
                "    {:?}: [{start}, {end}) = {:?}",
                std::mem::discriminant(block),
                text
            );
        }

        // KEY QUESTION: do inner paragraph spans include the > prefix?
        // If they do, then inner spans are non-contiguous (confirming the
        // indentation boundary classification).
        // If they don't, the span is just the content text without >.
    }
}

// =============================================================================
// Test: Nested list (indentation boundaries stacking)
// =============================================================================

#[test]
fn span_nested_list() {
    let input = "\
* Outer 1
  * Inner A
  * Inner B
* Outer 2

After.
";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Nested List ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    if let Some(Block::BulletList(bl)) = doc.blocks.first() {
        eprintln!("  Outer list items ({}):", bl.content.len());
        for (i, item) in bl.content.iter().enumerate() {
            eprintln!("    Outer item {i}:");
            for block in item {
                let (start, end) = block_span(block);
                let text = &input[start..end];
                let block_type = match block {
                    Block::BulletList(_) => "BulletList",
                    Block::Plain(_) => "Plain",
                    Block::Paragraph(_) => "Paragraph",
                    _ => "Other",
                };
                eprintln!("      {block_type}: [{start}, {end}) = {:?}", text);

                // If inner list, inspect its items too
                if let Block::BulletList(inner_bl) = block {
                    for (j, inner_item) in inner_bl.content.iter().enumerate() {
                        for inner_block in inner_item {
                            let (s, e) = block_span(inner_block);
                            let t = &input[s..e];
                            eprintln!("        Inner item {j}: [{s}, {e}) = {:?}", t);
                        }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Test: Definition list (desugared from div)
// =============================================================================

#[test]
fn span_definition_list() {
    let input = "\
::: {.definition-list}

* Term One

  * Definition one A.
  * Definition one B.

* Term Two

  * Definition two.

:::

After.
";
    let doc = parse_qmd(input);

    let descs = describe_blocks(input, &doc.blocks);

    eprintln!("=== Definition List ===");
    for (typ, start, end, text) in &descs {
        eprintln!("  {typ}: [{start}, {end}) = {:?}", text);
    }

    // After desugaring, should be a DefinitionList
    let dl_block = descs.iter().find(|d| d.0 == "DefinitionList");
    assert!(
        dl_block.is_some(),
        "Expected definition-list div to be desugared into DefinitionList, got: {:?}",
        descs.iter().map(|d| &d.0).collect::<Vec<_>>()
    );
}
