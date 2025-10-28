# HTML Comment Support - REVISED DESIGN

**Date:** 2025-10-28
**Status:** Design updated based on feedback
**Revision:** Added critical insight about block parser handling inline comments

## Critical Design Change

### The Key Insight

**The block parser must handle HTML comments that start at the inline level**, not just comments that occupy entire blocks by themselves.

### Why This Matters

Consider this document:

```markdown
This is a paragraph <!-- this is a comment

* this list cannot be parsed
* this is still a comment --> and now this is the end of the paragraph.

Another paragraph.
```

**What should happen:**
1. "This is a paragraph" starts a paragraph block
2. `<!--` starts an HTML comment **while inside the paragraph**
3. The comment consumes everything including the newline and list markers
4. `-->` ends the comment **still inside the paragraph**
5. "and now this is the end of the paragraph." continues the paragraph
6. The paragraph ends
7. "Another paragraph." is a separate paragraph

**What the original design would have done wrong:**
- Block parser would see the list markers on lines 3-4
- Block parser would try to create a list block
- Block parser would fail because it didn't know about the comment

### The Solution

**Both parsers need to handle HTML comments:**

1. **Block parser (tree-sitter-markdown):**
   - Must handle `<!--` when it appears **inside inline content** (not just at block boundaries)
   - The external scanner must be able to emit HTML_COMMENT tokens when parsing inline content
   - This prevents the block parser from seeing block-like structures (lists, headings, etc.) that are actually inside comments

2. **Inline parser (tree-sitter-markdown-inline):**
   - Handles `<!--` when parsing inline content as usual
   - This is the "normal" case for inline comments

## Revised Implementation Strategy

### Stage 1: Block Parser (CRITICAL - DO THIS FIRST)

The block parser is now **more critical** than initially thought because it needs to handle the cross-block-boundary case.

**Why block parser first:**
- It needs to consume comments that span what would otherwise be block boundaries
- Without this, documents like test 36 will fail at the block structure level
- The block parser uses the inline parser for paragraph content, so it's the outer layer

### Scanner Implementation for Block Parser

The block parser scanner needs to:

1. **Track when we're parsing inline content** (inside a paragraph, heading, etc.)
2. **Emit HTML_COMMENT tokens** when `<!--` is encountered in inline content
3. **Consume everything until `-->`** including newlines and what would be block markers

**Key difference from inline parser:**
- Block parser must **consume newlines and block markers** inside comments
- Inline parser only consumes inline content

### Grammar Integration

**In tree-sitter-markdown grammar:**

The HTML comment token needs to be available in **inline contexts**, not just as a standalone block:

```javascript
// WRONG (original design):
html_comment_block: $ => seq($._html_comment, choice($._newline, $_eof)),

_block_not_section: $ => choice(
    // ...
    $.html_comment_block,  // Only handles block-level comments
),

// RIGHT (revised design):
// HTML comments are handled at the inline level
// The block parser's inline content parsing will consume them
// No separate html_comment_block needed!
```

The key insight: **HTML comments are inline elements that can contain newlines and block-like text**. They're not block elements themselves (unless they stand alone on a line).

## Updated Test Classification

### Tests 1-35: Original tests (mostly inline-focused)

These test inline comments and simple block-level comments.

### Tests 36-48: NEW - Cross-boundary tests (THE CRITICAL ONES)

These test the case where comments span across what would be block boundaries:

- **Test 36**: Comment spans paragraph → list → paragraph (YOUR EXAMPLE)
- **Test 37**: Comment spans paragraph → heading
- **Test 38**: Comment spans through code block markers
- **Test 39**: Comment spans through blockquote
- **Test 40**: Comment spans multiple paragraphs
- **Test 41**: Comment spans blank lines
- **Test 42**: Comment spans fenced div
- **Test 43**: Comment spans thematic break
- **Test 44**: Comment starts at block boundary
- **Test 45**: Nested list inside comment
- **Test 46**: Comment spans pipe table
- **Test 47**: Multiple comments each spanning blocks
- **Test 48**: Unclosed comment to EOF spanning blocks

## How the Block Parser Handles This

### Current Block Parser Architecture

Looking at the block parser scanner, it has these states:
- `STATE_MATCHING` - at beginning of line, matching block structure
- `STATE_WAS_SOFT_LINE_BREAK` - last line break was inside a paragraph

### Where HTML Comments Fit

