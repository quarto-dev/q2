# SourceInfo Usage Analysis
**Date:** 2025-10-22
**Context:** Understanding how Substring, Concat, and Transformed variants are actually used

## Executive Summary

After analyzing the entire codebase, I found:

1. **Transformed is NOT used in production code** - only in tests
2. **Substring is heavily used** - always reducible to Original with adjusted offsets
3. **Concat is used via `combine()`** - for coalescing adjacent text
4. **Text transformations exist** but are handled WITHOUT Transformed SourceInfo
5. **The current approach accepts imprecise mappings** for transformed text

## Key Finding: Transformed Variant is Unused

Despite being implemented, `SourceInfo::Transformed` has **zero production uses**:

```bash
$ grep -r "SourceInfo::transformed\|\.transformed(" --include="*.rs" crates/ | grep -v test
# Only test files found:
crates/quarto-source-map/src/source_info.rs (tests)
crates/quarto-source-map/src/mapping.rs (tests)
```

This strongly suggests that **Transformed is not actually needed** for the current use cases.

## Substring Usage Patterns

**Use Case 1: YAML Frontmatter Extraction**

Location: `crates/quarto-markdown-pandoc/src/pandoc/meta.rs:646`

```rust
// RawBlock contains: "---\ntitle: My Doc\n---"
// YAML content "title: My Doc" is at offsets 4 to (4 + content.len())
let yaml_parent = quarto_source_map::SourceInfo::substring(
    parent,              // Points to the entire RawBlock
    yaml_start,          // Offset 4
    yaml_start + content.len()
);
```

**Semantics:** The YAML content is a **contiguous substring** of the original file. This is exactly equivalent to an Original with adjusted offsets.

**Use Case 2: Nested YAML Parsing**

Location: `crates/quarto-yaml/src/parser.rs:219`

```rust
if let Some(ref parent) = self.parent {
    // We're parsing a substring - create a Substring mapping
    SourceInfo::substring(parent.clone(), start_offset, end_offset)
} else {
    // We're parsing an original file - create an Original mapping
    SourceInfo::original(file_id, range)
}
```

**Semantics:** When parsing YAML that's embedded in another file (like frontmatter), each YAML value gets a Substring pointing to its location within the parent. The chain eventually terminates at an Original.

**Use Case 3: Error Token Highlighting**

Location: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs:437`

```rust
// Calculate token position within the input
let token_byte_offset = calculate_byte_offset(&input_str, token.row, token.column);
let token_span_end = token_byte_offset + token.size.max(1);

// Create SourceInfo for this specific token
let token_source_info = quarto_source_map::SourceInfo::substring(
    source_info.clone(),
    token_byte_offset,
    token_span_end,
);
```

**Semantics:** Highlighting a specific token within a larger parse error context. Again, contiguous substring.

### Pattern: All Substrings are Reducible

**Every Substring use case has these properties:**
1. Points to a **contiguous range** in the parent text
2. Can be **resolved to an Original** by walking up the parent chain
3. Offsets are simple additions: `parent_offset + substring_offset`

**User's observation is correct:** These are all equivalent to Original with appropriately mapped offsets.

## Concat Usage Patterns

**Primary Use: Text Node Coalescing**

Location: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs:149,768`

```rust
// Case 1: Coalescing adjacent Str nodes with intervening spaces
let source_info = if did_coalesce {
    start_info.combine(&end_info)  // Creates Concat
} else {
    start_info
};

// Case 2: Merging consecutive Str nodes
if let Some(ref mut current) = current_str {
    current.push_str(&str_text);
    if let Some(ref mut info) = current_source_info {
        *info = info.combine(&s.source_info);  // Creates Concat
    }
}
```

**Example:**
```
Input text: "Hello  world" (with 2 spaces)
Tree-sitter produces:
  Str("Hello") at offsets 0-5
  Str(" ")     at offset 5-6
  Str(" ")     at offset 6-7
  Str("world") at offsets 7-12

After coalescing:
  Str("Hello  world") with Concat([
    SourceInfo(0-5),
    SourceInfo(5-6),
    SourceInfo(6-7),
    SourceInfo(7-12)
  ])
```

