# Dead Code Report: treesitter_utils Module

**Date**: 2026-01-01
**Session**: Code Coverage Improvement
**Status**: COMPLETED - Dead code removed

## Summary

During code coverage investigation, discovered **8 dead code files** in `pampa/src/pandoc/treesitter_utils/`. All were declared as modules in `mod.rs` but never imported or used.

**Note**: Initial investigation incorrectly included `raw_attribute.rs` - that file IS used and was restored.

## Dead Code Files (Removed)

| File | Lines | Evidence |
|------|-------|----------|
| `code_span.rs` | 107 | Superseded by `code_span_helpers.rs` |
| `inline_link.rs` | 87 | Superseded by `span_link_helpers.rs` |
| `image.rs` | 75 | Superseded by `span_link_helpers.rs` |
| `indented_code_block.rs` | 58 | No imports found |
| `latex_span.rs` | 52 | No imports found |
| `quoted_span.rs` | 28 | Superseded by `quote_helpers.rs` |
| `raw_specifier.rs` | 22 | No imports found |
| `setext_heading.rs` | 24 | No imports found |

**Total Dead Lines Removed**: ~453 lines

## Investigation Method

For each file:
1. Ran `grep -r "<module>::" crates/pampa/src/` - returned no results
2. Verified module is declared in `treesitter_utils/mod.rs`
3. Checked for `*_helpers.rs` replacement files

## Active Helper Files (for context)

These files ARE used and provide the actual functionality:

| File | Coverage | Purpose |
|------|----------|---------|
| `code_span_helpers.rs` | 86.79% | Code span processing |
| `span_link_helpers.rs` | 95.00% | Links and images |
| `quote_helpers.rs` | 77.67% | Quoted text processing |
| `text_helpers.rs` | 76.87% | Text node handling |

## Recommendation

Create a cleanup task to:
1. Remove the 8 dead code files listed above
2. Remove their declarations from `treesitter_utils/mod.rs`
3. Verify build still passes after removal

This will:
- Remove ~471 lines of unmaintained code
- Improve apparent coverage (eliminating artificial "uncovered" lines)
- Reduce cognitive load when navigating the codebase

## Beads Issues Created

- `k-js4l`: Remove dead code: code_span.rs

**Additional issue needed** for bulk removal of remaining 8 files.
