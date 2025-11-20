# Q-2-28: Line Break Before Escaped Shortcode Close

**Date**: 2025-11-20
**Error Code**: Q-2-28
**Related**: Q-2-27 (regular shortcodes)

## Problem Statement

Create Q-2-28 to detect line breaks immediately before the closing delimiter `>}}}` in escaped shortcodes, parallel to Q-2-27 which detects the same issue for regular shortcodes.

**Escaped shortcodes** use triple braces:
- Opening: `{{{<`
- Closing: `>}}}`

**Example that should fail:**
```markdown
{{{< hello
   >}}}
```

**Valid escaped shortcode:**
```markdown
{{{< hello >}}}
```

## Grammar Analysis

From `tree-sitter-markdown/grammar.js` (lines 522-530):

```javascript
shortcode_escaped: $ => seq(
    alias($._shortcode_open_escaped, $.shortcode_delimiter),  // "{{{<"
    $._shortcode_sep,
    $.shortcode_name,
    repeat(seq($._shortcode_sep, $._shortcode_value)),
    repeat(seq($._shortcode_sep, alias($._commonmark_key_value_specifier, $.key_value_specifier))),
    $._shortcode_sep,  // ← Line break here is the problem
    alias($._shortcode_close_escaped, $.shortcode_delimiter), // ">}}}"
),
```

The `$._shortcode_sep` before the closing delimiter can include soft line breaks (`$._soft_line_break`). The error occurs when there's a hard line break (newline) in this position.

## Comparison with Q-2-27

| Aspect | Q-2-27 (Regular) | Q-2-28 (Escaped) |
|--------|------------------|------------------|
| Opening | `{{<` | `{{{<` |
| Closing | `>}}` | `>}}}` |
| Grammar rule | `shortcode` | `shortcode_escaped` |
| Error pattern | Line break before `>}}` | Line break before `>}}}` |
| Parser states | Multiple (16 unique) | TBD (will discover) |

## Implementation Tasks

### 1. Error Catalog Entry

**File**: `crates/quarto-error-reporting/error_catalog.json`

**Entry**:
```json
{
  "Q-2-28": {
    "subsystem": "markdown",
    "title": "Line Break Before Escaped Shortcode Close",
    "message_template": "Line breaks are not allowed immediately before the escaped shortcode closing delimiter `>}}}`.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-28",
    "since_version": "99.9.9"
  }
}
```

### 2. Error Corpus Entry

**File**: `crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-28.json`

