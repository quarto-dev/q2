# Implementation Plan: Unnumbered Section Specifier {-}

## Date: 2025-11-13

## Overview

Implement support for Pandoc's `{-}` syntax for unnumbered sections. This is a shorthand syntax that Pandoc desugars into an "unnumbered" class on Header elements.

## Current State Analysis

### What Works
- Tree-sitter grammar already recognizes `{-}` syntax as `unnumbered_specifier`
- Grammar change: `unnumbered_specifier: $ => "-"` in grammar.js:432
- Test case exists in tree-sitter-qmd/tree-sitter-markdown/test/corpus/qmd.txt:788-799

### What Doesn't Work
- The Rust code doesn't handle `unnumbered_specifier` nodes
- Current behavior: `## foo {-}` produces `Header 2 ("foo", [], [])`
- Expected behavior: `## foo {-}` should produce `Header 2 ("foo", ["unnumbered"], [])`

### Verification
```bash
# Pandoc's output (correct)
$ echo '## foo {-}' | pandoc -t native
[ Header 2 ( "foo" , [ "unnumbered" ] , [] ) [ Str "foo" ] ]

# Our current output (incorrect)
$ echo '## foo {-}' | cargo run -- -t native
[ Header 2 ( "foo" , [] , [] ) [Str "foo"] ]

# Pandoc also accepts explicit class (correct)
$ echo '## foo {.unnumbered}' | pandoc -t native
[ Header 2 ( "foo" , [ "unnumbered" ] , [] ) [ Str "foo" ] ]
```

## Tree-Sitter Grammar Structure

The grammar shows this hierarchy:
```
attribute_specifier
  {
  unnumbered_specifier
    -
  }
```

The `_pandoc_attr_specifier` can contain one of:
- `unnumbered_specifier` (just `-`)
- `commonmark_specifier` (id, classes, key-value pairs)
- Language specifier variants

## Rust Code Analysis

### Current Attribute Handling Flow

1. **Tree-sitter traversal** (`src/pandoc/treesitter.rs:967-985`)
   - Encounters `attribute_specifier` node
   - Looks for children: `commonmark_specifier`, `raw_specifier`, or `language_specifier`
   - **BUG**: Doesn't handle `unnumbered_specifier`
   - Falls through to return empty attr: `("", vec![], LinkedHashMap::new())`

2. **ATX Heading processing** (`src/pandoc/treesitter_utils/atx_heading.rs:53-60`)
   - Receives `IntermediateAttr` from attribute_specifier
   - Extracts and applies to Header

### Key Data Structures

```rust
// Pandoc Attr type
type Attr = (String, Vec<String>, LinkedHashMap<String, String>);
//          ^id     ^classes      ^key-value pairs

// IntermediateAttr variant
PandocNativeIntermediate::IntermediateAttr(Attr, AttrSourceInfo)

// AttrSourceInfo tracks source locations
pub struct AttrSourceInfo {
    pub id: Option<SourceInfo>,
    pub classes: Vec<Option<SourceInfo>>,
    pub key_values: LinkedHashMap<String, Option<SourceInfo>>,
}
```

## Implementation Strategy

### Location of Changes
**File**: `src/pandoc/treesitter.rs`
**Function**: Within the match arm for `"attribute_specifier"` (lines 967-985)

### Change Details

Current code:
```rust
"attribute_specifier" => {
    for (node_name, child) in children {
        if node_name == "commonmark_specifier" {
            return child;
        } else if node_name == "raw_specifier" {
            return child;
        } else if node_name == "language_specifier" {
            return child;
        }
    }
    // Falls through to empty attr
    use hashlink::LinkedHashMap;
    PandocNativeIntermediate::IntermediateAttr(
        ("".to_string(), vec![], LinkedHashMap::new()),
        AttrSourceInfo::empty(),
    )
}
```

