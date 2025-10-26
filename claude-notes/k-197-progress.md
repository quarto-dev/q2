# k-197 Progress: Create test fixtures for missing block types

**Date**: 2025-10-25
**Status**: In Progress
**Session**: Partial completion

## Completed ✅

### Test Fixtures Created:
1. **OrderedList** - `ordered-list.qmd/json` ✅
   - Basic ordered lists starting at 1
   - Lists starting at custom number (5)
   - Nested ordered lists
   - Tests passing

2. **Div with attributes** - `div-attrs.qmd/json` ✅
   - Div with class
   - Div with ID and class
   - Div with custom key-value attributes
   - Nested divs
   - Tests mostly passing (1 test failing - see below)

3. **Figure** - `figure.qmd/json` ✅
   - Simple figure with ID
   - Figure with caption
   - Figure layout with multiple images
   - Tests passing

4. **HorizontalRule** - `horizontal-rule.qmd/json` ✅
   - Multiple horizontal rule styles (---, ***, ___)
   - Tests passing

5. **RawBlock** - `raw-block.qmd/json` ✅
   - HTML raw blocks
   - LaTeX raw blocks
   - Tests passing

### Test File Created:
- `test/block-types.test.ts` with 7 test cases
- Tests for each block type
- Source mapping validation test

### Test Results:
- Total tests: 49
- Passing: 46
- Failing: 3

## Remaining Work ❌

### 1. DefinitionList (BLOCKED)
**Issue**: The definition list syntax using `~` is not supported by quarto-markdown-pandoc parser.

**Attempted**:
```markdown
Term 1
  ~ Definition for term 1
```

**Error**: "Parse error: unexpected character or token here" at the `~` character.

**Current workaround**: Using `div.definition-list` class which gets desugared to DefinitionList.

**Status**: definition-list.qmd exists but definition-list.json is empty (0 bytes) due to parse errors.

**Need**: Investigation of correct syntax for definition lists in quarto-markdown-pandoc, or confirmation that only div.definition-list syntax is supported.

### 2. Null Block
**Status**: Not started
**File**: Need to create `null-block.qmd/json`
**Issue**: Unclear how to create Null blocks in QMD syntax. May need to investigate if Null blocks are even user-creatable or only generated internally.

### 3. Test Failures

**Failure 1**: `definition-list.json - DefinitionList conversion`
- Cause: Empty JSON file due to parse error
- Fix: Resolve definition list syntax issue

**Failure 2**: `div-attrs.json - Div with attributes conversion`
- Error: "Should have Div with custom attributes"
- Test expectation: Looking for `attr-key` or `attr-value` components
- Actual: The div-attrs.json shows `"kvs":[[null,null],[null,null]]` - source info is null for custom attributes
- Issue: Custom attributes don't have source location tracking, or test needs adjustment
- The actual JSON has the attributes: `[["custom-key","test"],["data-value","42"]]`
- Fix needed: Either update test to check result field instead of components, or investigate why attrS.kvs are null

**Failure 3**: `all block types - source mapping validation`
- Cause: Cascading failure due to definition-list.json being empty

## Files Created

### Examples:
- `examples/ordered-list.qmd` + `.json` (5.0K)
- `examples/definition-list.qmd` (245 bytes, no JSON - parse error)
- `examples/div-attrs.qmd` + `.json`
- `examples/figure.qmd` + `.json`
- `examples/horizontal-rule.qmd` + `.json`
- `examples/raw-block.qmd` + `.json`

### Tests:
- `test/block-types.test.ts` (new file, ~200 lines)

## Next Steps

1. **Investigate definition list syntax**:
   - Check quarto-markdown-pandoc documentation/tests for definition list examples
   - Determine if only div.definition-list is supported
   - Update definition-list.qmd with correct syntax
   - Regenerate JSON

2. **Fix div-attrs test**:
   - Debug why custom attribute source info is null
   - Either fix test expectations or investigate attrS generation
   - Test currently checks for attr-key/attr-value components
   - May need to check result field instead: `result[0][2]` contains the kvPairs

3. **Create Null block fixture**:
   - Research how Null blocks are created
   - May need to create manually or investigate if they're user-facing

4. **Verify all tests passing**:
   - Run `npm test` and confirm 49/49 passing
   - Update README.md if needed

## Notes

- All fixtures use quarto-markdown-pandoc binary: `cargo run --bin quarto-markdown-pandoc -- -t json -i <file.qmd>`
- JSON files must be regenerated with stderr redirected: `2>/dev/null` to avoid Cargo output in JSON
- Custom attributes in Pandoc: Attr = [id, classes, [[key, value]]]
- AttrSourceInfo = {id, classes, kvs: [[keySourceId, valueSourceId]]}
- When kvs source IDs are null, test needs to validate differently
