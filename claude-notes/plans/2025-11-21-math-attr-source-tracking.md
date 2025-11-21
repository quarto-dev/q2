# Math+Attr Source Tracking Implementation Plan

## Problem Statement

In `postprocess.rs` line 667, when wrapping a Math element with its following Attr in a Span, we currently use `SourceInfo::default()` for the Span's source_info. This loses source tracking for the entire `$math$ {.attr}` construct.

## Current Code Pattern

```qmd
$x + y$ {.equation}
```

Gets transformed to:
- Math(source_info → "$x + y$")
- Space (optional)
- Attr({.equation}, AttrSourceInfo)

→ Wrapped in Span with source_info = default() ❌

## Root Cause Analysis

### Issue 1: Inline::Attr Structure
```rust
Inline::Attr(Attr, AttrSourceInfo)
```

- `Attr` is just data: `(String, Vec<String>, HashMap<String, String>)`
- `AttrSourceInfo` tracks individual pieces (id, classes, key-value pairs)
- **No overall SourceInfo for the entire attribute block**

### Issue 2: AttrSourceInfo is Fragmented
```rust
pub struct AttrSourceInfo {
    pub id: Option<SourceInfo>,           // {#id ...}
    pub classes: Vec<Option<SourceInfo>>, // {.class1 .class2 ...}
    pub attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>, // {key=value}
}
```

Individual pieces may be in any order and may have gaps. We need an overall span from first to last piece.

## Solution Options

### Option 1: Use Math source_info only (Simple but Incomplete)
**Approach**: Use `math.source_info.clone()` for the Span

**Pros**:
- Zero code changes needed
- Covers the main content

**Cons**:
- Doesn't include attribute location
- Errors pointing to Span won't highlight attributes
- Incomplete source tracking

**Verdict**: ❌ Not acceptable - we want complete tracking

### Option 2: Add SourceInfo to Inline::Attr (Correct but Invasive)
**Approach**: Change AST structure to include overall location

```rust
Inline::Attr(Attr, AttrSourceInfo, SourceInfo)
```

**Changes required**:
1. Update `PandocNativeIntermediate::IntermediateAttr`
2. Update all Inline::Attr creation sites (parsing)
3. Update all Inline::Attr pattern matches (everywhere)
4. Update JSON serialization/deserialization
5. Update all tests

**Pros**:
- Architecturally correct
- Enables proper source tracking everywhere

**Cons**:
- Major refactoring (50+ sites)
- Affects serialization format
- High risk of breaking changes

**Verdict**: ⏸️ Good long-term solution, but too invasive for this fix

### Option 3: Helper function to combine AttrSourceInfo pieces (Pragmatic)
**Approach**: Create helper to extract overall SourceInfo from AttrSourceInfo

```rust
/// Combine all source pieces in AttrSourceInfo into a single SourceInfo
fn combine_attr_source_pieces(attr_source: &AttrSourceInfo) -> Option<SourceInfo> {
    let mut result: Option<SourceInfo> = None;

    if let Some(id_src) = &attr_source.id {
        result = Some(id_src.clone());
    }

    for class_src in &attr_source.classes {
        if let Some(src) = class_src {
            result = match result {
                Some(r) => Some(r.combine(src)),
                None => Some(src.clone()),
            };
        }
    }

    for (key_src, val_src) in &attr_source.attributes {
        if let Some(src) = key_src {
            result = match result {
                Some(r) => Some(r.combine(src)),
                None => Some(src.clone()),
            };
        }
        if let Some(src) = val_src {
            result = match result {
                Some(r) => Some(r.combine(src)),
                None => Some(src.clone()),
            };
        }
    }

    result
}
```

Usage:
```rust
let span_source_info = if let Some(attr_overall) = combine_attr_source_pieces(attr_source) {
    math.source_info.combine(&attr_overall)
} else {
    // No attribute source info available (shouldn't happen but handle gracefully)
    math.source_info.clone()
};

math_processed.push(Inline::Span(Span {
    attr: (attr.0.clone(), classes, attr.2.clone()),
    content: vec![Inline::Math(math.clone())],
    source_info: span_source_info,
    attr_source: attr_source.clone(),
}));
```

**Pros**:
- Works with current AST structure
- Minimal code changes (one helper function, one call site)
- Preserves all source information (via Concat)
- Can be reused elsewhere if needed

**Cons**:
- Creates SourceInfo::Concat rather than a continuous Original range
- Order of pieces in Concat may not match source order
- Slightly more complex SourceInfo structure

**Verdict**: ✅ **RECOMMENDED** - Pragmatic solution that works with current architecture

## Recommended Approach: Option 3

### Implementation Steps

#### 1. Add helper function to attr.rs
Location: `crates/quarto-markdown-pandoc/src/pandoc/attr.rs`

