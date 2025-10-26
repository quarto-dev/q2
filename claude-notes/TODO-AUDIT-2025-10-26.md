# TODO Audit Report - @quarto/annotated-qmd Package
Date: 2025-10-26

## Executive Summary

Found **7 TODOs** in source code (src/ and test/ directories). Of these:
- **3 are tracked** in existing beads task k-193 (list helper APIs)
- **4 are untracked** and need immediate attention

## Critical Finding: Document-Level Source Mapping

**PRIORITY: CRITICAL - User-reported bug**

**Location:** src/document-converter.ts:66-68

```typescript
// Try to get overall document source if we have file context
// For now, use empty MappedString as we don't track document-level source
const source = asMappedString('');
const start = 0;
const end = 0;
```

**Impact:**
- `parseRustQmdDocument()` returns AnnotatedParse with empty source
- User cannot access full document content from top-level result
- Breaking issue for interactive usage

**Not labeled as TODO but clearly incomplete implementation.**

**Reproduction test created:** test/document-level-source.test.ts

---

## Complete TODO Inventory

### 1. Document-level source mapping (UNTRACKED - CRITICAL)

**File:** src/document-converter.ts:66-68
**Status:** ❌ Not tracked in beads
**Priority:** CRITICAL (user-reported bug)

**Code:**
```typescript
// Try to get overall document source if we have file context
// For now, use empty MappedString as we don't track document-level source
const source = asMappedString('');
```

**Description:** Document AnnotatedParse has empty source, start=0, end=0 instead of full document span.

**Proposed Fix:** Compute document span from first/last elements in components array.

**Needs beads task:** YES - URGENT

---

### 2. YAML tagged value encoding (UNTRACKED)

**File:** src/meta-converter.ts:288-290
**Status:** ❌ Not tracked in beads
**Priority:** MEDIUM (enhancement)

**Code:**
```typescript
/**
 * Extract kind with special tag handling for YAML tagged values
 *
 * TODO: For now, use simple encoding like "MetaInlines:tagged:expr"
 * Future enhancement: Modify @quarto/mapped-string to add optional tag field
 * to AnnotatedParse interface, then use that instead
 */
```

**Description:** Current implementation encodes YAML tags in kind string (e.g., "MetaInlines:tagged:expr"). Future enhancement would add optional tag field to AnnotatedParse interface.

**Impact:** Current implementation works but isn't ideal for consumers who need to parse kind string.

**Needs beads task:** YES (enhancement)

---

### 3. Concat source location handling (UNTRACKED)

**File:** src/source-map.ts:267-268
**Status:** ❌ Not tracked in beads
**Priority:** LOW (edge case)

**Code:**
```typescript
case 2: // Concat - use first piece's resolution
  // TODO: Concat doesn't have a single file location, so we use the first piece
  // For error reporting, this may not be ideal
```

**Description:** Concat SourceInfo spans multiple pieces. Currently uses first piece's location. May cause confusion in error reporting if the error is in a later piece.

**Impact:** Minor - only affects error reporting precision for concatenated source regions.

**Needs beads task:** YES (low priority)

---

### 4. Circular reference detection (UNTRACKED)

**File:** src/source-map.ts:302-303
**Status:** ❌ Not tracked in beads
**Priority:** LOW (robustness)

**Code:**
```typescript
// TODO: Implement circular reference detection
// This would require tracking visited IDs during resolveChain traversal
```

**Description:** SourceInfo resolution doesn't detect circular references. Could cause infinite loops if malformed data is provided.

**Impact:** Low - only matters if Rust parser produces malformed SourceInfo pools (should never happen).

**Needs beads task:** YES (robustness, defensive coding)

---

### 5-7. List helper APIs (TRACKED in k-193)

**Files:**
- src/block-converter.ts:126
- src/block-converter.ts:140
- src/block-converter.ts:188

**Status:** ✅ Tracked in beads task k-193
**Priority:** MEDIUM

**Code:**
```typescript
// TODO: Create helper API to navigate list items (tracked in beads)
```

**Description:** BulletList, OrderedList, and DefinitionList have flattened components arrays. Need helper APIs to reconstruct structure.

**Beads task:** k-193 "Create helper APIs for navigating flattened list structures in AnnotatedParse"
**Task status:** OPEN

**Action needed:** Update TODO comments to reference k-193 explicitly: `// TODO (k-193): Create helper API...`

---

## Recommendations

### Immediate Actions

1. **Create beads task for document-level source mapping** (CRITICAL)
   - Fix src/document-converter.ts to compute document span
   - Update test/document-level-source.test.ts to pass
   - This is a user-reported bug blocking interactive usage

2. **Update TODO comments** to reference beads task IDs
   - Change "(tracked in beads)" to "(k-193)" for list helper TODOs
   - Improves traceability

3. **Create beads tasks for untracked TODOs:**
   - YAML tagged value encoding (enhancement)
   - Concat source location handling (low priority)
   - Circular reference detection (low priority)

### Policy Enforcement

Going forward, enforce CLAUDE.md policy:
- **NO TODO comments without corresponding beads tasks**
- **TODO comments MUST reference beads task ID: `// TODO (k-XXX): description`**
- **Stop and ask user before adding TODO/FIXME/HACK comments**

### Testing

- test/document-level-source.test.ts created and failing ✅
- All other TODOs lack test coverage
- Consider adding test cases for edge cases (Concat, circular refs)

---

## Summary Statistics

- Total TODOs found: 7
- Tracked in beads: 3 (k-193)
- Untracked: 4
- Critical priority: 1 (document source mapping)
- Medium priority: 4 (YAML tags, list helpers)
- Low priority: 2 (Concat, circular refs)

---

## Next Steps

1. Create beads task for document-level source mapping
2. Fix document-level source mapping (compute from components)
3. Create beads tasks for remaining 3 untracked TODOs
4. Update all TODO comments to reference beads task IDs
5. Run full test suite to ensure no regressions
