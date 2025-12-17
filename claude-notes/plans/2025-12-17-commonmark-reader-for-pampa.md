# CommonMark Reader for Pampa

**Issue**: k-n74s
**Date**: 2025-12-17
**Status**: Complete

## Key Findings from Investigation

**comrak version**: 0.49.0 (not 0.35)

**comrak Sourcepos semantics** (verified by test):
- Line and column are **1-based**
- Columns are **byte-based** (not character-based) - this simplifies conversion!
- End position is **inclusive** (points to last byte of content)
- Example: "hello" → start=(1,1), end=(1,5); "héllo" (6 bytes) → start=(1,1), end=(1,6)

**quarto-source-map SourceInfo semantics**:
- Uses byte offsets
- End offset is **exclusive** (one past the last byte)
- `length() = end_offset - start_offset`

**Conversion formula**:
- `start_offset = line_start + (start.column - 1)` (convert 1-based to 0-based)
- `end_offset = line_start + end.column` (convert inclusive to exclusive)

## Overview

Add a `--from commonmark` option to pampa that uses comrak for parsing CommonMark and the existing `comrak-to-pandoc` crate for AST conversion. This provides an alternative parser for pure CommonMark content, leveraging work done for property testing.

The main feature enhancement is adding **source location tracking** from comrak's `Sourcepos` to `quarto-source-map::SourceInfo`.

## Background

### Existing Infrastructure

1. **comrak-to-pandoc crate** (`crates/comrak-to-pandoc/`):
   - Converts comrak's arena-based AST to `quarto-pandoc-types` AST
   - Currently uses `empty_source_info()` everywhere (no source tracking)
   - Only supports CommonMark subset (panics on GFM extensions)
   - Has normalization functions for AST comparison

2. **pampa readers** (`crates/pampa/src/readers/`):
   - `qmd.rs`: Tree-sitter-based QMD reader with full source tracking
   - `json.rs`: JSON reader with source info pool deserialization
   - Both return `(Pandoc, ASTContext, ...)` tuples

3. **comrak's source tracking**:
   - Each `Ast` node has a `sourcepos: Sourcepos` field
   - `Sourcepos` has `start: LineColumn` and `end: LineColumn`
   - `LineColumn` has `line: usize` and `column: usize` (1-based)
   - **No byte offsets** - only line/column positions

4. **quarto-source-map**:
   - `SourceInfo` enum with `Original`, `Substring`, `Concat`, `FilterProvenance`
   - Stores **byte offsets** (`start_offset`, `end_offset`)
   - Row/column computed on-demand via `FileInformation`

### Key Challenge: Offset Conversion

Comrak provides 1-based (line, column) positions but not byte offsets. `SourceInfo::Original` requires byte offsets. To bridge this gap, we need to:

1. Convert (line, column) to byte offsets using the source text
2. Pass source text into the converter to enable offset computation

## Implementation Plan

### Phase 1: Source Location Infrastructure in comrak-to-pandoc

#### 1.1 Create SourceLocationContext

Add a new struct to hold information needed for offset conversion:

```rust
// In crates/comrak-to-pandoc/src/source_location.rs

use comrak::nodes::{LineColumn, Sourcepos};
use quarto_source_map::{FileId, SourceInfo};

/// Context for converting comrak Sourcepos to quarto-source-map SourceInfo
pub struct SourceLocationContext {
    /// Precomputed line start offsets (byte offset of each line start)
    line_offsets: Vec<usize>,
    /// File ID for the source file
    file_id: FileId,
}

impl SourceLocationContext {
    /// Create a new context from source text
    pub fn new(source: &str, file_id: FileId) -> Self {
        // Precompute line start offsets
        // Line 1 starts at offset 0
        let mut line_offsets = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_offsets.push(i + 1);
            }
        }
        Self { line_offsets, file_id }
    }

    /// Convert a comrak Sourcepos to a quarto-source-map SourceInfo
    ///
    /// Note: comrak's end position is inclusive, but SourceInfo's end_offset is exclusive.
    pub fn sourcepos_to_source_info(&self, sourcepos: &Sourcepos) -> SourceInfo {
        let start_offset = self.start_offset(sourcepos);
        let end_offset = self.end_offset(sourcepos);
        SourceInfo::original(self.file_id, start_offset, end_offset)
    }

    /// Get the start byte offset for a sourcepos
    pub fn start_offset(&self, sourcepos: &Sourcepos) -> usize {
        self.line_column_to_offset(&sourcepos.start)
    }

    /// Get the end byte offset for a sourcepos (exclusive)
    ///
    /// Note: comrak's end is inclusive, so we add 1 to make it exclusive
    pub fn end_offset(&self, sourcepos: &Sourcepos) -> usize {
        // comrak end is inclusive (points to last byte)
        // SourceInfo end is exclusive (one past last byte)
        self.line_column_to_offset(&sourcepos.end) + 1
    }

    /// Convert 1-based (line, column) to byte offset
    ///
    /// Since comrak columns are byte-based (verified by testing),
    /// we just need to find the line start and add the column offset.
    fn line_column_to_offset(&self, lc: &LineColumn) -> usize {
        // comrak uses 1-based line numbers
        let line_idx = lc.line.saturating_sub(1);
        let line_start = self.line_offsets.get(line_idx).copied().unwrap_or(0);
        // Column is also 1-based; convert to 0-based
        line_start + lc.column.saturating_sub(1)
    }

    /// Get the file ID
    pub fn file_id(&self) -> FileId {
        self.file_id
    }
}
```

