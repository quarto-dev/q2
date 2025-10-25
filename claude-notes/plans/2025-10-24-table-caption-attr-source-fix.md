# Table Caption Attribute Source Location Bug Fix

**Date**: 2025-10-24
**Issue**: Compiler warnings for unused `caption_attr_source` variable in postprocess.rs
**Root Cause**: Incomplete implementation - caption attribute source locations are extracted but never merged into table

## Problem Analysis

### Current Behavior (postprocess.rs:698-726)

When processing table captions with attributes like:

```markdown
| Header |
|--------|
| Data   |
: Caption text {#tbl-id .my-class}
```

The code:
1. ✅ **Extracts** `caption_attr` (the attribute values)
2. ✅ **Extracts** `caption_attr_source` (the source locations)
3. ✅ **Merges** `caption_attr` into `table.attr`
4. ❌ **Never uses** `caption_attr_source` - it's just thrown away

### Code Location

```rust
// Lines 698-704: Extract both attr and attr_source
let mut caption_attr: Option<Attr> = None;
let mut caption_attr_source: Option<AttrSourceInfo> = None;

if let Some(Inline::Attr(attr, attr_source)) = caption_content.last() {
    caption_attr = Some(attr.clone());
    caption_attr_source = Some(attr_source.clone());  // ⚠️ Extracted but never used
    caption_content.pop();
}

// Lines 709-725: Merge caption_attr values into table.attr
if let Some(caption_attr_value) = caption_attr {
    // Merge key-value pairs
    for (key, value) in caption_attr_value.2 {
        table.attr.2.insert(key, value);
    }
    // Merge classes
    for class in caption_attr_value.1 {
        if !table.attr.1.contains(&class) {
            table.attr.1.push(class);
        }
    }
    // Use caption id if table doesn't have one
    if table.attr.0.is_empty() && !caption_attr_value.0.is_empty() {
        table.attr.0 = caption_attr_value.0;
    }
}
// ❌ caption_attr_source is never merged into table.attr_source
```

### Data Structures

**Attr** (tuple): `(id: String, classes: Vec<String>, attributes: HashMap<String, String>)`
**AttrSourceInfo** (struct):
```rust
pub struct AttrSourceInfo {
    pub id: Option<SourceInfo>,              // Source location of id
    pub classes: Vec<Option<SourceInfo>>,    // Source locations of each class
    pub attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>, // (key_source, value_source)
}
```

## Impact

Without this fix:
- ❌ Caption attribute source locations are lost
- ❌ Error messages about caption attributes point to wrong locations
- ❌ JSON output has missing/incorrect `attrS` fields for tables with caption attributes
- ❌ Tests in `test_attr_source_parsing.rs` cannot verify table caption attributes

## Solution Plan

### Phase 1: Write Failing Test (TDD)

Create test in `tests/test_attr_source_parsing.rs`:

```rust
#[test]
fn test_table_caption_with_id_has_attr_source() {
    let input = "| Header |\n|--------|\n| Data   |\n: Caption {#tbl-id}";
    let pandoc = parse_qmd(input);

    let Block::Table(table) = &pandoc.blocks[0] else {
        panic!("Expected Table block");
    };

    // Verify the table has the ID from the caption
    assert_eq!(table.attr.0, "tbl-id", "Table should have id from caption");

    // Verify attr_source.id is populated with caption's source location
    assert!(
        table.attr_source.id.is_some(),
        "Table attr_source.id should be Some (from caption)"
    );

    // Verify the source location points to "#tbl-id" in the caption
    let id_source = table.attr_source.id.as_ref().unwrap();
    assert_source_matches(input, id_source, "#tbl-id");
}

#[test]
fn test_table_caption_with_classes_has_attr_source() {
    let input = "| Header |\n|--------|\n| Data   |\n: Caption {.table .bordered}";
    let pandoc = parse_qmd(input);

    let Block::Table(table) = &pandoc.blocks[0] else {
        panic!("Expected Table block");
    };

    // Verify classes were merged
    assert_eq!(table.attr.1.len(), 2);
    assert!(table.attr.1.contains(&"table".to_string()));
    assert!(table.attr.1.contains(&"bordered".to_string()));

    // Verify attr_source has source locations for both classes
    assert_eq!(table.attr_source.classes.len(), 2);

    // Find the indices for each class
    let table_idx = table.attr.1.iter().position(|c| c == "table").unwrap();
    let bordered_idx = table.attr.1.iter().position(|c| c == "bordered").unwrap();

    assert!(table.attr_source.classes[table_idx].is_some());
    assert!(table.attr_source.classes[bordered_idx].is_some());

    let table_source = table.attr_source.classes[table_idx].as_ref().unwrap();
    let bordered_source = table.attr_source.classes[bordered_idx].as_ref().unwrap();

    assert_source_matches(input, table_source, ".table");
    assert_source_matches(input, bordered_source, ".bordered");
}
```

**Expected result**: Tests should FAIL with useful error messages showing that `table.attr_source.id` is `None` or points to wrong location.

### Phase 2: Implement the Fix

Modify `postprocess.rs` lines 726-736 to merge source locations:

