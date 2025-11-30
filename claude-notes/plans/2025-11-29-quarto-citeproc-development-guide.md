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

## Work Selection Policy

**Only work on "unknown" tests - never on deferred tests.**

Tests have three states:
- **Enabled**: Tests we expect to pass. Run with every build.
- **Deferred**: Tests we've attempted and consciously decided to skip. Do NOT work on these.
- **Unknown**: Tests we haven't attempted yet. This is where work should focus.

The workflow is:
1. Pick an unknown test
2. Attempt to fix it
3. If successful → enable it
4. If too difficult/blocked → defer it (with user approval)
5. Move to the next unknown test

Once all unknown tests are addressed (either enabled or deferred), we can revisit
deferred tests in priority order.

## Quick Start for New Sessions

### 1. Check Current Status

```bash
# Get full test status overview with category breakdown
python3 scripts/csl-test-helper.py status

# Or quick check via lockfile
head -10 crates/quarto-citeproc/tests/csl_conformance.lock

# Run all enabled tests - these should pass
cargo nextest run -p quarto-citeproc
```

### 2. Find Work

```bash
# Check beads for assigned tasks
bd ready

# See test breakdown by category (focus on "Unknown" counts)
python3 scripts/csl-test-helper.py status

# Check a specific category - look for "?" (unknown) tests
python3 scripts/csl-test-helper.py category name --run

# Find quick wins (tests that pass but aren't enabled)
python3 scripts/csl-test-helper.py quick-wins

# Check for regressions
python3 scripts/csl-test-helper.py regressions
```

**IMPORTANT**: Only work on tests marked with `?` (unknown). Skip tests already
in `deferred_tests.txt` - those have been consciously decided to skip.

### 3. Inspect and Run Tests

```bash
# Inspect a specific test (shows status, expected/actual diff, Pandoc comparison)
python3 scripts/csl-test-helper.py inspect name_AfterInvertedName --diff

# Also show the CSL style
python3 scripts/csl-test-helper.py inspect name_AfterInvertedName --diff --csl

# Run an enabled test directly
cargo nextest run -p quarto-citeproc csl_name_westersimple

# Run an ignored (disabled) test
cargo nextest run -p quarto-citeproc csl_name_sometest -- --include-ignored

# Run all tests in a category
cargo nextest run -p quarto-citeproc csl_name_ -- --include-ignored
```

### 4. Enable New Tests

```bash
# Add passing tests to enabled list
python3 scripts/csl-test-helper.py enable test_name1 test_name2

# Update the lockfile
UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest

# Verify all tests pass
cargo nextest run -p quarto-citeproc
```

### 5. Read a Test File

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

### Step 0: Check if Deferred (MUST DO FIRST)

```bash
grep -i "<testname>" crates/quarto-citeproc/tests/deferred_tests.txt
```

**If found, STOP.** Do not work on deferred tests. Move to a different unknown test.
Deferred tests have already been attempted and consciously skipped.

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

### Step 2: Check Reference Implementation

```bash
# Search Pandoc's citeproc for relevant code
grep -r "relevant_term" external-sources/citeproc/src/

# Check if Pandoc also fails this test
grep -i "<testname>" external-sources/citeproc/test/Spec.hs
```

If Pandoc also fails, consider deferring rather than implementing differently.

### Step 3: Find Similar Passing Tests

```bash
# Find tests with similar names
ls crates/quarto-citeproc/test-data/csl-suite/ | grep -i "<category>"

# Check which are enabled
grep -i "<category>" crates/quarto-citeproc/tests/enabled_tests.txt
```

Study passing tests to understand what patterns work.

### Step 4: Debug with Print Statements

The evaluation code is in `crates/quarto-citeproc/src/eval.rs`. Key functions:
- `evaluate_layout()` - Entry point for citation/bibliography
- `evaluate_elements()` - Renders a list of elements
- `evaluate_names()` / `format_names()` - Name rendering
- `evaluate_date()` - Date rendering
- `evaluate_group()` - Conditional groups

Add `eprintln!` statements to trace execution. The test harness captures stderr.

### Step 5: Make Minimal Changes

- Fix one thing at a time
- Run tests frequently
- Don't break passing tests

