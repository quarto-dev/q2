/*
 * test_treesitter_coverage.rs
 *
 * Tests specifically designed to improve coverage in treesitter.rs.
 * These tests exercise code paths for different block types in lists,
 * edge cases in inline processing, and error handling paths.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::pandoc::{Block, Inline};
use pampa::readers;

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

// ============================================================================
// List tests - exercise get_block_source_info with different block types
// ============================================================================

#[test]
fn test_list_with_code_block_item() {
    // This creates a list item ending with a code block, exercising Block::CodeBlock path
    // in get_block_source_info during loose list detection
    // Note: get_block_source_info is called on the LAST block of each item
    let input = r#"- First item

- Item ending with code:

  ```
  code block
  ```

- Third item"#;

    let pandoc = parse_qmd(input);

    // Verify we got a bullet list
    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Should have 3 items
        assert_eq!(list.content.len(), 3);
        // Second item should have a code block as the last block
        let second_item = &list.content[1];
        let last_block = second_item.last();
        assert!(
            matches!(last_block, Some(Block::CodeBlock(_))),
            "Expected code block as last block in second item, got {:?}",
            last_block
        );
    }
}

#[test]
fn test_block_quote_outside_list() {
    // Block quote as a standalone block, exercising Block::BlockQuote path
    // Note: Block quotes inside list items have parsing issues in QMD
    let input = r#"> This is a block quote
> spanning multiple lines"#;

    let pandoc = parse_qmd(input);

    assert!(
        matches!(&pandoc.blocks[0], Block::BlockQuote(_)),
        "Expected BlockQuote, got {:?}",
        &pandoc.blocks[0]
    );
}

#[test]
fn test_list_with_nested_list() {
    // This creates a list item with a nested bullet list, exercising Block::BulletList path
    let input = r#"- First item

- Item with nested list:

  - Nested item 1
  - Nested item 2

- Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        assert_eq!(list.content.len(), 3);
        // Second item should have a nested bullet list
        let second_item = &list.content[1];
        assert!(
            second_item
                .iter()
                .any(|b| matches!(b, Block::BulletList(_))),
            "Expected nested bullet list in second item"
        );
    }
}

#[test]
fn test_list_with_nested_ordered_list() {
    // This creates a list item with a nested ordered list, exercising Block::OrderedList path
    let input = r#"- First item

- Item with nested ordered list:

  1. Nested item 1
  2. Nested item 2

- Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        assert_eq!(list.content.len(), 3);
        // Second item should have a nested ordered list
        let second_item = &list.content[1];
        assert!(
            second_item
                .iter()
                .any(|b| matches!(b, Block::OrderedList(_))),
            "Expected nested ordered list in second item"
        );
    }
}

#[test]
fn test_list_with_table_item() {
    // This creates a list item ending with a pipe table, exercising Block::Table path
    // Note: get_block_source_info is called on the LAST block of each item
    let input = r#"- First item

- Item ending with table:

  | Col1 | Col2 |
  |------|------|
  | A    | B    |

- Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Second item should have a table as the last block
        let second_item = &list.content[1];
        let last_block = second_item.last();
        assert!(
            matches!(last_block, Some(Block::Table(_))),
            "Expected table as last block in second item, got {:?}",
            last_block
        );
    }
}

#[test]
fn test_list_with_div_item() {
    // This creates a list item ending with a fenced div, exercising Block::Div path
    // Note: get_block_source_info is called on the LAST block of each item
    let input = r#"- First item

- Item ending with div:

  ::: {.note}
  This is a note
  :::

- Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Second item should have a div as the last block
        let second_item = &list.content[1];
        let last_block = second_item.last();
        assert!(
            matches!(last_block, Some(Block::Div(_))),
            "Expected div as last block in second item, got {:?}",
            last_block
        );
    }
}

#[test]
fn test_list_with_horizontal_rule_item() {
    // This creates a list item ending with a horizontal rule, exercising Block::HorizontalRule path
    // Note: get_block_source_info is called on the LAST block of each item
    let input = r#"- First item

- Item ending with rule:

  Some text

  ---

- Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Second item should have a horizontal rule as the last block
        let second_item = &list.content[1];
        let last_block = second_item.last();
        assert!(
            matches!(last_block, Some(Block::HorizontalRule(_))),
            "Expected horizontal rule as last block in second item, got {:?}",
            last_block
        );
    }
}

// ============================================================================
// Tight list to Plain conversion test
// ============================================================================

#[test]
fn test_tight_list_converts_paragraphs_to_plain() {
    // Tight list should convert first paragraph to Plain block
    let input = r#"- Item 1
- Item 2
- Item 3"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Each item should have Plain (not Paragraph) as first block
        for item in &list.content {
            assert!(
                matches!(item.first(), Some(Block::Plain(_))),
                "Expected Plain block in tight list item, got {:?}",
                item.first()
            );
        }
    }
}

// ============================================================================
// Loose list detection tests
// ============================================================================

#[test]
fn test_loose_list_multiple_paragraphs_in_item() {
    // A list with multiple paragraphs in one item should be loose
    let input = r#"- First paragraph in item

  Second paragraph in same item

- Another item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // First item should have multiple paragraphs (loose list keeps Paragraph)
        let first_item = &list.content[0];
        let para_count = first_item
            .iter()
            .filter(|b| matches!(b, Block::Paragraph(_)))
            .count();
        assert!(
            para_count >= 2,
            "Expected at least 2 paragraphs in loose list item, got {}",
            para_count
        );
    }
}

#[test]
fn test_loose_list_blank_line_between_items() {
    // A list with blank lines between items should be loose
    let input = r#"- Item 1

- Item 2

- Item 3"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Loose list items should have Paragraph (not Plain)
        for item in &list.content {
            assert!(
                matches!(item.first(), Some(Block::Paragraph(_))),
                "Expected Paragraph block in loose list item"
            );
        }
    }
}

// ============================================================================
// Tight list with nested sublist tests (CommonMark spec section 5.3)
// https://spec.commonmark.org/0.31.2/#lists
// A list is loose only if items are separated by blank lines or contain
// two block elements with a blank line between them. A nested sublist
// immediately following content (no blank line) does NOT make the list loose.
// ============================================================================

#[test]
fn test_tight_list_with_nested_sublist_beginning() {
    // * foo
    //   * bar
    // * baz
    // The outer list is tight: no blank lines between items.
    // "foo" and "baz" should be Plain, not Para.
    let input = "* foo\n  * bar\n* baz";

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        assert_eq!(list.content.len(), 2, "Expected 2 items in outer list");

        // Item 1: should be [Plain("foo"), BulletList([Plain("bar")])]
        let item1 = &list.content[0];
        assert!(
            matches!(item1.first(), Some(Block::Plain(_))),
            "First item should start with Plain (tight list), got {:?}",
            item1.first()
        );
        assert!(
            item1.iter().any(|b| matches!(b, Block::BulletList(_))),
            "First item should contain a nested BulletList"
        );

        // Item 2: should be [Plain("baz")]
        let item2 = &list.content[1];
        assert!(
            matches!(item2.first(), Some(Block::Plain(_))),
            "Second item should start with Plain (tight list), got {:?}",
            item2.first()
        );
    }
}

#[test]
fn test_tight_list_with_nested_sublist_middle() {
    // * a
    // * b
    //   * c
    // * d
    // The outer list is tight: no blank lines between items.
    // All outer items should be Plain.
    let input = "* a\n* b\n  * c\n* d";

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        assert_eq!(list.content.len(), 3, "Expected 3 items in outer list");

        // Item 1: [Plain("a")]
        assert!(
            matches!(list.content[0].first(), Some(Block::Plain(_))),
            "Item 'a' should be Plain, got {:?}",
            list.content[0].first()
        );

        // Item 2: [Plain("b"), BulletList([Plain("c")])]
        assert!(
            matches!(list.content[1].first(), Some(Block::Plain(_))),
            "Item 'b' should be Plain, got {:?}",
            list.content[1].first()
        );

        // Item 3: [Plain("d")]
        assert!(
            matches!(list.content[2].first(), Some(Block::Plain(_))),
            "Item 'd' should be Plain, got {:?}",
            list.content[2].first()
        );
    }
}

#[test]
fn test_loose_list_with_nested_sublist_has_blank_lines() {
    // * foo
    //
    //   * bar
    //
    // * baz
    // The outer list is loose: blank lines between items.
    // "foo" and "baz" should be Para.
    let input = "* foo\n\n  * bar\n\n* baz";

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        // Item 1: should be [Para("foo"), BulletList(...)]
        assert!(
            matches!(list.content[0].first(), Some(Block::Paragraph(_))),
            "First item should be Para in loose list, got {:?}",
            list.content[0].first()
        );

        // Item 2: should be [Para("baz")]
        assert!(
            matches!(list.content[1].first(), Some(Block::Paragraph(_))),
            "Second item should be Para in loose list, got {:?}",
            list.content[1].first()
        );
    }
}

// ============================================================================
// Ordered list tests
// ============================================================================

#[test]
fn test_ordered_list_with_dot_marker() {
    let input = r#"1. First item
2. Second item
3. Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::OrderedList(_)));
}

#[test]
fn test_ordered_list_with_parenthesis_marker() {
    let input = r#"1) First item
2) Second item
3) Third item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::OrderedList(_)));
}

#[test]
fn test_example_list() {
    // Example lists use (@) syntax
    let input = r#"(@) First example

(@) Second example

(@) Third example"#;

    let pandoc = parse_qmd(input);

    assert!(
        matches!(&pandoc.blocks[0], Block::OrderedList(_)),
        "Expected OrderedList, got {:?}",
        &pandoc.blocks[0]
    );

    if let Block::OrderedList(list) = &pandoc.blocks[0] {
        // Example lists should use TwoParens delimiter
        assert_eq!(list.attr.2, pampa::pandoc::list::ListNumberDelim::TwoParens);
    }
}

// ============================================================================
// Empty list item test
// ============================================================================

#[test]
fn test_tight_list_with_code_block_item_no_leading_para() {
    // Test tight list conversion when first block is not a paragraph
    let input = r#"-   ```
    code
    ```
- text"#;

    let pandoc = parse_qmd(input);

    // Should parse without panicking
    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));
}

// ============================================================================
// Note definition tests - exercise NoteDefinitionPara and NoteDefinitionFencedBlock
// ============================================================================

#[test]
fn test_note_definition_para() {
    // Inline note definition
    let input = r#"Here is a note reference[^1].

[^1]: This is the note content."#;

    let pandoc = parse_qmd(input);

    // Should have NoteDefinitionPara somewhere
    let has_note_def = pandoc
        .blocks
        .iter()
        .any(|b| matches!(b, Block::NoteDefinitionPara(_)));
    assert!(has_note_def, "Expected NoteDefinitionPara block");
}

#[test]
fn test_note_definition_fenced_block() {
    // Fenced note definition uses ::: ^ref syntax in QMD
    let input = r#"Here is a note reference[^note].

::: ^note
First paragraph of note.

Second paragraph of note.
:::"#;

    let pandoc = parse_qmd(input);

    // Should parse without panicking
    // The note definition should be present
    assert!(!pandoc.blocks.is_empty());
    // Should have NoteDefinitionFencedBlock somewhere
    let has_fenced_note = pandoc
        .blocks
        .iter()
        .any(|b| matches!(b, Block::NoteDefinitionFencedBlock(_)));
    assert!(has_fenced_note, "Expected NoteDefinitionFencedBlock block");
}

#[test]
fn test_note_definition_fenced_block_with_heading() {
    // Test note definition fenced block containing a heading
    // This exercises the IntermediateSection path in process_note_definition_fenced_block
    let input = r#"::: ^note
# Heading in note

Paragraph in note.
:::"#;

    let pandoc = parse_qmd(input);

    // Should have NoteDefinitionFencedBlock
    let note_def = pandoc
        .blocks
        .iter()
        .find(|b| matches!(b, Block::NoteDefinitionFencedBlock(_)));
    assert!(
        note_def.is_some(),
        "Expected NoteDefinitionFencedBlock block"
    );

    if let Some(Block::NoteDefinitionFencedBlock(note)) = note_def {
        // The note should contain a Header and a Paragraph
        assert!(
            note.content.iter().any(|b| matches!(b, Block::Header(_))),
            "Expected Header in note content"
        );
        assert!(
            note.content
                .iter()
                .any(|b| matches!(b, Block::Paragraph(_))),
            "Expected Paragraph in note content"
        );
    }
}

// ============================================================================
// Inline note test
// ============================================================================

#[test]
fn test_inline_note() {
    let input = r#"Here is an inline note^[This is the note content]."#;

    let pandoc = parse_qmd(input);

    // Should have Note inline somewhere in the paragraph
    if let Block::Paragraph(para) = &pandoc.blocks[0] {
        let has_note = para.content.iter().any(|i| matches!(i, Inline::Note(_)));
        assert!(has_note, "Expected Note inline in paragraph");
    }
}

// ============================================================================
// Numeric character reference tests
// ============================================================================

#[test]
fn test_numeric_character_reference_decimal() {
    // Test decimal numeric character reference: &#64; -> @
    let input = r#"Test &#64; decimal"#;

    let pandoc = parse_qmd(input);

    if let Block::Paragraph(para) = &pandoc.blocks[0] {
        // Should have converted &#64; to @
        let has_at = para.content.iter().any(|i| {
            if let Inline::Str(s) = i {
                s.text == "@"
            } else {
                false
            }
        });
        assert!(has_at, "Expected @ character from decimal reference");
    }
}

#[test]
fn test_numeric_character_reference_uppercase_hex() {
    // Test uppercase hex numeric character reference: &#X0040; -> @
    let input = r#"Test &#X0040; uppercase"#;

    let pandoc = parse_qmd(input);

    if let Block::Paragraph(para) = &pandoc.blocks[0] {
        // Should have converted &#X0040; to @
        let has_at = para.content.iter().any(|i| {
            if let Inline::Str(s) = i {
                s.text == "@"
            } else {
                false
            }
        });
        assert!(has_at, "Expected @ character from uppercase hex reference");
    }
}

// ============================================================================
// Multiple paragraph detection path test
// ============================================================================

#[test]
fn test_list_item_multiple_blocks_with_paragraph() {
    // Test the path: blocks.len() > 1 && blocks.iter().any(Block::Paragraph)
    let input = r#"- Para one

  Para two

  ```
  code
  ```

- Another item"#;

    let pandoc = parse_qmd(input);

    assert!(matches!(&pandoc.blocks[0], Block::BulletList(_)));

    if let Block::BulletList(list) = &pandoc.blocks[0] {
        let first_item = &list.content[0];
        // Should have multiple blocks including paragraphs
        assert!(
            first_item.len() > 1,
            "Expected multiple blocks in list item"
        );
    }
}
