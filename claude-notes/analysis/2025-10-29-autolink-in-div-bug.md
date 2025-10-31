# Autolink in Div Bug Analysis

## Summary

Autolinks (`<https://example.com>`) work correctly in regular paragraphs but fail to parse inside fenced divs (`:::`), producing a parse error.

## Test Cases

### Working: Autolink in regular paragraph
```markdown
<https://example.com>
```

**Pandoc output:**
```
Link ( "" , ["uri"] , [] ) [Str "https://example.com"] ("https://example.com" , "")
```

**Our parser output:**
```
Link ( "" , ["uri"] , [] ) [Str "https://example.com"] ("https://example.com" , "")
```
✅ Works correctly

### Working: Autolink in block quote
```markdown
> <https://example.com>
```
✅ Works correctly

### Working: Autolink in list
```markdown
- <https://example.com>
```
✅ Works correctly

### FAILING: Autolink in div
```markdown
::: {}

<https://example.com>

:::
```

**Expected (Pandoc):**
```
Div ( "" , [] , [] )
  [ Para
      [ Link ( "" , [ "uri" ] , [] )
          [ Str "https://example.com" ]
          ( "https://example.com" , "" )
      ]
  ]
```

**Actual (Our parser):**
```
Error: Parse error at line 3, column 2
```

## Tree-Sitter Analysis

### Block Parser Output

```
(fenced_div_block [0, 0] - [5, 0]
  (block_continuation [1, 0] - [1, 0])
  (paragraph [2, 0] - [3, 0]
    (inline [2, 0] - [2, 21]
      (ERROR [2, 1] - [2, 6]           ← ERROR NODE!
        (block_continuation [2, 1] - [2, 1])
        (key_value_key [2, 1] - [2, 6])))  ← Trying to parse "https" as key_value_key
    (block_continuation [3, 0] - [3, 0]))
  (block_continuation [4, 0] - [4, 0])))
```

### Inline Parser Output (standalone)

When parsing `<https://example.com>` standalone:
```
(inline [0, 0] - [1, 0]
  (uri_autolink [0, 0] - [0, 21])  ← Correctly recognized!
  (soft_line_break [0, 21] - [1, 0]))
```

## Key Observations

1. **The inline parser works correctly** when tested standalone
2. **The block parser creates an ERROR node** when the autolink appears inside a div
3. **The ERROR contains `key_value_key`** trying to match "https" at position [2, 1] - [2, 6]
4. **The `<` is being consumed** somewhere, leaving "https" to be parsed
5. **Position [2, 1]** means the error starts at the 'h' in 'https', not the '<'

## Grammar Analysis

### From `common.js`:

```javascript
raw_specifier: $ => /[<=][a-zA-Z_][a-zA-Z0-9_-]*/,
```

This regex matches:
- `<html>` (for raw attributes like `{<html>}`)
- `<https` (!!!!) ← This is the problem!

The `raw_specifier` starts with `[<=]`, meaning it can begin with `<` or `=`. The pattern `<https` matches this regex perfectly:
- Starts with `<`
- Followed by letter `h`
- Followed by letters/digits/hyphens `ttps`

### Uri Autolink Definition (inline grammar):

```javascript
uri_autolink: $ => /<[a-zA-Z][a-zA-Z0-9+\.\-][a-zA-Z0-9+\.\-]*:[^ \t\r\n<>]*>/,
```

This requires the FULL pattern including the closing `>`.

## The Conflict

There's a **precedence/ambiguity issue** between:

1. **`uri_autolink`**: `/<[a-zA-Z]...>/` (wants the full `<https://example.com>`)
2. **`raw_specifier`**: `/[<=][a-zA-Z_][a-zA-Z0-9_-]*/` (matches just `<https`)

When the parser sees `<https://example.com>`:
- The `uri_autolink` rule should match the entire string
- But somewhere, `raw_specifier` is consuming `<https`, leaving `://example.com>` unparsed

## Why Only in Divs?

The `fenced_div_block` grammar has:

```javascript
fenced_div_block: $ => seq(
  $._fenced_div_start,               // :::
  $._whitespace,
  choice($.info_string, $._qmd_attribute, "{}"),  // Attribute on first line
  $._newline,
  repeat($._block),                  // Content blocks
  ...
),
```

