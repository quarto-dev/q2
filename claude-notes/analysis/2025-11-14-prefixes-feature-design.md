# Prefixes Feature Design

**Date**: 2025-11-14
**Status**: Design analysis for implementation

## Problem Statement

The error message system requires test cases in different parser contexts to capture different `lr_state` values. For example:
- `*a` at column 0 produces lr_state = 758
- `[*a` with `*` at column 1 produces lr_state = 863

Because runtime capture matching uses `(lr_state, sym)` pairs, we need separate test cases for each context. With 17 inline types and ~16 possible prefix contexts, this creates 272 potential combinations - too many to maintain manually.

## Proposed Solution

Add an optional `prefixes` field to case objects in the error corpus JSON files. The build script will automatically generate variant test cases for each prefix.

### JSON Format Extension

**Current format** (Q-2-12.json):
```json
{
  "code": "Q-2-12",
  "cases": [
    {
      "name": "simple",
      "content": "*Unclosed emphasis\n",
      "captures": [{"label": "emphasis-start", "row": 0, "column": 0, "size": 1}]
    },
    {
      "name": "in-link-text",
      "content": "[*a",
      "captures": [{"label": "emphasis-start", "row": 0, "column": 1, "size": 1}]
    }
  ]
}
```

**With prefixes**:
```json
{
  "code": "Q-2-12",
  "cases": [
    {
      "name": "simple",
      "content": "*Unclosed emphasis\n",
      "captures": [{"label": "emphasis-start", "row": 0, "column": 0, "size": 1}],
      "prefixes": ["[", "_", "^"]
    }
  ]
}
```

This single case generates 4 test files:
- `Q-2-12-simple.qmd` - original (no prefix)
- `Q-2-12-simple-prefix-bracket.qmd` - `[*Unclosed emphasis\n`
- `Q-2-12-simple-prefix-underscore.qmd` - `_*Unclosed emphasis\n`
- `Q-2-12-simple-prefix-caret.qmd` - `^*Unclosed emphasis\n`

### Assumptions and Constraints

Per user specification:
1. **Single-line content**: Cases using `prefixes` will have single-line content
2. **No row changes**: Prefixes are prepended to the same line (row 0)
3. **Column adjustment**: All captures on row 0 shift by `prefix.length`

### Implementation Design

#### 1. Code Structure

Current code structure (lines 132-187):
```typescript
for (const testCase of cases) {
  const { name, content, captures } = testCase;

  // Write file
  // Run parser
  // Match captures
  // Augment captures
  // Push to result
}
```

New structure:
```typescript
for (const testCase of cases) {
  const { name, content, captures, prefixes } = testCase;

  // Extract processing logic into helper function
  const processVariant = async (
    variantName: string,
    variantContent: string,
    variantCaptures: any[]
  ) => {
    // All the existing processing logic
    // - Write file
    // - Run parser
    // - Match captures
    // - Augment captures
    // - Push to result
  };

  // Always process base case
  await processVariant(name, content, captures);

  // Process prefixed variants if specified
  if (prefixes && Array.isArray(prefixes) && prefixes.length > 0) {
    for (let i = 0; i < prefixes.length; i++) {
      const prefix = prefixes[i];
      const variantName = `${name}-${i + 1}`;
      const variantContent = prefix + content;
      const variantCaptures = captures.map((cap: any) => ({
        ...cap,
        column: cap.column + prefix.length,
      }));

      await processVariant(variantName, variantContent, variantCaptures);
    }
  }
}
```

#### 2. Key Operations

**Filename generation**:
```typescript
const variantName = `${name}-${i + 1}`;
// Example: "simple-1", "simple-2", etc.
```

**Content generation**:
```typescript
const variantContent = prefix + content;
// "[" + "*a" = "[*a"
```

**Capture adjustment**:
```typescript
const variantCaptures = captures.map((cap: any) => ({
  ...cap,
  column: cap.column + prefix.length,
}));
// {row: 0, column: 0, size: 1} → {row: 0, column: 1, size: 1}
```

#### 3. Backward Compatibility

The implementation is fully backward compatible:
- Cases without `prefixes` field: processed as before (1 test file)
- Cases with `prefixes: []`: processed as before (1 test file)
- Cases with `prefixes: ["[", ...]`: processed as base + variants (N+1 test files)

### Example Transformation

**Input** (Q-2-12.json with prefixes):
```json
{
  "code": "Q-2-12",
  "title": "Unclosed Star Emphasis",
  "message": "I reached the end of the block before finding a closing '*' for the emphasis.",
  "notes": [...],
  "cases": [
    {
      "name": "simple",
      "content": "*a",
      "captures": [
        {"label": "emphasis-start", "row": 0, "column": 0, "size": 1}
      ],
      "prefixes": ["[", "_"]
    }
  ]
}
```