Add after the `AttrSourceInfo::empty()` method:

```rust
impl AttrSourceInfo {
    /// Creates an empty AttrSourceInfo with no source tracking.
    pub fn empty() -> Self { ... }

    /// Combine all source pieces into a single SourceInfo
    ///
    /// This iterates through id, classes, and attributes, combining
    /// all non-None SourceInfo pieces using SourceInfo::combine().
    /// The result is a SourceInfo::Concat that preserves all pieces.
    ///
    /// Returns None if no source info pieces are present.
    pub fn combine_all(&self) -> Option<quarto_source_map::SourceInfo> {
        // Implementation as shown above
    }
}
```

**Test**: Add unit test in attr.rs testing the combine_all method with various AttrSourceInfo configurations

#### 2. Update postprocess.rs Math+Attr handling
Location: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs:667`

Replace:
```rust
source_info: quarto_source_map::SourceInfo::default(),
```

With:
```rust
source_info: if let Some(attr_overall) = attr_source.combine_all() {
    math.source_info.combine(&attr_overall)
} else {
    math.source_info.clone()
},
```

Remove the TODO comment.

#### 3. Write tests

**Test file**: `crates/quarto-markdown-pandoc/tests/test_math_attr_source.rs`

Test cases:
1. Math with single class attribute
2. Math with multiple classes
3. Math with id attribute
4. Math with key-value attributes
5. Math with mixed attributes (id + classes + kv)
6. Math with Space then attribute
7. Math without attributes (regression test)

Each test should:
- Parse QMD with math+attr pattern
- Convert to JSON
- Verify the Span's source_info field is a Concat containing:
  - Math source info
  - Combined attribute source info
- Verify source locations map correctly using SourceContext

**Example test**:
```rust
#[test]
fn test_math_with_class_attr_source() {
    let qmd = "$x$ {.equation}";
    let result = parse_to_json(qmd);

    // Find the Span wrapping the Math
    let span = find_math_attr_span(&result);

    // Verify source_info is Concat
    assert!(matches!(span.source_info, SourceInfo::Concat { .. }));

    // Map source locations and verify they cover the entire construct
    let ctx = SourceContext::new();
    let file_id = ctx.add_file("test.qmd", qmd);

    // Should span from start of $ to end of }
    // Implementation details depend on exact JSON structure
}
```

#### 4. Consider meta.rs:449 yaml-tagged-string case

The same pattern exists in meta.rs:449 where a Span wraps content that has source_info, but the Span itself uses default(). Apply the same fix if the wrapped content has source tracking.

## Testing Strategy

### Unit Tests
- `AttrSourceInfo::combine_all()` method with various inputs
- Edge cases: empty AttrSourceInfo, single piece, multiple pieces

### Integration Tests
- Math+Attr patterns in real QMD documents
- Verify JSON output has correct source_info structure
- Verify source mapping resolves to correct locations

### Regression Tests
- Math without attributes still works
- Existing tests still pass

### Manual Testing
```bash
echo '$x$ {.equation}' | cargo run --bin quarto-markdown-pandoc -- -t json | jq '.blocks[0]'
```

Verify the Span has a Concat source_info, not default.

## Risks and Mitigations

### Risk 1: SourceInfo::Concat complexity
**Impact**: More complex source info structure
**Mitigation**: This is intentional - Concat preserves full provenance. Mapping logic already handles Concat.

### Risk 2: Non-source-ordered Concat
**Impact**: Pieces in Concat may not match source order (id, classes, attributes are iterated in struct order)
**Mitigation**: Acceptable - all pieces are preserved. Future improvement could sort by offset before combining.

### Risk 3: Breaking existing code that pattern matches Inline::Attr
**Impact**: None - we're not changing Inline::Attr structure
**Mitigation**: N/A

## Future Improvements

### Long-term: Add overall SourceInfo to Inline::Attr (Option 2)
When doing a major AST refactor, consider adding a third field:
```rust
Inline::Attr(Attr, AttrSourceInfo, SourceInfo)
```

This would make source tracking cleaner and avoid the need for combine_all().

### Optimization: Cache combined SourceInfo
If combine_all() is called frequently, consider caching the result in AttrSourceInfo.

## Success Criteria

- ✅ No more `SourceInfo::default()` at postprocess.rs:667
- ✅ Math+Attr Span has proper source_info (Concat of math + attr)
- ✅ All existing tests pass
- ✅ New tests verify correct source tracking
- ✅ Manual testing shows correct JSON output

## Estimated Effort

- Helper function: 30 minutes
- Postprocess fix: 15 minutes
- Unit tests: 1 hour
- Integration tests: 1-2 hours
- Testing and debugging: 1 hour
- **Total**: 3-4 hours
