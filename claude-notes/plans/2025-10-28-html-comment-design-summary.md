# HTML Comment Support - Design Summary

**Date:** 2025-10-28
**Status:** Ready for review and implementation

## Executive Summary

I've completed the ultrathinking phase for HTML comment support. This document summarizes the problem, proposed solution, test cases, and questions for your review before implementation.

## Problem Analysis

### Root Cause

HTML comments `<!-- ... -->` are not recognized by either tree-sitter parser (block or inline). Characters inside comments are parsed as regular markdown, causing failures when comments contain markdown-special characters.

**Example failure:**
```markdown
<!-- This comes from path/to/_quarto-rules.scss -->
                                   ↑
                        This underscore triggers "Unclosed Emphasis" error
```

### Why This Happens

The tree-sitter-markdown-inline scanner has a switch statement that dispatches on characters:
- `*` → parse emphasis/strong
- `_` → parse emphasis
- `` ` `` → parse code span
- etc.

But `<` is not handled, so `<!--` gets parsed character-by-character, with the `_` triggering emphasis parsing.

## Proposed Solution

### Architecture: External Scanner Tokens

Both scanners will add an `HTML_COMMENT` external token that:

1. **Triggers on:** `<` character at start of `<!--` sequence
2. **Consumes:** Everything from `<!--` through `-->`
3. **Returns:** Single atomic token containing the entire comment
4. **Stores:** No state needed (comments are self-contained)

### Why External Scanner?

External scanners are the correct approach because:
- Comments need to override ALL markdown syntax inside them
- Comments can span multiple lines
- We need to consume `<!--` through `-->` atomically
- This pattern is already used for code spans, latex spans, etc.

### Three-Stage Implementation

**Stage 1: Block Parser**
- Add external token `HTML_COMMENT` to tree-sitter-markdown scanner
- Add grammar rule `html_comment_block: $ => seq($_html_comment, choice($_newline, $_eof))`
- Result: Block-level comments work (commenting out paragraphs, list items, etc.)

**Stage 2: Inline Parser**
- Add external token `HTML_COMMENT` to tree-sitter-markdown-inline scanner
- Add grammar rule `html_comment: $ => $_html_comment`
- Add to `_inline_element` choices
- Result: Inline comments work (comments inside paragraphs, headings, etc.)

**Stage 3: Pandoc Integration**
- Add handler in quarto-markdown-pandoc for `html_comment` nodes
- Convert to `RawInline "quarto-html-comment" "<! content>"`
- Add handler for `html_comment_block` nodes
- Convert to `RawBlock "quarto-html-comment" "<!-- content -->"`

## Scanner Implementation Details

### Core Parsing Function

```c
static bool parse_html_comment(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[HTML_COMMENT]) {
        return false;
    }

    // Match '<!--'
    if (lexer->lookahead != '<') return false;
    lexer->advance(lexer, false);

    if (lexer->lookahead != '!') return false;
    lexer->advance(lexer, false);

    if (lexer->lookahead != '-') return false;
    lexer->advance(lexer, false);

    if (lexer->lookahead != '-') return false;
    lexer->advance(lexer, false);

    // Consume until '-->'
    while (!lexer->eof(lexer)) {
        if (lexer->lookahead == '-') {
            lexer->advance(lexer, false);
            if (lexer->lookahead == '-') {
                lexer->advance(lexer, false);
                if (lexer->lookahead == '>') {
                    lexer->advance(lexer, false);
                    lexer->mark_end(lexer);
                    lexer->result_symbol = HTML_COMMENT;
                    return true;
                }
                // Continue - this was not the end
            }
        } else {
            lexer->advance(lexer, false);
        }
    }

    // Unclosed comment - consume until EOF
    lexer->mark_end(lexer);
    lexer->result_symbol = HTML_COMMENT;
    return true;
}
```

### Integration into scan() function

**For inline scanner:**
```c
static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    if (valid_symbols[TRIGGER_ERROR]) {
        return error(lexer);
    }

    switch (lexer->lookahead) {
        case '<':
            // Try HTML comment first
            if (parse_html_comment(lexer, valid_symbols)) {
                return true;
            }
            return false;
        case '{':
            return parse_shortcode_open(s, lexer, valid_symbols);
        // ... rest of cases
    }
    // ... rest of function
}
```

**For block scanner:** Similar pattern in its scan function

## Test Coverage

I've created 35 granular test files in `test-html-comments/`:

### Core Functionality (1-7)
- Simple inline comment
- Comments with `*`, `**`, `_`, `[links]`, paths
- Multi-line inline comments

### Block-Level (8-9)
- Block-level single line
- Block-level multi-line

### Content Variations (10-13)
- Code syntax inside comments
- HTML tags inside comments
- Double dashes inside comments
- Multiple sequential comments

### Structural Tests (14, 22-24)
- Comments in lists
- Comments in blockquotes
- Comments in ordered lists

### Edge Cases (15-21, 25-35)
- Empty comments `<!-- -->`
- Whitespace-only comments
- Line positioning (start/end)
- Quarto-specific syntax in comments
- Very long comments
- Code blocks (comment should be literal)
- NOT comments (space after `<` or `!`)
- False endings (`->`, `-- >`)
- Multiple dashes (`---`, `----`)
- Comments with no spaces
- Newlines in comments

### Test Runner

Run with: `./test-html-comments/run-all-tests.sh`

Currently, all 35 tests will fail. After implementation, they should all pass (except possibly malformed comment tests, depending on design decisions).

## Edge Case Decisions Needed

### 1. Unclosed Comments

```markdown
<!-- This comment never closes
More content here
End of file
```

**Options:**
- A) Consume until EOF, emit HTML_COMMENT token
- B) Return false, let it parse as regular markdown
- C) Emit an error

**Recommendation:** Option A (consume until EOF)
- Matches HTML parser behavior (lenient)
- Prevents misleading errors about markdown syntax
- User gets HTML comment (possibly with warning)

### 2. Format Name

Should the Pandoc format be:
- A) `"quarto-html-comment"` (explicit, can be filtered/processed specially)
- B) `"html"` (standard, works with existing HTML output)

**Recommendation:** Option A initially, then desugar to B
- Allows special handling during Quarto processing
- Can track that these came from comments
- Easy to desugar to "html" later

### 3. Malformed Comments

```markdown
< !-- space after < -->
<! -- space after ! -->
```

**Options:**
- A) Don't recognize as comments (parse as markdown)
- B) Be lenient, recognize anyway

**Recommendation:** Option A (strict matching)
- Follows HTML spec more closely
- Avoids accidental comment recognition
- Simpler scanner logic

### 4. Comments in Code Blocks

````markdown
```
<!-- not a comment -->
```
````

**Decision:** Already handled correctly
- Code blocks are parsed by block scanner as `fenced_code_block`
- Content inside is not parsed as inline markdown
- No changes needed

## Implementation Order

1. **Start with inline parser** (tree-sitter-markdown-inline)
   - Simpler case (no block structure concerns)
   - Most failures happen here (Test 1-7 address inline)
   - Easier to debug

2. **Then block parser** (tree-sitter-markdown)
   - Builds on inline knowledge
   - Tests 8-9 address this
   - More complex due to block/inline interaction

3. **Finally Pandoc integration** (quarto-markdown-pandoc)
   - Both parsers working
   - Can test end-to-end
   - Can see actual AST output

## Key Design Insights

### 1. State Not Needed

Unlike code spans or emphasis, HTML comments don't need scanner state:
- No nesting (comments don't nest in HTML)
- No delimiter counting (always `<!--` and `-->`)
- Complete in single token
- No interaction with other structures

### 2. Performance Considerations

Long comments could be expensive to scan. Mitigations:
- Scanner already advances character-by-character (unavoidable)
- No backtracking needed
- Single pass through comment
- Mark end as we go

### 3. Source Mapping

The full comment text includes `<!--` and `-->`:
- Preserves exact source
- Source location tracks the entire comment
- Useful for error reporting
- Allows reconstruction of original

### 4. Precedence

HTML comments should have HIGHER precedence than any markdown:
- Check for `<` in scanner BEFORE other processing
- In grammar, no conflicts expected (atomic token)
- Processed early in the scan() switch

## Files to Modify

### tree-sitter-markdown-inline

1. `crates/tree-sitter-qmd/tree-sitter-markdown-inline/src/scanner.c`
   - Add `HTML_COMMENT` to TokenType enum (line ~50)
   - Add `parse_html_comment()` function
   - Add case `'<':` to `scan()` function (line ~530)
   - Update serialize/deserialize if needed (no state, so no changes)

2. `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`
   - Add `$._html_comment` to externals array (line ~30)
   - Add `html_comment: $ => $._html_comment` rule
   - Add `$.html_comment` to `_inline_element` choice (find via add_inline_rules)

### tree-sitter-markdown

1. `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
   - Add `HTML_COMMENT` to TokenType enum (line ~64)
   - Add `parse_html_comment()` function (can be nearly identical to inline version)
   - Add handling in scan function

