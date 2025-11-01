# Pipe Table Caption Implementation Plan
**Date**: 2025-10-31
**Issue**: k-304
**Status**: Ready to implement

## Summary

Implement caption support for pipe tables. The grammar has a quirk: captions can appear in two ways:
1. **Immediate**: As a field inside the pipe_table node (no empty line)
2. **Separated**: As a standalone block after the pipe_table (with empty line)

Both need to produce the same Pandoc output with caption attached to the table.

## Grammar Analysis

### Caption Definition (grammar.js:698-704)
```javascript
caption: $ => prec.right(seq(
    ':',
    optional(seq(
        $._whitespace,
        $._inlines
    )),
)),
```

### Two Locations

**1. Caption as field in pipe_table** (grammar.js:146)
```javascript
pipe_table: $ => prec.right(seq(
    ...
    optional($.caption),  // <-- HERE
    choice($._newline, $._eof),
)),
```

**2. Caption as standalone block** (grammar.js:32)
```javascript
_block_not_section: $ => prec.right(choice(
    ...
    $.caption, // <-- HERE: supports caption with empty line after table
    ...
))
```

### Parse Tree Examples

**Immediate caption** (no empty line):
```
| Col1 | Col2 |
|------|------|
| A    | B    |
: Immediate caption
```
Tree:
```
section:
  pipe_table:
    pipe_table_header: ...
    pipe_table_delimiter_row: ...
    pipe_table_row: ...
    caption:          <--- INSIDE pipe_table
      :
      pandoc_str: "Immediate"
      pandoc_space
      pandoc_str: "caption"
```

**Separated caption** (with empty line):
```
| Col1 | Col2 |
|------|------|
| A    | B    |

: Separated caption
```
Tree:
```
section:
  pipe_table:         <--- Caption NOT inside
    pipe_table_header: ...
    pipe_table_delimiter_row: ...
    pipe_table_row: ...
  caption:            <--- SIBLING to pipe_table
    :
    pandoc_str: "Separated"
    pandoc_space
    pandoc_str: "caption"
```

## Current Code State