**Output** (generated files):
1. `case-files/Q-2-12-simple.qmd`:
   - Content: `*a`
   - Captures: `[{label: "emphasis-start", row: 0, column: 0, size: 1}]`

2. `case-files/Q-2-12-simple-1.qmd`:
   - Content: `[*a`
   - Captures: `[{label: "emphasis-start", row: 0, column: 1, size: 1}]`

3. `case-files/Q-2-12-simple-2.qmd`:
   - Content: `_*a`
   - Captures: `[{label: "emphasis-start", row: 0, column: 1, size: 1}]`

**Autogen table entries** (3 entries):
```json
[
  {
    "state": 758,
    "sym": "_close_block",
    "row": 0,
    "column": 2,
    "errorInfo": {
      "code": "Q-2-12",
      "captures": [{..., "lrState": 758, "sym": "emphasis_delimiter"}]
    },
    "name": "Q-2-12/simple"
  },
  {
    "state": 863,
    "sym": "_close_block",
    "row": 0,
    "column": 3,
    "errorInfo": {
      "code": "Q-2-12",
      "captures": [{..., "lrState": 863, "sym": "emphasis_delimiter"}]
    },
    "name": "Q-2-12/simple-1"
  },
  {
    "state": 954,
    "sym": "_close_block",
    "row": 0,
    "column": 3,
    "errorInfo": {
      "code": "Q-2-12",
      "captures": [{..., "lrState": 954, "sym": "emphasis_delimiter"}]
    },
    "name": "Q-2-12/simple-2"
  }
]
```

Each entry has a different `state` and different `lrState` in captures, allowing the runtime system to match the correct error in each context.

### Benefits

1. **Eliminates duplication**: One case with `prefixes: ["[", "_", "^", ...]` replaces 17 manual cases
2. **Automatic maintenance**: Adding a new prefix context is a one-line change
3. **Consistent naming**: Automated naming convention for variant cases
4. **Correct column adjustment**: Mechanical transformation reduces errors
5. **Full coverage**: Easy to test all 17 inline contexts systematically

### Testing Strategy

After implementation:
1. Modify Q-2-12.json to use prefixes
2. Run `./scripts/build_error_table.ts`
3. Verify generated files in `case-files/`
4. Run `cargo test` to ensure all tests pass
5. Test runtime error messages with prefix examples

### Edge Cases

1. **Empty prefix**: `""` is valid, produces no column shift
2. **Multi-character prefix**: `"[_"` works, shifts by 2
3. **Special characters**: All printable ASCII handled by sanitization
4. **No prefixes field**: Backward compatible, processes as before
5. **Empty array**: `prefixes: []` processes as before
6. **Multiple captures**: All adjusted uniformly

### Implementation Checklist

- [ ] Extract processing logic into `processVariant()` helper
- [ ] Add prefix expansion loop with counter-based naming after base case processing
- [ ] Test with Q-2-12.json using `prefixes: ["["]`
- [ ] Verify generated files and autogen table
- [ ] Run full test suite
- [ ] Update documentation in error-message-system.md

## Code Changes Required

### File: `scripts/build_error_table.ts`

**Location**: Lines 132-187 (case processing loop)

**Change type**: Refactor + enhancement

**Pseudocode**:
```typescript
// In case processing loop (replace lines 133-187):
for (const testCase of cases) {
  const { name, content, captures, prefixes } = testCase;

  const processVariant = async (...) => {
    // Current processing logic (lines 135-186)
  };

  // Base case
  await processVariant(name, content, captures);

  // Prefix variants
  if (prefixes?.length > 0) {
    for (let i = 0; i < prefixes.length; i++) {
      const prefix = prefixes[i];
      const variantName = `${name}-${i + 1}`;
      const variantContent = prefix + content;
      const variantCaptures = captures.map(cap => ({
        ...cap,
        column: cap.column + prefix.length
      }));
      await processVariant(variantName, variantContent, variantCaptures);
    }
  }
}
```

## Conclusion

The prefixes feature is a clean extension to the existing architecture that:
- Solves the exponential case growth problem
- Maintains backward compatibility
- Requires minimal code changes (refactor + ~15 new lines)
- Automates error-prone manual processes
- Enables systematic coverage of parser contexts
- Uses simple counter-based naming (no character mapping needed)

The implementation is straightforward because it follows the existing pattern: generate test files, run parser, match captures, build autogen table. The only new logic is the prefix expansion loop and column adjustment.
