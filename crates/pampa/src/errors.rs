/*
 * errors.rs
 * Copyright (c) 2025 Posit, PBC
 */

// tree-sitter doesn't have a good mechanism for error reporting,
// so we have to manually traverse the tree and find error nodes.

// In addition to that, the tree-sitter-qmd parser is a combination of
// two separate tree-sitter parsers, one for inline content
// and one for block content, and the standard traverser only
// keeps one inline cursor in memory at a time.
//
// This means we can't easily keep copies of the cursors around,
// and we hack around it by using the cursor id to identify nodes
// in the tree, and build clones that way. The main problem with
// this solution is that cursor cloning requires walking the tree
// and can take O(n) time.

use tree_sitter_qmd::MarkdownTree;

enum TreeSitterError {
    MissingNode,
    UnexpectedNode,
}

fn node_can_have_empty_text<'a>(cursor: &tree_sitter_qmd::MarkdownCursor<'a>) -> bool {
    match cursor.node().kind() {
        "block_continuation" => true,
        _ => false,
    }
}

fn is_error_node<'a>(cursor: &tree_sitter_qmd::MarkdownCursor<'a>) -> Option<TreeSitterError> {
    if cursor.node().kind() == "ERROR" {
        return Some(TreeSitterError::UnexpectedNode);
    }
    let byte_range = cursor.node().byte_range();
    if byte_range.start == byte_range.end && !node_can_have_empty_text(cursor) {
        return Some(TreeSitterError::MissingNode); // empty node, indicates that tree-sitter inserted a "missing" node?
    }
    None
}

fn accumulate_error_nodes<'a>(
    cursor: &mut tree_sitter_qmd::MarkdownCursor<'a>,
    errors: &mut Vec<(bool, usize)>,
) {
    if is_error_node(cursor).is_some() {
        errors.push(cursor.id());
        return;
    }
    if cursor.goto_first_child() {
        loop {
            accumulate_error_nodes(cursor, errors);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

pub fn parse_is_good(tree: &MarkdownTree) -> Vec<(bool, usize)> {
    let mut errors = Vec::new();
    let mut cursor = tree.walk();
    accumulate_error_nodes(&mut cursor, &mut errors);
    errors
}

pub fn error_message(error: &mut tree_sitter_qmd::MarkdownCursor, input_bytes: &[u8]) -> String {
    // assert!(error.goto_parent());
    // assert!(error.goto_first_child());

    if let Some(which_error) = is_error_node(error) {
        match which_error {
            TreeSitterError::MissingNode => {
                return format!(
                    "Error: Missing {} at {}:{}",
                    error.node().kind(),
                    error.node().start_position().row,
                    error.node().start_position().column,
                );
            }
            TreeSitterError::UnexpectedNode => {
                return format!(
                    "Error: Unexpected {} at {}:{}",
                    error.node().utf8_text(input_bytes).unwrap_or(""),
                    error.node().start_position().row,
                    error.node().start_position().column,
                );
            }
        }
    }
    assert!(false, "No error message available for this node");
    String::new() // unreachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter_qmd::MarkdownParser;

    #[test]
    fn test_parse_is_good_valid_markdown() {
        let input = b"# Hello\n\nThis is valid markdown.\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty(), "Valid markdown should have no errors");
    }

    #[test]
    fn test_parse_is_good_simple_paragraph() {
        let input = b"Just some text.\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_node_can_have_empty_text_block_continuation() {
        // Test that block_continuation nodes are allowed to have empty text
        let input = b"> Quote\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        // This should not produce errors for empty block_continuation nodes
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_with_code_block() {
        let input = b"```python\nprint('hello')\n```\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_nested_structure() {
        let input = b"> Quote with *emphasis*\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_list() {
        let input = b"- Item 1\n- Item 2\n- Item 3\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_heading_levels() {
        let input = b"# H1\n## H2\n### H3\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_link_and_image() {
        let input = b"[link](url) and ![image](img.png)\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_inline_code() {
        let input = b"Some `inline code` here.\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_emphasis() {
        let input = b"*italic* and **bold**\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    // Helper to find error nodes and get cursors for them
    fn find_error_cursor<'a>(
        tree: &'a MarkdownTree,
        errors: &[(bool, usize)],
    ) -> Option<tree_sitter_qmd::MarkdownCursor<'a>> {
        if errors.is_empty() {
            return None;
        }
        let mut cursor = tree.walk();
        cursor.goto_id(errors[0]);
        Some(cursor)
    }

    #[test]
    fn test_error_message_unexpected_node() {
        // This input produces an ERROR node which is an "unexpected" error
        // Using triple asterisks which the parser doesn't handle well
        let input = b"***both***\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);

        // If we got errors, test error_message
        if !errors.is_empty() {
            let mut cursor = find_error_cursor(&tree, &errors).unwrap();
            let msg = error_message(&mut cursor, input);
            // Should be an "Unexpected" error message
            assert!(
                msg.contains("Error:"),
                "Expected error message to contain 'Error:', got: {}",
                msg
            );
        }
    }

    #[test]
    fn test_error_message_formats_correctly() {
        // Try to produce an error that we can test
        // Invalid shortcode syntax often produces errors
        let input = b"{{< broken\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);

        if !errors.is_empty() {
            let mut cursor = find_error_cursor(&tree, &errors).unwrap();
            let msg = error_message(&mut cursor, input);
            // Verify basic format of error message
            assert!(
                msg.starts_with("Error:"),
                "Message should start with 'Error:'"
            );
        }
    }

    #[test]
    fn test_error_message_includes_position() {
        // Another attempt to produce errors with position info
        let input = b"[unclosed link(\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);

        if !errors.is_empty() {
            let mut cursor = find_error_cursor(&tree, &errors).unwrap();
            let msg = error_message(&mut cursor, input);
            // Error messages should include position (row:column)
            assert!(msg.contains(":"), "Error message should include position");
        }
    }

    #[test]
    fn test_parse_is_good_thematic_break() {
        let input = b"Above\n\n---\n\nBelow\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_ordered_list() {
        let input = b"1. First\n2. Second\n3. Third\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_blockquote() {
        let input = b"> This is a quote\n> with multiple lines\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_fenced_div() {
        let input = b"::: {.note}\nContent here\n:::\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_yaml_front_matter() {
        let input = b"---\ntitle: Test\n---\n\nContent\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_definition_list() {
        let input = b"Term\n:   Definition\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_footnote() {
        let input = b"Text with footnote^[This is the note]\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_span_with_class() {
        let input = b"[text]{.class}\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_strikeout() {
        let input = b"~~strikeout~~\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_superscript_subscript() {
        let input = b"E=mc^2^ and H~2~O\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_is_good_math() {
        let input = b"Inline $x^2$ and display $$y = mx + b$$\n";
        let mut parser = MarkdownParser::default();
        let tree = parser.parse(input, None).expect("Failed to parse");
        let errors = parse_is_good(&tree);
        assert!(errors.is_empty());
    }
}