#### 1.2 Update Conversion Functions

Modify `convert_document`, `convert_block`, `convert_inline` to accept `SourceLocationContext`:

```rust
// Old signature
pub fn convert_document<'a>(root: &'a Node<'a, RefCell<Ast>>) -> Pandoc

// New signature
pub fn convert_document<'a>(
    root: &'a Node<'a, RefCell<Ast>>,
    source_ctx: Option<&SourceLocationContext>,
) -> Pandoc
```

When `source_ctx` is `Some`, use `sourcepos_to_source_info()` instead of `empty_source_info()`.

For backward compatibility, keep the old function and add a new one:

```rust
/// Convert without source tracking (original behavior)
pub fn convert_document<'a>(root: &'a Node<'a, RefCell<Ast>>) -> Pandoc {
    convert_document_with_source(root, None)
}

/// Convert with optional source tracking
pub fn convert_document_with_source<'a>(
    root: &'a Node<'a, RefCell<Ast>>,
    source_ctx: Option<&SourceLocationContext>,
) -> Pandoc {
    // ... implementation
}
```

### Phase 2: Update Block Converter

In `crates/comrak-to-pandoc/src/block.rs`:

1. Add `source_ctx: Option<&SourceLocationContext>` parameter to all conversion functions
2. Extract `sourcepos` from each node's `Ast`
3. Use `source_ctx.sourcepos_to_source_info(&sourcepos)` or `empty_source_info()`

Example for `convert_block`:

```rust
fn convert_block<'a>(
    node: &'a Node<'a, RefCell<Ast>>,
    source_ctx: Option<&SourceLocationContext>,
) -> Blocks {
    let ast = node.data.borrow();
    let source_info = source_ctx
        .map(|ctx| ctx.sourcepos_to_source_info(&ast.sourcepos))
        .unwrap_or_else(empty_source_info);

    match &ast.value {
        NodeValue::Paragraph => {
            let inlines = convert_children_to_inlines(node, source_ctx);
            vec![Block::Paragraph(Paragraph {
                content: inlines,
                source_info,
            })]
        }
        // ... other cases
    }
}
```

### Phase 3: Update Inline Converter

In `crates/comrak-to-pandoc/src/inline.rs`:

Similar changes - add `source_ctx` parameter and use proper source info.

**Important**: For `NodeValue::Text`, we need to pass the text node's computed byte offset to the tokenizer (Phase 4).

### Phase 4: Update Text Tokenizer with Source Tracking

In `crates/comrak-to-pandoc/src/text.rs`:

The `tokenize_text` function must be updated to produce precise source locations for each `Str` and `Space` inline. Since comrak's `Text` node only gives us the overall source position, we need to track byte offsets as we tokenize.

#### 4.1 New Function Signature

```rust
use quarto_source_map::{FileId, SourceInfo};

/// Tokenize text with source tracking
///
/// - `text`: The text content to tokenize
/// - `base_offset`: Byte offset where this text starts in the source file
/// - `file_id`: File identifier for SourceInfo
pub fn tokenize_text_with_source(
    text: &str,
    base_offset: usize,
    file_id: FileId,
) -> Inlines
```

#### 4.2 Tracking Algorithm

