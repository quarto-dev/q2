# Plan: Fix Definition List False Positives on Grid Table Captions

## Problem Statement

The definition list detector incorrectly identifies grid table captions as definition lists.

**Test case:**
```bash
$ cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'external-sites/quarto-web/docs/websites/website-tools.qmd' --verbose
```

**Current output:**
```
Found 2 definition list(s)
✗ Definition list found
✗ Definition list found
```

**Actual issues:** Lines 243 and 303 contain `: {tbl-colwidths="[20,80]"}`, which are grid table caption attributes, NOT definition lists.

## Root Cause Analysis

### Grid Table Caption Syntax

Pandoc grid tables can have captions defined as:

```markdown
+--------+--------+
| Cell 1 | Cell 2 |
+========+========+
| Data 1 | Data 2 |
+--------+--------+

: {tbl-colwidths="[50,50]"}
```

The line starting with `:` is a table caption attribute, not a definition list definition.

### Current Code Behavior

**File:** `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`

**Lines 43-66:**
```rust
// Look for a definition item (line starting with `:   `)
if self.def_item_regex.is_match(line) && !line.starts_with("::") {
    // Found a definition item, now scan backwards to find the term
    let mut start_idx = i;

    // Skip back over any blank lines
    while start_idx > 0 && lines[start_idx - 1].trim().is_empty() {
        start_idx -= 1;
    }

    // The line before the blank lines should be the term
    if start_idx > 0 {
        start_idx -= 1;
    }

    // Check if the "term" is actually a table row
    if lines[start_idx].matches('|').count() >= 2 {
        // This is likely a table caption, not a definition list
        i += 1;
        continue;
    }
    ...
}
```

**Current logic:**
1. Find line matching `^:\s+` pattern
2. Scan backwards past blank lines to find "term"
3. Check if "term" contains 2+ pipes (table row detection)
4. If yes, skip as table caption

**Problem:**
Grid table borders also contain pipes, but they use `+` and `-` characters:
- `+--------+--------+` (border)
- `+========+========+` (header separator)

So when we find `: {tbl-colwidths...}` and scan backwards, we encounter:
- Blank line (line 242)
- Grid table border `+--------+...+` (line 241)

Line 241 contains pipes, so the current check (line 62) should catch it... but it doesn't!

Let me verify by looking at the actual pattern more carefully.

### Investigation

Looking at line 243 in website-tools.qmd:
```
241: +----------------+---------------------------------------------------------------------...+
242:
243: : {tbl-colwidths="[20,80]"}
```

Wait, line 241 DOES contain pipes. The check on line 62 should work. Let me trace through the logic more carefully:

1. Line 243 matches `^:\s+` ✓
2. Scan backwards from i=243:
   - start_idx = 243
   - Line 242 is blank, so start_idx = 242
   - Line 241 is not blank, so exit while loop
   - start_idx = 242 - 1 = 241
3. Check if lines[241] has 2+ pipes
   - Line 241: `+----------------+-----...+`
   - Contains pipes! Should return true!

But wait... the regex is `^:\s+`, which requires a space after the colon. Let me check line 243 again:

```
: {tbl-colwidths="[20,80]"}
```

There's a space between `:` and `{`, so the regex matches.

OH! I see the issue now. Let me re-read the scanning logic:

```rust
// Skip back over any blank lines
while start_idx > 0 && lines[start_idx - 1].trim().is_empty() {
    start_idx -= 1;
}

// The line before the blank lines should be the term
if start_idx > 0 {
    start_idx -= 1;
}
```

Starting with i=243 (the `:` line):
1. start_idx = 243
2. Loop: lines[242] is empty, so start_idx = 242
3. Loop: lines[241] is not empty, exit loop
4. start_idx = 242 - 1 = 241

So start_idx should be 241, which is the grid table border.

But wait, maybe there's ANOTHER blank line? Let me check the actual file more carefully around line 243:

Looking at the Read output earlier:
```
   241→+----------------+-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
   242→
   243→: {tbl-colwidths="[20,80]"}
```

So:
- Line 241: grid table border (has pipes)
- Line 242: blank
- Line 243: `: {tbl-colwidths...}`

The logic should work. Let me check if the issue is that grid table borders use `+`, not `|`... Oh wait! Grid table borders use BOTH:

`+--------+--------+`

The border character is `+`, but they're separated by sections. Actually, looking more carefully at line 241:

`+----------------+-----...+`

This DOES contain the pipe character `|`? No wait, it contains PLUS signs `+`, not pipes `|`.

Let me verify this distinction. The pipe character is `|` (vertical bar), and the plus is `+`. Grid table borders use:
- `+` for corners and intersections
- `-` for horizontal lines
- `|` for vertical separators in data rows

So:
- Data rows: `| Cell 1 | Cell 2 |` (contains `|`)
- Header separator: `+========+========+` (contains `+`, no `|`)
- Border lines: `+--------+--------+` (contains `+`, no `|`)

