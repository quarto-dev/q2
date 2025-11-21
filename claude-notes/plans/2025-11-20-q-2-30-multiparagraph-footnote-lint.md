# Plan: Q-2-30 Multi-Paragraph Footnote Linting Diagnostic

**Date**: 2025-11-20
**Issue**: k-367 (case 2)
**Type**: Linting diagnostic (not a parse error)

## Problem Statement

Users may write Pandoc-style multi-paragraph footnotes that **parse successfully** but produce incorrect output:

```markdown
[^1]: First paragraph

    Second paragraph (indented - should be part of footnote)
```

**What happens**:
- Parser produces: `NoteDefinitionPara("First paragraph")` + `Para("Second paragraph...")`
- The second paragraph is **not** part of the footnote (it becomes a separate top-level paragraph)
- User expects Pandoc behavior (indentation continues the footnote)

**Correct qmd syntax**:
```markdown
::: ^1

First paragraph

Second paragraph

:::
```

## Why This Needs a Linting Diagnostic

1. **Document parses successfully** - no parse error occurs
2. **Silent semantic error** - output doesn't match intent
3. **Cannot detect during parsing** - need to analyze the AST
4. **Pattern is detectable** - can identify suspicious structure

## Detection Strategy

### Pattern to Detect

Look for consecutive blocks in the AST:
1. `NoteDefinitionPara` with id X and content
2. Immediately followed by `Para`
3. Where the `Para`'s source text starts with whitespace (indentation)

This pattern suggests the user intended a multi-paragraph footnote using Pandoc's indentation syntax.

### AST Structure (from test case)

```json
{
  "blocks": [
    {"t": "Para", "c": [..., {"t": "Span", "attrS": {"classes": ["quarto-note-reference"], ...}}]},
    {"t": "NoteDefinitionPara", "c": ["1", [{"t": "Str", "c": "First"}, ...]]},
    {"t": "Para", "c": [{"t": "Str", "c": "Second"}, ...]},  // <- This is the problem
    {"t": "Para", "c": [...]}
  ]
}
```

### Source Text Analysis

Using the `ASTContext` and `SourceInfo`, we can:
1. Get the source offset/range for the suspicious `Para` block
2. Extract the source text from the file
3. Check if it starts with whitespace characters

**Example**:
- Para source range: bytes 59-90
- Source text: `"    Second paragraph (this is bad)"`
- Starts with whitespace: YES → Report violation

## Implementation Plan

### Phase 1: Add Q-2-30 to Error Catalog

**File**: `crates/quarto-error-reporting/error_catalog.json`

Add entry:
```json
{
  "Q-2-30": {
    "subsystem": "markdown",
    "title": "Multi-Paragraph Footnote Indentation Not Supported",
    "message_template": "Indented paragraph following footnote definition suggests Pandoc multi-paragraph footnote syntax, which is not supported in quarto-markdown.",
    "docs_url": "https://quarto.org/docs/errors/Q-2-30",
    "since_version": "99.9.9"
  }
}
```

**Testing**: `cargo build -p quarto-error-reporting` should succeed

### Phase 2: Create Q-2-30 Linting Rule

**File**: `crates/qmd-syntax-helper/src/diagnostics/q_2_30.rs`