As we iterate through the text:
1. Use `char_indices()` to get byte positions
2. Track start position of current word
3. Track start position of whitespace runs
4. Create `SourceInfo::original(file_id, start, end)` for each emitted inline

#### 4.3 Backward Compatibility

Keep the original `tokenize_text` function for existing tests and property testing:

```rust
/// Tokenize without source tracking (backward compatible)
pub fn tokenize_text(text: &str) -> Inlines {
    tokenize_text_with_source(text, 0, FileId(0))
        .into_iter()
        .map(|inline| {
            // Reset source_info to empty for backward compat
            match inline {
                Inline::Str(mut s) => {
                    s.source_info = empty_source_info();
                    Inline::Str(s)
                }
                Inline::Space(mut sp) => {
                    sp.source_info = empty_source_info();
                    Inline::Space(sp)
                }
                other => other,
            }
        })
        .collect()
}
```

Or alternatively, just call the new function with a flag to skip source tracking.

#### 4.4 Integration with inline.rs

In `convert_inline`, when handling `NodeValue::Text`:

```rust
NodeValue::Text(text) => {
    if let Some(ctx) = source_ctx {
        let base_offset = ctx.sourcepos_start_offset(&ast.sourcepos);
        tokenize_text_with_source(text, base_offset, ctx.file_id)
    } else {
        tokenize_text(text)
    }
}
```

### Phase 5: Add CommonMark Reader to Pampa

#### 5.1 Create New Reader Module

Create `crates/pampa/src/readers/commonmark.rs`:

```rust
//! CommonMark reader using comrak

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::Pandoc;
use comrak::{parse_document, Arena, Options};
use comrak_to_pandoc::{convert_document_with_source, SourceLocationContext};
use quarto_source_map::FileId;

pub fn read(
    input: &str,
    filename: &str,
) -> Result<(Pandoc, ASTContext), Vec<quarto_error_reporting::DiagnosticMessage>> {
    // Set up comrak with pure CommonMark options (no GFM)
    let arena = Arena::new();
    let options = Options::default();

    // Parse with comrak
    let root = parse_document(&arena, input, &options);

    // Set up source location context
    let mut context = ASTContext::with_filename(filename.to_string());
    context.source_context.add_file(filename.to_string(), Some(input.to_string()));
    let file_id = FileId(0);

    let source_ctx = SourceLocationContext::new(input.to_string(), file_id);

    // Convert to Pandoc AST with source tracking
    let pandoc = convert_document_with_source(root, Some(&source_ctx));

    Ok((pandoc, context))
}
```

#### 5.2 Update readers/mod.rs

```rust
pub mod commonmark;
pub mod json;
pub mod qmd;
pub mod qmd_error_message_table;
pub mod qmd_error_messages;
```

#### 5.3 Update main.rs

Add "commonmark" case to the `--from` match:

```rust
let (pandoc, context) = match args.from.as_str() {
    "markdown" | "qmd" => {
        // existing QMD reader code
    }
    "commonmark" => {
        match readers::commonmark::read(&input, input_filename) {
            Ok((pandoc, context)) => (pandoc, context),
            Err(diagnostics) => {
                // error handling
            }
        }
    }
    "json" => {
        // existing JSON reader code
    }
    _ => {
        eprintln!("Unknown input format: {}", args.from);
        std::process::exit(1);
    }
};
```

### Phase 6: Add Dependency

Update `crates/pampa/Cargo.toml`:

```toml
[dependencies]
comrak = { version = "0.49.0", default-features = false }
comrak-to-pandoc = { path = "../comrak-to-pandoc" }
```

### Phase 7: Testing

#### 7.1 Unit Tests for Source Location Conversion

In `crates/comrak-to-pandoc/src/source_location.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_column_to_offset() {
        let source = "line1\nline2\nline3\n";
        let ctx = SourceLocationContext::new(source.to_string(), FileId(0));

        // Line 1, column 1 = offset 0
        assert_eq!(ctx.line_column_to_offset(LineColumn { line: 1, column: 1 }), 0);
        // Line 1, column 3 = offset 2
        assert_eq!(ctx.line_column_to_offset(LineColumn { line: 1, column: 3 }), 2);
        // Line 2, column 1 = offset 6
        assert_eq!(ctx.line_column_to_offset(LineColumn { line: 2, column: 1 }), 6);
    }

    #[test]
    fn test_sourcepos_to_source_info() {
        let source = "hello world\n";
        let ctx = SourceLocationContext::new(source, FileId(0));

        // comrak's end is inclusive: column 11 points to 'd' in "world"
        // SourceInfo's end is exclusive: should be 11 (one past 'd')
        let sourcepos = Sourcepos {
            start: LineColumn { line: 1, column: 1 },
            end: LineColumn { line: 1, column: 11 },
        };

        let info = ctx.sourcepos_to_source_info(&sourcepos);
        assert_eq!(info.start_offset(), 0);  // 'h' at offset 0
        assert_eq!(info.end_offset(), 11);   // exclusive end, one past 'd'
    }
}
```

