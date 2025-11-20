# Q-2-7 Converter Implementation Design

Date: 2025-11-20
File: claude-notes/plans/2025-11-20-q-2-7-converter-design.md

## Problem Analysis

### Q-2-7: Unclosed Single Quote

**Error**: Straight apostrophes (`'`) followed by Markdown syntax are misinterpreted as opening quote marks instead of apostrophes.

**Examples**:
- `d'`Arrow`` - apostrophe before code span
- `qu'**on**` - apostrophe before emphasis
- `l'[link](...)` - apostrophe before link

**Why it happens**: The parser correctly handles standalone apostrophes like `d'Avignon`, but when the apostrophe is immediately followed by Markdown punctuation (backticks, brackets, asterisks), it gets confused and treats the `'` as a quote delimiter.

## Current Error Corpus

File: `crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-7.json`

```json
{
  "code": "Q-2-7",
  "title": "Unclosed Single Quote",
  "message": "I reached the end of the block before finding a closing \"'\" for the quote.",
  "notes": [
    {
      "message": "This is the opening quote. If you need an apostrophe, escape it with a backslash.",
      "label": "quote-start",
      "noteType": "simple",
      "trimLeadingSpace": true
    }
  ],
  "cases": [
    {
      "name": "simple",
      "description": "Simple case",
      "content": "'Tis the season to make apostrophe mistakes.",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 0,
          "size": 1
        }
      ]
    },
    {
      "name": "in-link",
      "description": "Inside link",
      "content": "[`a`'s](b)",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 4,
          "size": 1
        }
      ]
    }
  ]
}
```

**Key observations**:
- The "quote-start" capture already exists and marks the apostrophe location
- Two test cases exist: "simple" and "in-link"
- The error corpus is sufficient for implementation - NO changes needed

## Diagnostic Structure

When parsing a file with Q-2-7 error:

```json
{
  "code": "Q-2-7",
  "title": "Unclosed Single Quote",
  "location": {
    "Original": {
      "start_offset": 448,
      "end_offset": 449,
      "file_id": 0
    }
  },
  "details": [
    {
      "kind": "info",
      "content": {
        "type": "markdown",
        "content": "This is the opening quote. If you need an apostrophe, escape it with a backslash."
      },
      "location": {
        "Original": {
          "start_offset": 161,
          "end_offset": 162,
          "file_id": 0
        }
      }
    }
  ],
  "problem": {
    "type": "markdown",
    "content": "I reached the end of the block before finding a closing \"'\" for the quote."
  }
}
```

**Critical fields**:
- `diagnostic.code`: "Q-2-7"
- `diagnostic.location`: End of block (where closing `'` was expected)
- `diagnostic.details[0].location`: **The apostrophe position** (what we need to fix!)

## Implementation Strategy

### Comparison with Q-2-13 and Q-2-10

**Q-2-13** (Unclosed Strong Star Emphasis):
- Uses: `diagnostic.location` (end of block)
- Action: Insert `**` at end of block
- Pattern: Add missing closing delimiter

**Q-2-10** (Closed Quote Without Matching Open Quote):
- Uses: `diagnostic.location` (space after apostrophe)
- Action: Insert `\` before apostrophe (at `offset - 1`)
- Pattern: Escape the character

**Q-2-7** (Unclosed Single Quote) - **NEW**:
- Uses: `diagnostic.details[0].location` (the apostrophe itself)
- Action: Insert `\` before the apostrophe
- Pattern: Escape character (similar to Q-2-10)

### Why Escape Instead of Replace?

From the error corpus notes: "If you need an apostrophe, escape it with a backslash."

**Two possible fixes**:
1. Escape: `d'` → `d\'` (what we will implement)
2. Replace: `d'` → `d'` (curly apostrophe - NOT chosen)

**We choose Escape because**:
- Keeps the source as pure ASCII - no non-ASCII Unicode introduced by tooling
- Curly apostrophes have different Pandoc AST representation
- Escaping is semantically clear: "this is a literal apostrophe, not a quote"
- Users can add curly apostrophes themselves if desired
- Avoids downstream typographical consequences

## Implementation Plan

### File: `crates/qmd-syntax-helper/src/conversions/q_2_7.rs`