AH! So the grid table BORDER lines don't contain pipe characters, only the data ROWS do!

That's why the current check fails. When we scan backwards from `: {tbl-colwidths...}`, we find the grid table border `+---+---+`, which has NO pipes, so it's not caught by the pipe count check.

## Solution

We need to detect grid table borders, not just table rows with pipes.

Grid table borders have the pattern:
- Starts with `+`
- Contains `-` or `=` characters
- Contains more `+` characters
- Pattern: `+[-=][-=]+\+`

More precisely:
- `+` followed by sequence of (`-`/`=`/`+`) characters
- Must have at least 2 `+` characters total

### Option A: Check for Grid Table Border Pattern

Add a check for grid table border lines specifically.

```rust
// Check if this is a grid table border
let is_grid_table_border = lines[start_idx].starts_with('+')
    && lines[start_idx].matches('+').count() >= 2
    && (lines[start_idx].contains('-') || lines[start_idx].contains('='));

if is_grid_table_border {
    i += 1;
    continue;
}
```

**Pros:**
- Simple and clear
- Specific to grid tables
- No regex needed

**Cons:**
- Might have false positives if someone uses `+` and `-` in text

### Option B: Check for Any Table-Like Pattern

Combine the pipe check with grid table border check.

```rust
// Check if the "term" is actually a table row or border
let has_pipes = lines[start_idx].matches('|').count() >= 2;
let is_grid_border = lines[start_idx].starts_with('+')
    && lines[start_idx].matches('+').count() >= 2;

if has_pipes || is_grid_border {
    // This is likely a table caption, not a definition list
    i += 1;
    continue;
}
```

**Pros:**
- Handles both pipe tables and grid tables
- More comprehensive
- Still simple

**Cons:**
- Slightly more complex
- Multiple checks

### Option C: Use Regex for Grid Table Border

Use a regex pattern to more precisely identify grid table borders.

```rust
// In the struct initialization:
grid_border_regex: Regex::new(r"^\+[-=+\s]+\+$").unwrap(),

// In the detection:
if lines[start_idx].matches('|').count() >= 2
    || self.grid_border_regex.is_match(lines[start_idx]) {
    i += 1;
    continue;
}
```

**Pros:**
- More precise pattern matching
- Less likely to have false positives

**Cons:**
- Additional regex to maintain
- Slightly more complex

## Recommended Solution: Option B

Use a simple check for grid table borders combined with the existing pipe table check.

The key insight is that grid table borders:
1. Start with `+`
2. Contain at least 2 `+` characters (corners/intersections)

This is simple, clear, and unlikely to have false positives in normal markdown text.

### Implementation

**File:** `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`

**Change at lines 59-66:**

```rust
// Check if the "term" is actually a table row or grid table border
let has_pipes = lines[start_idx].matches('|').count() >= 2;
let is_grid_border = lines[start_idx].starts_with('+')
    && lines[start_idx].matches('+').count() >= 2;

if has_pipes || is_grid_border {
    // This is likely a table caption, not a definition list
    i += 1;
    continue;
}
```

### Test Cases

**Test 1: Grid table caption (should NOT be detected)**

```markdown
+--------+--------+
| Cell 1 | Cell 2 |
+========+========+
| Data 1 | Data 2 |
+--------+--------+

: {tbl-colwidths="[50,50]"}
```

**Expected:** No definition lists found

**Test 2: Actual definition list (should be detected)**

```markdown
Term with description

:   This is a definition for the term.
```

**Expected:** 1 definition list found

**Test 3: Pipe table caption (should NOT be detected)**

```markdown
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |

: This is a table caption {#tbl-test}
```

**Expected:** No definition lists found (already works with current pipe check)

**Test 4: Definition with plus signs (edge case)**

```markdown
Addition operation

:   The plus sign (+) is used for addition.
```

**Expected:** 1 definition list found

This should still work because:
- The term line is "Addition operation" (no `+` at start)
- Even if term had `+`, it wouldn't have multiple `+` characters in grid border pattern

**Test 5: Code block with grid-like pattern (edge case)**

```markdown
```
+--------+
| Header |
+--------+
```

:   This is a definition about the code above.
```

**Expected:** 1 definition list found

This is actually a legitimate definition list (definition of the code block), so it should be detected. The "term" is the code fence start (````), not the content inside the code block, so the grid border check wouldn't trigger.

Actually, wait. Let me trace through this:
1. Find `:   This is a definition...`
2. Scan back past blank line
3. Find line before blank line: `` ``` `` (code fence end)
4. Check if starts with `+`: No
5. Proceed with detection: Yes, this is a definition list ✓

Good, this edge case is fine.

## Summary

The fix is to add a check for grid table borders when scanning backwards from `:   ` lines. Grid table borders start with `+` and contain multiple `+` characters, which distinguishes them from normal text.

Add this check alongside the existing pipe table check to handle both pipe tables and grid tables.
