# Investigation Report: qmd-syntax-helper Test Failures

**Date**: 2025-11-10
**Branch**: bugfix/92
**Issue**: crates/qmd-syntax-helper test suite failing after recent commits

## Summary

The qmd-syntax-helper test suite is failing because the tree-sitter grammar changes in commit b380be2 modified the parser's LR state numbers, causing the error message lookup table to become stale. The error table needs to be regenerated using `scripts/build_error_table.ts`.

## Test Failures

4 out of 6 tests in `tests/attribute_ordering_test.rs` are failing:
- `test_converts_single_violation` - expected 2 violations, found 0
- `test_converts_multiple_violations` - expected 3 violations, found 0
- `test_in_place_conversion` - expected 2 violations, found 0
- `test_check_mode` - expected 2 violations, found 0

All failures show the same pattern: the tool is finding 0 violations when it should find 2-3 violations of attribute ordering rules.

## Root Cause Analysis

### The Mechanism

1. **Error Detection System**: The system uses Clinton Jeffery's "Generating Syntax Errors from Examples" approach:
   - Error examples are stored in `crates/quarto-markdown-pandoc/resources/error-corpus/*.{qmd,json}`
   - Each example is parsed to capture the parser's LR state and lookahead symbol at the error point
   - This creates a mapping: `(state, symbol) -> specific error message`
   - The mapping is stored in `_autogen-table.json`
   - At runtime, when a parse error occurs, the parser state and symbol are looked up in this table

2. **The Lookup Code**: In `crates/quarto-markdown-pandoc/src/readers/qmd_error_message_table.rs`:
   ```rust
   pub fn lookup_error_entry(process_message: &ProcessMessage) -> Option<&'static ErrorTableEntry> {
       let table = get_error_table();
       for entry in table {
           if entry.state == process_message.state && entry.sym == process_message.sym {
               return Some(entry);
           }
       }
       None
   }
   ```

3. **The Attribute Ordering Checker**: In `crates/qmd-syntax-helper/src/conversions/attribute_ordering.rs`:
   ```rust
   // Line 62
   if diagnostic.title != "Key-value Pair Before Class Specifier in Attribute" {
       continue;
   }
   ```
   This code specifically looks for the Q-2-3 error diagnostic title.

### What Went Wrong

**Before (upstream/main)**:
- Input: `[span]{key=value .class #id}`
- Parser state at error: **2404**
- Lookahead symbol: `shortcode_naked_string_token1`
- Error table entry: `(state=2404, sym=shortcode_naked_string_token1) -> Q-2-3 "Key-value Pair Before Class Specifier in Attribute"`
- Result: Specific Q-2-3 error generated ✓

**After (bugfix/92)**:
- Input: `[span]{key=value .class #id}` (same)
- Parser state at error: **2600** (changed!)
- Lookahead symbol: `shortcode_naked_string_token1` (same)
- Error table lookup: `(state=2600, sym=shortcode_naked_string_token1)` -> NOT FOUND
- Fallback: Generic "Parse error" generated ✗
- Attribute ordering checker: Sees "Parse error" instead of "Key-value Pair Before Class Specifier in Attribute", skips the violation
- Result: 0 violations found when there should be 2

### Why the State Changed

