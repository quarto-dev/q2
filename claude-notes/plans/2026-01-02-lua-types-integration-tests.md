# Plan: Lua Filter Integration Tests for types.rs Coverage

**Issue**: k-4csc
**Baseline Coverage**: types.rs at 44.56% (after pure Rust unit tests)
**Target**: Improve to 70%+ through Lua filter-based integration tests

## Overview

The `lua/types.rs` file (1574 lines) contains Lua bindings for Pandoc AST types. The pure Rust unit tests already cover `tag_name()`, `field_names()`, and basic `LuaAttr` methods. The remaining uncovered code requires Lua runtime execution to test, specifically:

1. **`get_field()` methods** - Dynamic field access via `__index`
2. **`set_field()` methods** - Dynamic field assignment via `__newindex`
3. **`__pairs` iteration** - For k,v in pairs(elem) support
4. **`__tostring` methods** - Debug string output
5. **Helper conversion functions** - Meta value, citations, captions, etc.

## Test Infrastructure

All tests will follow the established pattern from `lua/filter.rs`:

```rust
#[test]
fn test_example() {
    let dir = TempDir::new().unwrap();
    let filter_path = dir.path().join("test_filter.lua");
    fs::write(&filter_path, r#"
-- Lua filter code here
function ElementType(elem)
    -- Access/modify elem, return result
end
"#).unwrap();

    let pandoc = Pandoc {
        meta: ConfigValue::default(),
        blocks: vec![/* test document */],
    };
    let context = ASTContext::new();
    let (filtered, _, _) = apply_lua_filter(&pandoc, &context, &filter_path, "html").unwrap();

    // Assert on filtered output
}
```

## Test Categories

### Phase 1: Inline Element Field Access (get_field)

These tests verify reading fields from inline elements via Lua.

| Test Name | Elements | Fields Tested | Lines Covered |
|-----------|----------|---------------|---------------|
| `test_strong_content_access` | Strong | content | 100 |
| `test_underline_content_access` | Underline | content | 101 |
| `test_strikeout_content_access` | Strikeout | content | 102 |
| `test_superscript_content_access` | Superscript | content | 103 |
| `test_subscript_content_access` | Subscript | content | 104 |
| `test_smallcaps_content_access` | SmallCaps | content | 105 |
| `test_quoted_fields_access` | Quoted | content, quotetype | 109-116 |
| `test_code_fields_access` | Code | text, attr | 119-120 |
| `test_math_fields_access` | Math | text, mathtype | 123-130 |
| `test_rawinline_fields_access` | RawInline | text, format | 133-134 |
| `test_link_fields_access` | Link | content, target, title, attr | 137-140 |
| `test_image_fields_access` | Image | content, src, title, attr | 143-146 |
| `test_note_content_access` | Note | content | 149 |
| `test_cite_fields_access` | Cite | content, citations | 155-156 |
| `test_insert_fields_access` | Insert | content, attr | 159-160 |
| `test_delete_fields_access` | Delete | content, attr | 163-164 |
| `test_highlight_fields_access` | Highlight | content, attr | 167-168 |
| `test_editcomment_fields_access` | EditComment | content, attr | 171-172 |
| `test_notereference_id_access` | NoteReference | id | 175 |

### Phase 2: Inline Element Field Setting (set_field)

These tests verify modifying fields on inline elements.

| Test Name | Elements | Fields Tested | Lines Covered |
|-----------|----------|---------------|---------------|
| `test_str_text_set` | Str | text | 207-210 |
| `test_emph_content_set` | Emph | content | 213-216 |
| `test_strong_content_set` | Strong | content | 217-220 |
| `test_span_content_and_attr_set` | Span | content, attr | 241-244, 309-312 |
| `test_link_fields_set` | Link | content, target, title, attr | 247-258, 321-324 |
| `test_image_fields_set` | Image | content, src, title, attr | 261-272, 327-330 |
| `test_code_text_and_attr_set` | Code | text, attr | 275-278, 315-318 |
| `test_rawinline_fields_set` | RawInline | text, format | 281-288 |
| `test_math_text_set` | Math | text | 291-294 |
| `test_quoted_content_set` | Quoted | content | 297-300 |
| `test_note_content_set` | Note | content | 303-306 |
| `test_cite_fields_set` | Cite | content, citations | 333-340 |
| `test_insert_fields_set` | Insert | content, attr | 343-350 |
| `test_delete_fields_set` | Delete | content, attr | 353-360 |
| `test_highlight_fields_set` | Highlight | content, attr | 363-370 |
| `test_editcomment_fields_set` | EditComment | content, attr | 373-380 |
| `test_notereference_id_set` | NoteReference | id | 383-386 |
| `test_readonly_tag_error` | Any | tag (readonly) | 389 |
| `test_unknown_field_error` | Any | unknown | 392 |

