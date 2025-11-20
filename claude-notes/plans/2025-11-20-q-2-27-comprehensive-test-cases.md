# Q-2-27 Comprehensive Test Cases Plan

**Date**: 2025-11-20
**Error Code**: Q-2-27 - Line Break Before Shortcode Close
**Status**: Planning

## Background

Q-2-27 detects when a line break appears immediately before the shortcode closing delimiter `>}}`. Currently, we only have one test case ("simple") that tests the most basic scenario.

## Shortcode Grammar Analysis

From `tree-sitter-markdown/grammar.js` (lines 532-565), a shortcode consists of:

```javascript
shortcode: $ => seq(
    alias($._shortcode_open, $.shortcode_delimiter),      // "{{<"
    $_shortcode_sep,                                       // whitespace
    $.shortcode_name,                                      // name
    repeat(seq($_shortcode_sep, $_shortcode_value)),      // positional params
    repeat(seq($_shortcode_sep, alias($_shortcode_key_value_specifier, $.key_value_specifier))), // key-value attrs
    $_shortcode_sep,                                       // whitespace
    alias($_shortcode_close, $.shortcode_delimiter),      // ">}}"
)
```

### Value Types (`_shortcode_value`)
- `shortcode_name` - identifier-like strings
- `shortcode_naked_string` - unquoted strings (allows URLs, special chars)
- `shortcode_string` - single or double-quoted strings
- `shortcode_number` - JSON-style numbers
- Nested `shortcode` - shortcodes can contain other shortcodes

### Key-Value Attributes
```javascript
_shortcode_key_value_specifier: $ => seq(
    alias($_key_specifier_token, $.key_value_key),
    optional($_inline_whitespace),
    '=',
    optional($_inline_whitespace),
    alias($_shortcode_value, $.key_value_value)
)
```

## Test Case Strategy

The error occurs when a line break appears in the final `$_shortcode_sep` before `>}}`. We need to test this scenario after different types of shortcode content to ensure the error is caught regardless of what comes before the problematic line break.

### Categories to Test

1. **Parameters Only** - Positional values
   - Naked strings
   - Single-quoted strings
   - Double-quoted strings
   - Numbers
   - Multiple parameters

2. **Key-Value Attributes Only**
   - Naked string values
   - Single-quoted values
   - Double-quoted values
   - Number values
   - Multiple attributes

3. **Mixed Content**
   - Parameters + attributes
   - Complex combinations

4. **Edge Cases**
   - Nested shortcodes (if relevant)
   - Special characters in values

## Proposed Test Cases

### Current Coverage
- ✅ `simple` - Name only: `{{< hello\n   >}}`

### Proposed New Cases

#### Category 1: Positional Parameters

1. **with-naked-param**
   - Description: Line break after naked string parameter
   - Content: `{{< meta key\n>}}`
   - Tests: Naked string value type

2. **with-single-quoted-param**
   - Description: Line break after single-quoted parameter
   - Content: `{{< meta 'value'\n>}}`
   - Tests: Single-quoted string value type

3. **with-double-quoted-param**
   - Description: Line break after double-quoted parameter
   - Content: `{{< meta "value"\n>}}`
   - Tests: Double-quoted string value type

4. **with-number-param**
   - Description: Line break after number parameter
   - Content: `{{< meta 42\n>}}`
   - Tests: Number value type

5. **with-multiple-params**
   - Description: Line break after multiple parameters
   - Content: `{{< meta param1 param2 param3\n>}}`
   - Tests: Multiple positional parameters

#### Category 2: Key-Value Attributes

6. **with-kv-naked**
   - Description: Line break after key-value with naked string
   - Content: `{{< meta key=value\n>}}`
   - Tests: Key-value with naked string value

7. **with-kv-single-quoted**
   - Description: Line break after key-value with single-quoted value
   - Content: `{{< meta key='value'\n>}}`
   - Tests: Key-value with single-quoted value

8. **with-kv-double-quoted**
   - Description: Line break after key-value with double-quoted value
   - Content: `{{< meta key="value"\n>}}`
   - Tests: Key-value with double-quoted value

9. **with-kv-number**
   - Description: Line break after key-value with number value
   - Content: `{{< meta key=42\n>}}`
   - Tests: Key-value with number value

10. **with-multiple-kvs**
    - Description: Line break after multiple key-value attributes
    - Content: `{{< meta key1=val1 key2=val2\n>}}`
    - Tests: Multiple key-value attributes

#### Category 3: Mixed Content

11. **mixed-params-and-kvs**
    - Description: Line break after mixed parameters and attributes
    - Content: `{{< meta param1 key=value\n>}}`
    - Tests: Combination of positional and named arguments

12. **complex-mixed**
    - Description: Line break after complex mixed content
    - Content: `{{< component "title" 123 key1=value key2='quoted' enabled=true\n>}}`
    - Tests: Multiple parameters and attributes of different types

#### Category 4: Whitespace Variations

13. **with-indented-break**
    - Description: Line break with indentation before close
    - Content: `{{< hello\n    >}}`
    - Tests: Whitespace on the line with closing delimiter

14. **with-spaces-then-break**
    - Description: Trailing spaces before line break
    - Content: `{{< meta key=value   \n>}}`
    - Tests: Trailing whitespace before the line break

## Expected JSON Structure

Each test case will add an entry to the `cases` array in `Q-2-27.json`:

```json
{
  "name": "test-case-name",
  "description": "Description of what this tests",
  "content": "{{< shortcode content\n>}}",
  "captures": [
    {
      "label": "shortcode-open",
      "row": 0,
      "column": 0,
      "size": 3
    }
  ]
}
```

## Implementation Steps

1. ✅ Analyze grammar and existing tests
2. ⏳ Design comprehensive test cases (this document)
3. ⏸️ Get user approval
4. ⏸️ Update `Q-2-27.json` with new cases
5. ⏸️ Run `./scripts/build_error_table.ts` to regenerate corpus
6. ⏸️ Test with `cargo test` to ensure all cases are detected correctly
7. ⏸️ Verify error messages display properly for each case

## Notes

- All captures use the same label (`shortcode-open`) pointing to the opening `{{<` delimiter
- The line break is the key element - it appears right before `>}}`
- Some cases use minimal indentation (`\n>}}`) while others use spaces for clarity (`\n   >}}`)
- Focus is on testing different content types before the line break, not variations in the error itself

## Questions for Review

1. Are there any other value types or shortcode patterns we should test?
2. Should we test with more complex nested shortcodes?
3. Are the whitespace variation tests (13-14) necessary, or is that over-testing?
4. Should we add tests with special characters in naked strings (URLs, paths)?
