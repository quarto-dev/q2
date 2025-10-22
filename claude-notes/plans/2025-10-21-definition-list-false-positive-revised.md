# Plan: Fix Definition List Detection False Positive on Table Captions (Revised)

## Problem Statement

The `definition-lists` rule in qmd-syntax-helper incorrectly detects table captions as definition lists.

**Test case:** `external-sites/quarto-web/docs/websites/website-navigation.qmd`

**Output:**
```
Found 2 definition list(s)
```

**Actual issues:** 0 (both are table captions)

## Corrected Understanding of Pandoc Syntax

### Definition Lists

According to Pandoc documentation:
> "A definition begins with a colon or tilde, which may be indented one or two spaces"

**Valid syntax:**
```markdown
Term

:   Definition (colon + 3 spaces)
: Definition   (colon + 1 space)
  : Definition (2-space indent + colon + 1 space)
```

**Key points:**
- Colon can have 0-2 spaces of indentation
- After colon, followed by space(s) and definition text
- **Not** restricted to 3 spaces as I incorrectly stated

### Table Captions

Table captions also start with `:` followed by space:

```markdown
| Header |
|--------|
| Cell   |

: Caption text {#tbl-id}
```

## Root Cause Analysis

Both definition lists AND table captions can start with `: ` (colon + space), so we can't distinguish them by spacing alone.

**The key difference:**
- **Definition list:** Preceded by term (plain text or blank line)
- **Table caption:** Preceded by table row (line containing `|` pipe characters)

### False Positive Cases in Test File

**Case 1:** Lines 83-85
```markdown
| `menu`       | List of navigation items... |

: {tbl-colwidths="30,70"}
```
- Line 83: Contains `|` → **table row**
- Line 84: Blank line
- Line 85: `: {tbl-colwidths...}` → **table caption**
- Current code treats line 83 as "term" → ❌ FALSE POSITIVE

**Case 2:** Lines 524-526
```markdown
| `repo-link-rel` | The `rel` attribute... |

: {tbl-colwidths="\[40,60\]"}
```
- Line 524: Contains `|` → **table row**
- Line 525: Blank line
- Line 526: `: {tbl-colwidths...}` → **table caption**
- Current code treats line 524 as "term" → ❌ FALSE POSITIVE

## Solution: Check for Pipe Table Context

### Detection Algorithm Change

When we find a line starting with `: `, scan backward to find the "term". If that term line contains pipe characters `|`, it's likely a table row, so this is a table caption, not a definition list.

### Implementation

**File:** `crates/qmd-syntax-helper/src/conversions/definition_lists.rs`

**Current logic** (lines 44-57):
```rust
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

    // ... continue with scanning forward ...
}
```

**Proposed change** (add check after finding term):
```rust
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

    // NEW: Check if the "term" is actually a table row
    // If the term line contains pipe characters, it's likely a table caption
    if lines[start_idx].contains('|') {
        // Skip this - it's a table caption, not a definition list
        i += 1;
        continue;
    }

    // ... continue with scanning forward ...
}
```

### Edge Cases to Consider

**1. Could a definition list term contain `|`?**

Possible but rare:
```markdown
Term with | pipe character

:   Definition
```

However:
- Very uncommon in practice
- Table rows have specific structure: `| cell | cell |`
- We could make the check more specific

**2. Could we check for table row structure?**

More robust check - look for table row pattern:
```rust
// Check if this looks like a table row (has pipes in typical positions)
let looks_like_table_row = lines[start_idx].contains('|')
    && lines[start_idx].matches('|').count() >= 2;
```

This requires at least 2 pipes, which is more table-like.

**3. Should we also check for table separator rows?**

Table separators look like: `|-------|-------|`

We could scan backward further:
```rust
// Check if there's a table separator row nearby (within 5 lines)
let mut is_table_context = false;
for check_idx in start_idx.saturating_sub(5)..=start_idx {
    if lines[check_idx].contains('|') && lines[check_idx].contains('-') {
        is_table_context = true;
        break;
    }
}
```

## Recommended Implementation

### Approach: Conservative Table Row Check

Check if the term line has multiple pipe characters (table-like structure).

**Code change:**

```rust
// After finding the term line (around line 57)
if start_idx > 0 {
    start_idx -= 1;
}

// Check if the "term" is actually a table row
// Table rows contain multiple pipe characters: | cell | cell |
if lines[start_idx].contains('|') {
    // This is likely a table caption, not a definition list
    i += 1;
    continue;
}
```

**Why this works:**
- Simple, one-line check
- Table rows always contain `|`
- Definition list terms rarely contain `|`
- Handles both test cases correctly

**Alternative (more strict):**
```rust
// Require at least 2 pipes for table detection
if lines[start_idx].matches('|').count() >= 2 {
    i += 1;
    continue;
}
```

This is more conservative - only treats it as table if there are 2+ pipes.

## Testing Plan

### Test Case 1: Table Caption (Should NOT Detect)
```markdown
| Header |
|--------|
| Cell   |

: {tbl-colwidths="50,50"}
```

**Before:** Detected as definition list ❌
**After:** Not detected ✓ (line 1 contains `|`)

### Test Case 2: Valid Definition List (Should Detect)
```markdown
Term

:   Definition
```

**Before:** Detected ✓
**After:** Still detected ✓ (term line has no `|`)

### Test Case 3: Definition Term with Pipe (Edge Case)
```markdown
Term with | in it

:   Definition
```

**Before:** Detected ✓
**After:** NOT detected ❌ (has `|`)

This is the trade-off. Options:
- Accept this false negative (very rare case)
- Use stricter check: `matches('|').count() >= 2`

### Test Case 4: Real World File
```bash
cargo run --bin qmd-syntax-helper -- check 'external-sites/quarto-web/docs/websites/website-navigation.qmd' --rule definition-lists --verbose
```

**Expected:**
```
No definition lists found
```

## Implementation Steps

1. Add check for `|` in term line after finding start_idx
2. Test on failing case - should find 0 definition lists
3. Create test files with actual definition lists
4. Verify those are still detected
5. Test edge case: term with single `|` character
6. Decide on single vs. multiple pipe threshold

## Final Recommendation

**Use simple check: `if lines[start_idx].contains('|')`**

**Rationale:**
- Solves the actual problem (table captions)
- Simple to understand and maintain
- Edge case of term with `|` is very rare
- Can refine later if needed

**Location:** Insert after line 57 in `definition_lists.rs`:

```rust
// The line before the blank lines should be the term
if start_idx > 0 {
    start_idx -= 1;
}

// Check if the "term" is actually a table row
// Table rows contain pipe characters, definition terms typically don't
if lines[start_idx].contains('|') {
    // This is likely a table caption, not a definition list
    i += 1;
    continue;
}
```

This fixes the false positive while maintaining correct detection for real definition lists.