```rust
// Q-2-7: Unclosed Single Quote
//
// This conversion rule fixes Q-2-7 errors by escaping straight apostrophes
// that are misinterpreted as opening quotes.
//
// The parser misinterprets straight apostrophes when they appear before
// Markdown syntax (e.g., `d'`code``, `qu'**emphasis**`).
//
// Fix strategy: Escape the apostrophe with a backslash `'` → `\'`
//
// Error catalog entry: crates/quarto-markdown-pandoc/resources/error-corpus/Q-2-7.json
// Error code: Q-2-7
// Title: "Unclosed Single Quote"
//
// Example:
//   Input:  d'`Arrow`
//   Output: d\'`Arrow`

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};
use crate::utils::file_io::read_file;

pub struct Q27Converter {}

#[derive(Debug, Clone)]
struct Q27Violation {
    offset: usize,                          // Offset of the apostrophe to replace
    error_location: Option<SourceLocation>, // For reporting
}

impl Q27Converter {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Get parse errors and extract Q-2-7 unclosed single quote violations
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q27Violation>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

        // Parse with quarto-markdown-pandoc to get diagnostics
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let result = quarto_markdown_pandoc::readers::qmd::read(
            content.as_bytes(),
            false, // not loose mode
            &filename,
            &mut sink,
            true,
            None,
        );

        let diagnostics = match result {
            Ok(_) => return Ok(Vec::new()), // No errors
            Err(diagnostics) => diagnostics,
        };

        let mut violations = Vec::new();

        for diagnostic in diagnostics {
            // Check if this is a Q-2-7 error
            if diagnostic.code.as_deref() != Some("Q-2-7") {
                continue;
            }

            // CRITICAL: For Q-2-7, we need the apostrophe location from details[0]
            // NOT the main diagnostic location (which points to end of block)
            if diagnostic.details.is_empty() {
                continue;
            }

            let detail_location = diagnostic.details[0].location.as_ref();
            if detail_location.is_none() {
                continue;
            }

            let offset = detail_location.unwrap().start_offset();

            violations.push(Q27Violation {
                offset,
                error_location: Some(SourceLocation {
                    row: self.offset_to_row(&content, offset),
                    column: self.offset_to_column(&content, offset),
                }),
            });
        }

        Ok(violations)
    }

    /// Apply fixes by inserting backslashes before apostrophes
    fn apply_fixes(&self, content: &str, mut violations: Vec<Q27Violation>) -> Result<String> {
        if violations.is_empty() {
            return Ok(content.to_string());
        }

        // Sort violations in reverse order to avoid offset invalidation
        violations.sort_by_key(|v| std::cmp::Reverse(v.offset));

        let mut result = content.to_string();

        for violation in violations {
            // Insert backslash before the apostrophe
            // The offset points to the apostrophe itself
            result.insert(violation.offset, '\\');
        }

        Ok(result)
    }

    /// Convert byte offset to row number (0-indexed)
    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    /// Convert byte offset to column number (0-indexed)
    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        offset - line_start
    }
}

impl Rule for Q27Converter {
    fn name(&self) -> &str {
        "q-2-7"
    }

