# Emoji Sequence Types: Complete Reference

**Date**: 2025-11-20
**Purpose**: Authoritative reference for emoji sequence support in tree-sitter grammar

## Official Sources

1. **Unicode TR51** (Unicode Technical Standard #51: Unicode Emoji)
   - URL: https://www.unicode.org/reports/tr51/
   - Defines all emoji sequence types

2. **emoji-sequences.txt** (Authoritative Data File)
   - URL: https://www.unicode.org/Public/emoji/latest/emoji-sequences.txt
   - Contains exact definitions of all valid sequences

3. **emoji-zwj-sequences.txt** (ZWJ Sequences)
   - URL: https://unicode.org/emoji/charts/emoji-zwj-sequences.html
   - Defines zero-width joiner sequences

4. **Reference Implementation**: mathiasbynens/emoji-regex
   - URL: https://github.com/mathiasbynens/emoji-regex
   - MIT licensed, comprehensive JavaScript implementation
   - Based on emoji-test-regex-pattern
   - Generates patterns directly from Unicode data files

## Emoji Sequence Types Overview

### 1. **Basic Emoji** (Already Supported ✅)

**Coverage**: `\p{Extended_Pictographic}`

**Examples**:
- Single pictographic characters: 🍎 🤗 ⌚
- Variation selectors: ↔️ (U+2194 + U+FE0F)

**Status**: Already handled by current EMOJI_REGEX in grammar.js

---

### 2. **Emoji Keycap Sequences** (NOT Supported ❌ - **Need to Add**)

**Official Definition** (from emoji-sequences.txt):
```
0023 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: #
002A FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: *
0030 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 0
0031 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 1
0032 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 2
0033 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 3
0034 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 4
0035 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 5
0036 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 6
0037 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 7
0038 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 8
0039 FE0F 20E3 ; Emoji_Keycap_Sequence ; keycap: 9
```

**Structure**:
- Base character: `#` (U+0023), `*` (U+002A), or `0-9` (U+0030-0039)
- Variation Selector: U+FE0F (optional but nearly always present)
- Combining Keycap: U+20E3 (required)

**Total**: Exactly 12 sequences

**Examples**: 0️⃣ 1️⃣ 2️⃣ 3️⃣ 4️⃣ 5️⃣ 6️⃣ 7️⃣ 8️⃣ 9️⃣ #️⃣ *️⃣

**Regex**: `[0-9#*]\uFE0F?\u20E3`

**Why Not Covered**: Base characters are ASCII, not in `\p{Extended_Pictographic}`

**Priority**: **HIGH** - This is what's breaking in the corpus (29+ errors)

---

### 3. **Emoji ZWJ Sequences** (Partially Supported ✅)

**Coverage**: Current EMOJI_REGEX handles some ZWJ sequences

**Structure**: `emoji + \u200D + emoji` (with optional modifiers/selectors)

**Current Regex**:
```javascript
\u200D\p{Extended_Pictographic}(\p{Emoji_Modifier}|\uFE0F)?
```

**Examples**:
- Family: 👨‍👩‍👧‍👦 (already in test suite, line 741)
- Profession: 👨‍⚕️ 👩‍🏫
- Multi-person: 👨‍❤️‍👨

**Status**: Already supported for pictographic bases

**Note**: There are ~1,400+ ZWJ sequences total, but current pattern handles the common cases

---

### 4. **Emoji Flag Sequences** (NOT Supported ❌ - Consider Adding)

**Structure**: Two Regional Indicator symbols (U+1F1E6-U+1F1FF)

**Examples**:
- 🇺🇸 = U+1F1E6 U+1F1F8 (US)
- 🇫🇷 = U+1F1EB U+1F1F7 (FR)
- 🇬🇧 = U+1F1EC U+1F1E7 (GB)

**Regex**: `[\u{1F1E6}-\u{1F1FF}]{2}`

**Total**: ~258 valid two-letter country codes

**Why Might Be Covered**: Regional indicators ARE in `\p{Extended_Pictographic}`, so the current regex might already handle pairs

**Priority**: **LOW** - Need to test if already working

---

### 5. **Emoji Modifier Sequences** (Already Supported ✅)

**Coverage**: `\p{Emoji_Modifier}` in current EMOJI_REGEX

**Structure**: `emoji + modifier`

**Modifiers**: U+1F3FB-U+1F3FF (5 skin tones)

**Examples**: 👍🏽 👩🏿 🤝🏻

**Current Regex**: `\p{Emoji_Modifier}`

**Status**: Already supported in line 61 of grammar.js

---

### 6. **Emoji Tag Sequences** (NOT Supported ❌ - Low Priority)

**Structure**: Base flag + tag characters + cancel tag

**Examples**:
- England: 🏴󠁧󠁢󠁥󠁮󠁧󠁿
- Scotland: 🏴󠁧󠁢󠁳󠁣󠁴󠁿
- Wales: 🏴󠁧󠁢󠁷󠁬󠁳󠁿

**Structure**: `U+1F3F4 + tag_spec + U+E007F`

**Priority**: **VERY LOW** - Rare, complex, probably not in corpus

---

## Recommendation: What to Add

### **Add Now** (Immediate Priority)

✅ **Keycap Sequences** (12 sequences)
- Simple, well-defined, causing actual errors
- Regex: `[0-9#*]\uFE0F?\u20E3`
- Impact: Fixes 29+ errors in corpus

### **Test Existing** (Before Adding)

🔍 **Flag Sequences** (258 sequences)
- Test if `\p{Extended_Pictographic}` already captures regional indicators
- If not working, easy to add: `[\u{1F1E6}-\u{1F1FF}]{2}`
- Check corpus for usage

### **Don't Add** (Not Needed)

❌ **Tag Sequences**
- Very rare (subdivision flags only)
- Complex structure
- No evidence in corpus
- High maintenance burden

❌ **Extended ZWJ Coverage**
- Current pattern already handles common cases
- Full coverage requires ~1,400+ sequence list
- Would need data file generation
- Probably overkill for markdown

## Current Grammar Analysis

**File**: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` line 61

**Current EMOJI_REGEX**:
```javascript
const EMOJI_REGEX =
  "(\\p{Extended_Pictographic}" +
  "(\\p{Emoji_Modifier}|\uFE0F)?" +
  "(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*" +
  ")";
```

**What This Covers**:
- ✅ Basic pictographic emoji
- ✅ Emoji with variation selectors (U+FE0F)
- ✅ Emoji with modifiers (skin tones)
- ✅ ZWJ sequences (family, professions, etc.)
- ❌ Keycap sequences (base is ASCII, not pictographic)
- ? Flag sequences (need to test)

## Proposed Addition

**Add to grammar.js**:
```javascript
// Emoji keycap sequences: digits 0-9, #, * + optional FE0F + keycap combiner
// Source: https://www.unicode.org/Public/emoji/latest/emoji-sequences.txt
// Type: Emoji_Keycap_Sequence (12 total sequences)
const KEYCAP_EMOJI_REGEX = "([0-9#*]\\uFE0F?\\u20E3)";

// Regular emoji with Extended_Pictographic property
const EMOJI_REGEX =
  "(\\p{Extended_Pictographic}" +
  "(\\p{Emoji_Modifier}|\uFE0F)?" +
  "(\u200D\\p{Extended_Pictographic}(\\p{Emoji_Modifier}|\uFE0F)?)*" +
  ")";
```

**Update PANDOC_REGEX_STR** (line 63-75):
```javascript
const PANDOC_REGEX_STR =
    regexOr(
        "\\\\.",
        KEYCAP_EMOJI_REGEX,  // Add keycap sequences FIRST (more specific)
        EMOJI_REGEX,          // Then regular emoji
        "[" + PANDOC_PUNCTUATION + "]",
        ...
```

**Why This Order**: Keycap regex must come first because it's more specific (starts with digit/symbol that might otherwise match other patterns).

## Testing Flag Sequences

Before adding flag support, test if current grammar handles them:

```bash
echo "🇺🇸 🇫🇷 🇬🇧" | tree-sitter parse -
```

If this works, no changes needed. If not, can add easily.

## References

- **Unicode TR51**: https://www.unicode.org/reports/tr51/tr51-27.html
- **emoji-sequences.txt**: https://www.unicode.org/Public/emoji/latest/emoji-sequences.txt
- **emoji-zwj-sequences.txt**: https://unicode.org/emoji/charts/emoji-zwj-sequences.html
- **emoji-regex (reference impl)**: https://github.com/mathiasbynens/emoji-regex (MIT license)
- **Emoji charts**: https://unicode.org/emoji/charts/emoji-sequences.html
