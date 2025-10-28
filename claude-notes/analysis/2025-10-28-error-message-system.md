# Error Message System in quarto-markdown-pandoc

**Date**: 2025-10-28
**Context**: Understanding how to add new high-quality error messages

## Overview

The error message infrastructure is based on Clinton Jeffery's TOPLAS 2003 paper "Generating Syntax Errors from Examples". The system allows compiler writers to associate diagnostic messages with syntax errors **by example**, avoiding the need to interpret integer parse states directly or modify the grammar.

## Key Insight

The fundamental problem: LR parsers maintain integer "parse states" that are hard to interpret. Traditional approaches require either:
1. Modifying the grammar to add error productions (tangled, hard to maintain)
2. Manually mapping parse states to messages (breaks on every grammar change)

**Jeffery's solution**: Write example error documents, run the parser on them to capture (state, symbol) pairs, and automatically build a lookup table mapping these pairs to error messages.

## System Architecture

### Components

1. **Error Corpus** (`resources/error-corpus/`)
   - `NNN.qmd` files: Minimal examples of specific parse errors
   - `NNN.json` files: Error message specifications with captures and notes
   - `_autogen-table.json`: Generated lookup table (DO NOT EDIT MANUALLY)

2. **Build Script** (`scripts/build_error_table.ts`)
   - Reads all `*.qmd` + `*.json` pairs from error-corpus
   - For each pair:
     - Runs parser with `--_internal-report-error-state` flag
     - Extracts (state, sym, row, column) from error location
     - Extracts all consumed tokens with their (row, column, size, lrState, sym)
     - Matches captures in JSON spec to consumed tokens by (row, column, size)
     - Augments captures with lrState and sym from matched tokens
   - Writes augmented entries to `_autogen-table.json`
   - Touches `src/readers/qmd_error_message_table.rs` to trigger rebuild

3. **Runtime Lookup** (`src/readers/qmd_error_message_table.rs`)
   - `include_error_table!()` macro embeds `_autogen-table.json` at compile time
   - `lookup_error_entry(process_message)` searches table for matching (state, sym)
   - Returns `ErrorTableEntry` with title, message, captures, and notes

4. **Error Rendering** (`src/readers/qmd_error_messages.rs`)
   - Takes error state + consumed tokens from tree-sitter log
   - Looks up entry in table
   - If found: Builds rich Ariadne report with:
     - Main error location (red label with error message)
     - Secondary locations from captures (blue labels with notes)
     - Two note types: "simple" (single location) and "label-range" (span between two captures)
   - If not found: Falls back to generic "Parse error / unexpected character or token here"

## Example Error Specification

**File: `001.qmd`**
```markdown
an [unclosed span
```

**File: `001.json`**
```json
{
    "title": "Unclosed Span",
    "message": "I reached the end of the block before finding a closing ']' for the span or link.",
    "captures": [
        {
            "label": "span-start",
            "row": 0,
            "column": 3,
            "size": 1
        }
    ],
    "notes": [
        {
            "message": "This is the opening bracket for the span",
            "label": "span-start",
            "noteType": "simple"
        }
    ]
}
```

**What happens during build:**

1. Script runs: `cargo run -- --_internal-report-error-state -i resources/error-corpus/001.qmd`

2. Parser outputs:
```json
{
  "errorStates": [
    {"state": 1283, "sym": "end", "row": 0, "column": 17}
  ],
  "tokens": [
    {"row": 0, "column": 0, "size": 2, "lrState": 1353, "sym": "_word_no_digit"},
    {"row": 0, "column": 2, "size": 1, "lrState": 859, "sym": "_whitespace_token1"},
    {"row": 0, "column": 3, "size": 1, "lrState": 301, "sym": "["}
    // ... more tokens
  ]
}
```

3. Script matches capture `{row: 0, column: 3, size: 1}` to token `{row: 0, column: 3, size: 1, lrState: 301, sym: "["}`

4. Script augments capture with `lrState: 301` and `sym: "["` from matched token

5. Script writes to `_autogen-table.json`:
```json
{
  "state": 1283,
  "sym": "end",
  "row": 0,
  "column": 17,
  "errorInfo": {
    "title": "Unclosed Span",
    "message": "I reached the end of the block before finding a closing ']' for the span or link.",
    "captures": [
      {
        "label": "span-start",
        "row": 0,
        "column": 3,
        "size": 1,
        "lrState": 301,  // ADDED by script
        "sym": "["       // ADDED by script
      }
    ],
    "notes": [...]
  },
  "name": "001"
}
```

6. At runtime, when parser encounters state=1283 + sym="end", it looks up this entry and produces:
```
Error: Unclosed Span
   ╭─[file.qmd:1:17]
   │
 1 │ an [unclosed span
   │    ┬             ┬
   │    ╰──────────────── This is the opening bracket for the span
   │                  │
   │                  ╰── I reached the end of the block before finding a closing ']' for the span or link.
───╯
```

## Capture Matching Algorithm

**Critical detail**: The script matches captures to consumed tokens using a **left join** on `(row, column, size)`.

This means:
1. The `row`, `column`, `size` in the JSON capture spec must **exactly match** a consumed token
2. The script will augment the capture with `lrState` and `sym` from the matched token
3. At runtime, the system uses `lrState` and `sym` to find the capture in the consumed tokens

**Why this matters**: When writing a new error spec, you must:
1. Run `--_internal-report-error-state` on your example
2. Look at the "tokens" array in the output
3. Choose the exact `(row, column, size)` from a token you want to highlight
4. Use those coordinates in your capture spec

## Note Types

### 1. Simple Note
Points to a single captured location:
```json
{
  "message": "This is the opening bracket",
  "label": "span-start",
  "noteType": "simple"
}
```

