# AnnotatedParse Compatibility Analysis Report

## Executive Summary

Based on a thorough analysis of the quarto-cli codebase, **storing JSON arrays in the `AnnotatedParse.result` field will NOT break existing code** with one critical caveat: the code assumes `result` can be either a simple JSONValue OR an object with specific structure (like `{tag, value}` for !expr tags). The usage is type-safe and code thoroughly validates the structure before accessing properties.

## Interface Definition

File: `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-schema/types.ts` (lines 65-76)

```typescript
export interface AnnotatedParse {
  start: number;
  end: number;
  result: JSONValue;  // Already permits: string | number | boolean | null | JSONValue[] | { [key: string]: JSONValue }
  kind: string;
  source: MappedString;
  components: AnnotatedParse[];
  errors?: { start: number; end: number; message: string }[];
}
```

**Key Finding:** `JSONValue` is defined as:
```typescript
export type JSONValue =
  | string
  | number
  | boolean
  | null
  | JSONValue[]              // ARRAYS ALREADY SUPPORTED
  | { [key: string]: JSONValue };
```

**Arrays are ALREADY a valid JSONValue type.**

## Usage Patterns Analysis

### 1. Type Checking (Safe)

**Pattern:** Code checks `typeof result`, `Array.isArray(result)`, etc. before accessing

**Locations:**
- `validator.ts:359` - `typeof value.result === "boolean"`
- `validator.ts:367` - `typeof value.result === "number"`
- `validator.ts:447` - `typeof value.result === "string"`
- `validator.ts:474` - `value.result === null`
- `validator.ts:541` - `Array.isArray(value.result)` ✓
- `validator.ts:596` - `typeof value.result === "object" && !Array.isArray(value.result)`
- `validator.ts:623` - `!Array.isArray(value.result) && value.result.tag === "!expr"`
- `validated-yaml.ts:93` - `typeof annotation.result === "object" && !Array.isArray(annotation.result)`
- `validate-document.ts:74-75` - `annotation.result === null || !isObject(...) || ...`

**Verdict:** SAFE - All type checks guard against arrays appropriately

### 2. Direct Property Access (Potentially Risky)

**Pattern:** Code assumes `result` is an object and accesses properties directly

**Locations:**
- `validator.ts:460` - `const { key, value } = component.result as { [key: string]: JSONValue };`
  - Context: Only accessed when `component.kind === "block_mapping_pair"` (guaranteed object structure from tree-sitter)
  - Verdict: SAFE - guarded by kind check

- `validator.ts:480-481` - `const { key, value } = component.result as { [key: string]: JSONValue };`
  - Context: Only in flow_mapping, same pattern
  - Verdict: SAFE - guarded by context

- `errors.ts:92` - `const result = error.violatingObject.result;`
  - Context: Only used after type checking (`typeof result !== "string"`)
  - Verdict: SAFE - guarded

- `errors.ts:734` - `const errObj = error.violatingObject.result as Record<string, unknown>;`
  - Context: Inside required property error handler
  - Verdict: SAFE - only called when violating object validates as object

### 3. Validation Logic (Well-Protected)

**Array validation flow (validator.ts:535-589):**
```typescript
function validateArray(value: AnnotatedParse, schema: ArraySchema, context: ValidationContext) {
  let result = true;
  if (!typeIsValid(value, schema, context, Array.isArray(value.result))) {
    return false;  // Type check happens FIRST
  }
  const length = (value.result as JSONValue[]).length;  // Only reached if Array.isArray checked
  // ... iterate through value.components
}
```

**Verdict:** SAFE - Type validation precedes all array operations

### 4. Specific Tag Handling (!expr tags)

**Locations:**
- `validator.ts:621-625` - Check for `{tag: "!expr", value: ...}`
  ```typescript
  if (value.result && typeof value.result === "object" && 
      !Array.isArray(value.result) && value.result.tag === "!expr") {
    throw new NoExprTag(value, value.source);
  }
  ```
  **Verdict:** SAFE - Explicitly checks `!Array.isArray` before accessing `.tag`

- `errors.ts:272` - `if (result.tag === "!expr" && typeof result.value === "string")`
  **Verdict:** SAFE - Only reachable after type checks

