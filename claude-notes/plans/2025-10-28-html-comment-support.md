# HTML Comment Support in Quarto Markdown

**Date:** 2025-10-28
**Status:** Design Phase
**Goal:** Add robust HTML comment support to quarto-markdown parsers

## Problem Statement

HTML comments `<!-- ... -->` are not currently recognized by the quarto-markdown parsers. This causes parsing failures when comments contain markdown-special characters like `_`, `*`, `[`, etc. These characters are incorrectly interpreted as markdown syntax within comments.

### Example Failure

File: `external-sites/quarto-web/docs/authoring/_mermaid-theming.qmd:68`

```markdown
<!-- This comes from quarto-dev/quarto-cli/src/resources/formats/html/_quarto-rules.scss -->
```

Error: "Unclosed Emphasis" at the `_` in `_quarto-rules.scss`

## Requirements

### Functional Requirements

1. **Consume HTML comments atomically**: `<!-- ... -->` should be consumed as a single token
2. **Support inline context**: Comments within paragraphs and inline text
3. **Support block context**: Comments that comment out entire blocks (lists, paragraphs, etc.)
4. **Handle multi-line comments**: Comments spanning multiple lines
5. **Preserve source location**: Maintain accurate source mapping for comments
6. **Proper escaping**: Handle edge cases like nested `--`, `>`, etc.

### Behavioral Requirements

1. **Inline comments** should become `RawInline "quarto-html-comment" "<!-- content -->"`
2. **Block-level comments** should become `RawBlock "quarto-html-comment" "<!-- content -->"`
3. **Comments in code blocks** should remain literal (not parsed as comments)
4. **Malformed comments** should be handled gracefully (unclear if error or passthrough)

## Design Overview

### Three-Stage Approach

1. **Stage 1**: Block parser (tree-sitter-markdown)
2. **Stage 2**: Inline parser (tree-sitter-markdown-inline)
3. **Stage 3**: Pandoc AST generation (quarto-markdown-pandoc)

### Token Design

Both scanners will add a new external token type:

```c
// In both scanner.c files
typedef enum {
    // ... existing tokens ...
    HTML_COMMENT,  // New token type
} TokenType;
```

## Stage 1: Block Parser (tree-sitter-markdown)

### Scanner Changes

**File:** `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`

Add HTML comment scanning function:

```c
static bool parse_html_comment(TSLexer *lexer, const bool *valid_symbols) {
    if (!valid_symbols[HTML_COMMENT]) {
        return false;
    }

    // Current position: '<'
    lexer->advance(lexer, false);  // consume '<'

    if (lexer->lookahead != '!') {
        return false;
    }
    lexer->advance(lexer, false);  // consume '!'

    if (lexer->lookahead != '-') {
        return false;
    }
    lexer->advance(lexer, false);  // consume first '-'

    if (lexer->lookahead != '-') {
        return false;
    }
    lexer->advance(lexer, false);  // consume second '-'

    // Now consume everything until we find '-->'
    // Handle the case where we might see multiple '-' chars
    while (true) {
        if (lexer->eof(lexer)) {
            // Unclosed comment - mark end here and return true
            // (or return false to indicate error?)
            lexer->mark_end(lexer);
            lexer->result_symbol = HTML_COMMENT;
            return true;
        }

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
            lexer->advance(lexer, false);
        }
    }
}
```

Add to scan function:

```c
// In tree_sitter_markdown_external_scanner_scan
switch (lexer->lookahead) {
    case '<':
        if (parse_html_comment(lexer, valid_symbols)) {
            return true;
        }
        break;
    // ... existing cases ...
}
```

### Grammar Changes

**File:** `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`

1. Add to externals list:
```javascript
externals: $ => [
    // ... existing externals ...
    $._html_comment,
],
```

2. Add as a block element:
```javascript
_block_not_section: $ => choice(
    // ... existing blocks ...
    $.html_comment_block,
),
```

3. Add rule:
```javascript
html_comment_block: $ => seq(
    $._html_comment,
    choice($._newline, $._eof)
),
```

### Test Cases for Stage 1

- Block-level comment by itself
- Comment commenting out list items
- Comment commenting out paragraphs
- Multi-line block comment
- Comment with markdown syntax inside
- Unclosed comment at EOF

## Stage 2: Inline Parser (tree-sitter-markdown-inline)

### Scanner Changes

**File:** `crates/tree-sitter-qmd/tree-sitter-markdown-inline/src/scanner.c`

Add the same `parse_html_comment` function (or extract to shared code).

Add to scan function:

```c
static bool scan(Scanner *s, TSLexer *lexer, const bool *valid_symbols) {
    // ... existing code ...

    switch (lexer->lookahead) {
        case '<':
            if (parse_html_comment(lexer, valid_symbols)) {
                return true;
            }
            break;
        // ... existing cases ...
    }

    return false;
}
```

### Grammar Changes

**File:** `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`

1. Add to externals:
```javascript
externals: $ => [
    // ... existing externals ...
    $._html_comment,
],
```