Requires one capture with matching label. Renders as a blue label at that location.

### 2. Label-Range Note
Points to a span between two captured locations:
```json
{
  "message": "This key-value pair cannot appear before the class specifier",
  "noteType": "label-range",
  "labelBegin": "key-value-begin",
  "labelEnd": "key-value-end"
}
```

Requires two captures with matching labels. Renders as a blue label spanning from begin to end.

## How to Add a New Error Message

### Step 1: Create the Example

Create `resources/error-corpus/NNN.qmd` (use next available number):
```markdown
Unfinished _emph.
```

### Step 2: Determine Error State

Run the internal error state reporter:
```bash
cargo run -- --_internal-report-error-state -i resources/error-corpus/NNN.qmd
```

Example output:
```json
{
  "errorStates": [
    {"state": 854, "sym": "end", "row": 0, "column": 17}
  ],
  "tokens": [
    {"row": 0, "column": 11, "size": 1, "lrState": 125, "sym": "emphasis_delimiter"},
    {"row": 0, "column": 12, "size": 4, "lrState": 1434, "sym": "_word_no_digit"},
    {"row": 0, "column": 16, "size": 1, "lrState": 854, "sym": "."}
  ]
}
```

### Step 3: Write the Error Spec

Identify which tokens you want to highlight. For emphasis, we want to highlight the opening `_`:

```json
{
    "title": "Unclosed Emphasis",
    "message": "I reached the end of the block before finding a closing '_' for the emphasis.",
    "captures": [
        {
            "label": "emphasis-start",
            "row": 0,
            "column": 11,
            "size": 1
        }
    ],
    "notes": [
        {
            "message": "This is the opening delimiter for the emphasis",
            "label": "emphasis-start",
            "noteType": "simple"
        }
    ]
}
```

Save as `resources/error-corpus/NNN.json`.

### Step 4: Build the Table

```bash
cd crates/quarto-markdown-pandoc
./scripts/build_error_table.ts
```

This will:
- Parse your example
- Match the capture to the consumed token
- Generate the entry in `_autogen-table.json`
- Touch `qmd_error_message_table.rs` to trigger rebuild

### Step 5: Test

```bash
cargo run -- -i resources/error-corpus/NNN.qmd
```

You should now see the rich error message instead of the generic "Parse error".

### Step 6: Verify in Test Suite

Add a test to verify the error message is produced correctly:
```rust
#[test]
fn test_unclosed_emphasis_error() {
    let input = "Unfinished _emph.";
    let result = readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut io::sink());
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].title(), "Unclosed Emphasis");
}
```

## Design Principles

### 1. Minimal Examples
Each error corpus file should demonstrate **exactly one** error. Keep examples as small as possible.

**Good**: `Unfinished _emph.`
**Bad**: A 50-line document with multiple issues

### 2. Clear, Actionable Messages

**Good**:
- Title: "Unclosed Span"
- Message: "I reached the end of the block before finding a closing ']' for the span or link."

**Bad**:
- Title: "Parse error"
- Message: "unexpected"

### 3. Helpful Context

Use captures and notes to highlight:
- Where the problematic construct **began**
- What the parser **expected** but didn't find
- What might help the user **fix** the issue

### 4. Consistency

Look at existing error messages for:
- Tone (first person: "I expected...")
- Terminology (use standard names: "span", "emphasis", "attribute specifier")
- Level of detail (concise but complete)

## Current Examples

| File | Error Type | State | Sym | Title |
|------|-----------|-------|-----|-------|
| 001.qmd | Unclosed span | 1283 | end | Unclosed Span |
| 002.qmd | Bad attribute delimiter | 2020 | _error | Mismatched Delimiter in Attribute Specifier |
| 003.qmd | Attr ordering | 2678 | class_specifier | Key-value Pair Before Class Specifier in Attribute |
| 004.qmd | Missing space in div | 932 | { | Missing Space After Div Fence |

## Missing Error Messages (Opportunities)

These parse errors currently produce generic "Parse error" messages:

1. **Unclosed emphasis** (state: 854, sym: "end")
   - Example: `Unfinished _emph.`
   - Should say: "Unclosed Emphasis"

2. **Unclosed strong emphasis** (state: ?, sym: "end")
   - Example: `Unfinished **strong.`
   - Should say: "Unclosed Strong Emphasis"

3. **Unclosed code span** (state: ?, sym: "end")
   - Example: `` Unclosed `code. ``
   - Should say: "Unclosed Code Span"

4. **Unclosed link** (state: ?, sym: "end")
   - Example: `Unclosed [link](url.`
   - Should say: "Unclosed Link"

5. **Mismatched heading level** (if applicable)
6. **Invalid div fence** (if applicable)
7. **Malformed YAML metadata** (if we can catch specific cases)

## Next Steps

To improve error messages for quarto-web parsing failures:

1. **Identify the failures**: Get a list of parse errors from the quarto-web corpus
2. **Extract error states**: For each failing file, run `--_internal-report-error-state` to get (state, sym)
3. **Create minimal examples**: Write the smallest possible `.qmd` that triggers each (state, sym) pair
4. **Write error specs**: Create `.json` files with good titles, messages, captures, and notes
5. **Build and test**: Run `build_error_table.ts` and verify the new messages appear
6. **Iterate**: Test on the actual quarto-web files to ensure the messages are helpful

## References

- Paper: Clinton Jeffery, "Generating Syntax Errors from Examples", TOPLAS 2003
- Implementation: `scripts/build_error_table.ts`
- Runtime: `src/readers/qmd_error_message_table.rs`, `src/readers/qmd_error_messages.rs`
- Examples: `resources/error-corpus/`