### Phase 3: Block Element Field Access (get_field)

| Test Name | Elements | Fields Tested | Lines Covered |
|-----------|----------|---------------|---------------|
| `test_plain_content_access` | Plain | content | 568 |
| `test_para_content_access` | Para | content | 569 |
| `test_header_fields_access` | Header | level, content, attr, identifier, classes | 572-582 |
| `test_codeblock_fields_access` | CodeBlock | text, attr, identifier, classes | 585-594 |
| `test_rawblock_fields_access` | RawBlock | text, format | 597-598 |
| `test_blockquote_content_access` | BlockQuote | content | 601 |
| `test_div_fields_access` | Div | content, attr, identifier, classes | 604-613 |
| `test_bulletlist_content_access` | BulletList | content | 616-622 |
| `test_orderedlist_fields_access` | OrderedList | content, start, style | 625-644 |
| `test_figure_fields_access` | Figure | content, attr, identifier, caption | 647-650, 679 |
| `test_lineblock_content_access` | LineBlock | content | 652-658 |
| `test_definitionlist_content_access` | DefinitionList | content | 661-676 |
| `test_table_fields_access` | Table | attr, caption, identifier | 682-684 |

### Phase 4: Block Element Field Setting (set_field)

| Test Name | Elements | Fields Tested | Lines Covered |
|-----------|----------|---------------|---------------|
| `test_plain_content_set` | Plain | content | 716-719 |
| `test_para_content_set` | Para | content | 720-723 |
| `test_header_fields_set` | Header | level, content, identifier, attr | 726-737, 789-793 |
| `test_codeblock_fields_set` | CodeBlock | text, identifier, attr | 740-747, 795-799 |
| `test_rawblock_fields_set` | RawBlock | text, format | 750-757 |
| `test_blockquote_content_set` | BlockQuote | content | 760-763 |
| `test_div_fields_set` | Div | content, identifier, attr | 766-773, 801-805 |
| `test_figure_fields_set` | Figure | content, identifier, attr | 776-787 |
| `test_table_fields_set` | Table | attr, identifier | 807-815 |

### Phase 5: LuaAttr Field Access and Setting

| Test Name | Description | Lines Covered |
|-----------|-------------|---------------|
| `test_attr_positional_access` | attr[1], attr[2], attr[3] | 1327-1341 |
| `test_attr_named_access` | attr.identifier, attr.classes, attr.attributes | 1346-1361 |
| `test_attr_positional_set` | attr[1] = ..., attr[2] = ..., attr[3] = ... | 1374-1385 |
| `test_attr_named_set` | attr.identifier = ..., attr.classes = ..., attr.attributes = ... | 1390-1402 |
| `test_attr_readonly_tag_error` | attr.tag = ... (should error) | 1403 |
| `test_attr_unknown_field_error` | attr.unknown = ... (should error) | 1404 |
| `test_attr_clone` | attr:clone() | 1433-1435 |
| `test_attr_tostring` | tostring(attr) | 1438-1445 |
| `test_attr_len` | #attr == 3 | 1448 |

### Phase 6: Helper Conversion Functions

