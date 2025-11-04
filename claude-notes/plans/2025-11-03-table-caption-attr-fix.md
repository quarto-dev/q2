# Fix table caption attribute handling

## Problem

Two issues in `tests/snapshots/json/table-caption-attr.qmd`:

1. **Missing attribute on table**: The `tbl-colwidths="[30,70]"` attribute from the caption is not being applied to the Table element
2. **Stray Space in caption**: A trailing space appears before the attribute in the caption text

## Investigation

From verbose tree-sitter output:
```
caption: {Node caption (4, 0) - (5, 0)}
  :: {Node : (4, 0) - (4, 1)}
  pandoc_str: "Table"
  pandoc_space:
  pandoc_str: "caption"
  pandoc_space: <--- TRAILING SPACE BEFORE ATTRIBUTE
  attribute_specifier: {Node attribute_specifier (4, 16) - (4, 41)}
    commonmark_specifier: {tbl-colwidths="[30,70]"}
```

The grammar correctly parses the attribute, but:
- `process_caption` (caption.rs:16-41) ignores `attribute_specifier` nodes
- `process_pipe_table` (pipe_table.rs:148) uses hardcoded `empty_attr()`
- `CaptionBlock` struct has no fields for attributes

## Solution

### 1. Study trim_inlines function
Understand how to remove trailing spaces from inline content

### 2. Extend CaptionBlock struct
Add `attr` and `attr_source` fields to `CaptionBlock` in `src/pandoc/block.rs`

### 3. Update process_caption
In `src/pandoc/treesitter_utils/caption.rs`:
- Extract `attribute_specifier` nodes
- Trim trailing space before attribute from inline content
- Store attribute in CaptionBlock

### 4. Update process_pipe_table
In `src/pandoc/treesitter_utils/pipe_table.rs`:
- Extract attribute from CaptionBlock
- Apply to Table instead of using `empty_attr()`

### 5. Update construction sites
Find and update all places that create CaptionBlock:
- JSON reader
- Other construction sites

### 6. Update reader sites
Find and update all places that read from CaptionBlock:
- JSON writer
- Native writer
- QMD writer
- Other readers

### 7. Test
Run `cargo check` and `cargo test` to ensure everything compiles and tests pass

### 8. Verify
Confirm `tests/snapshots/json/table-caption-attr.qmd` test passes

## Files to modify

- `crates/quarto-markdown-pandoc/src/pandoc/block.rs` - CaptionBlock struct
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/caption.rs` - process_caption
- `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/pipe_table.rs` - process_pipe_table
- `crates/quarto-markdown-pandoc/src/readers/json.rs` - JSON reader
- `crates/quarto-markdown-pandoc/src/writers/json.rs` - JSON writer
- `crates/quarto-markdown-pandoc/src/writers/native.rs` - Native writer
- `crates/quarto-markdown-pandoc/src/writers/qmd.rs` - QMD writer
- Other files that use CaptionBlock