#### 7.2 Unit Tests for Text Tokenizer Source Tracking

In `crates/comrak-to-pandoc/src/text.rs`:

```rust
#[cfg(test)]
mod source_tracking_tests {
    use super::*;
    use quarto_source_map::FileId;

    #[test]
    fn test_tokenize_single_word_source() {
        let result = tokenize_text_with_source("hello", 10, FileId(0));
        assert_eq!(result.len(), 1);
        if let Inline::Str(s) = &result[0] {
            assert_eq!(s.source_info.start_offset(), 10);
            assert_eq!(s.source_info.end_offset(), 15); // 10 + 5
        } else {
            panic!("Expected Str");
        }
    }

    #[test]
    fn test_tokenize_two_words_source() {
        // "hello world" at offset 0
        let result = tokenize_text_with_source("hello world", 0, FileId(0));
        assert_eq!(result.len(), 3);

        // "hello" at 0..5
        if let Inline::Str(s) = &result[0] {
            assert_eq!(s.source_info.start_offset(), 0);
            assert_eq!(s.source_info.end_offset(), 5);
        }

        // Space at 5..6
        if let Inline::Space(sp) = &result[1] {
            assert_eq!(sp.source_info.start_offset(), 5);
            assert_eq!(sp.source_info.end_offset(), 6);
        }

        // "world" at 6..11
        if let Inline::Str(s) = &result[2] {
            assert_eq!(s.source_info.start_offset(), 6);
            assert_eq!(s.source_info.end_offset(), 11);
        }
    }

    #[test]
    fn test_tokenize_utf8_source() {
        // "héllo" - é is 2 bytes
        let result = tokenize_text_with_source("héllo", 0, FileId(0));
        assert_eq!(result.len(), 1);
        if let Inline::Str(s) = &result[0] {
            assert_eq!(s.source_info.start_offset(), 0);
            assert_eq!(s.source_info.end_offset(), 6); // h(1) + é(2) + llo(3) = 6 bytes
        }
    }
}
```

#### 7.3 Integration Tests for CommonMark Reader

Create `crates/pampa/tests/commonmark_reader_tests.rs`:

```rust
#[test]
fn test_commonmark_reader_basic() {
    let input = "# Hello\n\nWorld\n";
    let (pandoc, _context) = pampa::readers::commonmark::read(input, "test.md").unwrap();

    assert_eq!(pandoc.blocks.len(), 2);
    // Verify first block is Header
    assert!(matches!(pandoc.blocks[0], Block::Header(_)));
}

#[test]
fn test_commonmark_reader_source_locations() {
    let input = "hello\n";
    let (pandoc, _context) = pampa::readers::commonmark::read(input, "test.md").unwrap();

    // Verify source_info is populated, not empty
    if let Block::Paragraph(p) = &pandoc.blocks[0] {
        assert!(p.source_info.start_offset() == 0);
        assert!(p.source_info.end_offset() > 0);
    }
}
```

#### 7.4 CLI Test

```bash
# Test the new --from commonmark option
echo "# Hello" | cargo run -p pampa -- --from commonmark -t json
```

## Edge Cases and Considerations

### 1. Column Semantics (RESOLVED)

**Verified by testing**: Comrak's columns are **byte-based**, not character-based.
This simplifies the implementation - no UTF-8 character iteration needed.

The conversion is straightforward:
```rust
fn line_column_to_offset(&self, lc: &LineColumn) -> usize {
    let line_idx = lc.line.saturating_sub(1);
    let line_start = self.line_offsets.get(line_idx).copied().unwrap_or(0);
    line_start + lc.column.saturating_sub(1)
}
```

### 2. End Offset Semantics (RESOLVED)

