/*
 * test_link_title_provenance.rs
 *
 * Pins `TargetSourceInfo.title` (link/image `[text](url "title")` titles)
 * to **content provenance** — a `SourceInfo` over the decoded title text,
 * quotes excluded and escapes collapsed piecewise — rather than the raw,
 * quote-inclusive node span.
 *
 * This is deliberately a parsing test, not a unit test of
 * `extract_quoted_text` (already covered by
 * `pandoc::treesitter_utils::text_helpers::tests`). D1's revert audit found
 * that reverting *only* the "title" node's call site in `treesitter.rs`
 * (handing back the raw node span instead of `extract_quoted_text`'s
 * result) left the entire workspace suite green — nothing pinned the
 * wiring from that call site through `process_target` /
 * `process_pandoc_span` / `process_pandoc_image` into
 * `TargetSourceInfo.title`. These tests parse real qmd through the real
 * `treesitter_to_pandoc` entry point and read `target_source.title` off
 * the resulting `Link`/`Image`, so a regression in that wiring — not just
 * in the decoder itself — turns them red.
 *
 * Per `quarto_config::span_assert`'s module docs, span assertions must be
 * bound to real parsed text, never to `SourceInfo::for_test()`.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::{ASTContext, Block, Inline, treesitter_to_pandoc};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_source_map::{FileId, SourceInfo};
use tree_sitter_qmd::MarkdownParser;

/// Parse `input` as qmd and return the resulting Pandoc AST.
fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");

    let context = ASTContext::anonymous();
    let mut error_collector = DiagnosticCollector::new();
    treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        input_bytes,
        &context,
        &mut error_collector,
    )
    .expect("Failed to convert to Pandoc AST")
}

/// The first `Link` inline found anywhere in `pandoc`'s first paragraph.
fn first_link(pandoc: &pampa::pandoc::Pandoc) -> &pampa::pandoc::inline::Link {
    let Block::Paragraph(para) = &pandoc.blocks[0] else {
        panic!("expected a Paragraph block, got {:?}", pandoc.blocks[0]);
    };
    para.content
        .iter()
        .find_map(|inline| match inline {
            Inline::Link(link) => Some(link),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected a Link inline, got {:?}", para.content))
}

/// The first `Image` inline found anywhere in `pandoc`'s first block.
///
/// A lone `![alt](url "title")` on its own line parses to a block-level
/// `Figure` wrapping a `Plain`, not a `Paragraph` — so this walks the
/// `Plain`'s content rather than assuming the `Paragraph` shape `first_link`
/// uses for an inline link.
fn first_image(pandoc: &pampa::pandoc::Pandoc) -> &pampa::pandoc::inline::Image {
    let content: &[Inline] = match &pandoc.blocks[0] {
        Block::Paragraph(para) => &para.content,
        Block::Figure(figure) => match figure.content.first() {
            Some(Block::Plain(plain)) => &plain.content,
            other => panic!("expected a Plain block inside the Figure, got {other:?}"),
        },
        other => panic!("expected a Paragraph or Figure block, got {other:?}"),
    };
    content
        .iter()
        .find_map(|inline| match inline {
            Inline::Image(image) => Some(image),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected an Image inline, got {content:?}"))
}

#[test]
fn link_title_span_excludes_the_surrounding_quotes() {
    let input = r#"[text](https://example.com "Example Title")"#;
    let pandoc = parse_qmd(input);
    let link = first_link(&pandoc);

    let title_source = link
        .target_source
        .title
        .as_ref()
        .expect("a non-empty title must have a source");

    let title_start = input.find("Example Title").unwrap();
    let title_end = title_start + "Example Title".len();
    assert_eq!(
        *title_source,
        SourceInfo::original(FileId(0), title_start, title_end),
        "the title span must exclude both surrounding quotes"
    );
}

#[test]
fn link_title_with_escape_maps_piecewise() {
    // `a\*b` decodes to `a*b`: the `\*` pair is two source bytes collapsing
    // to one content byte, so no affine map exists and the wiring under
    // test must produce a piecewise `Concat` rather than the raw,
    // quote-inclusive node span (which the D1 revert handed back instead).
    let input = r#"[text](https://example.com "a\*b")"#;
    let pandoc = parse_qmd(input);
    let link = first_link(&pandoc);

    assert_eq!(
        link.target,
        ("https://example.com".to_string(), "a*b".to_string())
    );

    let title_source = link
        .target_source
        .title
        .as_ref()
        .expect("a non-empty title must have a source");

    let base = input.find(r"a\*b").unwrap();
    let SourceInfo::Concat { pieces } = title_source else {
        panic!("expected a Concat for a collapsed escape, got {title_source:?}");
    };
    assert_eq!(pieces.len(), 3);
    assert_eq!(
        pieces[0].source_info,
        SourceInfo::original(FileId(0), base, base + 1)
    );
    assert_eq!(pieces[0].length, 1);
    // Two source bytes (`\*`), one content byte (`*`).
    assert_eq!(
        pieces[1].source_info,
        SourceInfo::original(FileId(0), base + 1, base + 3)
    );
    assert_eq!(pieces[1].length, 1);
    assert_eq!(
        pieces[2].source_info,
        SourceInfo::original(FileId(0), base + 3, base + 4)
    );
    assert_eq!(pieces[2].length, 1);
}

#[test]
fn image_title_span_excludes_the_surrounding_quotes() {
    // The image path shares `process_target` with the link path, but it is
    // a distinct call site (`process_pandoc_image`) — checked directly
    // rather than assumed to inherit the link test's coverage.
    let input = r#"![alt](./pic.png "A Caption")"#;
    let pandoc = parse_qmd(input);
    let image = first_image(&pandoc);

    let title_source = image
        .target_source
        .title
        .as_ref()
        .expect("a non-empty title must have a source");

    let title_start = input.find("A Caption").unwrap();
    let title_end = title_start + "A Caption".len();
    assert_eq!(
        *title_source,
        SourceInfo::original(FileId(0), title_start, title_end),
        "the title span must exclude both surrounding quotes"
    );
}