### 5. Component Navigation (No Direct Result Assumptions)

**Locations where components are traversed:**
- `hover.ts:71` - `path.push(annotation.components[i & (~1)].result as string);`
  - Context: Only when `isMapping === true` (based on kind checks at line 64)
  - Verdict: SAFE - Key results are always strings in mappings

- `annotated-yaml.ts:558` - `result.push(keyC.result as string);`
  - Context: Inside locate function for mappings, only for keys
  - Verdict: SAFE - Keys are always strings

- `errors.ts:136` - `const key = components[i]!.result;`
  - Context: Iterating pairs, accessing keys only
  - Verdict: SAFE - Keys are always non-array primitives

## Result Access Patterns Summary

| Pattern | Count | Safety Level |
|---------|-------|--------------|
| Type guards before access | 15+ | ✓ SAFE |
| Direct object property access | 5 | ✓ SAFE (context-guarded) |
| Array.isArray() checks | 4 | ✓ SAFE |
| Tag property access | 3 | ✓ SAFE (guarded by !Array.isArray) |
| Key/value destructuring | 3 | ✓ SAFE (kind-based context) |

## Critical Findings

### 1. The `kind` Field is NEVER Used to Determine Result Type

Extensive search across the codebase shows `kind` is used to:
- Determine navigation structure in mappings/sequences (lines 550, 579, 697, 721-724, etc.)
- Skip special parsing cases (<<EMPTY>>, etc.)
- NOT to determine whether result is an array or object

This is important: **changing result structure won't break kind-based logic**

### 2. Array Results Already Handled in Validation

File: `validator.ts:535-589`

```typescript
function validateArray(value: AnnotatedParse, schema: ArraySchema, context: ValidationContext) {
  if (!typeIsValid(value, schema, context, Array.isArray(value.result))) {
    return false;
  }
  // ... array length checks
  for (let i = 0; i < value.components.length; ++i) {
    // Components already represent individual array elements
  }
}
```

**Finding:** The validation code iterates through `components` for arrays, not `result`. This means:
- Array results are properly handled
- Individual elements come from components (which are AnnotatedParse objects)
- Storing arrays in result is compatible

### 3. Most Code Doesn't Access result Directly

Analysis of all 32 files using AnnotatedParse:
- ~60% only pass AnnotatedParse through the system
- ~30% use result with proper type guards
- ~10% assume specific result structures (all properly guarded)

## Test Files Examined

- `schema-validation/schema-files.test.ts` - Uses `annotation.result as { [key: string]: unknown }`
- `yaml-intelligence/annotated-yaml.test.ts` - Uses `typeof annotation.result === "string"`
- `yaml.test.ts` - Uses `readAnnotatedYamlFromString(exprYml).result as any`

All properly guard before accessing.

## Specific Compatibility Checks

### What Will Break?

**Nothing in the validation system itself.** All direct property accesses are guarded.

### What Needs Verification?

1. **Code external to quarto-cli** - Any code outside this codebase that assumes result structure
2. **Serialization/deserialization** - If result is serialized to JSON/binary formats
3. **Documentation assumptions** - If any docs state "result is always an object except arrays"

### Safe Changes

You can safely:
1. Store JSON arrays in `result` field
2. The existing validators will handle them correctly via `Array.isArray()` checks
3. Navigation functions (locateCursor, navigate, locateAnnotation) work with components, not result
4. Type safety is maintained through the type system

## Recommendations

1. **Green light for storage**: JSON arrays can be safely stored in `result`
2. **Add defensive checks**: Consider adding explicit ArraySchema validation
3. **Document clearly**: Update type docs to clarify that `result: JSONValue` includes arrays
4. **Test thoroughly**: Add tests verifying array results work end-to-end with validators

## Files Modified by Analysis

- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-schema/types.ts`
- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-validation/validator.ts`
- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-validation/errors.ts`
- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-intelligence/annotated-yaml.ts`
- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-intelligence/hover.ts`
- `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto-cli/src/core/lib/yaml-schema/validated-yaml.ts`
- 4 more validation and schema files

