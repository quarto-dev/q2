# CSL Conformance Roadmap

This document serves as a roadmap for the implementation of quarto-citeproc, a
Rust port of https://github.com/jgm/citeproc within the larger workspace of
Quarto's Rust port. It also serves as a workflow for LLM coding agents to
identify and improve the state of the suite.

The goal of quarto-citeproc is not to achieve full conformance with the CSL
conformance suite. **The goal is to get close to feature parity with citeproc**.

**ALWAYS consult `external-sources/citeproc` (Pandoc's Haskell citeproc) as the reference implementation.** Do NOT consult citeproc-js via web searches. The Pandoc citeproc is the authoritative reference for this project.

**Parent Issue**: k-422 (CSL conformance testing)
**Created**: 2025-11-27
**Status**: In Progress

## Important Implementation Notes

### Reference Implementation

When an implementation or a bugfix seems particularly challenging, consider studying the reference implementation alongside quarto-citeproc to identify architectural differences. Our goal is to get to the same level of CSL Conformance as citeproc, and its implementation is a good guide.

Key files in `external-sources/citeproc/src/`:
- `Citeproc/Types.hs` - Core types including `Output a`, `CiteprocOutput` typeclass
- `Citeproc/Eval.hs` - Evaluation logic including collapse, disambiguation
- `Citeproc/CslJson.hs` - HTML output format for CSL test suite
- `Citeproc/Pandoc.hs` - Pandoc Inlines output format

### Output Format Architecture

Pandoc's citeproc uses a **format-agnostic design**:

1. `Output a` - Parameterized AST where `a` is the output type
2. `CiteprocOutput` typeclass - Defines formatting operations (`addFontWeight`, `addFontStyle`, etc.)
3. **Two implementations provided:**
   - `CslJson Text` - Renders to HTML (`<b>`, `<i>`, `<sup>`, etc.) - **used by CSL test suite**
   - `Inlines` - Renders to Pandoc AST - used for Pandoc integration

Our implementation uses Pandoc's AST directly (Blocks and Inlines), from which downstream
tooling can produce format-specific output. This implementation includes a minimal renderer
from Inlines and Blocks to "CSL HTML", the HTML dialect that is expected in the conformance
suite.

### CSL Specification Reference

Detailed CSL spec documentation has been created in `claude-notes/csl-spec/`.
If you need to consult the CSL spec, start by reading `claude-notes/csl-spec/00-index.md` and
following the instructions there.

## Test Organization

### Test Files

- `tests/enabled_tests.txt` - Tests that are expected to pass (one test name per line)
- `tests/deferred_tests.txt` - Tests intentionally set aside with documented reasons
- `tests/csl_conformance.lock` - Lockfile tracking test suite state (auto-generated, do not edit manually)

### Test Manifest Validation

The build system includes validation to prevent common errors:

1. **Duplicate Detection**: Build emits warnings for duplicate entries in `enabled_tests.txt`
2. **Non-existent Test Detection**: Build emits warnings for test names that don't exist in the suite
3. **Lockfile Tracking**: A `csl_validate_manifest` test verifies the current state matches the committed lockfile

**Workflow for adding new tests:**

```bash
# 1. Add test names to enabled_tests.txt
# 2. Run the validation test (it will fail showing the mismatch)
cargo nextest run -p quarto-citeproc csl_validate_manifest

# 3. Auto-update the lockfile
UPDATE_CSL_LOCKFILE=1 cargo nextest run -p quarto-citeproc csl_validate_manifest

# 4. Commit both enabled_tests.txt and csl_conformance.lock together
```

**Regression Detection:**
- If `enabled_count` decreases, the lockfile diff will show which tests were removed
- Review any decreases carefully - they may indicate accidental regressions

**Current counts** (see `tests/csl_conformance.lock` for authoritative numbers):
- Suite total: 858 tests
- Unit tests: 73 tests (non-CSL)
- Validation test: 1 test

### Deferred Tests Policy

**IMPORTANT**: Adding tests to `deferred_tests.txt` requires user approval. These are tests we've decided to intentionally skip, not just "not yet attempted" tests.

Before proposing to defer a test:
1. Investigate the test thoroughly
2. Understand why it fails and what would be needed to fix it
3. Ask the user for approval, explaining the rationale

Valid reasons for deferring:
- CSL style quirk that produces technically-correct but undesirable output
- Edge case that would require disproportionate effort relative to its value
- Test that conflicts with other more important behaviors

Format in `deferred_tests.txt`:
```
# Date and reason for deferring
# Explanation of why this test is deferred
test_name
```

## Current State

We maintain the current state of CSL conformance in `tests/csl_conformance.lock`.

## Test Coverage Analysis (Updated 2025-11-28)

| Category | Enabled | Total | Gap | Coverage |
|----------|---------|-------|-----|----------|
| nameattr | 89 | 97 | 8 | 91% |
| condition | 15 | 17 | 2 | 88% |
| date | 83 | 101 | 18 | 82% |
| textcase | 23 | 31 | 8 | 74% |
| disambiguate | 43 | 72 | 29 | 59% |
| collapse | 12 | 21 | 9 | 57% |
| substitute | 4 | 7 | 3 | 57% |
| label | 10 | 19 | 9 | 52% |
| sort | 30 | 66 | 36 | 45% |
| name | 48 | 111 | 63 | 43% |
| locale | 9 | 23 | 14 | 39% |
| position | 6 | 16 | 10 | 37% |
| number | 6 | 20 | 14 | 30% |
| bugreports | 21 | 83 | 62 | 25% |
| affix | 2 | 9 | 7 | 22% |
| flipflop | 4 | 19 | 15 | 21% |
| magic | 2 | 40 | 38 | 5% |

### Remaining Date Tests (26 ignored)

- **Year suffix** tests - Part of disambiguation system (larger feature)
- Various edge cases and complex scenarios

## Roadmap

Completed parts of the roadmap are recorded in `claude-notes/minutes/quarto-citeproc/csl-conformance.md`

### Outstanding tasks

- [ ] Display attribute, full disambiguation support