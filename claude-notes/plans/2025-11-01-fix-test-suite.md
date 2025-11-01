# Test Suite Fixing Plan - November 1, 2025

## Overview

After completing the k-274 tree-sitter refactoring, we need to fix the failing test suite for quarto-markdown-pandoc. 11 out of 14 integration tests are failing.

## Test Failure Analysis

### Critical Issues (Block Core Functionality)

#### 1. Document-level parsing crashes
**Tests affected:** test_do_not_smoke, unit_test_snapshots_qmd

**Problem:** "Expected Block or Section, got IntermediateUnknown"

**Root causes:**
- tests/smoke/001.qmd: `^ he llo^` - reference note definition syntax not handled
- tests/snapshots/qmd/horizontal-rules-vs-metadata.qmd: Document with YAML frontmatter crashes

**Fix approach:**
- Investigate what node type `^` produces in the tree-sitter grammar
- Add missing node handler for reference note definitions
- Fix document-level parsing to handle YAML metadata blocks

**Priority:** CRITICAL - crashes prevent testing other functionality

---

### High Priority Issues (Wrong Core Logic)

#### 2. Citation parsing completely wrong
**Tests affected:** unit_test_snapshots_native, test_qmd_roundtrip_consistency

**Problem:**
- Input: `[prefix @c1 suffix; @c2; @c3]`
- Expected: Single Cite with 3 citations, prefix on first, suffix distributed correctly
- Actual: Span containing multiple separate Cites with wrong modes (AuthorInText vs NormalCitation)

**Root cause:** Citation handling in process_pandoc_span() is incorrectly unwrapping and transforming citations. See claude-notes/citation-grammar-limitation.md for context.

**Fix approach:**
1. Review citation processing logic in pandoc/treesitter_utils/spans.rs
2. Fix citation mode detection (bracketed [@cite] should be NormalCitation, not AuthorInText)
3. Fix prefix/suffix distribution across multiple citations
4. Add comprehensive citation tests

**Priority:** HIGH - citations are a core feature

---

#### 3. LineBreak vs SoftBreak handling
**Tests affected:** test_html_writer, test_json_writer

**Problem:**
- Input: `Line one  \nLine two` (two trailing spaces + newline)
- Expected: LineBreak
- Actual: SoftBreak

**Root cause:** Hard line breaks (two spaces at end of line) are not being recognized by the tree-sitter grammar or not handled correctly.

**Fix approach:**
1. Check if tree-sitter grammar has a node for hard line breaks
2. Add handler for hard_line_break or similar node
3. Test with various line break scenarios

**Priority:** HIGH - affects output quality significantly

---

#### 4. Invalid syntax validation
**Tests affected:** test_disallowed_in_qmd_fails

**Problem:**
- Input: `# Hello {=world}` (raw attribute syntax)
- Expected: Parse error (not allowed in QMD)
- Actual: Parses successfully

**Root cause:** Grammar may be too permissive, or validation logic is missing

**Fix approach:**
1. Review attribute parsing in grammar
2. Add validation to reject raw attributes (=value syntax)
3. Ensure error is properly reported

**Priority:** HIGH - validation prevents user errors

---

### Medium Priority Issues (Quality/Edge Cases)

#### 5. Nested inline formatting
**Tests affected:** unit_test_corpus_matches_pandoc_commonmark

**Problem:**
- Input: `~he~~l~~lo~` (subscript with strikeout inside)
- Expected: Subscript containing [Str "he", Strikeout [Str "l"], Str "lo"]
- Actual: Subscript containing [Str "he", RawInline "leftover", Str "lo"]

**Root cause:** Strikeout (~~ inside ~) not being parsed correctly when nested in subscript

**Fix approach:**
1. Review inline formatting nesting in grammar
2. May need grammar changes to handle nested ~~ and ~
3. Add comprehensive nesting tests

**Priority:** MEDIUM - edge case but affects complex documents

---

#### 6. Underline class conversion
**Tests affected:** unit_test_corpus_matches_pandoc_markdown

**Problem:**
- Input: `[underline]{.underline}`
- Expected: Underline inline
- Actual: Span with class="underline"

**Root cause:** Need special case to convert .underline class to Underline inline

**Fix approach:**
1. Add logic in span processing to detect .underline class
2. Convert to Underline inline instead of Span
3. Test with other special classes (if any)

**Priority:** MEDIUM - compatibility feature

---

#### 7. Roundtrip consistency
**Tests affected:** test_empty_blockquote_roundtrip

**Problem:**
- Input: `> ## Header in blockquote`
- QMD→JSON→QMD produces: `> ## Header in blockquote {#header-in-blockquote}`
- The regenerated JSON has an extra Space at the end of the header content

**Root cause:** Writer is adding explicit ID when it should omit auto-generated IDs, and/or adding extra space

**Fix approach:**
1. Review header auto-ID generation in writer
2. Don't emit auto-generated IDs if they match what Pandoc would generate
3. Fix extra space issue in header inline content

**Priority:** MEDIUM - roundtrip quality issue

---

#### 8. Source map accuracy
**Tests affected:** unit_test_snapshots_json

**Problem:** Source info pool ranges are incorrect (snapshot mismatch)

**Root cause:** Source location tracking during tree-sitter node processing

**Fix approach:**
1. Review source map generation in tree-sitter processing
2. Ensure ranges are calculated correctly for all node types
3. May require careful debugging

**Priority:** LOW - functionality works, just source maps are off

---

## Implementation Strategy

### Phase 1: Critical Crashes (Issues 1)
1. Fix reference note definition handling
2. Fix YAML frontmatter handling
3. Verify test_do_not_smoke and unit_test_snapshots_qmd pass

### Phase 2: Core Functionality (Issues 2-4)
1. Fix citation parsing logic
2. Fix LineBreak handling
3. Fix invalid syntax validation
4. Verify affected tests pass

### Phase 3: Quality Issues (Issues 5-7)
1. Fix nested inline formatting
2. Fix underline class conversion
3. Fix roundtrip consistency
4. Verify affected tests pass

### Phase 4: Source Maps (Issue 8)
1. Fix source map range calculation
2. Verify snapshot tests pass

---

## Success Criteria

- All 14 integration tests pass
- No test failures or panics
- `cargo test -p quarto-markdown-pandoc` runs clean
