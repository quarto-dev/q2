# Phase 4 Components Tree Validation Plan (k-228)

**Date**: 2025-10-26
**Status**: In Progress
**Owner**: Claude Code
**Parent Task**: k-192 (Phase 5: Write comprehensive tests for annotated Pandoc AST)
**Beads Issue**: k-228

## Objective

Systematically validate the component tree structure of AnnotatedParse nodes. Ensure proper nesting, ordering, and structural integrity across all document types.

## Current Context

After Phases 1-3:
- ✅ 118 tests passing
- ✅ DocumentConverter, complex documents, and edge cases tested
- **Gap**: No systematic validation of tree structure properties

## What is the Components Tree?

Every AnnotatedParse node has a `components` array containing child nodes:
- **Document**: `[metadata, ...blocks]`
- **Para**: `[...inlines]`
- **Header**: `[...attrComponents, ...inlines]`
- **BulletList/OrderedList**: `[...listItems]`
- **Emph/Strong**: `[...inlines]`
- **Link**: `[...inlines]` (link text)
- **Note**: `[...blocks]` (footnote content)

## Tree Properties to Validate

### 1. Structural Integrity
- All nodes have valid `components` array
- No undefined/null components
- No circular references
- Components are proper AnnotatedParse objects

### 2. Ordering Rules
- **Document**: Metadata (if present) is always first component
- **Lists**: Items appear in source order
- **Headers**: Attr components (if any) come before inline text
- **Table**: Caption, headers, body in correct order

### 3. Nesting Constraints
- Maximum depth is reasonable (< 50 levels)
- Block nodes contain appropriate children (blocks or inlines)
- Inline nodes contain only inline children (except Note which can have blocks)
- Metadata contains only meta values

### 4. Consistency
- Source ranges of children are within parent's range
- Children don't overlap in source positions
- Children appear in source order (start positions increasing)

### 5. Navigation Patterns
- Can traverse from root to leaves
- Can find all nodes of a specific kind
- Can extract text content from subtree
- Can compute depth of any node

## Implementation Plan

### Step 1: Create Tree Validation Helpers (45 min)

Create `test/helpers/tree-validators.ts`:

```typescript
/**
 * Validate basic tree structure
 */
export function validateTreeStructure(node: AnnotatedParse): void {
  // Node itself must be valid
  assert.ok(node !== undefined, 'Node should not be undefined');
  assert.ok(node !== null, 'Node should not be null');
  assert.ok('kind' in node, 'Node should have kind');
  assert.ok('components' in node, 'Node should have components');
  assert.ok(Array.isArray(node.components), 'Components should be array');

  // Recursively validate children
  for (const component of node.components) {
    validateTreeStructure(component);
  }
}

/**
 * Check for circular references using Set
 */
export function detectCircularReferences(node: AnnotatedParse, visited = new Set<AnnotatedParse>()): void {
  if (visited.has(node)) {
    throw new Error('Circular reference detected');
  }

  visited.add(node);

  for (const component of node.components) {
    detectCircularReferences(component, visited);
  }
}

/**
 * Validate component ordering (Document-specific)
 */
export function validateDocumentOrdering(doc: AnnotatedParse): void {
  assert.equal(doc.kind, 'Document', 'Should be Document');
  assert.ok(doc.components.length > 0, 'Document should have components');

  // If first component is metadata, check it comes first
  const firstComponent = doc.components[0];
  if (firstComponent.kind === 'mapping') {
    // Metadata is first - correct!
  } else {
    // No metadata, first should be a block
    assert.ok(isBlockKind(firstComponent.kind), 'First component should be a block if no metadata');
  }
}

/**
 * Validate children source ranges are within parent
 */
export function validateSourceRangeNesting(node: AnnotatedParse): void {
  for (const component of node.components) {
    // Child must be within parent's range
    assert.ok(component.start >= node.start,
      `Child start ${component.start} should be >= parent start ${node.start}`);
    assert.ok(component.end <= node.end,
      `Child end ${component.end} should be <= parent end ${node.end}`);

    // Recursively validate
    validateSourceRangeNesting(component);
  }
}

/**
 * Validate children appear in source order (non-overlapping)
 */
export function validateSourceOrdering(node: AnnotatedParse): void {
  for (let i = 0; i < node.components.length - 1; i++) {
    const current = node.components[i];
    const next = node.components[i + 1];

    // Next should start at or after current ends
    assert.ok(next.start >= current.start,
      `Components should be in source order: ${next.start} >= ${current.start}`);
  }

  // Recursively validate
  for (const component of node.components) {
    validateSourceOrdering(component);
  }
}

/**
 * Get maximum depth of tree
 */
export function getTreeDepth(node: AnnotatedParse): number {
  if (node.components.length === 0) {
    return 1;
  }

  let maxChildDepth = 0;
  for (const component of node.components) {
    const childDepth = getTreeDepth(component);
    maxChildDepth = Math.max(maxChildDepth, childDepth);
  }

  return 1 + maxChildDepth;
}

/**
 * Count total nodes in tree
 */
export function countTreeNodes(node: AnnotatedParse): number {
  let count = 1; // Count this node

  for (const component of node.components) {
    count += countTreeNodes(component);
  }

  return count;
}

/**
 * Extract all text content from tree (Str nodes only)
 */
export function extractTextContent(node: AnnotatedParse): string {
  if (node.kind === 'Str') {
    return node.result as string;
  }

  if (node.kind === 'Space') {
    return ' ';
  }

  let text = '';
  for (const component of node.components) {
    text += extractTextContent(component);
  }

  return text;
}

/**
 * Helper to check if kind is a block
 */
function isBlockKind(kind: string): boolean {
  const blockKinds = [
    'Plain', 'Para', 'Header', 'CodeBlock', 'RawBlock',
    'BlockQuote', 'OrderedList', 'BulletList', 'DefinitionList',
    'HorizontalRule', 'Table', 'Div', 'Null', 'Figure'
  ];
  return blockKinds.includes(kind);
}
```