### Helper Exists: `process_caption()` (treesitter_utils/caption.rs:16-39)
```rust
pub fn process_caption(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut caption_inlines: Inlines = Vec::new();

    for (node_name, child) in children {
        if node_name == "inline" {  // <-- OLD GRAMMAR STRUCTURE
            match child {
                PandocNativeIntermediate::IntermediateInlines(inlines) => {
                    caption_inlines.extend(inlines);
                }
                _ => panic!("Expected Inlines in caption, got {:?}", child),
            }
        }
        // Skip other nodes like ":", blank_line, etc.
    }

    PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(CaptionBlock {
        content: caption_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

**Problem**: Expects `"inline"` wrapper nodes from OLD grammar. New grammar produces direct inline nodes (`pandoc_str`, `pandoc_space`, etc.).

### Handler Commented Out (treesitter.rs:1123)
```rust
// "caption" => process_caption(node, children, context),
```

### Consumer Ready: `process_pipe_table()` (pipe_table.rs:186-192)
```rust
} else if node == "caption" {
    match child {
        PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(caption_block)) => {
            caption_inlines = Some(caption_block.content);
        }
        _ => panic!("Expected CaptionBlock in caption, got {:?}", child),
    }
}
```
This handles **immediate captions** (inside pipe_table node).

### Section Processing (section.rs:14-44)
```rust
pub fn process_section(
    _section_node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(node, child)| {
        // ... collects blocks into Vec<Block>
    });
    PandocNativeIntermediate::IntermediateSection(blocks)
}
```
This is where we need to handle **separated captions** (siblings to table).

## Implementation Plan

### Phase 1: Update process_caption() for New Grammar

**File**: `src/pandoc/treesitter_utils/caption.rs`

**Change**: Handle direct inline nodes instead of wrapped `"inline"` nodes
```rust
pub fn process_caption(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut caption_inlines: Inlines = Vec::new();

    for (_node_name, child) in children {
        match child {
            PandocNativeIntermediate::IntermediateInline(inline) => {
                caption_inlines.push(inline);
            }
            PandocNativeIntermediate::IntermediateInlines(inlines) => {
                caption_inlines.extend(inlines);
            }
            _ => {
                // Skip other nodes (colon marker, whitespace markers, etc.)
            }
        }
    }

    PandocNativeIntermediate::IntermediateBlock(Block::CaptionBlock(CaptionBlock {
        content: caption_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

**Rationale**: Same pattern as fixed in `process_pipe_table_cell()` during k-303.

### Phase 2: Uncomment Caption Handler

**File**: `src/pandoc/treesitter.rs:1123`

**Change**: Uncomment the line:
```rust
"caption" => process_caption(node, children, context),
```

### Phase 3: Add Caption Post-Processing to process_section()

**File**: `src/pandoc/treesitter_utils/section.rs`

**Change**: After collecting all blocks, scan for CaptionBlock following Table and attach caption to previous table.

```rust
pub fn process_section(
    _section_node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut blocks: Vec<Block> = Vec::new();
    children.into_iter().for_each(|(node, child)| {
        // ... existing collection logic ...
    });

    // POST-PROCESS: Attach standalone captions to previous tables
    let mut i = 0;
    while i < blocks.len() {
        if i > 0 {
            // Check if current block is a CaptionBlock
            if let Block::CaptionBlock(caption_block) = &blocks[i] {
                // Check if previous block is a Table
                if let Block::Table(ref mut table) = blocks[i - 1] {
                    // Extract caption inlines
                    let caption_inlines = caption_block.content.clone();

                    // Attach caption to table
                    table.caption = Caption {
                        short_caption: None,
                        content: vec![Block::Plain(Plain {
                            content: caption_inlines,
                            source_info: caption_block.source_info.clone(),
                        })],
                        source_info: caption_block.source_info.clone(),
                    };

                    // Remove the standalone CaptionBlock
                    blocks.remove(i);
                    continue; // Don't increment i, check the same index again
                }
            }
        }
        i += 1;
    }

    PandocNativeIntermediate::IntermediateSection(blocks)
}
```

**Rationale**:
- Simple single-pass algorithm
- Handles the pattern: Table followed by CaptionBlock
- Removes standalone CaptionBlock after attaching to table
- Doesn't require traversing entire AST

### Phase 4: Write Comprehensive Tests

**File**: `tests/test_treesitter_refactoring.rs`

**Tests needed**:

1. **test_pipe_table_caption_immediate()** - Caption without empty line
2. **test_pipe_table_caption_separated()** - Caption with empty line (post-processing case)
3. **test_pipe_table_caption_multiline()** - Multi-line caption text
4. **test_pipe_table_caption_formatted()** - Caption with inline formatting (bold, code)
5. **test_pipe_table_caption_empty()** - Caption with just `:` and no text
6. **test_pipe_table_no_caption()** - Table without caption (regression test)
7. **test_standalone_caption_no_table()** - Caption without preceding table (should remain standalone)

**Assertion pattern**:
```rust
// Check table has caption
assert!(result.contains("Caption"), "Should contain Caption");
// Check caption content
assert!(result.contains("caption text"), "Should contain caption text");
// Check no standalone CaptionBlock remains (for separated case)
assert!(!result.contains("CaptionBlock"), "Should not contain standalone CaptionBlock");
```

## Edge Cases to Handle

1. **Caption without table before it**: Leave as standalone CaptionBlock (valid Pandoc)
2. **Empty caption** (just `:`): Should create empty caption
3. **Multiple captions**: Each should attach to its preceding table
4. **Caption between other blocks**: Only attach if previous block is Table
5. **Non-table followed by caption**: Leave caption standalone

## Success Criteria

- [ ] Immediate captions (inside pipe_table) work correctly
- [ ] Separated captions (sibling blocks) are post-processed and attached to table
- [ ] Multi-line captions work
- [ ] Captions with inline formatting work
- [ ] No CaptionBlock remains standalone when following a table
- [ ] All 175 existing tests still pass
- [ ] New caption tests pass (7 tests)
- [ ] No panics or MISSING NODE warnings for captions

## Estimate

- Phase 1: 30 minutes (update process_caption)
- Phase 2: 5 minutes (uncomment handler)
- Phase 3: 1 hour (post-processing logic + testing)
- Phase 4: 1.5 hours (comprehensive tests)

**Total**: ~3 hours

## Files to Modify

1. `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/caption.rs` - Update helper
2. `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs` - Uncomment handler
3. `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/section.rs` - Add post-processing
4. `tests/test_treesitter_refactoring.rs` - Add comprehensive tests
