# k-87: SourceInfo::default() Audit

## Summary
Total instances: 43 across 8 files

## Categorization

### 1. JSON Reader (src/readers/json.rs) - 6 instances
**Context**: Reading JSON, may not have source info in legacy JSON

Lines 1621, 1624, 1627, 1639, 1659, 1739, 1762

**Category**: LEGITIMATE DEFAULT
- Reading from JSON format that may not include source info
- Backward compatibility with old JSON without location data
- **Action**: Document as legitimate, no fix needed

### 2. QMD Reader (src/readers/qmd.rs) - 2 instances
**Context**: Creating MetaMapEntry for metadata parsed from YAML

Lines 225, 261

**Category**: CAN BE FIXED
- These are creating metadata from parsed YAML
- YAML parser should have source info available
- **Action**: Get source info from YAML parse tree

### 3. Treesitter (src/pandoc/treesitter.rs) - 3 instances

#### Lines 391, 404: RawInline for "quarto-internal-leftover"
**Context**: Error recovery - unrecognized nodes
```rust
Inline::RawInline(RawInline {
    format: "quarto-internal-leftover".to_string(),
    text: node_text_fn(),
    source_info: quarto_source_map::SourceInfo::default(),
})
```
**Category**: CAN BE FIXED
- `node` is available in scope
- **Action**: Use `node_source_info(node)` or `node_source_info_with_context(node, context)`

#### Line 619: Paragraph wrapper for Note content
**Context**: Creating synthetic Paragraph wrapper for Note inlines
```rust
Block::Paragraph(Paragraph {
    content: inlines,
    source_info: quarto_source_map::SourceInfo::default(),
})
```
**Category**: SHOULD PROPAGATE
- `inlines` vec contains elements with source info
- **Action**: Combine/propagate source info from first/last inline elements

### 4. Document (src/pandoc/treesitter_utils/document.rs) - 1 instance

#### Line 40: Default MetaValueWithSourceInfo
**Context**: Creating Pandoc struct with empty metadata
```rust
PandocNativeIntermediate::IntermediatePandoc(Pandoc {
    meta: MetaValueWithSourceInfo::default(),
    blocks,
})
```
**Category**: LEGITIMATE DEFAULT (but check if YAML metadata present)
- Default empty metadata when no frontmatter
- **Action**: Check if this is correct - might need to parse YAML metadata with source info

### 5. Postprocess (src/pandoc/treesitter_utils/postprocess.rs) - 14 instances

All are creating synthetic/transformed AST nodes:

#### Lines 355, 360, 362: Figure transformation
**Context**: Creating Figure wrapper with Caption from Image
**Category**: SHOULD PROPAGATE
- Source elements (image, content) have source info
- **Action**: Propagate/combine source info from original elements

#### Line 389: Div for class transformation
**Context**: Creating Div for class-based transformation
**Category**: SHOULD PROPAGATE
- **Action**: Propagate from original element

#### Lines 402, 415, 428, 441: Span wrappers for editorial marks
**Context**: Wrapping Insert/Delete/Highlight/EditComment in Span
**Category**: SHOULD PROPAGATE
- Original editorial mark has source info
- **Action**: Use source_info from original Insert/Delete/etc.

#### Line 471: Math span wrapper
**Context**: Wrapping Math in Span with class
**Category**: SHOULD PROPAGATE
- Math element has source info
- **Action**: Use math.source_info

#### Lines 552, 611, 622, 637: Synthetic Space in citation processing
**Context**: Adding Space between citation and following content
**Category**: LEGITIMATE DEFAULT (synthetic)
- These are newly created Space nodes for formatting
- Could argue for propagating nearby source info
- **Action**: Document as synthetic, or propagate from adjacent element

### 6. Block (src/pandoc/block.rs) - 1 instance