### Step 6: If Fix is Too Difficult

If after investigation you determine the fix requires:
- A major new feature (e.g., subsequent-author-substitute, citation collapsing)
- Disproportionate effort for an edge case
- Changes that would break other tests

Then propose deferring the test to the user with:
1. Clear explanation of what's needed
2. Whether Pandoc also fails (check `external-sources/citeproc/test/Spec.hs`)
3. Rough estimate of effort

**Wait for user approval before adding to `deferred_tests.txt`.**

## CSL Test Helper Utility

The `scripts/csl-test-helper.py` utility simplifies test management. It handles
all the annoying normalization issues (case, hyphens vs underscores) automatically.

### Commands Reference

```bash
# Overall status - shows counts and category breakdown
python3 scripts/csl-test-helper.py status

# Category analysis - see all tests in a category
python3 scripts/csl-test-helper.py category <name>         # List tests
python3 scripts/csl-test-helper.py category <name> --run   # Run and show pass/fail

# Quick wins - find tests that pass but aren't enabled
python3 scripts/csl-test-helper.py quick-wins

# Regressions - find enabled tests that now fail
python3 scripts/csl-test-helper.py regressions

# Inspect a specific test
python3 scripts/csl-test-helper.py inspect <test>          # Basic info
python3 scripts/csl-test-helper.py inspect <test> --diff   # Show expected vs actual
python3 scripts/csl-test-helper.py inspect <test> --csl    # Show the CSL style
python3 scripts/csl-test-helper.py inspect <test> --input  # Show input JSON

# Run tests matching a pattern
python3 scripts/csl-test-helper.py run <pattern> --include-ignored -v

# Enable tests (adds to enabled_tests.txt)
python3 scripts/csl-test-helper.py enable test1 test2 test3
```

### Example Workflow

```bash
# 1. Start by checking overall status
python3 scripts/csl-test-helper.py status

# 2. Pick a category to work on
python3 scripts/csl-test-helper.py category name --run

# 3. Investigate a failing test
python3 scripts/csl-test-helper.py inspect name_SomeTest --diff --csl

# 4. After fixing, check for quick wins
python3 scripts/csl-test-helper.py quick-wins

# 5. Enable any newly-passing tests
python3 scripts/csl-test-helper.py enable name_SomeTest
UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest
```

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

**Deferred tests are OFF LIMITS for regular work.**

The `deferred_tests.txt` file contains tests we've already attempted and consciously
decided to skip. Do not work on these tests during normal development.

### Three Test States

| State | Meaning | Action |
|-------|---------|--------|
| **Enabled** | Expected to pass | Maintain - don't break these |
| **Unknown** | Not yet attempted | **Work on these** |
| **Deferred** | Attempted and skipped | Do not touch |

### When to Defer a Test

Only propose deferring after you've:
1. Investigated the test thoroughly
2. Understood why it fails and what would fix it
3. Checked if Pandoc's citeproc also fails it
4. Determined the fix is blocked or requires major new features

**Always get user approval before adding to `deferred_tests.txt`.**

### Valid Reasons for Deferring

- Pandoc's citeproc also fails (we aim for parity, not perfection)
- Requires a major unimplemented feature (e.g., citation collapsing)
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

### Revisiting Deferred Tests

Only revisit deferred tests when:
1. All unknown tests have been addressed (enabled or deferred)
2. A major feature has been implemented that unblocks multiple deferred tests
3. User explicitly requests work on a specific deferred test

## Common Pitfalls

1. **Test name case**: Names in `enabled_tests.txt` are case-insensitive
2. **Test counts confusion**: There are multiple numbers that can cause confusion:
   - **858**: Total CSL conformance tests in `test-data/csl-suite/`
   - **~73**: Unit tests in the crate (not CSL conformance tests)
   - **1**: The `csl_validate_manifest` test
   - When `cargo nextest run` reports "N tests run, M skipped":
     - N = enabled CSL tests + unit tests + validate_manifest
     - M = disabled/ignored CSL tests
   - When `csl-test-helper.py status` reports "N enabled", that's CSL tests only
   - **Bottom line**: Don't compare `nextest` totals to `csl-test-helper.py` totals directly
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
