# Table Caption Postprocessing Plan
**Date:** 2025-10-27
**Approach:** Pure Rust postprocessing to fix malformed caption parsing

## Problem Statement

When a table caption is written without a blank line before it:
```markdown
| Name  | Age |
|-------|-----|
| Alice | 30  |
| Bob   | 25  |
: Caption without blank line
```

The tree-sitter parser treats the caption line as a `pipe_table_row` with a single cell, resulting in:
- An empty caption in the Table AST node
- An extra row at the end of the table body containing `: Caption without blank line`

However, Pandoc treats both cases (with and without blank line) identically, placing the caption text in the Table's caption field.

## Tree-sitter Parse Results

### With blank line (CORRECT):
```
(pipe_table [0, 0] - [4, 0]
  (pipe_table_header ...)
  (pipe_table_delimiter_row ...)
  (pipe_table_row ...) ; Alice
  (pipe_table_row ...)) ; Bob
(caption [4, 0] - [6, 0]
  (inline [5, 2] - [5, 25]))
```

### Without blank line (NEEDS FIXING):
```
(pipe_table [0, 0] - [5, 0]
  (pipe_table_header ...)
  (pipe_table_delimiter_row ...)
  (pipe_table_row ...) ; Alice
  (pipe_table_row ...) ; Bob
  (pipe_table_row [4, 0] - [4, 28] ; Caption as malformed row!
    (pipe_table_cell [4, 0] - [4, 28]
      (inline [4, 0] - [4, 28]))))
```

## Current vs Expected Output

### Current (INCORRECT):
```
Table (...) (Caption Nothing []) [...] (TableHead ...)
  [TableBody (...) []
    [Row [...] [Alice...],
     Row [...] [Bob...],
     Row [...] [Cell [...] [Plain [Str ":", Space, Str "Caption", ...]]]]]
```

### Expected (matching Pandoc):
```
Table (...) (Caption Nothing [Plain [Str "Caption", Space, Str "without", ...]])
  [...] (TableHead ...)
  [TableBody (...) []
    [Row [...] [Alice...],
     Row [...] [Bob...]]]
```

Note: Pandoc strips the ":" prefix from the caption text.

## Postprocessing Strategy

Add a `with_table` filter to `postprocess.rs` that detects and fixes the malformed pattern.

### Detection Pattern

A table needs caption postprocessing if ALL of these conditions are true:

1. **Empty caption**: `table.caption.long.is_none() || table.caption.long == Some(vec![])`
2. **Has bodies**: `!table.bodies.is_empty()`
3. **Last body has rows**: `!table.bodies.last().unwrap().body.is_empty()`
4. **Last row has exactly one cell**: `last_row.cells.len() == 1`
5. **Cell has exactly one block**: `cell.content.len() == 1`
6. **That block is Plain**: `matches!(cell.content[0], Block::Plain(_))`
7. **Plain starts with ":" Str**: First inline is `Str` and `text.starts_with(':')`

### Transformation Steps

If pattern matches:

1. **Extract**:
   ```rust
   let last_body = table.bodies.last_mut().unwrap();
   let caption_row = last_body.body.pop().unwrap();
   let caption_cell = &caption_row.cells[0];
   let Block::Plain(plain) = &caption_cell.content[0] else { unreachable!() };
   let mut caption_inlines = plain.content.clone();
   ```

2. **Strip leading colon**:
   ```rust
   if let Some(Inline::Str(first_str)) = caption_inlines.first_mut() {
       // Remove leading ":" and any following whitespace
       first_str.text = first_str.text
           .strip_prefix(':')
           .unwrap_or(&first_str.text)
           .trim_start()
           .to_string();

       // If the string is now empty, remove it entirely
       if first_str.text.is_empty() {
           caption_inlines.remove(0);
           // Also remove following Space if present
           if matches!(caption_inlines.first(), Some(Inline::Space(_))) {
               caption_inlines.remove(0);
           }
       }
   }
   ```

3. **Create caption**:
   ```rust
   table.caption = Caption {
       short: None,
       long: Some(vec![Block::Plain(Plain {
           content: caption_inlines,
           source_info: caption_row.source_info.clone(),
       })]),
       source_info: caption_row.source_info.clone(),
   };
   ```

4. **Return transformed table**:
   ```rust
   FilterResult(vec![Block::Table(table)], false)
   ```

### Edge Cases

1. **Caption is just ":"** - Results in empty caption (empty inlines)
2. **Multiple spaces after colon** - `trim_start()` handles this
3. **Cell with multiple Plain blocks** - Skip transformation (not a caption pattern)
4. **Multiple bodies** - Only check last body (where captions would appear)
5. **Source location tracking** - Use `caption_row.source_info` for caption location

## Implementation Plan

1. **Add the filter** to `postprocess.rs`:
   - Add `.with_table(|table| { ... })` to the filter chain
   - Implement detection logic
   - Implement transformation logic
   - Return `FilterResult` if transformed, `Unchanged` if not

2. **Test with existing snapshots**:
   - Run `cargo test` to ensure existing tests pass
   - Run the two new test files to verify they produce correct output
   - Compare with Pandoc output using `pandoc -t native`

3. **Update snapshots**:
   - Run `cargo insta test --review` to accept new snapshots
   - Verify both test cases produce identical output

## Testing Strategy

### Test files already created:
- `tests/snapshots/native/table-caption-no-blank-line.qmd`
- `tests/snapshots/native/table-caption-with-blank-line.qmd`

### Expected behavior:
Both files should produce IDENTICAL Pandoc AST output, matching Pandoc's behavior.

### Verification commands:
```bash
# Our output
cargo run -- -i tests/snapshots/native/table-caption-no-blank-line.qmd -t native
cargo run -- -i tests/snapshots/native/table-caption-with-blank-line.qmd -t native

# Pandoc output (for comparison)
pandoc -f markdown -t native < tests/snapshots/native/table-caption-no-blank-line.qmd
pandoc -f markdown -t native < tests/snapshots/native/table-caption-with-blank-line.qmd
```

Both pairs should produce identical output, with the caption in the Caption field and only 2 rows in the table body.

## Code Location

All changes will be in:
- `/Users/cscheid/repos/github/cscheid/kyoto/crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs`

The new `with_table` filter should be added to the filter chain in the `postprocess()` function, after the existing filters but before the final `topdown_traverse()` call.

## Success Criteria

1. ✅ Both test files produce identical output
2. ✅ Output matches Pandoc's behavior exactly
3. ✅ Caption text has ":" stripped
4. ✅ Caption is in the Table's caption field, not as a row
5. ✅ All existing tests continue to pass
6. ✅ Source location information is preserved correctly