New code needs to add:
```rust
"attribute_specifier" => {
    for (node_name, child) in children {
        if node_name == "commonmark_specifier" {
            return child;
        } else if node_name == "raw_specifier" {
            return child;
        } else if node_name == "language_specifier" {
            return child;
        } else if node_name == "unnumbered_specifier" {
            // Handle {-} syntax by returning attr with "unnumbered" class
            // Need to extract source location from the node
            return process_unnumbered_specifier(child);
        }
    }
    // Falls through to empty attr
    use hashlink::LinkedHashMap;
    PandocNativeIntermediate::IntermediateAttr(
        ("".to_string(), vec![], LinkedHashMap::new()),
        AttrSourceInfo::empty(),
    )
}
```

### Implementation Options

**Option 1: Process inline in attribute_specifier handler**
- Extract location info from the child (which is the unnumbered_specifier node result)
- Build IntermediateAttr directly

**Option 2: Create helper function `process_unnumbered_specifier`**
- More consistent with the codebase pattern (process_commonmark_attribute, etc.)
- Cleaner separation of concerns
- **PREFERRED**

### Extracting Source Location

The `child` parameter in the loop will be the result of processing the `unnumbered_specifier` node.

Looking at the grammar, `unnumbered_specifier: $ => "-"`, this means it's a terminal that directly contains the `-` character.

We need to understand what `child` will be when we receive it. Let me trace through the code:
- The `unnumbered_specifier` node contains a `-` child
- The `-` character will be processed by the default handler
- Looking at the bottom of the match statement, unhandled nodes likely become `IntermediateUnknown`

Actually, we need to look at what gets passed as `child`. The traversal happens in a specific way - we need to understand the full node structure.

### Getting Node Information

The current code has access to:
- `node_name`: The string name of the node kind
- `child`: The processed result (PandocNativeIntermediate)
- `node`: The original tree-sitter node (available in the parent context)

For source tracking, we need the original tree-sitter node for `unnumbered_specifier`. But in the loop, we only have access to the processed children.

**Solution**: We need to access the original tree-sitter node. Looking at similar code:
- `process_commonmark_attribute` receives `children` but works with the processed results
- The source location is extracted from `IntermediateBaseText` which contains ranges

For `unnumbered_specifier`, we could:
1. Check if we can access the original node in the attribute_specifier handler
2. Or extract location info from the child if it's wrapped in IntermediateBaseText

Actually, looking more carefully at the code structure - in the `treesitter_to_pandoc_ast_dfs` function, before calling the match arm, it processes children. So we have both:
- `node` - the original tree-sitter node for attribute_specifier
- `children` - the processed children

So we can find the unnumbered_specifier child node directly!

```rust
"attribute_specifier" => {
    for (node_name, child) in children {
        if node_name == "unnumbered_specifier" {
            // Get the source location for the unnumbered specifier
            // We need to find the original node to get its location
            // ... implementation details
        }
    }
}
```

Wait, let me look at how the code actually works. The children come from traversing child nodes. Let me check the traversal code.

Looking at the broader structure:
- `treesitter_to_pandoc_ast_dfs` is the main traversal function
- It processes children first, then matches on the node kind
- The `children` vec contains `(String, PandocNativeIntermediate)` tuples

For getting source locations, I see that some nodes wrap their results with location info. For example, looking at how `attribute_class` is handled in `commonmark_attribute.rs`, it receives `IntermediateBaseText(text, range)` and extracts the range.

But `unnumbered_specifier` is just `-`, which might be processed as Unknown. Let me check what `-` would be processed as in the match statement.

Looking at the match statement, I don't see a handler for `-` as a standalone token. It would fall through to the default case.

Actually, I should check what the verbose output shows for the child processing:
```
unnumbered_specifier: {Node unnumbered_specifier (0, 8) - (0, 9)}
  -: {Node - (0, 8) - (0, 9)}
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: unnumbered_specifier
```

So both `-` and `unnumbered_specifier` are unhandled. This means they'll likely return `IntermediateUnknown`.

Looking at the PandocNativeIntermediate enum, I need to check what IntermediateUnknown contains:
- It probably wraps a location/range

