# Attribute Processing Implementation Plan

**Date**: 2025-10-31
**Context**: Add support for processing attribute nodes in tree-sitter refactoring

## Problem Statement

Currently, `code_span` nodes with attributes (e.g., `` `code`{.lang} ``) don't properly parse the attributes. The attributes are ignored and we get warnings about unhandled nodes.

**Current behavior**:
```
Input: `code`{.lang}
Output: Code ( "" , [] , [] ) "code"
Expected: Code ( "" , [ "lang" ] , [] ) "code"
```

## Root Cause

The attribute-related nodes are not being handled in the `native_visitor` function:
- `attribute_specifier` (wrapper)
- `{` and `}` (delimiters)
- `commonmark_specifier` (contains parsed attributes)
- `attribute_id` (e.g., `#myid`)
- `attribute_class` (e.g., `.class`)
- `key_value_specifier` (e.g., `key="value"`)
- `key_value_key`
- `=` (equals sign)
- `key_value_value`

## Tree Structure

From verbose output:
```
attribute_specifier: {Node attribute_specifier (0, 6) - (0, 41)}
  {: {Node { (0, 6) - (0, 7)}
  commonmark_specifier: {Node commonmark_specifier (0, 7) - (0, 40)}
    attribute_id: {Node attribute_id (0, 7) - (0, 12)}      // #myid
    attribute_class: {Node attribute_class (0, 13) - (0, 20)}  // .class1
    attribute_class: {Node attribute_class (0, 21) - (0, 28)}  // .class2
    key_value_specifier: {Node key_value_specifier (0, 29) - (0, 40)}
      key_value_key: {Node key_value_key (0, 29) - (0, 32)}    // key
      =: {Node = (0, 32) - (0, 33)}
      key_value_value: {Node key_value_value (0, 33) - (0, 40)}  // "value"
  }: {Node } (0, 40) - (0, 41)}
```

## Existing Infrastructure

**Already exists**:
1. `process_commonmark_attribute()` in `commonmark_attribute.rs` - expects children to already be:
   - `IntermediateBaseText` for `id_specifier` and `class_specifier`
   - `IntermediateKeyValueSpec` for key-value pairs

2. `process_attribute()` in `attribute.rs` - wrapper that handles `commonmark_attribute` vs `raw_attribute`

**Problem**: The low-level nodes (`attribute_id`, `attribute_class`, etc.) are not being converted to the intermediate types that these functions expect.

## Node Name Mapping

The tree uses different names than what the processors expect:
- Tree: `attribute_id` → Processor expects: `id_specifier`
- Tree: `attribute_class` → Processor expects: `class_specifier`
- Tree: `key_value_specifier` → Processor expects: (intermediate key-value spec)

## Implementation Plan

### Phase 1: Add Low-Level Node Handlers

Add handlers in `native_visitor` for the leaf nodes:

1. **`attribute_id`** → Extract text, strip `#`, return `IntermediateBaseText` with node name `"id_specifier"`
   ```rust
   "attribute_id" => {
       let text = node.utf8_text(input_bytes).unwrap();
       let id = &text[1..]; // Strip leading #
       PandocNativeIntermediate::IntermediateBaseText(
           id.to_string(),
           node_location(node)
       )
   }
   ```

2. **`attribute_class`** → Extract text, strip `.`, return `IntermediateBaseText` with node name `"class_specifier"`
   ```rust
   "attribute_class" => {
       let text = node.utf8_text(input_bytes).unwrap();
       let class = &text[1..]; // Strip leading .
       PandocNativeIntermediate::IntermediateBaseText(
           class.to_string(),
           node_location(node)
       )
   }
   ```

3. **`key_value_key`** → Extract text, return `IntermediateBaseText`
   ```rust
   "key_value_key" => {
       let text = node.utf8_text(input_bytes).unwrap().to_string();
       PandocNativeIntermediate::IntermediateBaseText(text, node_location(node))
   }
   ```

4. **`key_value_value`** → Extract text, strip quotes if present, return `IntermediateBaseText`
   ```rust
   "key_value_value" => {
       let text = node.utf8_text(input_bytes).unwrap();
       let value = extract_quoted_text(text); // Use existing helper
       PandocNativeIntermediate::IntermediateBaseText(value, node_location(node))
   }
   ```