#### Line 155: make_raw_leftover function
**Context**: Creating RawBlock for leftover content
```rust
Block::RawBlock(RawBlock {
    format: "quarto-internal-leftover".to_string(),
    text,
    source_info: quarto_source_map::SourceInfo::default(), // TODO already marked
})
```
**Category**: CAN BE FIXED (TODO exists)
- Function receives tree-sitter node as parameter
- **Action**: Use node to get source info

### 7. Inline (src/pandoc/inline.rs) - 1 instance

#### Line 479: make_raw_leftover function
**Context**: Similar to block.rs
**Category**: CAN BE FIXED (TODO exists)
- **Action**: Use node to get source info

### 8. Meta (src/pandoc/meta.rs) - 15 instances

#### Line 78: Default trait implementation
**Context**: Default for MetaValueWithSourceInfo
**Category**: LEGITIMATE DEFAULT
- Part of Default trait, expected behavior

#### Lines 155, 162: meta_value_from_legacy_map
**Context**: Converting from legacy MetaValue (no source info)
**Category**: LEGITIMATE DEFAULT
- Legacy format doesn't have source info
- **Action**: Document

#### Lines 171, 175, 179, 183, 187, 194, 200: meta_value_from_legacy
**Context**: Converting from legacy MetaValue
**Category**: LEGITIMATE DEFAULT
- Legacy format conversion
- **Action**: Document

#### Lines 283, 407, 409, 584, 586: Synthetic Span wrappers
**Context**: Creating Span wrappers for metadata values
**Category**: MIXED
- Some could propagate from contained Str
- Some are truly synthetic
- **Action**: Review case-by-case

## Priority Fixes

### High Priority (Easy wins with tree-sitter nodes available)
1. **treesitter.rs lines 391, 404** - Have `node`, use `node_source_info(node)`
2. **block.rs line 155** - Have node parameter, already has TODO
3. **inline.rs line 479** - Have node parameter, already has TODO

### Medium Priority (Need to propagate from existing AST)
4. **treesitter.rs line 619** - Propagate from inlines
5. **postprocess.rs lines 355, 360, 362** - Propagate from image/caption
6. **postprocess.rs lines 402, 415, 428, 441** - Propagate from editorial marks
7. **postprocess.rs line 471** - Propagate from math element

### Low Priority (Check if fixable)
8. **qmd.rs lines 225, 261** - Check if YAML parser provides source info
9. **document.rs line 40** - Check if metadata parsing provides source info

### Document Only (Legitimate defaults)
10. All json.rs instances - backward compat
11. All meta.rs legacy conversion instances
12. meta.rs Default trait impl

## Testing Strategy

For each fix:
1. Create test markdown file that exercises the code path
2. Parse to JSON and verify source_info fields are populated
3. Check filenameIndex, offset, row, column are correct
4. Add to test suite to prevent regression

## Progress Update

### Completed Fixes

#### 1. treesitter.rs leftover nodes (lines 391, 404) - ✅ DONE
- Added `node_source_info_fn` parameter to `process_native_inline()`
- Created closure in `native_visitor` to capture node and context
- Both RawInline leftover cases now use proper source info
- **Tests**: All existing tests pass (leftover cases are error paths, not tested)

#### 2. block.rs leftover (line 155) - ✅ DONE  
- Changed `_node` parameter to `node`
- Removed TODO comment
- Now uses `node_source_info(node)`
- **Tests**: All existing tests pass

#### 3. inline.rs leftover (line 479) - ✅ DONE
- Changed `_node` parameter to `node`
- Removed TODO comment
- Now uses `node_source_info(node)`
- **Tests**: All existing tests pass

### Summary
- Fixed 5 instances out of 43 (12%)
- All "easy win" cases with tree-sitter nodes available are now complete
- Remaining work focuses on propagation cases and documentation

### Next Steps
1. Fix Note paragraph wrapper (line 622) - propagate from inlines
2. Fix postprocess.rs cases - propagate from source elements
3. Add tests for important user-facing fixes (skip error case tests)
4. Document remaining legitimate default() uses