2. `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
   - Add `$._html_comment` to externals
   - Add `html_comment_block: $ => seq($_html_comment, choice($_newline, $_eof))`
   - Add to `_block_not_section` choice

### quarto-markdown-pandoc

1. Find block conversion code
   - Likely in `crates/quarto-markdown-pandoc/src/...`
   - Add handler for `"html_comment_block"` nodes
   - Convert to `RawBlock` with format "quarto-html-comment"

2. Find inline conversion code
   - Add handler for `"html_comment"` nodes
   - Convert to `RawInline` with format "quarto-html-comment"

## Build and Test Process

After each modification:

```bash
# For tree-sitter changes
cd crates/tree-sitter-qmd/tree-sitter-markdown-inline  # or tree-sitter-markdown
tree-sitter generate
tree-sitter build
tree-sitter test  # Should pass existing tests

# For Rust changes
cargo build

# Run test suite
./test-html-comments/run-all-tests.sh

# Test specific case
cargo run --bin quarto-markdown-pandoc -- -i test-html-comments/04-comment-with-underscore.qmd
```

## Questions for Review

1. **Unclosed comments:** Consume until EOF (recommended) or error?

2. **Format name:** `"quarto-html-comment"` initially then desugar to `"html"`, or just use `"html"` directly?

3. **Should we distinguish block vs inline comments** in the Pandoc AST, or let the context determine it? (Currently planning to distinguish: RawBlock vs RawInline)

4. **Error reporting:** Should unclosed comments generate a warning/error in addition to being parsed, or just silently accept them?

5. **HTML5 spec strictness:** HTML5 technically forbids `--` inside comments, but browsers and parsers are lenient. Should we error on `<!-- foo -- bar -->` or allow it? (Recommending: allow it, be lenient)

## Next Steps

1. **Get feedback** on edge case decisions
2. **Implement inline parser** first (Stage 1)
3. **Verify inline tests** pass
4. **Implement block parser** (Stage 2)
5. **Verify block tests** pass
6. **Implement Pandoc integration** (Stage 3)
7. **Verify all tests** pass
8. **Test with real files** (quarto-web corpus)

## Success Criteria

- [ ] All 35 test files in `test-html-comments/` parse successfully
- [ ] Original failing file `external-sites/quarto-web/docs/authoring/_mermaid-theming.qmd` parses
- [ ] No regressions in existing tree-sitter test suites
- [ ] HTML comments appear in Pandoc AST as RawInline/RawBlock with appropriate format
- [ ] Source locations are accurate
- [ ] Comments in code blocks remain literal (not parsed as comments)

---

**Ready for implementation once design decisions are confirmed.**