5. **`{`, `}`, `=`** → Delimiter nodes, return `IntermediateUnknown`
   ```rust
   "{" | "}" | "=" => PandocNativeIntermediate::IntermediateUnknown(node_location(node))
   ```

### Phase 2: Add Mid-Level Handler

Add handler for `key_value_specifier` that collects key and value:

```rust
"key_value_specifier" => {
    let mut key = String::new();
    let mut value = String::new();
    let mut key_range = node_location(node);
    let mut value_range = node_location(node);

    for (node_name, child) in children {
        if node_name == "key_value_key" {
            if let IntermediateBaseText(text, range) = child {
                key = text;
                key_range = range;
            }
        } else if node_name == "key_value_value" {
            if let IntermediateBaseText(text, range) = child {
                value = text;
                value_range = range;
            }
        }
        // Ignore "="
    }

    PandocNativeIntermediate::IntermediateKeyValueSpec(vec![(key, value, key_range, value_range)])
}
```

### Phase 3: Modify `commonmark_specifier` Handler

The `commonmark_specifier` should collect its children and rename node types:

**Option A**: Add handler in `native_visitor` that renames and passes through:
```rust
"commonmark_specifier" => {
    // Rename node types to match what process_commonmark_attribute expects
    let renamed_children = children.into_iter().map(|(name, child)| {
        let new_name = match name.as_str() {
            "attribute_id" => "id_specifier",
            "attribute_class" => "class_specifier",
            _ => &name
        };
        (new_name.to_string(), child)
    }).collect();

    process_commonmark_attribute(renamed_children, context)
}
```

**Option B**: Modify `process_commonmark_attribute` to accept both old and new names:
```rust
if node == "id_specifier" || node == "attribute_id" {
    // ...
}
```

**Recommendation**: Option A is cleaner - keep the rename logic in one place.

### Phase 4: Add `attribute_specifier` Handler

Add handler that filters delimiters and processes the `commonmark_specifier` child:

```rust
"attribute_specifier" => {
    // Filter out delimiter nodes and pass through the commonmark_specifier result
    for (node_name, child) in children {
        if node_name == "commonmark_specifier" {
            return child; // Should be IntermediateAttr
        }
    }
    // If no commonmark_specifier found, return empty attr
    PandocNativeIntermediate::IntermediateAttr(
        ("".to_string(), vec![], HashMap::new()),
        AttrSourceInfo::empty()
    )
}
```

### Phase 5: Testing

Create tests for:
1. Simple class: `` `code`{.lang} ``
2. ID: `` `code`{#myid} ``
3. Multiple classes: `` `code`{.class1 .class2} ``
4. Key-value: `` `code`{key="value"} ``
5. Combined: `` `code`{#myid .class1 .class2 key="value"} ``
6. Edge cases: empty attributes, special characters, etc.

## Success Criteria

- ✅ No "[TOP-LEVEL MISSING NODE]" warnings for attribute nodes
- ✅ Code spans with attributes parse correctly
- ✅ All attribute components (id, classes, key-value) extracted correctly
- ✅ Output matches Pandoc native format
- ✅ All existing tests still pass

## Files to Modify

1. `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs` - Add node handlers
2. `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs` - Add tests

## Potential Issues

1. **Node name mismatch**: Tree uses `attribute_id`/`attribute_class` but processor expects `id_specifier`/`class_specifier`
   - Solution: Rename in the handler

2. **Delimiter handling**: Need to filter out `{`, `}`, `=` nodes
   - Solution: Handle them as IntermediateUnknown and filter in parent handlers

3. **Quote stripping**: `key_value_value` may have quotes that need removal
   - Solution: Use existing `extract_quoted_text()` helper

## Estimate

- Phase 1 (leaf nodes): 30 minutes
- Phase 2 (key_value_specifier): 20 minutes
- Phase 3 (commonmark_specifier): 15 minutes
- Phase 4 (attribute_specifier): 15 minutes
- Phase 5 (testing): 45 minutes
- **Total**: ~2 hours

## References

- Existing attribute processors: `commonmark_attribute.rs`, `attribute.rs`
- Helper function: `extract_quoted_text()` in `text_helpers.rs`
- Test examples: Look at headings with attributes (already working in atx_heading handler)