2. Add as inline element:
```javascript
_inline_element: $ => choice(
    // ... existing inline elements ...
    $.html_comment,
),
```

3. Add rule:
```javascript
html_comment: $ => $._html_comment,
```

### Test Cases for Stage 2

- Inline comment in paragraph
- Comment with emphasis/strong/links inside
- Multiple comments in sequence
- Comment at start/end of line
- Comment in heading
- Comment in blockquote

## Stage 3: Quarto-Markdown-Pandoc Integration

### AST Conversion

**File:** `crates/quarto-markdown-pandoc/src/...` (find the inline/block conversion code)

Add handlers for `html_comment` and `html_comment_block` nodes:

```rust
// For inline context
"html_comment" => {
    let text = node.utf8_text(source)?;
    vec![Inline::RawInline(
        Format("quarto-html-comment".to_string()),
        text.to_string()
    )]
}

// For block context
"html_comment_block" => {
    let text = node.utf8_text(source)?;
    vec![Block::RawBlock(
        Format("quarto-html-comment".to_string()),
        text.trim_end().to_string()  // trim trailing newline
    )]
}
```

### Desugaring (Future Work)

Eventually, we may want to desugar these to actual HTML comments:

```rust
// In a desugaring pass:
RawInline(Format("quarto-html-comment"), content) =>
    RawInline(Format("html"), content)

RawBlock(Format("quarto-html-comment"), content) =>
    RawBlock(Format("html"), content)
```

## Edge Cases to Consider

### 1. Comment-like but not comments

```markdown
< !-- space after < -->
<! -- space after ! -->
<!- single dash ->
```

**Decision:** Do not recognize these as comments (fail to match in scanner)

### 2. Unclosed comments

```markdown
<!-- This comment never closes
```

**Decision:** Consume until EOF and mark as HTML comment (or error?)

### 3. Comments in code blocks

````markdown
```
<!-- This should NOT be a comment -->
```
````

**Decision:** Already handled - code blocks bypass inline parsing

### 4. Multiple dashes

```markdown
<!-- This has --- three dashes --->
<!-- This has ---- four dashes ---->
```

**Decision:** Scanner should handle this correctly by looking for '-->' specifically

### 5. False ending

```markdown
<!-- This has -> which is not an ending -->
<!-- This has -- > with space -->
```

**Decision:** Only recognize '-->' (no space) as ending

### 6. Empty comments

```markdown
<!-- -->
<!---->
```

**Decision:** Both should be valid HTML comments

## Implementation Plan

### Phase 1: Block Parser
- [ ] Add HTML_COMMENT token to scanner enum
- [ ] Implement parse_html_comment in block scanner
- [ ] Add external token to grammar
- [ ] Add grammar rules for html_comment_block
- [ ] Rebuild parser: `tree-sitter generate && tree-sitter build`
- [ ] Run existing tests: `tree-sitter test`
- [ ] Add new tests for HTML comments at block level
- [ ] Test with real files

### Phase 2: Inline Parser
- [ ] Add HTML_COMMENT token to inline scanner enum
- [ ] Implement parse_html_comment in inline scanner
- [ ] Add external token to inline grammar
- [ ] Add grammar rules for html_comment
- [ ] Rebuild parser: `tree-sitter generate && tree-sitter build`
- [ ] Run existing tests: `tree-sitter test`
- [ ] Add new tests for HTML comments at inline level
- [ ] Test with real files

### Phase 3: Pandoc Integration
- [ ] Find inline conversion code in quarto-markdown-pandoc
- [ ] Add handler for html_comment nodes
- [ ] Find block conversion code
- [ ] Add handler for html_comment_block nodes
- [ ] Test end-to-end with test-html-comments.qmd
- [ ] Test with the original failing file

## Testing Strategy

1. **Unit tests in tree-sitter**: Add test cases to tree-sitter test corpus
2. **Integration tests**: Test files in test-html-comments.qmd
3. **Real-world test**: The original failing file from quarto-web

## Questions for User

1. **Malformed comments**: Should we error on unclosed comments or consume until EOF?
2. **Format name**: Is "quarto-html-comment" the right format name? Should it be just "html"?
3. **Desugaring**: Should desugaring to regular HTML happen in pandoc integration or later?
4. **Error reporting**: How should we report errors for malformed comments?

## Code Locations

- Block scanner: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
- Block grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Inline scanner: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/src/scanner.c`
- Inline grammar: `crates/tree-sitter-qmd/tree-sitter-markdown-inline/grammar.js`
- Pandoc integration: TBD (need to locate)

## Related Files

- Test corpus: `test-html-comments.qmd` (created)
- Original failing file: `external-sites/quarto-web/docs/authoring/_mermaid-theming.qmd`

## Notes

- HTML comment syntax per HTML5 spec: `<!--` followed by any text not containing `--`, followed by `-->`
- However, many parsers are more lenient and allow `--` inside comments
- We should be lenient to match common practice
- The scanner approach ensures comments are atomic and can't be broken by markdown syntax inside