So the strategy should be:
1. When we see `unnumbered_specifier` in the children loop
2. Extract the location from the child (which should be IntermediateUnknown with a range)
3. Build an IntermediateAttr with "unnumbered" class and the source location

### Detailed Implementation

```rust
"attribute_specifier" => {
    for (node_name, child) in children {
        if node_name == "commonmark_specifier" {
            return child;
        } else if node_name == "raw_specifier" {
            return child;
        } else if node_name == "language_specifier" {
            return child;
        } else if node_name == "unnumbered_specifier" {
            // Extract source location from the child
            let range = match child {
                PandocNativeIntermediate::IntermediateUnknown(r) => r,
                _ => {
                    // Fallback: create empty range if unexpected type
                    quarto_source_map::Range::default()
                }
            };

            // Build IntermediateAttr with "unnumbered" class
            use hashlink::LinkedHashMap;
            let attr = (
                "".to_string(),                    // No id
                vec!["unnumbered".to_string()],    // "unnumbered" class
                LinkedHashMap::new(),              // No key-value pairs
            );

            // Build AttrSourceInfo with source location for the class
            let mut attr_source = AttrSourceInfo::empty();
            attr_source.classes.push(Some(
                SourceInfo::from_range(context.current_file_id(), range)
            ));

            return PandocNativeIntermediate::IntermediateAttr(attr, attr_source);
        }
    }
    // Falls through to empty attr if none of the above matched
    use hashlink::LinkedHashMap;
    PandocNativeIntermediate::IntermediateAttr(
        ("".to_string(), vec![], LinkedHashMap::new()),
        AttrSourceInfo::empty(),
    )
}
```

Wait, but looking at the tree structure again:
```
attribute_specifier: {Node attribute_specifier (0, 7) - (0, 10)}
  {: {Node { (0, 7) - (0, 8)}
  unnumbered_specifier: {Node unnumbered_specifier (0, 8) - (0, 9)}
    -: {Node - (0, 8) - (0, 9)}
  }: {Node } (0, 9) - (0, 10)}
```

The `attribute_specifier` has children:
- `{` (delimiter)
- `unnumbered_specifier` (the actual specifier)
- `}` (delimiter)

And `unnumbered_specifier` has a child:
- `-` (the literal character)

So when we iterate over children of `attribute_specifier`, we'll see `unnumbered_specifier` as one of the children. The processing of `unnumbered_specifier` will have already descended into it and processed its `-` child.

Actually, I need to also handle the `unnumbered_specifier` node itself in the match statement! Currently it's unhandled, so let me add a handler for it too.

Let me revise the plan:

1. Add a match arm for `"unnumbered_specifier"` that returns the node location
2. Modify the `"attribute_specifier"` handler to check for `unnumbered_specifier` and build the appropriate attr

Actually, let me reconsider. Looking at how `commonmark_specifier` works:
- There's a handler for `"commonmark_specifier"` that calls `process_commonmark_attribute`
- That function receives the processed children and builds an IntermediateAttr
- The attribute_specifier handler then just passes through the result

So for consistency, I should:
1. Add a handler for `"unnumbered_specifier"` that processes it and returns IntermediateAttr
2. The `attribute_specifier` handler will just pass through that result

Let me look at where to add the handler:

```rust
"commonmark_specifier" => {
    process_commonmark_attribute(children, context)
}
"unnumbered_specifier" => {
    process_unnumbered_specifier(node, context)
}
"attribute_specifier" => {
    for (node_name, child) in children {
        if node_name == "commonmark_specifier" {
            return child;
        } else if node_name == "raw_specifier" {
            return child;
        } else if node_name == "language_specifier" {
            return child;
        } else if node_name == "unnumbered_specifier" {
            return child;  // Pass through the IntermediateAttr
        }
    }
    // ...
}
```

And create a helper function:

```rust
fn process_unnumbered_specifier(
    node: &tree_sitter::Node,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    use hashlink::LinkedHashMap;

    let attr = (
        "".to_string(),
        vec!["unnumbered".to_string()],
        LinkedHashMap::new(),
    );

    let mut attr_source = AttrSourceInfo::empty();
    attr_source.classes.push(Some(
        node_source_info_with_context(node, context)
    ));

    PandocNativeIntermediate::IntermediateAttr(attr, attr_source)
}
```

This is much cleaner and follows the existing pattern!

## Test Plan

### Test Cases

1. **Basic unnumbered section**
   ```markdown
   ## foo {-}
   ```
   Expected: `Header 2 ("foo", ["unnumbered"], [])`

2. **Unnumbered with ID**
   ```markdown
   ## foo {#myid -}
   ```
   Expected: `Header 2 ("myid", ["unnumbered"], [])`

   Note: Need to verify if this syntax is valid in Pandoc. The grammar shows `unnumbered_specifier` and `commonmark_specifier` as alternatives in a choice, so `{#id -}` might not be valid. Need to test with Pandoc.

3. **Unnumbered with other classes**
   ```markdown
   ## foo {- .myclass}
   ```
   Expected: `Header 2 ("foo", ["unnumbered", "myclass"], [])`

   Same note as above - need to verify syntax validity.

4. **Different heading levels**
   ```markdown
   # H1 {-}
   ## H2 {-}
   ### H3 {-}
   ```

5. **Explicit unnumbered class (should already work)**
   ```markdown
   ## foo {.unnumbered}
   ```

### Test Implementation

Looking at existing tests:
- `tests/roundtrip_tests/` - for roundtrip tests
- `tests/json_location_test.rs` - for JSON output testing
- Need to find where basic parsing tests are

Add test in appropriate location that:
1. Parses `## foo {-}`
2. Verifies output matches Pandoc's native format
3. Verifies JSON output is correct

## Edge Cases & Questions

1. **Can {-} be combined with other attributes?**
   - Need to test: `{#id -}`, `{- .class}`, `{#id - .class}`
   - Grammar suggests these might not be valid (choice vs sequence)

2. **What if there are spaces?**
   - `{ - }` vs `{-}`
   - Need to test

3. **Does {-} work on other elements?**
   - Pandoc docs suggest it's specific to headers
   - But grammatically it could appear on other elements with attributes
   - Need to understand scope

4. **Source location tracking**
   - Ensure the source location for the "unnumbered" class points to the `-` character
   - Important for error reporting and IDE features

## Implementation Steps (Test-Driven)

1. ✅ Write failing test for basic case: `## foo {-}`
2. ✅ Run test, verify it fails with empty classes
3. ✅ Add `process_unnumbered_specifier` handler
4. ✅ Add match arm for `"unnumbered_specifier"`
5. ✅ Update `"attribute_specifier"` to pass through unnumbered_specifier
6. ✅ Run test, verify it passes
7. ✅ Test edge cases
8. ✅ Run full test suite for regressions

## Files to Modify

1. **src/pandoc/treesitter.rs**
   - Add match arm for `"unnumbered_specifier"`
   - Update `"attribute_specifier"` handler
   - Add helper function or inline handling

2. **src/pandoc/treesitter_utils/** (if using separate module)
   - Create `unnumbered_specifier.rs` or add to existing module
   - Add `process_unnumbered_specifier` function

3. **tests/** (appropriate test file)
   - Add test cases for unnumbered specifier

## Success Criteria

- [ ] `echo '## foo {-}' | cargo run -- -t native` matches Pandoc output
- [ ] Source locations are correctly tracked
- [ ] All existing tests still pass
- [ ] New tests added and passing
- [ ] Edge cases handled (or documented as unsupported)

## References

- Tree-sitter grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:424-432`
- Test corpus: `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/qmd.txt:788-799`
- Attribute processing: `src/pandoc/treesitter_utils/commonmark_attribute.rs`
- Main traversal: `src/pandoc/treesitter.rs:967-985`
