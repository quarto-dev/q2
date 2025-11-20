# Shortcode Line Break Error Message Design

Date: 2025-11-20
File: claude-notes/plans/2025-11-20-shortcode-linebreak-error.md

## Problem

The parser cannot handle a line break immediately before the shortcode closing delimiter `>}}`.

**Example that fails:**
```markdown
{{< hello
   >}}
```

**Parser state:**
- State: 2605
- Symbol: "_close_block"
- Location: After "hello" (column 9, row 0)

The parser successfully recognizes:
1. `{{<` - shortcode opening delimiter
2. ` ` - whitespace
3. `hello` - shortcode name
4. Then expects closing delimiter but encounters newline + `>}}`

## Error Code Selection

Looking at existing error codes:
- Q-1-xxx: Block-level errors
- Q-2-xxx: Inline-level errors
- Q-3-xxx: Special construct errors

Shortcodes are inline constructs, so this should be **Q-2-xxx**.

Next available in Q-2 series: Q-2-27 (after Q-2-26)

**Error Code: Q-2-27**

## Error Message Design

Following tidyverse guidelines and existing patterns:

**Title**: "Line Break Before Shortcode Close"

**Message**: "Line breaks are not allowed immediately before the shortcode closing delimiter `>}}`."

**Problem**: "I found a line break after the shortcode name `{name}`, but the closing `>}}` must be on the same line."

**Details**:
1. Point to the shortcode opening: "This is the opening `{{<`"
2. Point to where the closing should be: "The closing `>}}` should appear here (on the same line)"

**Hints**:
- "Move the `>}}` to the same line as the shortcode name"
- "Line breaks are allowed after parameter names but not before the final `>}}`"

## Error Corpus Entry

File: `crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-27.json`

```json
{
  "code": "Q-2-27",
  "title": "Line Break Before Shortcode Close",
  "message": "Line breaks are not allowed immediately before the shortcode closing delimiter `>}}`.",
  "notes": [
    {
      "message": "This is the opening `{{<`",
      "label": "shortcode-open",
      "noteType": "simple"
    },
    {
      "message": "The closing `>}}` should appear here (on the same line)",
      "label": "expected-close",
      "noteType": "simple"
    }
  ],
  "cases": [
    {
      "name": "simple",
      "description": "Line break before closing delimiter",
      "content": "{{< hello\n   >}}",
      "captures": [
        {
          "label": "shortcode-open",
          "row": 0,
          "column": 0,
          "size": 3
        },
        {
          "label": "expected-close",
          "row": 0,
          "column": 9
        }
      ]
    },
    {
      "name": "with-param",
      "description": "Line break before closing with parameter",
      "content": "{{< include file.qmd\n>}}",
      "captures": [
        {
          "label": "shortcode-open",
          "row": 0,
          "column": 0,
          "size": 3
        },
        {
          "label": "expected-close",
          "row": 0,
          "column": 20
        }
      ]
    }
  ]
}
```

## Implementation Steps

### 1. Find where to emit the error

The error occurs at state 2605 with symbol "_close_block". I need to find where this state is handled in the parser.

Look for:
- Grammar rule that produces state 2605
- Code that handles shortcode parsing
- Where we can detect newline before `>}}`

### 2. Create the error corpus entry

- File: `crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-27.json`
- Two test cases: simple and with-param

### 3. Run build_error_table.ts

Generate the autogen table with the new (state, sym) mapping.

### 4. Implement error emission in parser

Find the shortcode parsing code and add logic to emit Q-2-27 when:
- We're in a shortcode (after `{{<` and shortcode name)
- We encounter a newline
- The next non-whitespace token is `>}}`

### 5. Test

**Test file**: `~/today/bad-shortcode-linebreak.qmd`

**Expected output**:
```
Error: [Q-2-27] Line Break Before Shortcode Close
╭─[bad-shortcode-linebreak.qmd:1:1]
│
1 │ {{< hello
  │ ─┬─
  │  ╰── This is the opening `{{<`
  │         ┬
  │         ╰── The closing `>}}` should appear here (on the same line)
2 │    >}}
───╯

Line breaks are not allowed immediately before the shortcode closing delimiter `>}}`.

i The closing `>}}` should appear here (on the same line)
? Move the `>}}` to the same line as the shortcode name
? Line breaks are allowed after parameter names but not before the final `>}}`
```

### 6. qmd-syntax-helper rule (future)

A `q-2-27` converter rule could automatically fix this by:
1. Detecting Q-2-27 errors
2. Finding the newline before `>}}`
3. Removing the newline and any leading whitespace on the next line
4. Result: `{{< hello >}}`

## Grammar Context

Need to understand:
- Where is state 2605 in the grammar?
- What rule produces "_close_block" symbol?
- Why does newline cause this state?

Let me search the grammar files for shortcode handling.

## Implementation Location

Likely files to modify:
- `crates/quarto-markdown-pandoc/src/readers/qmd/shortcode.rs` (if exists)
- `crates/quarto-markdown-pandoc/src/readers/qmd/inline.rs`
- Tree-sitter grammar for shortcode inline construct

## Next Steps

1. ✅ Understand the problem (done)
2. ✅ Design error message (done)
3. ✅ Choose error code Q-2-27 (done)
4. Create error corpus JSON file
5. Run build_error_table.ts
6. Find shortcode parsing code
7. Implement error emission
8. Test with example file
9. Add to test suite