**Structure** (mirror Q-2-27 with prefixes and suffixes):
```json
{
  "code": "Q-2-28",
  "title": "Line Break Before Escaped Shortcode Close",
  "message": "Line breaks are not allowed immediately before the escaped shortcode closing delimiter `>}}}`.",
  "notes": [
    {
      "message": "This is the opening `{{{<` for the escaped shortcode",
      "label": "shortcode-open",
      "noteType": "simple"
    }
  ],
  "cases": [
    {
      "name": "simple",
      "description": "Line break before closing delimiter in various escaped shortcode forms",
      "content": "{{{< hello",
      "captures": [
        {
          "label": "shortcode-open",
          "row": 0,
          "column": 0,
          "size": 4
        }
      ],
      "prefixes": ["", "[", "_", "__", "![", "[++", "[--", "[!!", "[>>", "^", "~", "~~", "'", "*", "**", "^["],
      "suffixes": [
        "\n   >}}}",
        " key\n>}}}",
        " 'value'\n>}}}",
        " \"value\"\n>}}}",
        " 42\n>}}}",
        " param1 param2 param3\n>}}}",
        " key=value\n>}}}",
        " key='value'\n>}}}",
        " key=\"value\"\n>}}}",
        " key=42\n>}}}",
        " key1=val1 key2=val2\n>}}}",
        " param1 key=value\n>}}}",
        " \"title\" 123 key1=value key2='quoted' enabled=true\n>}}}",
        "\n    >}}}",
        " key=value   \n>}}}"
      ]
    }
  ]
}
```

**Key differences from Q-2-27**:
- Opening delimiter is 4 chars (`{{{<`) not 3 (`{{<`)
- Closing delimiter is `>}}}` not `>}}`
- Capture size is 4, not 3

### 3. qmd-syntax-helper Conversion Rule

**File**: `crates/qmd-syntax-helper/src/conversions/q_2_28.rs`

**Purpose**: Automatically fix Q-2-28 errors by removing line breaks before `>}}}`

**Algorithm**:
1. Parse file with `quarto-markdown-pandoc` to get diagnostics
2. Filter diagnostics for `code == "Q-2-28"`
3. For each Q-2-28 error:
   - Find the location of the line break before `>}}}`
   - Remove the line break and any leading whitespace on the next line
   - Keep the content on the same line
4. Apply fixes in reverse offset order to avoid invalidation

**Example transformation**:
```markdown
Input:  {{{< include file.qmd
        >}}}

Output: {{{< include file.qmd >}}}
```

**Implementation pattern** (based on q_2_11.rs):
```rust
// Q-2-28: Line Break Before Escaped Shortcode Close
//
// This conversion rule fixes Q-2-28 errors by removing line breaks
// before the escaped shortcode closing delimiter >}}}

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};

pub struct Q228Converter {}

#[derive(Debug, Clone)]
struct Q228Violation {
    offset: usize,                          // Offset of line break to remove
    error_location: Option<SourceLocation>,
}

impl Q228Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q228Violation>> {
        // Parse with quarto-markdown-pandoc
        // Filter for Q-2-28 errors
        // Extract violation locations
        todo!()
    }
}

impl Rule for Q228Converter {
    fn name(&self) -> &str {
        "q-2-28"
    }

    fn description(&self) -> &str {
        "Fix Q-2-28: Remove line breaks before escaped shortcode closing delimiter"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        // Return check results for each violation
        todo!()
    }

    fn convert(&self, file_path: &Path, _verbose: bool) -> Result<ConvertResult> {
        // Apply fixes: remove line breaks and whitespace
        todo!()
    }
}
```

### 4. Register Conversion Rule

**File**: `crates/qmd-syntax-helper/src/conversions/mod.rs`

Add:
```rust
pub mod q_2_28;
```

And register in the conversions list.

## Implementation Steps

### Phase 1: Error Detection (Corpus)

1. ✅ **Read related files** (Q-2-27, grammar, existing rules)
2. ✅ **Analyze grammar** for escaped shortcodes
3. ✅ **Create plan document** (this file)
4. ⏸️ **Add error catalog entry** for Q-2-28
5. ⏸️ **Create Q-2-28.json** corpus file
6. ⏸️ **Run `build_error_table.ts`** to generate test cases
7. ⏸️ **Run `cargo test`** to verify error detection works
8. ⏸️ **Test manually** with example files

### Phase 2: Automatic Fix (qmd-syntax-helper)

9. ⏸️ **Create `q_2_28.rs`** conversion module
10. ⏸️ **Implement violation detection** (parse diagnostics)
11. ⏸️ **Implement fix logic** (remove line breaks)
12. ⏸️ **Register in mod.rs**
13. ⏸️ **Add tests** for conversion rule
14. ⏸️ **Run qmd-syntax-helper tests**
15. ⏸️ **Test end-to-end** with example files

## Expected Test Cases

With 16 prefixes × 15 suffixes = **241 test cases** (mirroring Q-2-27):

**Prefixes** (different markdown contexts):
- `""` - bare
- `"[", "![", "[++", "[--", "[!!", "[>>", "^["` - bracket contexts
- `"_", "__", "*", "**"` - emphasis contexts
- `"^", "~", "~~"` - superscript/subscript contexts
- `"'"` - quote context

**Suffixes** (different shortcode content before line break):
1. Just name: `\n   >}}}`
2. Naked param: ` key\n>}}}`
3. Single-quoted param: ` 'value'\n>}}}`
4. Double-quoted param: ` "value"\n>}}}`
5. Number param: ` 42\n>}}}`
6. Multiple params: ` param1 param2 param3\n>}}}`
7. KV naked: ` key=value\n>}}}`
8. KV single-quoted: ` key='value'\n>}}}`
9. KV double-quoted: ` key="value"\n>}}}`
10. KV number: ` key=42\n>}}}`
11. Multiple KVs: ` key1=val1 key2=val2\n>}}}`
12. Mixed: ` param1 key=value\n>}}}`
13. Complex: ` "title" 123 key1=value key2='quoted' enabled=true\n>}}}`
14. Indented break: `\n    >}}}`
15. Trailing spaces: ` key=value   \n>}}}`

## Parser State Analysis

After running `build_error_table.ts`, we'll analyze:
- How many unique parser states are captured
- Whether escaped shortcodes have different states than regular shortcodes
- Duplicate state detection output

**Expected**: Similar to Q-2-27, we should get ~16 unique parser states representing different contexts where the error can occur.

## Verification

### Manual Tests

**Test file**: `test-q-2-28.qmd`
```markdown
{{{< hello
>}}}

[{{{< meta key
>}}}]

{{{< include
    file.qmd
>}}}
```

**Expected**: 3 Q-2-28 errors detected

### Conversion Test

```bash
# Check for errors
qmd-syntax-helper check test-q-2-28.qmd

# Apply fixes
qmd-syntax-helper convert test-q-2-28.qmd

# Verify result
cat test-q-2-28.qmd
# Should show: {{{< hello >}}}
#              [{{{< meta key >}}}]
#              {{{< include file.qmd >}}}
```

## Edge Cases to Consider

1. **Multiple line breaks**: `{{{< hello\n\n\n>}}}`
   - Should remove all whitespace between content and `>}}}`

2. **Nested shortcodes**: `{{{< meta {{{< inner >}}} >}}}`
   - Make sure we only fix the outer shortcode

3. **Mixed regular and escaped**: `{{< regular >}} {{{< escaped\n>}}}`
   - Should only fix Q-2-28, not touch regular shortcode

4. **In different contexts**: Inside links, emphasis, etc.
   - Prefixes test this comprehensively

## Success Criteria

✅ Error catalog entry added
✅ Q-2-28.json corpus file created
✅ 241 test case files generated
✅ All cargo tests pass
✅ Manual test shows Q-2-28 detected
✅ qmd-syntax-helper rule created
✅ Conversion rule tests pass
✅ End-to-end conversion works correctly

## Notes

- **Parallel to Q-2-27**: This error is structurally identical to Q-2-27, just for escaped shortcodes instead of regular ones
- **Grammar difference**: Only difference is triple braces vs double braces in delimiters
- **Parser behavior**: The parser handles escaped shortcodes separately (`shortcode_escaped` vs `shortcode`), so we get different parser states
- **User benefit**: Users of escaped shortcodes get the same helpful error messages as regular shortcode users

## References

- Q-2-27 implementation and structure
- `tree-sitter-markdown/grammar.js` lines 522-540
- `tree-sitter-markdown/test/corpus/shortcode.txt` test 7 (escaped shortcode)
- `crates/qmd-syntax-helper/src/conversions/q_2_11.rs` (conversion pattern)