| Test Name | Function | Lines Covered |
|-----------|----------|---------------|
| `test_caption_to_lua` | caption_to_lua_table | 918-932 |
| `test_citations_to_lua` | citations_to_lua_table | 935-955 |
| `test_lua_value_to_attr_userdata` | lua_value_to_attr (userdata path) | 960-963 |
| `test_lua_value_to_attr_table` | lua_value_to_attr (table path) | 964-995 |
| `test_lua_table_to_citations` | lua_table_to_citations | 1001-1035 |
| `test_meta_value_to_lua_string` | meta_value_to_lua (MetaString) | 1046-1053 |
| `test_meta_value_to_lua_bool` | meta_value_to_lua (MetaBool) | 1054-1061 |
| `test_meta_value_to_lua_inlines` | meta_value_to_lua (MetaInlines) | 1062-1069 |
| `test_meta_value_to_lua_blocks` | meta_value_to_lua (MetaBlocks) | 1070-1077 |
| `test_meta_value_to_lua_list` | meta_value_to_lua (MetaList) | 1078-1087 |
| `test_meta_value_to_lua_map` | meta_value_to_lua (MetaMap) | 1088-1097 |
| `test_lua_to_meta_value_string` | lua_to_meta_value (string) | 1106 |
| `test_lua_to_meta_value_bool` | lua_to_meta_value (bool) | 1105 |
| `test_lua_to_meta_value_number` | lua_to_meta_value (number/integer) | 1107-1108 |
| `test_lua_to_meta_value_table` | lua_to_meta_value (table variants) | 1109-1183 |
| `test_meta_to_lua_table` | meta_to_lua_table | 1191-1197 |
| `test_lua_table_to_meta` | lua_table_to_meta | 1200-1211 |
| `test_lua_table_to_inlines` | lua_table_to_inlines | 1215-1236 |
| `test_lua_table_to_blocks` | lua_table_to_blocks | 1239-1260 |
| `test_filter_source_info` | filter_source_info (already tested in filter.rs) | 1266-1290 |
| `test_lua_table_to_strings` | lua_table_to_strings | 1465-1476 |
| `test_lua_table_to_string_map` | lua_table_to_string_map | 1479-1494 |

### Phase 7: Iteration and Walk Methods

| Test Name | Description | Lines Covered |
|-----------|-------------|---------------|
| `test_inline_pairs_iteration_with_integer_key` | pairs() with integer control variable | 448-452 |
| `test_inline_pairs_iteration_all_fields` | Complete pairs() iteration | 432-472 |
| `test_block_pairs_iteration` | pairs() on Block elements | 853-901 |
| `test_inline_walk_method` | elem:walk{...} on inlines | 191-196, 1532-1538 |
| `test_block_walk_method` | elem:walk{...} on blocks | 700-705, 1542-1545 |

## Implementation Strategy

### Grouping for Efficiency

To minimize test file proliferation, group related tests:

1. **Content-bearing inline tests** - One test file with multiple filter functions
2. **Inline field setting tests** - Grouped by commonality
3. **Block element tests** - Similar grouping
4. **LuaAttr tests** - All in one test
5. **Meta value tests** - All in one test
6. **Helper function tests** - Grouped by conversion direction

### Test File Organization

Add tests to `lua/filter.rs` in the existing `#[cfg(test)]` module, grouped by phase:

```rust
// Phase 1: Inline get_field tests
#[test] fn test_inline_content_access() { ... }
#[test] fn test_inline_field_variants() { ... }

// Phase 2: Inline set_field tests
#[test] fn test_inline_content_set() { ... }
#[test] fn test_inline_field_modifications() { ... }

// ... etc
```

## Priority Order

1. **High Impact (Phase 1-2)**: Inline get/set - Most frequently used in filters
2. **High Impact (Phase 3-4)**: Block get/set - Second most used
3. **Medium Impact (Phase 5)**: LuaAttr - Common but simpler
4. **Medium Impact (Phase 6)**: Meta value conversion - Important for document metadata
5. **Lower Impact (Phase 7)**: Iteration/walk - Already partially tested

## Estimated Coverage Improvement

| Phase | Lines Covered | Estimated Improvement |
|-------|---------------|----------------------|
| 1-2   | ~200 lines    | +10% |
| 3-4   | ~150 lines    | +7% |
| 5     | ~80 lines     | +4% |
| 6     | ~200 lines    | +10% |
| 7     | ~50 lines     | +2% |
| **Total** | ~680 lines | **+33%** (to ~78%) |

## Questions for Review

1. **Test granularity**: Should we have one test per element type, or group similar elements?
   - Recommendation: Group by category (content-bearing, field-heavy, etc.) for maintainability

2. **Error path testing**: Should we test error paths like "cannot set read-only field"?
   - Recommendation: Yes, at least one test per error type for completeness

3. **Test location**: Add to filter.rs or create new types_integration_test.rs?
   - Recommendation: Add to filter.rs since the infrastructure already exists there

4. **Parallelization**: Should tests be parallelized?
   - Recommendation: Yes, each test uses its own TempDir so they're independent

## Next Steps

1. Review and approve this plan
2. Create implementation issues for each phase
3. Implement Phase 1-2 first (highest impact)
4. Verify coverage improvement after each phase
5. Adjust plan based on actual coverage gains
