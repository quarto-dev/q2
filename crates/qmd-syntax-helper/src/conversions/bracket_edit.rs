// Source-editing helpers shared by the `reference-links` and
// `literal-brackets` rules (bd-reference-links-unsupported-ddc4skac).
//
// Both rules work from byte offsets produced by the AST rather than from
// line numbers, because a bracketed group can straddle a soft line break
// (`[multi\nline]` is one span). Edits are therefore expressed as byte
// ranges and applied back-to-front, the same technique `apostrophe_quotes.rs`
// uses.

use crate::rule::SourceLocation;

/// A replacement of one byte range in the source.
#[derive(Debug, Clone)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

impl Edit {
    pub fn replace(start: usize, end: usize, replacement: String) -> Self {
        Self {
            start,
            end,
            replacement,
        }
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            replacement: String::new(),
        }
    }
}

/// Apply edits to `source`, back-to-front so earlier offsets stay valid.
///
/// Overlapping edits would corrupt the output, so any edit that overlaps one
/// already applied is skipped. The rules are built not to produce overlaps —
/// a definition's own brackets are consumed during analysis and never become
/// a separate finding — so this is a guard, not a routine path.
pub fn apply_edits(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));

    let mut result = source.to_string();
    let mut last_start = usize::MAX;

    for edit in edits {
        if edit.end > last_start {
            continue;
        }
        if edit.start > edit.end || edit.end > result.len() {
            continue;
        }
        if !result.is_char_boundary(edit.start) || !result.is_char_boundary(edit.end) {
            continue;
        }
        result.replace_range(edit.start..edit.end, &edit.replacement);
        last_start = edit.start;
    }

    result
}

/// Normalize whitespace left behind by deleted definition lines.
///
/// Removing a definition paragraph leaves the blank line that preceded it,
/// which would otherwise show up as a spurious diff hunk. Runs of three or
/// more newlines collapse to a paragraph break, and the file ends with
/// exactly one newline.
pub fn tidy_blank_lines(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut newline_run = 0;

    for ch in source.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                result.push(ch);
            }
        } else {
            newline_run = 0;
            result.push(ch);
        }
    }

    if result.ends_with('\n') {
        while result.ends_with("\n\n") {
            result.pop();
        }
    }

    result
}

/// Convert a byte offset to a zero-indexed row and column.
pub fn source_location(source: &str, offset: usize) -> SourceLocation {
    let offset = offset.min(source.len());
    let prefix = &source[..offset];
    SourceLocation {
        row: prefix.matches('\n').count(),
        column: offset - prefix.rfind('\n').map_or(0, |pos| pos + 1),
    }
}
