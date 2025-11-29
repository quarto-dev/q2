# quarto-citeproc Development Guide

This document serves as a workflow guide for LLM coding sessions working on
quarto-citeproc, a Rust implementation of CSL (Citation Style Language) processing.

## Goal

**Feature parity with Pandoc's citeproc, not full CSL conformance.**

The reference implementation is `external-sources/citeproc` (Pandoc's Haskell citeproc).
Do NOT consult citeproc-js via web searches. Pandoc's citeproc is the authoritative
reference for this project.

When the CSL test suite and Pandoc's citeproc disagree, we follow Pandoc's behavior.
Tests that Pandoc also fails are candidates for our `deferred_tests.txt`.

## Quick Start for New Sessions

### 1. Check Current Status

```bash
# View current test counts (authoritative source)
head -10 crates/quarto-citeproc/tests/csl_conformance.lock

# Run all tests - these should pass
cargo nextest run -p quarto-citeproc

# Run just the validation test to see counts
cargo nextest run -p quarto-citeproc csl_validate_manifest
```

### 2. Find Work

```bash
# Check beads for assigned tasks
bd ready

# Or explore failing tests by category
cargo nextest run -p quarto-citeproc -- --include-ignored 2>&1 | grep FAIL | head -20
```

Before working on a failing test, check if it's in `tests/deferred_tests.txt` -
these are intentionally skipped.

### 3. Run a Specific Test

```bash
# Run an enabled test
cargo nextest run -p quarto-citeproc csl_name_westersimple

# Run an ignored (disabled) test
cargo nextest run -p quarto-citeproc csl_name_sometest -- --include-ignored

# Run all tests in a category
cargo nextest run -p quarto-citeproc csl_name_ -- --include-ignored
```

### 4. Read a Test File

Test files are in `crates/quarto-citeproc/test-data/csl-suite/`. Format:

```
>>===== MODE =====>>
citation                          # or "bibliography"
<<===== MODE =====<<

>>===== RESULT =====>>
Expected output here
<<===== RESULT =====<<

>>===== CSL =====>>
<?xml version="1.0" encoding="utf-8"?>
<style>...</style>
<<===== CSL =====<<

>>===== INPUT =====>>
[{"id": "ITEM-1", "type": "book", "title": "..."}]
<<===== INPUT =====<<

>>===== CITATION-ITEMS =====>>     # optional
[[{"id": "ITEM-1"}]]
<<===== CITATION-ITEMS =====<<
```

## Codebase Architecture

### Crate Structure

```
crates/quarto-csl/           # CSL style parser
├── src/types.rs             # CSL data types (Style, Layout, Names, Formatting, etc.)
├── src/parser.rs            # XML parsing into types
└── src/error.rs

crates/quarto-citeproc/      # CSL evaluation engine
├── src/eval.rs              # Main evaluation (~3000 lines) - this is where most work happens
├── src/output.rs            # Output AST, rendering to Pandoc Inlines, CSL HTML
├── src/disambiguation.rs    # Name/year suffix disambiguation
├── src/locale.rs            # Locale term lookup, date formatting
├── src/locale_parser.rs     # Locale XML parsing
├── src/reference.rs         # CSL-JSON reference parsing
├── src/types.rs             # Processor, Citation, CitationItem types
└── tests/
    ├── csl_conformance.rs   # Test harness (parses test files, runs them)
    ├── enabled_tests.txt    # Tests expected to pass (one per line)
    ├── deferred_tests.txt   # Tests intentionally skipped (with reasons)
    └── csl_conformance.lock # Authoritative state (auto-generated)
```

### Key Types

```
quarto-csl:
  Style           # Root CSL style
  Citation        # Citation element with layout, sort, options
  Bibliography    # Bibliography element with layout, sort, options
  Layout          # Contains rendering elements
  Names/Name      # Name formatting configuration
  Formatting      # Font style, weight, text-case, affixes, etc.

quarto-citeproc:
  Processor       # Main entry point - holds style, references, state
  Reference       # A citable item (book, article, etc.)
  Citation        # A citation instance with items
  Output          # Output AST (Literal, Formatted, Sequence, Tagged)
  EvalContext     # Evaluation state passed through rendering
```

### Data Flow

```
CSL Style (XML) ──parse──> Style ─┐
                                  ├──> Processor ──process──> Output AST ──render──> Pandoc Inlines
References (JSON) ──parse──> Vec<Reference> ─┘                              └──────> CSL HTML (for tests)
```

## Debugging a Failing Test

### Step 1: Understand the Test

```bash
# Read the test file
cat crates/quarto-citeproc/test-data/csl-suite/<testname>.txt

# Look at:
# - MODE: citation or bibliography?
# - CSL: What style features are used?
# - INPUT: What reference data?
# - RESULT: What's expected?
```

### Step 2: Check if Intentionally Deferred

```bash
grep -i "<testname>" crates/quarto-citeproc/tests/deferred_tests.txt
```

If found, read the reason. Don't work on deferred tests without user approval.

### Step 3: Check Reference Implementation

```bash
# Search Pandoc's citeproc for relevant code
grep -r "relevant_term" external-sources/citeproc/src/

# Check if Pandoc also fails this test
grep -i "<testname>" external-sources/citeproc/test/Spec.hs
```

If Pandoc also fails, consider deferring rather than implementing differently.

### Step 4: Find Similar Passing Tests

```bash
# Find tests with similar names
ls crates/quarto-citeproc/test-data/csl-suite/ | grep -i "<category>"

# Check which are enabled
grep -i "<category>" crates/quarto-citeproc/tests/enabled_tests.txt
```

Study passing tests to understand what patterns work.

### Step 5: Debug with Print Statements

The evaluation code is in `crates/quarto-citeproc/src/eval.rs`. Key functions:
- `evaluate_layout()` - Entry point for citation/bibliography
- `evaluate_elements()` - Renders a list of elements
- `evaluate_names()` / `format_names()` - Name rendering
- `evaluate_date()` - Date rendering
- `evaluate_group()` - Conditional groups

Add `eprintln!` statements to trace execution. The test harness captures stderr.

### Step 6: Make Minimal Changes

- Fix one thing at a time
- Run tests frequently
- Don't break passing tests

## Testing Workflow

### Test Manifest Files

| File | Purpose |
|------|---------|
| `enabled_tests.txt` | Tests that should pass (one name per line, case-insensitive) |
| `deferred_tests.txt` | Tests intentionally skipped with documented reasons |
| `csl_conformance.lock` | Auto-generated lockfile tracking exact state |

### Adding Newly-Passing Tests

```bash
# 1. Add test names to enabled_tests.txt
echo "testname" >> crates/quarto-citeproc/tests/enabled_tests.txt

# 2. Run validation (will fail, showing count mismatch)
cargo nextest run -p quarto-citeproc csl_validate_manifest

# 3. Auto-update the lockfile
UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest

# 4. Verify all tests pass
cargo nextest run -p quarto-citeproc

# 5. Commit both files together
git add crates/quarto-citeproc/tests/enabled_tests.txt
git add crates/quarto-citeproc/tests/csl_conformance.lock
```

### Validation Checks

The build system validates `enabled_tests.txt`:
- **Duplicates**: Warns if same test listed twice
- **Non-existent**: Warns if test file doesn't exist in test-data/csl-suite/
- **Lockfile mismatch**: Fails if lockfile doesn't match current state

### Regression Detection

If `enabled_count` in the lockfile decreases:
1. Check git diff to see which tests were removed
2. Investigate whether removal was intentional. Ask user before deferring any tests.
3. If accidental, restore the tests

## Reference Materials

### Reference Implementation

`external-sources/citeproc/` - Pandoc's Haskell citeproc

| File | Contains |
|------|----------|
| `src/Citeproc/Types.hs` | Core types, `Output a`, `CiteprocOutput` typeclass |
| `src/Citeproc/Eval.hs` | Evaluation logic, disambiguation, collapsing |
| `src/Citeproc/CslJson.hs` | HTML rendering for CSL test suite |
| `test/Spec.hs` | Expected failures (search for test names) |

### CSL Specification

Summarized in `claude-notes/csl-spec/`:
- `00-index.md` - Start here, has quick reference
- `03-names.md` - Name formatting (complex!)
- `04-disambiguation.md` - Disambiguation algorithm

Original spec: `external-sources/csl-spec/specification.rst`

### Test Suite

- 858 total CSL conformance tests
- Tests cover: names, dates, sorting, disambiguation, collapsing, locale, etc.
- Test file format documented above

## Deferred Tests Policy

**Adding tests to `deferred_tests.txt` requires user approval.**

These are tests we've decided to intentionally skip, not "not yet attempted" tests.

### Before Proposing to Defer

1. Investigate the test thoroughly
2. Understand why it fails and what would fix it
3. Check if Pandoc's citeproc also fails it
4. Ask the user for approval with rationale

### Valid Reasons for Deferring

- Pandoc's citeproc also fails (we aim for parity, not perfection)
- CSL style quirk producing technically-correct but undesirable output
- Edge case requiring disproportionate effort
- Test conflicts with more important behaviors

### Format in `deferred_tests.txt`

```
# ------------------------------------------------------------------------------
# Category - brief description
# ------------------------------------------------------------------------------

# Specific reason this test is deferred
# Additional context if needed
test_name
```

## Common Pitfalls

1. **Test name case**: Names in `enabled_tests.txt` are case-insensitive
2. **Test counts**: The 858 is CSL tests only; nextest shows +73 unit tests +1 validation
3. **Deferred overlap**: A test can be in both `enabled_tests.txt` and `deferred_tests.txt` if it was enabled then later deferred
4. **Output format**: Tests expect "CSL HTML" (`<i>`, `<b>`), not Markdown
5. **Pandoc differences**: When in doubt, check what Pandoc does

## Outstanding Work

Check beads for current tasks:
```bash
bd ready
bd show k-422  # Parent issue for CSL conformance
```

Major remaining areas:
- Full disambiguation support
- Display attribute (`left-margin`, `right-inline`, etc.)
- Additional position tracking edge cases

---

*See also: `CLAUDE.md` for general repo instructions (beads, testing, coding guidelines)*