```rust
// After merging caption_attr values (line 726), add:
if let Some(caption_attr_source_value) = caption_attr_source {
    // Merge source locations parallel to how we merged the values

    // 1. Merge key-value attribute source locations
    // Note: HashMap doesn't preserve order, but we need to match the order
    // from caption_attr_value.2 iteration above
    for ((key_source, value_source)) in caption_attr_source_value.attributes {
        table.attr_source.attributes.push((key_source, value_source));
    }

    // 2. Merge class source locations
    // For each class we added from caption, add its source location
    for class_source in caption_attr_source_value.classes {
        table.attr_source.classes.push(class_source);
    }

    // 3. Use caption id source if table doesn't have one
    if table.attr_source.id.is_none() && caption_attr_source_value.id.is_some() {
        table.attr_source.id = caption_attr_source_value.id;
    }
}
```

**Note**: There's a subtle ordering issue - when we merge classes, we check `!table.attr.1.contains(&class)` before adding. We need to maintain the same index correspondence between `table.attr.1[i]` and `table.attr_source.classes[i]`. This might require refactoring the merge logic.

**Better approach**:
```rust
if let Some(caption_attr_value) = caption_attr {
    // Merge key-value pairs (both values and sources)
    if let Some(ref caption_attr_source_value) = caption_attr_source {
        for ((key, value), (key_source, value_source)) in
            caption_attr_value.2.iter().zip(caption_attr_source_value.attributes.iter())
        {
            table.attr.2.insert(key.clone(), value.clone());
            table.attr_source.attributes.push((key_source.clone(), value_source.clone()));
        }
    } else {
        // Fallback: merge values without sources
        for (key, value) in caption_attr_value.2 {
            table.attr.2.insert(key, value);
        }
    }

    // Merge classes (both values and sources)
    if let Some(ref caption_attr_source_value) = caption_attr_source {
        for (class, class_source) in
            caption_attr_value.1.iter().zip(caption_attr_source_value.classes.iter())
        {
            if !table.attr.1.contains(class) {
                table.attr.1.push(class.clone());
                table.attr_source.classes.push(class_source.clone());
            }
        }
    } else {
        // Fallback: merge classes without sources
        for class in caption_attr_value.1 {
            if !table.attr.1.contains(&class) {
                table.attr.1.push(class);
            }
        }
    }

    // Use caption id if table doesn't have one
    if table.attr.0.is_empty() && !caption_attr_value.0.is_empty() {
        table.attr.0 = caption_attr_value.0;
        // Also merge the source location
        if let Some(caption_attr_source_value) = caption_attr_source {
            if table.attr_source.id.is_none() {
                table.attr_source.id = caption_attr_source_value.id;
            }
        }
    }
}
```

### Phase 3: Run Tests and Verify

```bash
# Run the new tests - should now PASS
cargo test --test test_attr_source_parsing test_table_caption

# Run all tests to ensure no regressions
cargo test

# Verify warnings are gone
cargo check
```

Expected:
- ✅ New tests pass
- ✅ No compiler warnings about unused `caption_attr_source`
- ✅ All existing tests still pass

### Phase 4: Integration Test

Create end-to-end test with JSON serialization:

```rust
#[test]
fn test_table_caption_attr_source_json_roundtrip() {
    let input = "| Header |\n|--------|\n| Data   |\n: Caption {#tbl-1 .bordered}";
    let pandoc = parse_qmd(input);
    let context = ASTContext::anonymous();

    // Serialize to JSON
    let mut buffer = Cursor::new(Vec::new());
    quarto_markdown_pandoc::writers::json::write(&pandoc, &context, &mut buffer)
        .expect("Failed to write JSON");

    // Parse JSON and verify attrS is present
    let json_output = String::from_utf8(buffer.into_inner()).expect("Invalid UTF-8");
    let json: serde_json::Value = serde_json::from_str(&json_output)
        .expect("Failed to parse JSON");

    let table = &json["blocks"][0];
    assert_eq!(table["t"], "Table");

    // Verify attrS has id and classes sources
    assert!(table["attrS"]["id"].is_object() || table["attrS"]["id"].is_number());
    assert!(table["attrS"]["classes"].as_array().unwrap().len() > 0);
}
```

## Related Work

- **k-177** (in_progress): Phase 2-6 attr/target source tracking - this fix is part of that work
- **k-162** (in_progress): Implement Attr/Target source location sideloading
- **k-183** (closed): Just completed - improved attr_source tests
- **qmd-37** (closed): Document table caption attribute desugaring

## Success Criteria

- [ ] Failing test written and verified to fail for the right reason
- [ ] Fix implemented in postprocess.rs
- [ ] Tests pass
- [ ] No compiler warnings about unused variables
- [ ] JSON serialization includes correct attrS for table captions
- [ ] All existing tests still pass
- [ ] Code is properly formatted with `cargo fmt`

## Notes

- This is a small, focused bug fix
- Should be completed before continuing with broader k-177 work
- Good opportunity to add more test coverage for table caption attributes
- The merge logic needs to maintain index correspondence between attr values and attr_source