When the block parser is:
1. **Matching block structure** (start of line) - can encounter `<!--`
   - If followed by newline: standalone comment block
   - If followed by text: starts inline comment that may span blocks

2. **Inside paragraph/inline content** - can encounter `<!--`
   - This is where the cross-boundary case happens
   - Scanner must consume until `-->` **regardless of newlines**

### Modified Scanner Logic

The block parser scanner needs to:

```c
// In the block parser scan function
// When we see '<', check for HTML comment
if (lexer->lookahead == '<') {
    // Save position in case this isn't a comment
    // Try to parse HTML comment
    if (parse_html_comment(lexer, valid_symbols)) {
        // Successfully parsed HTML comment
        // The comment has consumed everything from <!-- to -->
        // including any newlines and block-like markers
        return true;
    }
    // Not a comment, continue with other parsing
}
```

### The parse_html_comment Function (Block Parser)

```c
static bool parse_html_comment(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[HTML_COMMENT]) {
        return false;
    }

    // Current position: '<'
    // Save start position
    lexer->mark_end(lexer);  // Mark in case we need to backtrack

    lexer->advance(lexer, false);
    if (lexer->lookahead != '!') {
        return false;
    }

    lexer->advance(lexer, false);
    if (lexer->lookahead != '-') {
        return false;
    }

    lexer->advance(lexer, false);
    if (lexer->lookahead != '-') {
        return false;
    }

    lexer->advance(lexer, false);

    // Now consume EVERYTHING until '-->'
    // This includes:
    // - newlines
    // - what looks like list markers
    // - what looks like headings
    // - ANY character

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
            }
        } else {
            // Consume ANY character, including:
            // - \n (newlines)
            // - * (list markers)
            // - # (heading markers)
            // - > (blockquote markers)
            // - etc.
            lexer->advance(lexer, false);
        }
    }

    // Unclosed comment - consumed until EOF
    lexer->mark_end(lexer);
    lexer->result_symbol = HTML_COMMENT;
    return true;
}
```

## Interaction Between Block and Inline Parsers

### How They Work Together

1. **Block parser** (tree-sitter-markdown):
   - Parses document structure
   - When it encounters a paragraph, it asks the **inline parser** to parse the paragraph content
   - If the paragraph content contains `<!--`, the **inline parser** will tokenize it
   - But the **block parser** also needs to know about `<!--` to handle cross-boundary cases

2. **Inline parser** (tree-sitter-markdown-inline):
   - Parses inline content within blocks
   - Handles `<!--` as an inline element
   - Does NOT see block structure

### The Key Question: Where Does the Comment Get Parsed?

For test 36:
```markdown
This is a paragraph <!-- this is a comment

* this list cannot be parsed
* this is still a comment --> and now this is the end of the paragraph.
```

**Scenario A: Block parser handles it**
- Block parser sees "This is a paragraph" and enters paragraph parsing mode
- Block parser's scanner sees `<!--` while in paragraph inline content
- Block parser's scanner consumes until `-->` (including the list markers)
- Block parser continues paragraph with "and now this is the end"
- ✅ This works!

**Scenario B: Inline parser handles it**
- Block parser sees "This is a paragraph" and calls inline parser for line 1
- Inline parser sees `<!--` but the line ends before `-->`
- Inline parser might treat this as unclosed and only consume line 1
- Block parser sees line 2-3 starting with `*` and tries to make a list
- ❌ This fails!

**Conclusion: The BLOCK parser must handle HTML comments that span lines**

But the INLINE parser should also handle comments for the simple case where they don't span lines.

## Revised Grammar Design

### Block Parser Grammar (tree-sitter-markdown)

```javascript
externals: $ => [
    // ... existing tokens ...
    $._html_comment,  // NEW
],

rules: {
    // HTML comment is an inline element
    // It gets parsed when the block parser is processing inline content
    // No separate block-level rule needed!

    // The paragraph rule will use inline parser which handles comments
    // But the block parser scanner needs the token defined so it can
    // emit it when appropriate
}
```

Actually, looking more carefully at the block parser: it delegates inline parsing to the inline parser. So:

**The block parser needs to detect `<!--` and consume it** to prevent block structure from being recognized inside comments, but **the inline parser is what actually produces the token** when parsing inline content.

### Wait, Let Me Reconsider...

Let me look at how the block and inline parsers interact:

1. Block parser parses block structure
2. For paragraph content, block parser creates an `inline` node
3. The `inline` node content is **parsed by the inline parser**

