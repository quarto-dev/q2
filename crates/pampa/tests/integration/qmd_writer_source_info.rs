//! Tests for `write_with_source_info` — verifying that the QMD writer
//! produces a SourceInfo::Concat that tiles the entire output and maps
//! byte offsets back to the correct source locations.

use pampa::writers::qmd::write_with_source_info;
use quarto_pandoc_types::block::{CodeBlock, Header, Paragraph};
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::inline::Str;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Block, Inline};
use quarto_source_map::{FileId, SourceContext, SourceInfo};

fn si(file: usize, start: usize, end: usize) -> SourceInfo {
    SourceInfo::original(FileId(file), start, end)
}

fn str_inline(text: &str, source_info: SourceInfo) -> Inline {
    Inline::Str(Str {
        text: text.to_string(),
        source_info,
    })
}

fn paragraph(text: &str, source_info: SourceInfo) -> Block {
    Block::Paragraph(Paragraph {
        content: vec![str_inline(text, source_info.clone())],
        source_info,
    })
}

fn code_block(code: &str, source_info: SourceInfo) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: quarto_pandoc_types::attr::empty_attr(),
        text: code.to_string(),
        source_info,
        attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
    })
}

fn header(text: &str, level: usize, source_info: SourceInfo) -> Block {
    Block::Header(Header {
        level,
        attr: quarto_pandoc_types::attr::empty_attr(),
        content: vec![str_inline(text, source_info.clone())],
        source_info,
        attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
    })
}

#[test]
fn concat_piece_lengths_sum_to_buffer_length() {
    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            paragraph("Hello", si(0, 0, 5)),
            paragraph("World", si(0, 6, 11)),
        ],
    };

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    match &source_info {
        SourceInfo::Concat { pieces } => {
            let total_len: usize = pieces.iter().map(|p| p.length).sum();
            assert_eq!(
                total_len,
                buf.len(),
                "Concat pieces must tile the entire buffer. \
                 Pieces total: {}, buffer len: {}",
                total_len,
                buf.len()
            );
        }
        _ => panic!("Expected Concat, got {:?}", source_info),
    }
}

#[test]
fn concat_covers_output_with_frontmatter() {
    // Build an AST with YAML frontmatter and a block
    let mut meta = ConfigValue::new_string("My Title", si(0, 4, 14));
    // Wrap in a map with key "title"
    let entries = vec![quarto_pandoc_types::config_value::ConfigMapEntry {
        key: "title".to_string(),
        key_source: SourceInfo::for_test(),
        value: meta,
    }];
    meta = ConfigValue::new_map(entries, si(0, 0, 25));

    let pandoc = Pandoc {
        meta,
        blocks: vec![paragraph("Content", si(0, 30, 37))],
    };

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    match &source_info {
        SourceInfo::Concat { pieces } => {
            // Should have 2 pieces: frontmatter + paragraph
            assert_eq!(pieces.len(), 2, "Expected 2 pieces (frontmatter + block)");
            let total_len: usize = pieces.iter().map(|p| p.length).sum();
            assert_eq!(total_len, buf.len());
        }
        _ => panic!("Expected Concat, got {:?}", source_info),
    }
}

#[test]
fn blocks_from_different_files_map_correctly() {
    // Simulate include expansion: blocks from two different files
    let block_main = paragraph("Main content", si(0, 0, 12));
    let block_included = code_block("x = 1", si(1, 0, 5));

    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![block_main, block_included],
    };

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    // Set up a SourceContext so map_offset can resolve
    let mut ctx = SourceContext::new();
    let _fid0 = ctx.add_file("main.qmd".to_string(), Some("Main content".to_string()));
    let _fid1 = ctx.add_file("included.qmd".to_string(), Some("x = 1".to_string()));

    let output = String::from_utf8(buf).unwrap();

    // Find where "x = 1" starts in the output (inside the code block)
    let code_pos = output.find("x = 1").expect("code should be in output");

    let mapped = source_info
        .map_offset(code_pos, &ctx)
        .expect("should resolve");
    assert_eq!(
        mapped.file_id,
        FileId(1),
        "Code block offset should map to the included file (FileId(1))"
    );
}

#[test]
fn map_offset_resolves_block_in_single_file() {
    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![
            header("Title", 1, si(0, 0, 8)),
            paragraph("Body text", si(0, 9, 18)),
        ],
    };

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    let mut ctx = SourceContext::new();
    ctx.add_file(
        "test.qmd".to_string(),
        Some("# Title\nBody text".to_string()),
    );

    let output = String::from_utf8(buf).unwrap();

    // Offset in the "Body text" paragraph
    let body_pos = output.find("Body text").expect("should find body");
    let mapped = source_info
        .map_offset(body_pos, &ctx)
        .expect("should resolve");
    assert_eq!(mapped.file_id, FileId(0));
    // The mapped offset should be within the paragraph's source range (9..18)
    assert!(
        mapped.location.offset >= 9 && mapped.location.offset <= 18,
        "Expected offset in range 9..18, got {}",
        mapped.location.offset
    );
}

#[test]
fn no_blocks_produces_empty_or_frontmatter_only() {
    // No blocks, no meta
    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![],
    };

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    match &source_info {
        SourceInfo::Concat { pieces } => {
            let total_len: usize = pieces.iter().map(|p| p.length).sum();
            assert_eq!(total_len, buf.len());
        }
        _ => {
            // Empty concat or default is fine if buffer is empty
            assert!(
                buf.is_empty(),
                "Non-Concat SourceInfo but buffer is not empty"
            );
        }
    }
}

#[test]
fn round_trip_code_block_offset_accuracy() {
    // Parse a real file, serialize, check offset maps back approximately
    let input = "---\ntitle: test\n---\n\n# Header\n\n```python\nprint('hello')\n```\n";

    let mut stderr = Vec::new();
    let (pandoc, _ast_context, _warnings) =
        pampa::readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut stderr, true, None)
            .expect("parse failed");

    let (buf, source_info) = write_with_source_info(&pandoc).unwrap();

    let mut ctx = SourceContext::new();
    ctx.add_file("test.qmd".to_string(), Some(input.to_string()));

    let output = String::from_utf8(buf).unwrap();

    // Find the code content in the serialized output
    if let Some(code_pos) = output.find("print('hello')") {
        let mapped = source_info.map_offset(code_pos, &ctx);
        assert!(mapped.is_some(), "Code block offset should be resolvable");
        let mapped = mapped.unwrap();
        assert_eq!(mapped.file_id, FileId(0));
        // The original "print('hello')" starts around offset 42 in the input
        let original_pos = input.find("print('hello')").unwrap();
        // Allow some tolerance for fence formatting differences
        let diff = (mapped.location.offset as isize - original_pos as isize).unsigned_abs();
        assert!(
            diff <= 10,
            "Mapped offset {} should be close to original {}, diff was {}",
            mapped.location.offset,
            original_pos,
            diff
        );
    }
}