**Structure**:
```rust
// Q-2-30: Multi-Paragraph Footnote Indentation
//
// Detects when a paragraph immediately follows a NoteDefinitionPara
// and starts with indentation, suggesting an attempted multi-paragraph
// footnote using Pandoc's indentation syntax.
//
// This is a LINTING diagnostic - the document parses successfully but
// likely has a semantic error.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::rule::{CheckResult, ConvertResult, Rule, SourceLocation};

pub struct Q230Checker {}

#[derive(Debug, Clone)]
struct Q230Violation {
    note_id: String,
    para_offset: usize,  // Location of the suspicious Para
    row: usize,
    column: usize,
}

impl Q230Checker {
    pub fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Parse document and detect multi-paragraph footnote pattern
    fn get_violations(&self, file_path: &Path) -> Result<Vec<Q230Violation>> {
        let content = fs::read_to_string(file_path)?;

        // Parse with quarto-markdown-pandoc to get AST
        let mut sink = std::io::sink();
        let filename = file_path.to_string_lossy();

        let (pandoc_doc, ast_context, _diagnostics) =
            quarto_markdown_pandoc::readers::qmd::read(
                content.as_bytes(),
                false,
                &filename,
                &mut sink,
                true,
                None,
            )?;  // Note: Using ? here because we need successful parse

        let mut violations = Vec::new();
        let blocks = &pandoc_doc.blocks;

        // Walk through consecutive block pairs
        for i in 0..blocks.len().saturating_sub(1) {
            let current = &blocks[i];
            let next = &blocks[i + 1];

            // Check if current is NoteDefinitionPara
            if let pandoc::Block::NoteDefinitionPara(note_id, _content, _source_info) = current {
                // Check if next is Para
                if let pandoc::Block::Para(_inlines, para_source_info) = next {
                    // Check if Para's source starts with whitespace
                    if self.para_starts_with_indent(&content, &ast_context, para_source_info)? {
                        let offset = para_source_info.start_offset();
                        violations.push(Q230Violation {
                            note_id: note_id.clone(),
                            para_offset: offset,
                            row: self.offset_to_row(&content, offset),
                            column: self.offset_to_column(&content, offset),
                        });
                    }
                }
            }
        }

        Ok(violations)
    }

    /// Check if a Para block's source text starts with whitespace
    fn para_starts_with_indent(
        &self,
        content: &str,
        ast_context: &quarto_markdown_pandoc::ASTContext,
        source_info: &quarto_source_map::SourceInfo,
    ) -> Result<bool> {
        let start = source_info.start_offset();
        let end = source_info.end_offset();

        if start >= content.len() || end > content.len() {
            return Ok(false);
        }

        let para_text = &content[start..end];

        // Check if starts with space or tab
        Ok(para_text.starts_with(' ') || para_text.starts_with('\t'))
    }

    fn offset_to_row(&self, content: &str, offset: usize) -> usize {
        content[..offset].matches('\n').count()
    }

    fn offset_to_column(&self, content: &str, offset: usize) -> usize {
        let line_start = content[..offset]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        offset - line_start
    }
}

impl Rule for Q230Checker {
    fn name(&self) -> &str {
        "q-2-30"
    }

    fn description(&self) -> &str {
        "Detect multi-paragraph footnotes using Pandoc indentation syntax"
    }

    fn check(&self, file_path: &Path, _verbose: bool) -> Result<Vec<CheckResult>> {
        // If file doesn't parse, return empty (let parse rule handle it)
        let violations = match self.get_violations(file_path) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };

        let results: Vec<CheckResult> = violations
            .into_iter()
            .map(|v| CheckResult {
                rule_name: self.name().to_string(),
                file_path: file_path.to_string_lossy().to_string(),
                has_issue: true,
                issue_count: 1,
                message: Some(format!(
                    "Q-2-30: Indented paragraph after footnote [^{}] suggests multi-paragraph footnote",
                    v.note_id
                )),
                location: Some(SourceLocation {
                    row: v.row,
                    column: v.column,
                }),
                error_code: Some("Q-2-30".to_string()),
                error_codes: None,
            })
            .collect();

        Ok(results)
    }

    fn convert(
        &self,
        file_path: &Path,
        _in_place: bool,
        _check_mode: bool,
        _verbose: bool,
    ) -> Result<ConvertResult> {
        // This is a linting diagnostic - no auto-fix available
        // Requires manual conversion to div syntax
        Ok(ConvertResult {
            rule_name: self.name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            fixes_applied: 0,
            message: Some(
                "Q-2-30 violations cannot be automatically fixed. \
                 Manual conversion to div syntax required: ::: ^ref ... :::".to_string()
            ),
        })
    }
}
```

**Key Design Decisions**:

1. **Parse must succeed**: Use `?` on `read()` result - if parse fails, return empty violations list (parse errors handled by parse rule)

2. **Consecutive block iteration**: Use `for i in 0..blocks.len().saturating_sub(1)` to safely iterate pairs

3. **Source text checking**: Extract source text using SourceInfo offsets and check `starts_with(' ')` or `starts_with('\t')`

4. **No auto-fix**: Convert returns 0 fixes - this requires manual intervention

### Phase 3: Register the Rule

**File**: `crates/qmd-syntax-helper/src/diagnostics/mod.rs`

```rust
pub mod parse_check;
pub mod q_2_30;  // Add this line
```