### Step 2: Create test/tree-validation.test.ts (60 min)

Comprehensive test file with sections:

**Section 1: Basic Structure**
- Test tree structure validation on simple document
- Test tree structure on complex documents
- Test circular reference detection (should pass - no circular refs)
- Test all nodes are valid AnnotatedParse objects

**Section 2: Ordering Rules**
- Test Document ordering (metadata first if present)
- Test list items in source order
- Test header components (attr before inlines)
- Test children in source order (start positions)

**Section 3: Source Range Nesting**
- Test children within parent bounds
- Test no overlapping siblings
- Test source ordering of components
- Test on deeply nested structures

**Section 4: Tree Metrics**
- Test maximum depth calculation
- Test total node count
- Test depth is reasonable (< 50 for all test docs)
- Test node count matches expectations

**Section 5: Navigation Patterns**
- Test find all nodes of kind
- Test extract text content
- Test traversal from root to leaves
- Test breadth-first and depth-first traversal

**Section 6: Type Constraints**
- Test block nodes contain appropriate children
- Test inline nodes contain only inlines (except Note)
- Test metadata contains only meta values
- Test mixing constraints are enforced

### Step 3: Enhance Existing Tests (30 min)

Add tree validation calls to existing test files:
- `test/block-types.test.ts` - Add `validateTreeStructure()` calls
- `test/inline-types.test.ts` - Add `validateTreeStructure()` calls
- `test/complex-documents.test.ts` - Add comprehensive tree validation
- `test/edge-cases.test.ts` - Ensure edge cases have valid trees

### Step 4: Run and Verify (15 min)

```bash
npm test
```

Expected: ~20-25 new tests, all passing, total ~138-143 tests.

## Test Examples

```typescript
test('tree structure validation - simple document', () => {
  const json = loadExample('simple');
  const doc = parseRustQmdDocument(json);

  // No errors should be thrown
  validateTreeStructure(doc);
  detectCircularReferences(doc);
  validateDocumentOrdering(doc);
  validateSourceRangeNesting(doc);
  validateSourceOrdering(doc);
});

test('maximum tree depth is reasonable', () => {
  const json = loadExample('tutorial'); // Has nested callouts
  const doc = parseRustQmdDocument(json);

  const depth = getTreeDepth(doc);
  assert.ok(depth < 50, `Tree depth ${depth} should be < 50`);
  assert.ok(depth >= 5, 'Tutorial should have some nesting');
});

test('children appear in source order', () => {
  const json = loadExample('blog-post');
  const doc = parseRustQmdDocument(json);

  // Validate entire tree
  validateSourceOrdering(doc);
});

test('extract text content from paragraph', () => {
  const json = loadExample('simple');
  const doc = parseRustQmdDocument(json);

  // Find first Para
  const paras = findNodesByKind(doc, 'Para');
  const firstPara = paras[0];

  const text = extractTextContent(firstPara);
  assert.ok(text.length > 0, 'Should extract text');
  assert.ok(text.includes('Quarto'), 'Should contain expected text');
});
```

## Success Criteria

✅ Tree validation helpers created
✅ 20-25 new tree validation tests
✅ All 6 sections covered (structure, ordering, nesting, metrics, navigation, constraints)
✅ Existing tests enhanced with tree validation
✅ All tests pass (138-143 total)
✅ No circular references detected
✅ All trees have reasonable depth (< 50)
✅ Source range nesting validated

## Risk Assessment

**Low Risk**:
- Tree structure is already working (118 tests pass)
- This phase adds additional validation only

**Medium Risk**:
- Source ordering might have edge cases (e.g., metadata vs blocks)
- Text extraction might be tricky for complex nodes

**If tests fail**:
- Document the failure
- Create new beads issues for structural bugs found
- Don't fix inline - use TDD approach

## Estimated Effort

- Helper functions: 45 min
- Test file creation: 60 min
- Enhance existing tests: 30 min
- Run and verify: 15 min
- **Total: ~2.5 hours**

## Dependencies

- Phases 1-3 completed ✅
- `loadExample()` helper
- `findNodesByKind()` helper
- `parseRustQmdDocument()` function

## Deliverables

1. `test/helpers/tree-validators.ts` - Reusable validation helpers
2. `test/tree-validation.test.ts` - Comprehensive tree validation tests
3. Enhanced existing test files with tree validation calls
4. Documentation of any structural issues found

## Notes

- This phase focuses on **structural validation**, not fixing bugs
- Tree validation can be reused in other projects
- Helps ensure AnnotatedParse trees are always well-formed
- Good for catching regressions in tree construction

## Next Steps After Phase 4

If Phase 4 completes successfully:
- Consider Phase 5 (k-229): Performance baseline (optional)
- Consider Phase 6 (k-230): Documentation (optional)
- Close k-192 if satisfied with test coverage