Commit b380be2 ("fix a variety of bugs, including #92") modified the tree-sitter grammar:
- Changed `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Regenerated `parser.c` (214,854 line changes)
- Modified scanner logic in `scanner.c` (351 line changes)

These grammar changes caused tree-sitter to assign different LR state numbers to the same parsing situations. This is a normal consequence of grammar changes - LR state numbers are implementation details that can change with any grammar modification.

## Verification Experiments

### Experiment 1: Parser Output Comparison

**On upstream/main**:
```bash
$ cargo run --bin quarto-markdown-pandoc -- -i test_diagnostic.qmd --json-errors
{"code":"Q-2-3","title":"Key-value Pair Before Class Specifier in Attribute",...}
```

**On bugfix/92**:
```bash
$ cargo run --bin quarto-markdown-pandoc -- -i test_diagnostic.qmd --json-errors
{"kind":"error","title":"Parse error","problem":{"content":"unexpected character or token here",...}
```

### Experiment 2: Parser State Analysis

**On bugfix/92 with verbose output**:
```
process version:0, version_count:1, state:2600, row:0, col:17
lex_external state:65, row:0, column:17
lexed_lookahead sym:shortcode_naked_string_token1
detect_error lookahead:shortcode_naked_string_token1
```
State is **2600**, not 2404 as expected by the error table.

### Experiment 3: Error Table Content

From `_autogen-table.json`:
```json
{
  "column": 20,
  "row": 0,
  "state": 2404,  // <- Looking for this state
  "sym": "shortcode_naked_string_token1",
  "errorInfo": {
    "code": "Q-2-3",
    "title": "Key-value Pair Before Class Specifier in Attribute",
    ...
  }
}
```
The table was not updated after the grammar changes, so it still references the old state number.

## Solution

The error table needs to be regenerated to reflect the new parser state numbers. According to the CLAUDE.md documentation in `crates/quarto-markdown-pandoc`:

> After changing any of the resources/error-corpus/*.{json,qmd} files, run the script `scripts/build_error_table.ts`. It's executable with a deno hashbang line.

The same process must be run whenever the grammar changes, even if the error corpus files themselves haven't changed.

## Proposed Fix Plan

1. **Regenerate the error table**:
   ```bash
   ./scripts/build_error_table.ts
   ```
   This will:
   - Parse each example in `resources/error-corpus/*.qmd` using the current parser
   - Capture the new state numbers and symbols at error points
   - Regenerate `resources/error-corpus/_autogen-table.json`

2. **Verify the fix**:
   ```bash
   # Test parser output
   cargo run --bin quarto-markdown-pandoc -- -i test_diagnostic.qmd --json-errors

   # Should now see Q-2-3 error again

   # Test qmd-syntax-helper
   cd crates/qmd-syntax-helper
   cargo test

   # All 6 tests should pass
   ```

3. **Add to commit**:
   ```bash
   git add crates/quarto-markdown-pandoc/resources/error-corpus/_autogen-table.json
   git commit --amend -m "fix a variety of bugs, including #92

   Also regenerate error table after grammar changes"
   ```

## Prevention for Future

Whenever the tree-sitter grammar is modified (`grammar.js`, `scanner.c`), the error table MUST be regenerated as part of the same commit. This should be documented in the grammar maintenance workflow.

Consider adding a CI check or pre-commit hook to detect when the parser has changed but the error table hasn't been updated.

## Files Involved

### Modified by commit b380be2:
- `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` - Grammar definition
- `crates/tree-sitter-qmd/tree-sitter-markdown/src/parser.c` - Auto-generated parser (214,854 lines changed)
- `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` - Scanner logic (351 lines changed)

### Needs updating:
- `crates/quarto-markdown-pandoc/resources/error-corpus/_autogen-table.json` - Error lookup table

### Test files failing:
- `crates/qmd-syntax-helper/tests/attribute_ordering_test.rs`

### Code that depends on error table:
- `crates/quarto-markdown-pandoc/src/readers/qmd_error_message_table.rs` - Lookup logic
- `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs` - Error message generation
- `crates/qmd-syntax-helper/src/conversions/attribute_ordering.rs` - Checks for specific error title

## Additional Notes

- The error corpus examples themselves (003.qmd, 003.json) are still correct and don't need changes
- The error catalog (`crates/quarto-error-reporting/error_catalog.json`) still has the Q-2-3 entry and is correct
- No changes are needed to the qmd-syntax-helper code itself - it's correctly checking for the diagnostic title
- The system design is sound; this is purely a matter of keeping the generated error table in sync with the grammar