**File**: `crates/qmd-syntax-helper/src/rule.rs` (in `RuleRegistry::new()`)

After line 80:
```rust
        registry.register(Arc::new(
            crate::diagnostics::q_2_30::Q230Checker::new()?,
        ));
```

### Phase 4: Testing

**Test file**: `test-footnote-case2.qmd`
```markdown
Some text with a footnote[^1].

[^1]: First paragraph

    Second paragraph (this is bad)

More text here.
```

**Test commands**:
```bash
# Check for Q-2-30 violations
cargo run --bin qmd-syntax-helper -- check --rule q-2-30 test-footnote-case2.qmd

# Expected output:
# ✗ q-2-30: test-footnote-case2.qmd
#   Q-2-30: Indented paragraph after footnote [^1] suggests multi-paragraph footnote
#   Location: row 4, column 4

# Verify no auto-fix
cargo run --bin qmd-syntax-helper -- convert --rule q-2-30 test-footnote-case2.qmd

# Expected output:
# Q-2-30 violations cannot be automatically fixed.
# Manual conversion to div syntax required: ::: ^ref ... :::
```

## Challenges and Solutions

### Challenge 1: Parse Must Succeed

**Problem**: The `read()` function returns `Result<..., DiagnosticMessages>`. If parse fails, we get `Err(...)`.

**Solution**: Use `?` operator on `read()` - if parse fails, return empty violations. The `parse` rule will handle parse errors.

### Challenge 2: Accessing Source Text

**Problem**: Need to check if Para starts with whitespace, but only have SourceInfo.

**Solution**:
1. Use `source_info.start_offset()` and `end_offset()`
2. Extract substring from file content
3. Check `starts_with(' ')` or `starts_with('\t')`

### Challenge 3: Pandoc AST Type Access

**Problem**: Need to pattern match on `Block` variants and extract fields.

**Solution**: Use standard Rust pattern matching:
```rust
if let pandoc::Block::NoteDefinitionPara(note_id, _content, _si) = current {
    if let pandoc::Block::Para(_inlines, para_si) = next {
        // Check for violation
    }
}
```

### Challenge 4: False Positives

**Problem**: A legitimately indented paragraph after a footnote (different context).

**Mitigation**:
- Check for **immediately following** Para only
- Document that this is a linting hint, not a hard error
- User can verify and ignore if false positive

## Alternative Approaches Considered

### 1. Parse-time detection
**Rejected**: Document parses successfully, can't inject error at parse time

### 2. Writer-time diagnostic
**Rejected**: Too late - user needs feedback during editing, not at render time

### 3. Tree-sitter pattern matching
**Rejected**: Tree-sitter AST doesn't distinguish NoteDefinitionPara from Para - need semantic Pandoc AST

## Success Criteria

1. ✅ Q-2-30 added to error catalog
2. ✅ `qmd-syntax-helper check --rule q-2-30` detects pattern
3. ✅ Location information is accurate (row/column)
4. ✅ No false positives on valid qmd files
5. ✅ Helpful message guides user to div syntax
6. ✅ Convert command explains manual fix needed

## Future Enhancements

1. **Auto-fix capability**: Parse footnote content, generate div syntax (complex)
2. **Multiple paragraphs**: Detect 3+ indented paragraphs in sequence
3. **Code blocks in footnotes**: Handle indented code blocks (4+ spaces)
4. **Integration**: Add to qmd-syntax-helper default rule set

## Estimated Complexity

- **Error catalog update**: 5 minutes
- **Core implementation**: 2-3 hours
  - Skeleton: 30 min
  - AST walking logic: 1 hour
  - Source text extraction: 30 min
  - Testing/debugging: 1 hour
- **Registration**: 10 minutes
- **Testing**: 30 minutes
- **Documentation**: 30 minutes

**Total**: ~4 hours

## Dependencies

- `quarto-error-reporting` crate (already exists)
- `quarto-markdown-pandoc` crate (for AST access)
- `quarto-source-map` crate (for SourceInfo)
- `pandoc` crate (AST types)

## References

- Case 1 implementation: Q-2-29 (indented footnote with colon)
- Similar pattern: Q-2-7 converter (iterates diagnostics)
- Parse error handling: `parse_check.rs`
- Error catalog: `quarto-error-reporting/error_catalog.json`
