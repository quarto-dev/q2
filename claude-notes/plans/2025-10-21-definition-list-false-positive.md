# Plan: Fix Definition List Detection False Positive on Table Captions

## Problem Statement

The `definition-lists` rule in qmd-syntax-helper incorrectly detects table captions as definition lists.

**Test case:** `external-sites/quarto-web/docs/websites/website-navigation.qmd`

**Output:**
```
Found 2 definition list(s)
```

**Actual issues:** 0 (both are table captions)

## Root Cause Analysis

### What is a Table Caption?

In Pandoc/Quarto markdown, table captions are specified using this syntax:

```markdown
| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |

: Caption text {#tbl-id .class key=value}
```

The caption line:
- Starts with `: ` (colon + space)
- Follows a table (pipe table or grid table)
- Often has attributes in curly braces `{...}`
- Contains caption text (can be empty if just attributes)

### What is a Definition List?

Definition lists use this syntax:

```markdown
Term 1

:   Definition for term 1

Term 2

:   Definition for term 2
```

Characteristics:
- Term on its own line
- Blank line(s) after term
- Definition starts with `:   ` (colon + 3+ spaces)
- No attributes typically

### Current Detection Logic

**File:** `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`

**Regex:** `^:\s+` (line 29)
- Matches any line starting with `:` followed by whitespace
- Too broad - matches both definition lists AND table captions

**Detection algorithm** (lines 40-127):
1. Find line matching `^:\s+` and not starting with `::`
2. Scan backwards to find "term" (previous non-blank line)
3. Scan forwards for more definition items

**Problem:** The algorithm doesn't distinguish between:
- Definition list: `:\s\s\s` (colon + 3+ spaces)
- Table caption: `:\s` followed by text/attributes

### False Positive Cases in Test File

**Case 1:** Lines 83-85
```markdown
| `menu` | List of navigation items... |

: {tbl-colwidths="30,70"}
```
- Line 85 starts with `: {`
- Matches `^:\s+`
- Line 83 (last table row) is treated as "term"
- ❌ FALSE POSITIVE

**Case 2:** Lines 524-526
```markdown
| `repo-link-rel` | The `rel` attribute... |

: {tbl-colwidths="\[40,60\]"}
```
- Line 526 starts with `: {`
- Matches `^:\s+`
- Line 524 (last table row) is treated as "term"
- ❌ FALSE POSITIVE

### Key Distinguishing Features

| Feature | Definition List | Table Caption |
|---------|----------------|---------------|
| Pattern | `:   ` (3+ spaces) | `: ` (1 space) |
| Prev line | Term (plain text) | Table row (`\|`) |
| Attributes | Rare | Common `{...}` |
| Context | Prose | After table |

## Solution Options

### Option A: Check for 3+ Spaces (Pandoc Standard)

Pandoc's definition list syntax **requires** 3 or more spaces after the colon:

```
:   Definition (3 spaces - valid)
:  Definition  (2 spaces - invalid, treated as paragraph)
: Definition   (1 space  - invalid, treated as paragraph)
```

**Change regex from:**
```rust
r"^:\s+"  // Matches : followed by any whitespace
```

**To:**
```rust
r"^:\s{3,}"  // Matches : followed by 3+ spaces/tabs
```

**Pros:**
- Simple one-line fix
- Follows Pandoc specification exactly
- Eliminates table caption false positives
- Most correct solution

**Cons:**
- Might miss malformed definition lists with only 1-2 spaces
- But those aren't valid anyway per Pandoc spec

### Option B: Check Previous Line for Table Row

Add logic to check if the previous non-blank line is a table row (contains `|`).

**Implementation:**
```rust
// After finding `: ` line, check if previous line is table
if start_idx > 0 && lines[start_idx].contains('|') {
    // This is likely a table caption, skip it
    i += 1;
    continue;
}
```

**Pros:**
- Explicitly checks for table context
- More semantic check