**Semantics:** Preserves the fact that the coalesced text came from multiple source locations, even though the result is a single string.

**Why Concat instead of Original?**

The pieces might come from different locations (though in practice they're always adjacent). Concat preserves the granular provenance.

**Alternative approach:** Since the pieces are always adjacent in practice, we could use `Original(file_id, start, end)` where start is the first piece's start and end is the last piece's end. This would be simpler but lose granularity.

## Text Transformation Analysis

**Smart Typography Transformations**

Location: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/text_helpers.rs:110`

```rust
pub fn apply_smart_quotes(text: String) -> String {
    text.replace('\'', "\u{2019}")  // ' → ' (1 byte → 3 bytes UTF-8)
}
```

Location: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/postprocess.rs:740-749`

```rust
fn as_smart_str(s: String) -> String {
    if s == "..." {
        "…".to_string()      // 3 bytes → 3 bytes (same!)
    } else if s == "--" {
        "–".to_string()      // 2 bytes → 3 bytes (grows!)
    } else if s == "---" {
        "—".to_string()      // 3 bytes → 3 bytes (same!)
    } else {
        s
    }
}
```

**Critical Finding: Transformations Don't Use Transformed SourceInfo!**

When applying these transformations, the code does this:

```rust
Inline::Str(Str {
    text: apply_smart_quotes(text),  // Text is transformed
    source_info: quarto_source_map::SourceInfo::original(
        context.current_file_id(),
        range,  // <-- Original range, NOT adjusted for new byte length!
    ),
})
```

**Example:**
```
Original file: "don't" at bytes 10-15 (5 bytes)
Transformed:   "don't" (7 bytes due to ' → ' transformation)
SourceInfo:    Points to bytes 10-15 (original range!)
```

**The SourceInfo points to a 5-byte range but the text is 7 bytes!**

### Current Approach: Accept Imprecision

The current code accepts that:
1. The transformed text's byte offsets don't match the SourceInfo
2. The SourceInfo points to the **approximate location** in the original file
3. We don't support mapping offsets within the transformed text back to precise original positions

**This is actually reasonable** because:
- Error messages only need to point to the general location ("the word 'don't' on line 5")
- We don't need character-level precision within transformed strings
- The complexity of Transformed SourceInfo isn't justified

## Implications for Redesign

### User's Observation is Correct

> "If there's a Substring pointing to an Original, that's equivalent to an Original with appropriately mapped offsets."

**Confirmed.** Every Substring chain resolves to an Original with computable offsets.

### The Transformed Dilemma

User's insight:
> "For Transformed nodes... imagine if a transformation introduced or removed line breaks; we would have no good way to write the code that would take an offset and produce row/col information."

**This is exactly right.** There are two approaches:

**Approach A: Complex Transformed SourceInfo (not currently used)**
```rust
// Transform "---" → "—"
let mapping = vec![RangeMapping {
    from_start: 0,
    from_end: 1,    // "—" is 1 char but 3 bytes in transformed text
    to_start: 0,
    to_end: 3,      // "---" is 3 bytes in original
}];
let transformed = SourceInfo::Transformed { parent, mapping };
```

Problems:
- Complex to build correctly
- Need to track byte vs character offsets
- Mapping offsets in transformed text is ambiguous if text has different line breaks

**Approach B: Imprecise Original SourceInfo (current approach)**
```rust
// Transform "---" → "—" but keep original range
Inline::Str(Str {
    text: "—",
    source_info: SourceInfo::original(file_id, original_range), // Points to "---"
})
```

Advantages:
- Simple
- Good enough for error reporting
- No complex mapping logic

Disadvantages:
- Can't map offsets within transformed text precisely
- SourceInfo range doesn't match text length

### User's Alternative: Anonymous Sources

User suggests:
> "We might just need to use more 'anonymous' sources... when we replace --- with em-dashes, we might simply have to point to the new string with an em-dash as a new 'leaf', an Original that is a constructed string."

**This would mean:**
```rust
// Create an anonymous "file" containing the transformed text
let anon_file_id = ctx.add_file("<anonymous>".to_string(), Some("—".to_string()));
let transformed_source = SourceInfo::Original {
    file_id: anon_file_id,
    start_offset: 0,
    end_offset: 3,  // Length of "—" in UTF-8
};
```

**Advantages:**
- SourceInfo accurately reflects the transformed text
- Can map offsets within transformed text precisely
- No need for Transformed variant

**Disadvantages:**
- Loses connection to original source location
- Error messages would show "<anonymous>" instead of the actual file
- Need to track relationship between anonymous source and original separately

### User's Pragmatic Alternative

> "Or, we might have to accept that strings that have been transformed will lack accurate source information, and simply make a judgment call about how to map them to the original file."

**This is what we currently do!** And it works fine because:
- We only need approximate locations for error messages
- The transformations are small and local
- Users understand "em-dash at line 5" even if the precise byte offset is slightly off

## Recommendations

Based on this analysis, here are my recommendations for the SourceInfo redesign:

### 1. Eliminate the Transformed Variant

**Rationale:**
- Not used anywhere in production code
- The complexity isn't justified for current use cases
- Text transformations work fine with imprecise SourceInfo

**Action:** Remove `SourceMapping::Transformed` entirely.

### 2. Keep Substring but Simplify Semantics

**Current semantics:** Substring can point to any SourceInfo (including other Substrings).

**Proposed semantics:** Substring is always a contiguous range that can be resolved to an Original.

**Design:**
```rust
pub enum SourceInfo {
    Original {
        file_id: FileId,
        start_offset: usize,
        end_offset: usize,
    },
    Substring {
        parent: Rc<SourceInfo>,
        start_offset: usize,  // Relative to parent
        end_offset: usize,    // Relative to parent
    },
    Concat {
        pieces: Vec<SourcePiece>,
    },
}
```

### 3. Keep Concat for Provenance Tracking

**Rationale:**
- Used for coalescing adjacent text nodes
- Preserves granular source information
- Useful for debugging and source tracking

**Alternative considered:** Merge adjacent SourceInfo into a single Original. This would work but loses useful provenance information.

### 4. Document the Imprecision for Transformed Text

Add explicit documentation that when text is transformed (smart quotes, dashes, etc.):
- The SourceInfo points to the **original** text location
- Byte offsets in the transformed text may not match the SourceInfo range
- This is acceptable for error reporting purposes

**Example comment:**
```rust
// NOTE: This text has been transformed from "---" to "—"
// The source_info points to the original "---" in the file
// The byte length doesn't match because "—" is 3 bytes in UTF-8
// This is acceptable since we only need approximate location for errors
Inline::Str(Str {
    text: "—",
    source_info: original_source_info,  // Points to "---"
})
```

### 5. Future: Consider Anonymous Sources if Precision is Needed

If we later need precise mapping within transformed text (unlikely), we can:
1. Add anonymous file support to SourceContext
2. Create anonymous files for transformed strings
3. Use Original SourceInfo pointing to those anonymous files

But this isn't needed now.

## Summary Table

| Variant | Production Uses | Can Resolve to Original? | Needed? |
|---------|----------------|-------------------------|---------|
| **Original** | Heavy (hundreds) | N/A - already Original | ✅ Yes |
| **Substring** | Heavy (~10 sites) | ✅ Yes, always | ✅ Yes |
| **Concat** | Moderate (~2 sites) | ✅ Yes (each piece resolves) | ✅ Yes |
| **Transformed** | **ZERO** | ❌ Not for different line breaks | ❌ No |

## Conclusion

The user's intuition is correct:
1. **Substring chains always resolve to Original** - they're equivalent to Original with offset arithmetic
2. **Transformed is problematic** for text with different line breaks
3. **Current code doesn't use Transformed** - instead accepts imprecise SourceInfo for transformed text
4. **This works fine** for error reporting needs

**Recommendation:** Eliminate Transformed variant. Keep Original, Substring, and Concat. Document that transformed text uses approximate SourceInfo pointing to original locations.
