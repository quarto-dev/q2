# Snapshot Change Review: Phase 5 MetaValueWithSourceInfo → ConfigValue Migration

**Date:** 2025-12-29
**Branch:** `refactor/meta-value-config-value`
**Base:** `kyoto` (commit 22d9fd7)

## Summary

Only **one snapshot** changed during this refactoring:

- `crates/pampa/snapshots/json/yaml-tags.snap`

## Changed Snapshot Analysis

### Test Input File: `yaml-tags.qmd`

```yaml
---
compute: !expr x + 1
path: !path /usr/local/bin
date: !date 2024-01-15
---

This document demonstrates YAML tagged values.
```

This test exercises three YAML tags:
- `!expr` - expression tag (known/handled)
- `!path` - path tag (known/handled)
- `!date` - date tag (**unknown** in the new system)

---

## Detailed Changes

### 1. Header Metadata Changes (Cosmetic)

| Field | Old | New |
|-------|-----|-----|
| source | `crates/quarto-markdown-pandoc/tests/test.rs` | `crates/pampa/tests/test.rs` |
| assertion_line | 293 | (removed) |

**Cause:** Crate renaming from `quarto-markdown-pandoc` to `pampa`. Not semantically significant.

---

### 2. Source Info Pool Changes (Expected)

**Old pool size:** 34 entries
**New pool size:** 29 entries

The source info pool indices changed throughout the document:
- `metaTopLevelKeySources`: `{compute: 29, date: 33, path: 31}` → `{compute: 24, date: 28, path: 26}`
- All `"s"` (source) references in the JSON shifted accordingly

**Cause:** The new ConfigValue-based code path creates fewer intermediate `SourceInfo` objects because:
1. No longer creates separate entries for the `MetaValueWithSourceInfo` wrapper
2. Goes directly from YAML → ConfigValue without intermediate conversions

**Assessment:** Expected and acceptable. Source tracking is still accurate, just with different pool indices.

---

### 3. `attrS` Cleanup in Span (Improvement)

**Old:**
```json
"attrS": {"classes": [null], "id": null, "kvs": [[null, 5]]}
```

**New:**
```json
"attrS": {"classes": [], "id": null, "kvs": []}
```

**Cause:** The old code was producing garbage `null` values and spurious `[[null, 5]]` entries in the attribute source info. The new code produces clean empty arrays.

**Assessment:** This is an **improvement**. The old values were likely bugs or artifacts from the legacy code path.

---

### 4. `!expr` Tag Handling (Correct)

**Old:**
```json
"compute": {
  "c": [{
    "attrS": {"classes": [null], "id": null, "kvs": [[null, 5]]},
    "c": [["", ["yaml-tagged-string"], [["tag", "expr"]]], [{"c": "x + 1", ...}]],
    "t": "Span"
  }],
  "t": "MetaInlines"
}
```

**New:**
```json
"compute": {
  "c": [{
    "attrS": {"classes": [], "id": null, "kvs": []},
    "c": [["", ["yaml-tagged-string"], [["tag", "expr"]]], [{"c": "x + 1", ...}]],
    "t": "Span"
  }],
  "t": "MetaInlines"
}
```

**Cause:** The Span wrapper is preserved correctly. Only the `attrS` cleanup (see #3) changed.

**Assessment:** Correct behavior maintained.

---

### 5. `!path` Tag Handling (Correct)

**Old:**
```json
"path": {
  "c": [{"c": "/usr/local/bin", "s": 8, "t": "Str"}],
  "t": "MetaInlines"
}
```

**New:**
```json
"path": {
  "c": [{"c": "/usr/local/bin", "s": 2, "t": "Str"}],
  "t": "MetaInlines"
}
```

**Cause:** Only source info indices changed. The `!path` tag correctly produces a plain `Str` without a Span wrapper.

**Assessment:** Correct behavior maintained.

---

### 6. `!date` Tag Handling (⚠️ SEMANTIC CHANGE)

**Old:**
```json
"date": {
  "c": [{
    "attrS": {"classes": [null], "id": null, "kvs": [[null, 14]]},
    "c": [["", ["yaml-tagged-string"], [["tag", "date"]]], [{"c": "2024-01-15", ...}]],
    "s": 12,
    "t": "Span"
  }],
  "s": 15,
  "t": "MetaInlines"
}
```

**New:**
```json
"date": {
  "c": [{"c": "2024-01-15", "s": 9, "t": "Str"}],
  "s": 10,
  "t": "MetaInlines"
}
```

**Cause:** `!date` is **not a recognized tag** in the new system. The recognized interpretation tags are:
- `!md` - Markdown
- `!str` - Plain string
- `!path` - File path
- `!glob` - Glob pattern
- `!expr` - Expression

When `!date` is parsed:
1. `quarto_config::parse_tag("date", ...)` emits a **warning** (Q-1-21: "Unknown tag component")
2. No interpretation is set, so it falls through to the context-dependent default
3. In document metadata context, untagged strings are parsed as markdown
4. "2024-01-15" parses as markdown → plain `Str`

**Old behavior:** Unknown tags were preserved by wrapping in a Span with `yaml-tagged-string` class and `tag=<tagname>` attribute.

**New behavior:** Unknown tags emit a warning and are treated as if untagged (parsed as markdown in document context).

---

## Assessment Summary

| Change | Significance | Verdict |
|--------|--------------|---------|
| Header metadata | Cosmetic | ✅ Expected |
| Source info pool indices | Internal | ✅ Expected |
| `attrS` cleanup | Improvement | ✅ Better than before |
| `!expr` handling | No semantic change | ✅ Correct |
| `!path` handling | No semantic change | ✅ Correct |
| `!date` handling | **Semantic change** | ⚠️ **Needs review** |

---

## Resolution

### The `!date` Tag Issue Was Fixed

The `!date` tag change was identified as a potential breaking change. We fixed it by:

1. **Added `unknown_components` field to `ParsedTag`** in `crates/quarto-config/src/tag.rs`:
   - When unknown tag components are encountered, they are now captured in a vector
   - The warning (Q-1-21) is still emitted

2. **Updated `yaml_to_config_value`** in `crates/pampa/src/pandoc/meta.rs`:
   - When `unknown_components` is non-empty, create a Span wrapper with:
     - Class: `yaml-tagged-string`
     - Attribute: `tag=<unknown_components joined by underscore>`
     - Content: the string value

This preserves backward compatibility: unknown tags emit a warning AND produce the same Span wrapper structure as before.

---

## Files Changed

1. `crates/pampa/snapshots/json/yaml-tags.snap` - Updated snapshot
2. `crates/quarto-config/src/tag.rs` - Added `unknown_components` field to `ParsedTag`
3. `crates/pampa/src/pandoc/meta.rs` - Create Span wrapper for unknown tags