    fn description(&self) -> &str {
        "Fix Q-2-7: Escape apostrophes misinterpreted as opening quotes"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        let violations = self.get_violations(file_path)?;

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-7 unclosed single quote at offset {}",
                    v.offset
                )),
                location: v.error_location,
                error_code: Some("Q-2-7".to_string()),
                error_codes: None,
            })
            .collect();

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        in_place: bool,
        check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        let content = read_file(file_path)?;
        let violations = self.get_violations(file_path)?;

        if violations.is_empty() {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: 0,
                message: Some("No Q-2-7 unclosed single quote issues found".to_string()),
            });
        }

        let fixed_content = self.apply_fixes(&content, violations.clone())?;

        if check_mode {
            return Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Would fix {} Q-2-7 unclosed single quote violation(s)",
                    violations.len()
                )),
            });
        }

        if in_place {
            crate::utils::file_io::write_file(file_path, &fixed_content)?;
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(format!(
                    "Fixed {} Q-2-7 unclosed single quote violation(s)",
                    violations.len()
                )),
            })
        } else {
            Ok(ConvertResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                fixes_applied: violations.len(),
                message: Some(fixed_content),
            })
        }
    }
}
```

### Integration Steps

1. **Create the converter file**:
   - File: `crates/qmd-syntax-helper/src/conversions/q_2_7.rs`
   - Pattern: Follow q_2_13.rs structure exactly
   - Key difference: Use `diagnostic.details[0].location` instead of `diagnostic.location`

2. **Register in mod.rs**:
   ```rust
   // File: crates/qmd-syntax-helper/src/conversions/mod.rs

   pub mod q_2_7;  // Add this line

   // In the conversions list:
   Box::new(q_2_7::Q27Converter::new()?),
   ```

3. **Create test file**:
   ```rust
   // File: crates/qmd-syntax-helper/tests/q_2_7_test.rs

   use qmd_syntax_helper::rule::Rule;
   use qmd_syntax_helper::conversions::q_2_7::Q27Converter;
   use std::path::Path;

   #[test]
   fn test_q_2_7_simple() {
       let converter = Q27Converter::new().unwrap();
       let test_file = Path::new("tests/fixtures/q_2_7_simple.qmd");

       // Create test fixture with: d'`Arrow`

       let result = converter.convert(test_file, false, false, false).unwrap();
       assert_eq!(result.fixes_applied, 1);

       // Verify output contains: d\'`Arrow`
   }

   #[test]
   fn test_q_2_7_multiple() {
       let converter = Q27Converter::new().unwrap();
       let test_file = Path::new("tests/fixtures/q_2_7_multiple.qmd");

       // Create test fixture with multiple apostrophes:
       // d'`Arrow` and qu'**on**

       let result = converter.convert(test_file, false, false, false).unwrap();
       assert_eq!(result.fixes_applied, 2);
   }
   ```

4. **Add test fixtures**:
   - `tests/fixtures/q_2_7_simple.qmd`: Single apostrophe before backtick
   - `tests/fixtures/q_2_7_multiple.qmd`: Multiple apostrophes in different contexts

## Testing Strategy

### Unit Tests

1. **Simple case**: `'Tis` (from corpus)
2. **In link**: `[`a`'s](b)` (from corpus)
3. **French text**: `d'`Arrow``
4. **Multiple in one line**: `qu'**on**` and `d'[link](...)`
5. **Edge cases**:
   - Apostrophe at end of line
   - Multiple apostrophes in same file
   - Mixed with valid quotes

### Integration Test

Run on actual lino-galiana corpus files:
```bash
cargo run --bin qmd-syntax-helper -- convert -r q-2-7 --check -v \
  external-sites/lino-galiana/**/*.qmd
```

Expected: Should identify all 19 Q-2-7 violations

### Validation Test

After fix, re-parse:
```bash
cargo run --bin quarto-markdown-pandoc -- -i fixed-file.qmd
```

Expected: No Q-2-7 errors

## Key Differences from Similar Converters

| Aspect | Q-2-13 | Q-2-10 | **Q-2-7 (new)** |
|--------|--------|--------|------------------|
| Location source | `diagnostic.location` | `diagnostic.location` | **`diagnostic.details[0].location`** |
| Operation | Insert `**` | Insert `\` at offset-1 | **Insert `\` at offset** |
| Offset adjustment | None (end of block) | -1 (before char) | **None (before char)** |
| Character handling | String insertion | String insertion | **String insertion** |

## Error Corpus Status

**NO changes needed to Q-2-7.json**:
- ✅ "quote-start" capture exists
- ✅ Multiple test cases exist
- ✅ Diagnostic already provides `details[0].location`

The error corpus is complete and ready for use.

## Implementation Checklist

- [ ] Create `crates/qmd-syntax-helper/src/conversions/q_2_7.rs`
- [ ] Add module to `crates/qmd-syntax-helper/src/conversions/mod.rs`
- [ ] Register converter in conversions list
- [ ] Create test file `crates/qmd-syntax-helper/tests/q_2_7_test.rs`
- [ ] Create test fixtures in `tests/fixtures/`
- [ ] Run `cargo test` - verify all tests pass
- [ ] Test on lino-galiana corpus with `--check` mode
- [ ] Apply fixes with `--in-place` if tests pass
- [ ] Re-parse fixed files to verify Q-2-7 errors are gone
- [ ] Run full test suite: `cargo test`
- [ ] Format code: `cargo fmt`

## Success Criteria

1. All unit tests pass
2. Tool detects all 19 Q-2-7 errors in lino-galiana corpus
3. After applying fixes, files parse cleanly (no Q-2-7 errors)
4. No regressions in other tests
5. Code follows existing patterns (q_2_13.rs, q_2_11.rs)

## Notes

- This is the first converter that uses `diagnostic.details[].location` instead of `diagnostic.location`
- Very similar to Q-2-10, but uses the detail location (apostrophe position) instead of main location (space after)
- Unlike Q-2-10 which uses `offset - 1`, Q-2-7 uses the offset directly (it already points to the apostrophe)
- Simple backslash insertion - keeps source as ASCII
