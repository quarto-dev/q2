/*
 * code_fence_content.rs
 *
 * Functions for processing code fence content nodes in the tree-sitter AST.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::location::node_source_info_with_context;

use super::pandocnativeintermediate::PandocNativeIntermediate;

/// Tab stop used by the scanner's `advance()` (GFM §2.2).
const TAB_STOP: usize = 4;

pub fn process_code_fence_content(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let start = node.range().start_byte;
    let end = node.range().end_byte;

    // Build the body by removing the container prefixes (`block_continuation`
    // markers: list-item indentation, `> `) from the content's byte range.
    let mut content = String::new();
    let mut current_location = start;
    for (child_node, child) in children {
        if child_node == "block_continuation" {
            let PandocNativeIntermediate::IntermediateUnknown(child_range) = child else {
                panic!(
                    "Expected IntermediateUnknown in block_continuation, got {:?}",
                    child
                )
            };
            let slice_before_continuation =
                &input_bytes[current_location..child_range.start.offset];
            content.push_str(std::str::from_utf8(slice_before_continuation).unwrap());
            // A list item's continuation runs over blank lines: the scanner's
            // LIST_ITEM `match()` advances past a line ending and keeps
            // matching on the next line, so one token covers the blank
            // line(s) *and* the next line's indentation. Outside code those
            // line endings carry no meaning; inside a fence they are blank
            // body lines, so keep them (as written, `\n` or `\r\n`).
            let continuation = &input_bytes[child_range.start.offset..child_range.end.offset];
            push_line_endings(&mut content, continuation);
            current_location = child_range.end.offset;
        }
    }
    // Add the remaining content after the last block_continuation
    if current_location < end {
        let slice_after_continuation = &input_bytes[current_location..end];
        content.push_str(std::str::from_utf8(slice_after_continuation).unwrap());
    }

    // CommonMark §4.5: if the opening fence is indented N columns (past its
    // container's content column), up to N columns of indentation are
    // removed from each body line. The excess indentation lives inside the
    // opening delimiter token, after the container's continuation.
    let fence_indent = opening_fence_indent(node, input_bytes);
    if fence_indent > 0 {
        content = strip_fence_indent(&content, fence_indent);
    }

    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    PandocNativeIntermediate::IntermediateBaseText(content, range)
}

/// Append every line ending (`\n` or `\r\n`) found in `bytes`, dropping
/// everything else.
fn push_line_endings(content: &mut String, bytes: &[u8]) {
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            if i > 0 && bytes[i - 1] == b'\r' {
                content.push('\r');
            }
            content.push('\n');
        }
    }
}

/// Indentation width, in columns, of the opening fence of the code block
/// that owns `content_node`, measured from the start of the delimiter token
/// (i.e. past any container prefix).
fn opening_fence_indent(content_node: &tree_sitter::Node, input_bytes: &[u8]) -> usize {
    let Some(parent) = content_node.parent() else {
        return 0;
    };
    let mut cursor = parent.walk();
    let Some(delimiter) = parent
        .children(&mut cursor)
        .find(|c| c.kind() == "fenced_code_block_delimiter")
    else {
        return 0;
    };
    let bytes = &input_bytes[delimiter.start_byte()..delimiter.end_byte()];
    let mut column = 0;
    for &b in bytes {
        match b {
            b' ' => column += 1,
            b'\t' => column += TAB_STOP - column % TAB_STOP,
            _ => break,
        }
    }
    column
}

/// Remove up to `indent` columns of leading whitespace from each line of
/// `content`. A tab that straddles the limit is split: the columns past the
/// limit are kept as spaces.
fn strip_fence_indent(content: &str, indent: usize) -> String {
    let mut out = String::with_capacity(content.len());
    for line in content.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let mut column = 0;
        let mut i = 0;
        while i < bytes.len() && column < indent {
            match bytes[i] {
                b' ' => column += 1,
                b'\t' => {
                    let width = TAB_STOP - column % TAB_STOP;
                    if column + width > indent {
                        for _ in 0..(column + width - indent) {
                            out.push(' ');
                        }
                    }
                    column += width;
                }
                _ => break,
            }
            i += 1;
        }
        out.push_str(&line[i..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_up_to_indent_spaces() {
        assert_eq!(strip_fence_indent("  a\n    b\n c\nd", 2), "a\n  b\nc\nd");
    }

    #[test]
    fn strip_keeps_empty_and_whitespace_only_lines() {
        // A blank line stays blank; a whitespace-only line deeper than the
        // fence keeps its excess (CommonMark, not pandoc's blanking).
        assert_eq!(strip_fence_indent("  a\n\n     \n  b", 2), "a\n\n   \nb");
    }

    #[test]
    fn strip_preserves_crlf() {
        assert_eq!(strip_fence_indent("  a\r\n\r\n  b\r\n", 2), "a\r\n\r\nb\r\n");
    }

    #[test]
    fn strip_splits_a_straddling_tab() {
        // Tab at column 0 spans 4 columns; removing 2 leaves 2 spaces.
        assert_eq!(strip_fence_indent("\ta", 2), "  a");
        // Tab fully inside the limit is removed whole.
        assert_eq!(strip_fence_indent("\ta", 4), "a");
    }

    #[test]
    fn line_endings_keep_crlf_and_drop_everything_else() {
        let mut s = String::new();
        push_line_endings(&mut s, b"  \r\n\n   ");
        assert_eq!(s, "\r\n\n");
    }
}