**Hypothesis**: There may be scanner state or parsing context that causes different token precedence inside divs vs. regular paragraphs. The block parser's external scanner might be affecting how the inline parser tokenizes content within div contexts.

## ROOT CAUSE IDENTIFIED

The problem is NOT a conflict between `uri_autolink` and `raw_specifier`. The actual issue is:

**When a line inside a fenced div starts with `<letter>`, the block parser tries to parse it as a `_caption_line` instead of a regular paragraph.**

### Evidence

1. Verbose parse output shows the block parser using `_caption_line_repeat1`
2. When the URL `<https://example.com>` appears, the block parser tokenizes it character-by-character:
   - `<` at position 0
   - `https` as a `_word`
   - `:` at position 6 - **TRIGGERS CAPTION PARSING!**
   - `//example.com>` continues as caption content

3. The caption rule expects: `_blank_line` + `:` + optional caption content
4. When the parser sees the `:` in `https:`, it mis-identifies it as the start of a caption

### Confirmation Tests

✅ `<https://example.com>` at start of line in div → **FAILS**
✅ `Visit <https://example.com>` with text before → **WORKS**
✅ `<https://example.com>` in regular paragraph → **WORKS**
✅ `<https://example.com>` in blockquote → **WORKS**
✅ `<https://example.com>` in list → **WORKS**

## The Real Problem

The issue is in how the block grammar handles lines that start with `<` inside fenced divs. The block parser is incorrectly entering caption parsing mode when it encounters certain patterns.

Looking at the grammar:

```javascript
caption: $ => prec.right(seq(
    $._blank_line,
    ':',
    optional(seq(
        $._whitespace,
        alias($._caption_line, $.inline)
    )),
    choice($._newline, $._eof),
)),
```

The caption should only trigger after a `_blank_line`, but something in the div context is allowing it to trigger incorrectly.

## Further Investigation

After testing with and without blank lines, the issue persists in both cases:
- Verbose output shows `_caption_line_repeat1` being used for ALL content inside divs
- When content is simple text like "hello world", the parser recovers successfully
- When content is `<https://example.com>`, the `:` in the URL triggers caption matching and causes parse failure

### Why Caption Parsing?

The parser appears to be using `_caption_line` tokenization for the first line of content inside divs, regardless of whether there's a blank line. This is visible in verbose output for both cases:
- **With blank line**: `_caption_line_repeat1` appears
- **Without blank line**: `_caption_line_repeat1` STILL appears

This suggests the grammar might have ambiguity where lines inside divs are being pre-parsed as potential caption lines, then recovered to paragraphs if they don't match caption syntax. The recovery fails when `:` appears in the line (like in URLs).

## Proposed Solutions

### Option 1: Prevent caption syntax inside fenced divs
Modify the grammar so that captions cannot appear inside `fenced_div_block`. This is the safest option if captions inside divs aren't needed.

### Option 2: Fix caption rule to require `:` at line start
Ensure the caption rule only matches when `:` is at the BEGINNING of a line, not in the middle. This might require using the scanner to detect line-start context.

### Option 3: Change block precedence inside divs
Adjust precedence so that `paragraph` is tried before `caption` inside fenced divs.

### Recommended Approach

**Option 2** seems most correct - captions should only trigger when `:` appears at the start of a line after a blank line. The current grammar allows the `:` to be matched anywhere in the line, causing false matches with URLs.

The fix likely requires:
1. Adding a scanner token or grammar constraint to ensure caption `:` is at line position 0
2. OR using a negative lookahead/different tokenization strategy
3. Writing tests to ensure both autolinks in divs AND real captions continue to work

## Next Steps

1. ✅ Identified root cause (caption line tokenization inside divs)
2. ✅ Confirmed it happens with and without blank lines
3. ⏳ Implement fix to ensure caption `:` must be at line start
4. ⏳ Write comprehensive test cases
5. ⏳ Verify fix doesn't break existing caption functionality

## Test File Locations

- `/Users/cscheid/today/autolinks-in-div.qmd` - Failing test case
- `/Users/cscheid/repos/github/cscheid/kyoto/test-autolink.qmd` - Comprehensive test cases
