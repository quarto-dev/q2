# Grammar Changes Checklist

This document provides guidelines for making changes to Quarto Markdown syntax.

## 🚨 CRITICAL QUESTION

Before making any grammar change, ask yourself:

**"What existing valid QMD documents might this change break?"**

If you can't confidently answer "none", you need to:
1. Create test cases for those documents
2. Run the parser on them before and after your change
3. Verify they still parse correctly

## When to Use This Checklist

Use this checklist whenever you're adding new syntax to Quarto Markdown, either by tree-sitter changes or changes in the Pandoc AST
processing steps.

## Pre-Implementation Phase

### 1. Plan Your Test Cases

Before writing any code, plan tests for:

#### Positive Cases (Feature Works)
- [ ] Basic valid example
- [ ] Valid example with edge cases (empty, very long, special characters, etc.)
- [ ] Valid example in different contexts (nested, at start/end of document, etc.)

#### Negative Cases (Similar But Different)
- [ ] Syntax that looks similar but shouldn't be treated as this feature
- [ ] Old/deprecated syntax that should be handled differently

#### Edge Cases (Feature in Unexpected Contexts)
- [ ] Feature appears where it's not expected (e.g., caption without preceding table)
- [ ] Feature appears between other constructs
- [ ] Feature appears at document boundaries
- [ ] Feature appears nested in other constructs

#### Compatibility Cases (Doesn't Break Existing Valid Syntax)
- [ ] **CRITICAL:** Documents that look like they might conflict with new syntax
- [ ] Documents with similar punctuation patterns
- [ ] Documents with similar structure patterns

#### Roundtrip Cases
- [ ] At least one roundtrip test: QMD → JSON → QMD → JSON
- [ ] Verify the feature survives roundtripping intact

### 2. Write and Run Failing Tests FIRST
- [ ] Write test cases in appropriate test directories
- [ ] Run tests and verify they fail in the expected way
- [ ] If tests don't fail as expected, revise your understanding

## Implementation Phase

Use your best judgment to implement changes, studying the source code and similar features.
Ask clarification questions when you conclude that you don't know enough to go on.

### 3. Update Writers If Needed

- [ ] QMD writer (`src/writers/qmd.rs`) - for roundtripping
- [ ] JSON writer (`src/writers/json.rs`) - if adding new node types
- [ ] Native writer (`src/writers/native.rs`) - usually auto-handled
- [ ] Look in the writers/ directory to find others

## Testing Phase

### 4. Run All Tests

- [ ] `cargo check` - verify code compiles
- [ ] `cargo test` - verify all tests pass
- [ ] **ALL tests must pass** - don't ignore failures even if they seem unrelated

If you can't write the feature so that your tests pass, do not erase the tests.
Stop and report them to the user.

### 9. Verify Test Coverage

- [ ] All planned test cases are implemented
- [ ] Tests are in the appropriate directories:
  - `tests/snapshots/native/` - for native format output tests
  - `tests/roundtrip_tests/qmd-json-qmd/` - for roundtrip tests
  - `tests/pandoc-match-corpus/markdown/` - for Pandoc compatibility tests
  - `tests/smoke/` - for basic smoke tests

### 10. Manual Testing

- [ ] Test with `-v` flag to see tree-sitter output
- [ ] Test edge cases interactively
- [ ] If something unexpected happens, add a test for it

## Quick Reference: Test File Locations

- **Snapshot tests (native format):** `tests/snapshots/native/*.qmd` + `.qmd.snapshot`
- **Snapshot tests (JSON format):** `tests/snapshots/json/*.qmd` + `.qmd.snapshot`
- **Snapshot tests (QMD format):** `tests/snapshots/qmd/*.qmd` + `.qmd.snapshot`
- **Roundtrip tests:** `tests/roundtrip_tests/qmd-json-qmd/*.qmd`
- **Pandoc compatibility:** `tests/pandoc-match-corpus/markdown/*.qmd`
- **Smoke tests:** `tests/smoke/*.qmd`

## Remember

The goal is not just to make the feature work, but to make it work **without breaking anything else**.

When in doubt, ask the user for guidance, especially about:
- Whether there are existing QMD documents to test against
- How edge cases should be handled
- Whether a grammar-based or postprocessing-based approach is preferred