So for test 36:
- Block parser: "I see a paragraph starting with 'This is a paragraph'"
- Block parser: "The paragraph continues until I see a blank line or new block"
- Block parser: "Let me check... line 2-3 start with `*`, that's a list marker!"
- Block parser: "So the paragraph ends at line 1"
- **THIS IS WRONG** - the `<!--` should have prevented the list from being recognized

### The Real Solution

The **block parser** needs to handle HTML comments **at the scanner level** to prevent block structure recognition inside comments.

When the block parser is scanning line by line for block structure, it needs to:
1. Check if the line (or previous line) contains `<!--`
2. If so, enter "comment mode" and skip block structure recognition
3. Continue skipping until `-->`

This is similar to how it handles code blocks - inside a code block, list markers don't create lists.

### Implementation in Block Parser

The block parser scanner should:

1. **Add scanner state**: `inside_html_comment` flag
2. **When matching blocks**: Check for `<!--` and set flag
3. **While flag is set**: Skip all block structure recognition
4. **When `-->` seen**: Clear flag

```c
// In Scanner struct for block parser
typedef struct {
    // ... existing fields ...
    uint8_t inside_html_comment;  // NEW
} Scanner;

// In serialize/deserialize: add this field

// In scan function:
// Before trying to match block markers, check for HTML comment
if (lexer->lookahead == '<' && !s->inside_html_comment) {
    // Check for <!--
    // If found, set s->inside_html_comment = 1
    // Consume until --> and set s->inside_html_comment = 0
    // Or emit HTML_COMMENT token and let grammar handle it
}

// If inside_html_comment, skip block structure recognition
if (s->inside_html_comment) {
    // Don't recognize list markers, headings, etc.
    // Just consume characters until -->
}
```

## Final Design Decision

After thinking through this carefully, here's what we need:

### Block Parser (tree-sitter-markdown)

**Scanner changes:**
1. Add `HTML_COMMENT` token type
2. Add `parse_html_comment` function that consumes `<!-- ... -->`
3. **Check for `<!--` before processing block structure**
4. Emit `HTML_COMMENT` token when found
5. The token includes all content from `<!--` to `-->`

**Grammar changes:**
1. Add `$._html_comment` to externals
2. Add `html_comment` as an inline element choice
3. HTML comments can appear **within inline content of paragraphs, headings, etc.**

### Inline Parser (tree-sitter-markdown-inline)

**Scanner changes:**
1. Add `HTML_COMMENT` token type
2. Add `parse_html_comment` function (same as block parser)
3. Add case for `<` in scan switch

**Grammar changes:**
1. Add `$._html_comment` to externals
2. Add `html_comment: $ => $._html_comment`
3. Add to `_inline_element` choices

### The Key Difference

**Block parser**: Must handle comments that span multiple lines including block-like markers
**Inline parser**: Handles comments within single inline contexts

Both use the same scanning logic, but block parser is invoked at a different level of the parsing hierarchy.

## Updated Test Count

- **Original tests**: 35 files (tests 1-35)
- **Cross-boundary tests**: 13 files (tests 36-48)
- **Total**: 48 test files

All tests should pass after implementation.

## Implementation Order (REVISED)

1. **Start with block parser** (more critical than originally thought)
   - Handles the cross-boundary case
   - Tests 36-48 depend on this

2. **Then inline parser**
   - Handles simple inline comments
   - Tests 1-35 depend on this

3. **Then Pandoc integration**
   - Converts to AST
   - All tests need this for end-to-end validation

## Questions for User (Updated)

1. **Unclosed comments spanning blocks**: Consume until EOF or error? (Still relevant)

2. **Format name**: `"quarto-html-comment"` or `"html"`? (Still relevant)

3. **How should comments that span blocks be represented in AST?**
   - Option A: Single `RawInline` in the paragraph inline content
   - Option B: Special handling to split paragraph and continue after?
   - **Recommendation**: Option A - treat as inline element that happens to contain newlines

4. **Scanner state**: Should we track `inside_html_comment` state in block parser or rely on token emission?
   - **Recommendation**: Don't track state - emit token and let grammar handle it

## Success Criteria (Updated)

- [ ] All 48 test files parse successfully
- [ ] Test 36 (your example) produces a single paragraph with comment in middle
- [ ] Comments prevent block structure recognition inside them
- [ ] Original failing file from quarto-web parses
- [ ] No regressions in tree-sitter test suites

---

**This revised design correctly handles HTML comments that span block boundaries.**