- comrak's end position is **inclusive** (points to last byte)
- SourceInfo's end_offset is **exclusive** (one past last byte)
- Solution: `end_offset = line_column_to_offset(end) + 1`

### 3. Empty Documents

Handle documents with no blocks gracefully.

### 4. Nested Structures

Ensure source info propagates correctly through:
- Nested blockquotes
- Lists with multiple paragraphs
- Emphasized text within links

### 5. Text Node Tokenization

The current `tokenize_text` function in `text.rs` splits text into words and spaces. Each resulting `Str` and `Space` **must** have appropriate source info with precise byte offsets. This is complex because:
- A single comrak `Text` node becomes multiple Pandoc inlines
- Each inline needs a subset of the parent's source range

**Solution**: Update `tokenize_text` to accept the parent source info and track byte positions:

```rust
/// Tokenize text with source tracking
pub fn tokenize_text_with_source(
    text: &str,
    base_offset: usize,
    file_id: FileId,
) -> Inlines {
    let mut result = Vec::new();
    let mut current_word_start: Option<usize> = None;
    let mut current_word = String::new();
    let mut whitespace_start: Option<usize> = None;

    for (byte_idx, c) in text.char_indices() {
        let abs_offset = base_offset + byte_idx;

        if c.is_whitespace() {
            // Emit accumulated word
            if !current_word.is_empty() {
                let start = current_word_start.unwrap();
                let end = abs_offset;
                result.push(Inline::Str(Str {
                    text: std::mem::take(&mut current_word),
                    source_info: SourceInfo::original(file_id, start, end),
                }));
                current_word_start = None;
            }
            // Track whitespace start
            if whitespace_start.is_none() {
                whitespace_start = Some(abs_offset);
            }
        } else {
            // Emit space if we were in whitespace
            if let Some(ws_start) = whitespace_start {
                result.push(Inline::Space(Space {
                    source_info: SourceInfo::original(file_id, ws_start, abs_offset),
                }));
                whitespace_start = None;
            }
            // Track word start
            if current_word_start.is_none() {
                current_word_start = Some(abs_offset);
            }
            current_word.push(c);
        }
    }

    // Handle remaining content at end of string
    let end_offset = base_offset + text.len();

    if !current_word.is_empty() {
        let start = current_word_start.unwrap();
        result.push(Inline::Str(Str {
            text: current_word,
            source_info: SourceInfo::original(file_id, start, end_offset),
        }));
    } else if let Some(ws_start) = whitespace_start {
        // Trailing whitespace or pure whitespace
        result.push(Inline::Space(Space {
            source_info: SourceInfo::original(file_id, ws_start, end_offset),
        }));
    }

    result
}
```

## Implementation Order

1. **Phase 1**: `SourceLocationContext` struct and conversion functions
2. **Phase 2**: Update `block.rs` with source tracking
3. **Phase 3**: Update `inline.rs` with source tracking (except text tokenization)
4. **Phase 4**: Update `text.rs` with precise Str/Space source tracking
5. **Phase 5**: Add pampa reader module
6. **Phase 6**: Add dependencies
7. **Phase 7**: Testing

## Files to Create/Modify

### Create
- `crates/comrak-to-pandoc/src/source_location.rs` - Source location conversion
- `crates/pampa/src/readers/commonmark.rs` - CommonMark reader

### Modify
- `crates/comrak-to-pandoc/src/lib.rs` - Add module, export new functions
- `crates/comrak-to-pandoc/src/block.rs` - Add source tracking
- `crates/comrak-to-pandoc/src/inline.rs` - Add source tracking
- `crates/comrak-to-pandoc/src/text.rs` - Add precise Str/Space source tracking
- `crates/pampa/src/readers/mod.rs` - Add commonmark module
- `crates/pampa/src/main.rs` - Add --from commonmark case
- `crates/pampa/Cargo.toml` - Add comrak dependency

## Future Enhancements

1. **GFM support**: Optionally enable GFM extensions (tables, strikethrough, etc.)
2. **Error recovery**: Handle comrak parse errors gracefully
3. **Performance**: Consider whether to recompute line offsets or reuse from comrak

## References

- comrak source: `external-sources/comrak/src/nodes.rs` (Sourcepos, LineColumn)
- Property testing plan: `claude-notes/plans/2025-12-16-property-testing-commonmark-subset.md`
- quarto-source-map: `crates/quarto-source-map/src/source_info.rs`