**Cons:**
- More complex
- Could have edge cases (what if there's a `|` in definition term?)
- Doesn't address the fundamental issue

### Option C: Check for Attributes Pattern

Table captions often start with `: {` (colon, space, opening brace).

**Implementation:**
```rust
// Skip lines that look like table captions: `: {`
if line.starts_with(": {") {
    i += 1;
    continue;
}
```

**Pros:**
- Targets the specific false positive pattern
- Simple check

**Cons:**
- Not comprehensive (table captions can have text before `{`)
- Band-aid solution
- Captions without attributes would still false positive

### Option D: Combination Approach

Combine Option A (strict spacing) with Option B (table check) for robustness.

**Pros:**
- Most robust
- Handles both spec-compliant and edge cases

**Cons:**
- More complex
- Probably overkill

## Recommended Solution: Option A

**Change the regex to require 3+ spaces after the colon.**

This is the correct fix because:
1. **Pandoc specification**: Definition lists require 3+ spaces
2. **Simple**: One-line change
3. **Comprehensive**: Fixes all table caption false positives
4. **Correct**: Aligns with what Pandoc actually recognizes

### Implementation

**File:** `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`

**Line 29:** Change regex pattern
```rust
// OLD:
def_item_regex: Regex::new(r"^:\s+").unwrap(),

// NEW:
def_item_regex: Regex::new(r"^:\s{3,}").unwrap(),
```

**Line 28:** Update comment
```rust
// OLD:
// Matches definition list items that start with `:` followed by spaces

// NEW:
// Matches definition list items that start with `:` followed by 3+ spaces (Pandoc spec)
```

### Additional Checks to Update

The code checks for `::` to avoid div fences. After changing the regex, also check lines 45, 82, 92:

**Line 45, 82, 92:** These check `&& !line.starts_with("::")`
- This is still needed for `::::` div fences
- Keep as is

**Line 91:** Check against the regex:
```rust
if j < lines.len()
    && self.def_item_regex.is_match(lines[j])
    && !lines[j].starts_with("::")
```
- With new regex, this will only match `:   ` (3+ spaces)
- No changes needed

## Testing

### Test Case 1: Table Caption (Should NOT Detect)
```markdown
| Header |
|--------|
| Cell   |

: {tbl-colwidths="50,50"}
```

**Before:** Detected as definition list ❌
**After:** Not detected ✓

### Test Case 2: Valid Definition List (Should Detect)
```markdown
Term

:   Definition
```

**Before:** Detected ✓
**After:** Still detected ✓

### Test Case 3: Invalid Definition List (Should NOT Detect)
```markdown
Term

: Definition with only 1 space
```

**Before:** Incorrectly detected ❌
**After:** Correctly not detected ✓
(This isn't a valid definition list per Pandoc spec)

### Test Case 4: Multiple Definitions
```markdown
Term 1

:   Definition 1

Term 2

:   Definition 2
```

**Before:** Detected ✓
**After:** Still detected ✓

### Real World Test
```bash
cargo run --bin qmd-syntax-helper -- check 'external-sites/quarto-web/docs/websites/website-navigation.qmd' --rule definition-lists --verbose
```

**Expected output:**
```
No definition lists found
```

## Verification Steps

1. Change regex pattern
2. Run on test file - should find 0 definition lists
3. Create test cases for actual definition lists
4. Verify those are still detected
5. Run existing tests

## References

- [Pandoc Manual - Definition Lists](https://pandoc.org/MANUAL.html#definition-lists)
  - Requires 3+ spaces or tab after `:`
- [Pandoc Manual - Table Captions](https://pandoc.org/MANUAL.html#extension-table_captions)
  - Caption starts with `:` followed by caption text

## Summary

The fix is simple: change regex from `^:\s+` to `^:\s{3,}` to match Pandoc's specification that definition list items require 3 or more spaces after the colon. This eliminates false positives on table captions (which use only 1 space) while correctly detecting actual definition lists.
