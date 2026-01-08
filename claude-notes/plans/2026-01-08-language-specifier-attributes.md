# Plan: Support Attributes in Language Specifier for Code Blocks

## Problem Statement

The tree-sitter grammar now supports the following syntax:

```
```{language #id .class key=value}
code()
```
```

This is achieved by allowing `language_specifier` to contain an optional `commonmark_specifier`. However, the conversion from tree-sitter AST to Pandoc AST currently ignores the nested attributes.

### Current Behavior

Input:
```
```{python #fig-test .myclass key=value}
code()
```
```

Current output:
```json
{
  "t": "CodeBlock",
  "c": [
    ["", ["{python #fig-test .myclass key=value}"], []],
    "code()"
  ]
}
```

### Desired Behavior

```json
{
  "t": "CodeBlock",
  "c": [
    ["fig-test", ["{python}", "myclass"], [["key", "value"]]],
    "code()"
  ]
}
```

## Root Cause Analysis

### Tree-sitter AST Structure

The grammar produces this structure for `{python #fig-test .myclass key=value}`:

```
attribute_specifier: (0, 3) - (0, 40)
  {: (0, 3) - (0, 4)
  language_specifier: (0, 4) - (0, 39)
    commonmark_specifier: (0, 10) - (0, 39)
      attribute_id: (0, 11) - (0, 20)       # "#fig-test"
      attribute_class: (0, 21) - (0, 29)    # ".myclass"
      key_value_specifier: (0, 30) - (0, 39) # "key=value"
  }: (0, 39) - (0, 40)
```

Key observations:
1. `language_specifier` spans (0, 4) to (0, 39)
2. `commonmark_specifier` spans (0, 10) to (0, 39)
3. The language "python" is the text from (0, 4) to (0, 10) - the gap before `commonmark_specifier`
4. The `_language_specifier_token` is an external scanner token, so it's NOT a named child

### Current Processing Flow

1. **Bottom-up traversal**: Children are processed before parents
2. **`commonmark_specifier`** → Returns `IntermediateAttr(("fig-test", ["myclass"], {"key": "value"}), source_info)`
3. **`language_specifier`** (line 971 in `treesitter.rs`) → Calls `create_base_text_from_node_text()` which:
   - Extracts the ENTIRE text: `"python #fig-test .myclass key=value"`
   - Returns `IntermediateBaseText("python #fig-test .myclass key=value", range)`
   - **IGNORES the processed children!**
4. **`attribute_specifier`** → Passes through `language_specifier` result as-is
5. **`fenced_code_block.rs`** → Wraps the text in braces: `"{python #fig-test .myclass key=value}"`

## Implementation Plan

### Step 1: Modify `language_specifier` Processing

**File**: `crates/pampa/src/pandoc/treesitter.rs`

Change the `language_specifier` case (line 971) from:
```rust
"language_specifier" => create_base_text_from_node_text(node, input_bytes),
```

To a new function `process_language_specifier(node, children, input_bytes, context)` that:

1. Checks if there's a `commonmark_specifier` child in `children`
2. If NO `commonmark_specifier`:
   - Return `IntermediateBaseText(full_text, range)` (current behavior)
3. If YES `commonmark_specifier`:
   - Extract the language portion: text from start of `language_specifier` to start of `commonmark_specifier`
   - Get the `IntermediateAttr` from the `commonmark_specifier` child
   - Create a new `IntermediateAttr` that:
     - Sets id from commonmark_specifier's id
     - Prepends `{language}` to the classes (for roundtripping)
     - Adds remaining classes from commonmark_specifier
     - Adds attributes from commonmark_specifier
   - Track source locations appropriately

### Step 2: Create Helper Function

**File**: `crates/pampa/src/pandoc/treesitter_utils/language_specifier.rs` (new file)

```rust
pub fn process_language_specifier(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate
```

Logic:
1. Find `commonmark_specifier` child (if any)
2. If none found, return `IntermediateBaseText` with full node text
3. If found:
   - Calculate language substring: `node_start..commonmark_specifier_start`
   - Trim trailing whitespace from language
   - Extract `IntermediateAttr` from commonmark_specifier child
   - Build combined `IntermediateAttr`:
     - id: from commonmark_specifier
     - classes: `["{language}", ...commonmark_classes]`
     - attributes: from commonmark_specifier
   - Build combined `AttrSourceInfo`:
     - id source: from commonmark_specifier
     - classes sources: `[language_source, ...commonmark_class_sources]`
     - attributes sources: from commonmark_specifier

### Step 3: Handle Nested `{language_specifier}` Case

The grammar also allows `{python}` to be written as `{{python}}` (nested braces). This case:
```javascript
seq('{', $.language_specifier, '}')
```

The processing should handle this recursively - if the child is already processed, just pass it through.

### Step 4: Update Module Structure

**File**: `crates/pampa/src/pandoc/treesitter_utils/mod.rs`

Add:
```rust
pub mod language_specifier;
```

**File**: `crates/pampa/src/pandoc/treesitter.rs`

Add import:
```rust
use crate::pandoc::treesitter_utils::language_specifier::process_language_specifier;
```

### Step 5: Add Tests

**File**: `crates/pampa/tests/test_code_block_attributes.rs` (new file)

Test cases:
1. `{python}` → classes: `["{python}"]`, id: `""`, attrs: `[]`
2. `{python #fig-foo}` → classes: `["{python}"]`, id: `"fig-foo"`, attrs: `[]`
3. `{python .myclass}` → classes: `["{python}", "myclass"]`, id: `""`, attrs: `[]`
4. `{python #fig-foo .myclass}` → classes: `["{python}", "myclass"]`, id: `"fig-foo"`, attrs: `[]`
5. `{python key=value}` → classes: `["{python}"]`, id: `""`, attrs: `[["key", "value"]]`
6. `{python #fig-foo .myclass key=value}` → full combination
7. `{{python}}` → nested case, should work same as `{python}`

### Step 6: Verify Source Location Tracking

The `AttrSourceInfo` must correctly track:
- The language portion source location (for the `{python}` class)
- The id source location (from `commonmark_specifier`)
- Each class source location
- Each key-value pair source locations

## Edge Cases to Consider

1. **Whitespace handling**: `{python  #id}` has extra space before `#id` - commonmark_specifier includes leading whitespace, so language extraction should trim trailing whitespace

2. **Nested braces**: `{{python}}` is handled by the recursive grammar rule - the inner `language_specifier` will be processed first

3. **Empty attributes**: `{python}` with no additional attributes should produce just `["{python}"]` as before

4. **Raw format with language**: `{=html}` should still produce `RawBlock` (this is `raw_specifier`, not `language_specifier`)

## Files to Modify

1. `crates/pampa/src/pandoc/treesitter.rs` - Update `language_specifier` case
2. `crates/pampa/src/pandoc/treesitter_utils/mod.rs` - Add module
3. `crates/pampa/src/pandoc/treesitter_utils/language_specifier.rs` - New file
4. `crates/pampa/tests/test_code_block_attributes.rs` - New test file

## Acceptance Criteria

- [x] `{python #fig-test .myclass key=value}` produces correct attributes
- [x] Source locations are tracked correctly for each attribute component
- [x] Existing `{python}` and `{r}` cases still work
- [x] All existing tests pass (2961 tests)
- [x] New tests added for all syntax variations (12 tests)

## Implementation Notes (Completed 2026-01-08)

### Files Modified

1. `crates/pampa/src/pandoc/treesitter_utils/language_specifier.rs` - **NEW FILE**
   - `process_language_specifier()` function that checks for `commonmark_specifier` children
   - Extracts language portion by computing byte range before `commonmark_specifier`
   - Merges language (wrapped in braces) with attributes from `commonmark_specifier`
   - Properly tracks source locations for each attribute component

2. `crates/pampa/src/pandoc/treesitter_utils/mod.rs`
   - Added `pub mod language_specifier;`

3. `crates/pampa/src/pandoc/treesitter.rs`
   - Added import for `process_language_specifier`
   - Changed `language_specifier` case from `create_base_text_from_node_text()` to `process_language_specifier()`
   - Updated comments in `attribute_specifier` case

4. `crates/pampa/tests/test_code_block_attributes.rs` - **NEW FILE**
   - 12 test cases covering all syntax variations

5. `crates/pampa/resources/error-corpus/Q-2-8.json` - **REMOVED**
   - This error case was for detecting `{r eval=FALSE}` syntax and suggesting YAML block syntax
   - Now that this syntax is valid, the error case is obsolete

### Key Implementation Insight

The `language_specifier` node contains:
1. A hidden external scanner token (`_language_specifier_token`) for the language name
2. An optional `commonmark_specifier` child for additional attributes

Since the language token is hidden, we extract it by computing the byte range from `language_specifier.start` to `commonmark_specifier.start` (if present).

## Q-2-8 Warning Implementation (Added 2026-01-08)

The Q-2-8 warning was converted from a parse error to a semantic warning. It now warns when:
- A code block has a braced language specifier (like `{r}` or `{python}`)
- AND has key-value attributes (like `eval=FALSE`)
- AND does NOT have additional classes (like `.marimo`)

Examples:
- `{r eval=FALSE}` → **WARNS** (has options but no class)
- `{python .marimo}` → No warning (has a class)
- `{r .class key=value}` → No warning (has a class)
- `{python #id key=value}` → **WARNS** (id doesn't count, only classes suppress the warning)

Implementation location: `treesitter.rs` in the `pandoc_code_block` case, after calling `process_fenced_code_block`.

Tests added: 5 new tests in `test_warnings.rs`:
- `test_code_block_with_header_options_produces_warning`
- `test_code_block_with_class_no_warning`
- `test_code_block_with_class_and_options_no_warning`
- `test_simple_code_block_no_warning`
- `test_code_block_with_id_and_options_produces_warning`
