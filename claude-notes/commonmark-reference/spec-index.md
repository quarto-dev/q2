# CommonMark Spec Annotated Index

**Spec version:** 0.31.2 (2024-01-28)
**Spec location:** `external-sources/commonmark-spec/spec.txt`
**Author:** John MacFarlane

This annotated index provides quick navigation and notes relevant to the pampa/comrak comparison work in the `comrak-to-pandoc` crate.

---

## Table of Contents

1. [Introduction](#introduction) (lines 9-289)
2. [Preliminaries](#preliminaries) (lines 290-823)
3. [Blocks and Inlines](#blocks-and-inlines) (lines 825-866)
4. [Leaf Blocks](#leaf-blocks) (lines 867-3658)
5. [Container Blocks](#container-blocks) (lines 3670-5868)
6. [Inlines](#inlines) (lines 5870-9457)
7. [Appendix: Parsing Strategy](#appendix-parsing-strategy) (lines 9459-9800+)

---

## 1. Introduction

**Lines 9-289**

### What is Markdown? (lines 11-102)
Background on Markdown's history and design philosophy. Emphasizes readability.

### Why is a spec needed? (lines 103-255)
Lists 14 ambiguous cases that the original Markdown description doesn't resolve:
1. Sublist indentation
2. Blank lines before block quotes/headings
3. Blank lines before indented code blocks
4. Tight vs loose list rules
5. List marker indentation
6. Thematic break in list items
7. List marker type changes
8. **Inline precedence rules** - relevant to our work
9. **Emphasis precedence** - relevant to our work
10. Block vs inline precedence
11. Headings in list items
12. Empty list items
13. Link refs in block quotes/list items
14. Multiple definitions for same reference

### About this document (lines 256-289)
- Examples serve as conformance tests
- `spec_tests.py` can run tests against any implementation
- `->` character represents tabs

---

## 2. Preliminaries

**Lines 290-823**

### Characters and lines (lines 292-342)
**Key definitions:**
- **character** - Unicode code point
- **line** - sequence of chars not including LF/CR, followed by line ending or EOF
- **line ending** - LF, CR (not followed by LF), or CRLF
- **blank line** - only spaces/tabs or empty
- **Unicode whitespace character** - Zs category, tab, LF, FF, CR
- **tab** - U+0009
- **space** - U+0020
- **ASCII punctuation character** - `!-/`, `:-@`, `[-``, `{-~`
- **Unicode punctuation character** - P or S category

### Tabs (lines 343-478)
Tabs are NOT expanded to spaces, but behave as if replaced with spaces at 4-character tab stops for block structure.

### Insecure characters (lines 479-484)
U+0000 is replaced with U+FFFD.

### Backslash escapes (lines 485-622)
Any ASCII punctuation can be backslash-escaped.

**RELEVANCE:** Important for understanding when `\` creates LineBreak vs literal backslash.

### Entity and numeric character references (lines 623-823)
HTML entities (`&nbsp;`, `&#32;`, `&#x20;`) are recognized.

---

## 3. Blocks and Inlines

**Lines 825-866**

### Precedence (lines 834-859)
Block structure is determined before inline structure. Inline parsing happens within block boundaries.

### Container blocks and leaf blocks (lines 860-866)
- **Container blocks:** Can contain other blocks (block quotes, lists)
- **Leaf blocks:** Cannot contain other blocks (paragraphs, headings, code blocks, etc.)

---

## 4. Leaf Blocks

**Lines 867-3658**

### Thematic breaks (lines 872-1094)
`---`, `***`, `___` with optional spaces between characters.

**Key rules:**
- Up to 3 spaces of indentation allowed
- 4+ spaces = code block
- No other characters allowed
- Can interrupt paragraphs
- When `---` could be setext heading underline, setext wins

**RELEVANCE:** We disabled HR inside blockquotes in generators because pampa interprets `---` as YAML delimiter.

### ATX headings (lines 1096-1317)
`#` to `######` with required space/tab or end of line after.

**Key rules:**
- 1-6 `#` characters
- Leading/trailing content stripped of leading/trailing spaces
- Optional closing `#` sequence (must be preceded by space/tab)
- Up to 3 spaces of indentation allowed

**RELEVANCE:** We strip heading IDs in normalization (pampa generates them, comrak doesn't).

### Setext headings (lines 1318-1733)
Underlined headings with `=` (H1) or `-` (H2).

**Key rules:**
- Underline can have any number of `=` or `-`
- Up to 3 spaces of indentation allowed
- Cannot interrupt paragraphs (need blank line before)
- Has higher precedence than thematic break when ambiguous

### Indented code blocks (lines 1734-1933)
4+ spaces of indentation creates code block.

**Key rules:**
- Cannot interrupt a paragraph
- Blank lines preserved
- Content not parsed as inline

### Fenced code blocks (lines 1934-2359)
```` ``` ```` or `~~~` delimiters with optional info string.

**Key rules:**
- Opening fence: 3+ backticks or tildes
- Closing fence: at least as many characters as opening, same char
- Info string on first line (language)
- No info string on closing fence
- Closing fence can be omitted at end of document

**RELEVANCE:** We normalize code block attributes (keep only language class) and strip trailing newlines.

### HTML blocks (lines 2360-3180)
Raw HTML blocks. Seven types with different start/end conditions.

### Link reference definitions (lines 3181-3535)
`[label]: destination "title"` syntax.

**Key rules:**
- Label: Up to 999 characters, no unescaped `[` or `]`
- Destination: In angle brackets or raw (balanced parens)
- Title: Optional, in quotes or parens
- First definition wins for duplicate labels

**NOTE:** qmd format does NOT support reference-style links (inline syntax only).

### Paragraphs (lines 3536-3645)
Consecutive non-blank lines that can't be interpreted as other block types.

### Blank lines (lines 3646-3668)
Separate block-level elements.

---

## 5. Container Blocks

**Lines 3670-5868**

### Block quotes (lines 3690-4118)
`>` prefix.

**Key rules:**
1. **Basic case:** Prepend `>` to sequence of blocks
2. **Laziness:** Can omit `>` for paragraph continuation text
3. **Consecutiveness:** Blank line separates block quotes

**RELEVANCE:** We found issues with:
- HR inside blockquotes (YAML delimiter interpretation in pampa)
- Consecutive lists inside blockquotes (markdown ambiguity)

### List items (lines 4119-5051)
**List markers:**
- Bullet: `-`, `+`, `*`
- Ordered: 1-9 digits followed by `.` or `)`

**Key rules:**
1. **Basic case:** Content after marker, subsequent lines indented to align
2. **Indentation:** Position of text after marker determines required indentation
3. **Interrupt paragraph:** Only if not starting with blank line, and ordered lists must start with 1
4. **Thematic breaks:** Line matching both thematic break and list item = thematic break

**RELEVANCE:** Critical for understanding tight vs loose lists and list continuation.

### Motivation (lines 5052-5237)
Explains list item rules using "four-space rule" vs "indentation-based" approach.

### Lists (lines 5238-5868)
**Key definitions:**
- **List:** Sequence of list items of same type (bullet or ordered)
- **Tight list:** No blank lines between items, items contain only block content
- **Loose list:** Any other list

**Tight vs loose:**
- Tight: Items wrapped in `<li>` only
- Loose: Items have `<p>` tags inside

**RELEVANCE:**
- Pampa uses Plain (tight), comrak uses Paragraph (loose) in some cases
- Consecutive lists with same marker type merge in markdown

---

## 6. Inlines

**Lines 5870-9457**

### Code spans (lines 5887-6119)
Backtick-delimited inline code.

**Key rules:**
- Opening and closing backtick strings must be equal length
- Line endings converted to spaces
- Leading/trailing space stripped if both present (and not all spaces)
- No backslash escapes inside code spans
- **Precedence:** Code spans > emphasis, links

**RELEVANCE:** Our property tests include code spans. Normalization handles code block trailing newlines.

### Emphasis and strong emphasis (lines 6120-7483)

**CRITICAL SECTION FOR PAMPA/COMRAK COMPARISON**

**Key definitions:**
- **Delimiter run:** Sequence of `*` or `_` not adjacent to same char
- **Left-flanking:** Not followed by whitespace, and either not followed by punctuation or preceded by whitespace/punctuation
- **Right-flanking:** Not preceded by whitespace, and either not preceded by punctuation or followed by whitespace/punctuation

**17 Rules for emphasis:**
1-4: Single `*` or `_` can open/close emphasis
5-8: Double `**` or `__` can open/close strong emphasis
9-10: Opening/closing delimiters must match, "rule of 3" for ambiguity
11-12: Literal `*`/`_` at boundaries need escaping
13: Minimize nesting (`<strong>` preferred over `<em><em>`)
14: `<em><strong>` preferred over `<strong><em>`
15-16: Overlapping spans, first/shorter takes precedence
17: Code, links, images, HTML > emphasis

**RELEVANCE:**
We disabled nested emphasis in generators because:
- `*some **strong** text*` - pampa produces separate Emph spans, comrak nests Strong inside Emph
- The spec allows both interpretations under rule 13-14

### Links (lines 7484-8553)
`[text](destination "title")` syntax.

**Key definitions:**
- **Link text:** Content between `[]`, can contain inline content
- **Link destination:** URL, can be in angle brackets or raw
- **Link title:** Optional, in quotes or parens

**Key rules:**
- Links can contain emphasis, code, images
- **Links CANNOT contain other links** - CommonMark spec
- Nested brackets require escaping
- Unbalanced brackets = not a link

**RELEVANCE:** We disabled autolinks in link content generation (spec forbids links in links).

### Images (lines 8554-8780)
`![alt](src "title")` syntax.

Same as links but with `!` prefix. Alt text can contain inline content.

### Autolinks (lines 8781-8967)
`<URL>` and `<email>` syntax.

**Key rules:**
- URI autolinks: `<scheme:path>` where scheme matches `[a-zA-Z][a-zA-Z0-9+.-]*`
- Email autolinks: Standard email pattern

**RELEVANCE:**
- Pampa adds "uri" class to autolinks, comrak doesn't (we strip in normalization)
- We disabled autolinks in generators because they create deeply nested structures

### Raw HTML (lines 8968-9243)
Inline HTML tags pass through.

### Hard line breaks (lines 9244-9393)

**CRITICAL FOR LINEBREAK HANDLING**

**Two syntaxes:**
1. Two or more spaces before line ending
2. Backslash before line ending

**Key rules:**
- Must not be at end of block (paragraph/heading)
- Works inside emphasis, links, images
- Does NOT work inside code spans or HTML tags
- Leading spaces on next line are ignored

**RELEVANCE:**
- `foo\` at end of paragraph = literal backslash, not hard break
- `foo\<newline>bar` in middle = hard break
- We found LineBreak as first/only content differs between parsers

### Soft line breaks (lines 9394-9428)
Regular line endings (not hard breaks) become soft breaks.
- May be rendered as space or line ending
- Trailing/leading spaces stripped

### Textual content (lines 9429-9457)
Everything else is plain text.

---

## 7. Appendix: Parsing Strategy

**Lines 9459-9800+**

### Overview (lines 9464-9501)
Two-phase parsing:
1. Block structure (line-by-line)
2. Inline structure (character-by-character)

### Phase 1: block structure (lines 9502-9643)
Build tree of blocks. Key concepts:
- Open vs closed blocks
- Block continuation
- Lazy continuation lines

### Phase 2: inline structure (lines 9644-9800+)
Parse inline content. Key algorithm:
- Delimiter stack for emphasis/links
- "look for link or image" procedure
- "process emphasis" procedure

---

## Key Findings for pampa/comrak Comparison

### Known Parser Differences (cannot normalize)

1. **Nested emphasis:** `*text **strong** text*`
   - Spec allows multiple valid interpretations (rules 13-14)
   - Pampa: separate Emph spans
   - Comrak: nests Strong inside Emph

2. **LineBreak at end of block:**
   - `foo\` at paragraph end: comrak = literal `\`, pampa = LineBreak
   - Spec says hard breaks don't work at end of block

3. **Consecutive lists merge:**
   - Two adjacent lists with same marker type merge
   - Markdown ambiguity, not a parser bug

### Normalized Differences (handled in normalize.rs)

1. **Heading IDs:** Pampa generates, comrak doesn't - stripped
2. **Figure wrapping:** Pampa wraps standalone images in Figure - unwrapped
3. **Autolink uri class:** Pampa adds, comrak doesn't - stripped
4. **Code block trailing newline:** Comrak includes, pampa doesn't - stripped
5. **Empty Spans:** Pampa wraps some content in empty-attr Spans - unwrapped
6. **Header leading/trailing spaces:** Stripped in normalization

### Generator Constraints (in generators.rs)

1. No consecutive lists (merge ambiguity)
2. No nested emph/strong (parsing difference)
3. No HR inside blockquotes (YAML delimiter in pampa)
4. No autolinks (deeply nested structures)
5. No linebreak in headers/first position (parsing differences)
6. No autolinks in link content (spec forbids links in links)

---

## Quick Reference: Line Numbers

| Section | Start Line |
|---------|------------|
| Introduction | 9 |
| Preliminaries | 290 |
| Characters and lines | 292 |
| Tabs | 343 |
| Backslash escapes | 485 |
| Blocks and inlines | 825 |
| Leaf blocks | 867 |
| Thematic breaks | 872 |
| ATX headings | 1096 |
| Setext headings | 1318 |
| Indented code blocks | 1734 |
| Fenced code blocks | 1934 |
| HTML blocks | 2360 |
| Link reference definitions | 3181 |
| Paragraphs | 3536 |
| Container blocks | 3670 |
| Block quotes | 3690 |
| List items | 4119 |
| Lists | 5238 |
| Inlines | 5870 |
| Code spans | 5887 |
| Emphasis and strong | 6120 |
| Links | 7484 |
| Images | 8554 |
| Autolinks | 8781 |
| Raw HTML | 8968 |
| Hard line breaks | 9244 |
| Soft line breaks | 9394 |
| Appendix: Parsing | 9459 |
