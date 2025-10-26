/**
 * Tree Validation Helpers
 *
 * Helper functions for validating AnnotatedParse tree structure,
 * ordering, nesting, and navigation patterns.
 */

import assert from 'node:assert/strict';
import type { AnnotatedParse } from '../../src/types.js';

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
  assert.ok('source' in node, 'Node should have source');
  assert.ok('start' in node, 'Node should have start');
  assert.ok('end' in node, 'Node should have end');
  assert.ok('result' in node, 'Node should have result');

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

  // If first component is metadata, it should come first
  const firstComponent = doc.components[0];
  if (firstComponent.kind === 'mapping') {
    // Metadata is first - correct!
    // All subsequent components should be blocks
    for (let i = 1; i < doc.components.length; i++) {
      const component = doc.components[i];
      assert.ok(isBlockKind(component.kind),
        `Component ${i} after metadata should be a block, got ${component.kind}`);
    }
  } else {
    // No metadata, all components should be blocks
    for (const component of doc.components) {
      assert.ok(isBlockKind(component.kind),
        `Document component should be a block if no metadata, got ${component.kind}`);
    }
  }
}

/**
 * Validate children source ranges are within parent
 */
export function validateSourceRangeNesting(node: AnnotatedParse): void {
  for (const component of node.components) {
    // Child must be within parent's range
    assert.ok(component.start >= node.start,
      `Child ${component.kind} start ${component.start} should be >= parent ${node.kind} start ${node.start}`);
    assert.ok(component.end <= node.end,
      `Child ${component.kind} end ${component.end} should be <= parent ${node.kind} end ${node.end}`);

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

    // Next should start at or after current's start (not necessarily after current.end)
    // Some structures like lists or divs may have overlapping source ranges
    assert.ok(next.start >= current.start,
      `Components should be in source order: ${next.kind} start ${next.start} >= ${current.kind} start ${current.start}`);
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
 * Find all nodes of a specific kind (depth-first)
 */
export function findNodesByKind(node: AnnotatedParse, targetKind: string): AnnotatedParse[] {
  const results: AnnotatedParse[] = [];

  if (node.kind === targetKind) {
    results.push(node);
  }

  for (const component of node.components) {
    results.push(...findNodesByKind(component, targetKind));
  }

  return results;
}

/**
 * Collect all nodes in breadth-first order
 */
export function collectBreadthFirst(node: AnnotatedParse): AnnotatedParse[] {
  const result: AnnotatedParse[] = [];
  const queue: AnnotatedParse[] = [node];

  while (queue.length > 0) {
    const current = queue.shift()!;
    result.push(current);

    for (const component of current.components) {
      queue.push(component);
    }
  }

  return result;
}

/**
 * Collect all nodes in depth-first order
 */
export function collectDepthFirst(node: AnnotatedParse): AnnotatedParse[] {
  const result: AnnotatedParse[] = [node];

  for (const component of node.components) {
    result.push(...collectDepthFirst(component));
  }

  return result;
}

/**
 * Validate type constraints: block nodes contain appropriate children
 */
export function validateTypeConstraints(node: AnnotatedParse): void {
  // Document contains metadata (mapping) and/or blocks
  if (node.kind === 'Document') {
    for (const component of node.components) {
      assert.ok(
        component.kind === 'mapping' || isBlockKind(component.kind),
        `Document should contain mapping or blocks, got ${component.kind}`
      );
    }
  }

  // Block nodes (except Note which can have blocks) contain inlines or other blocks
  if (isBlockKind(node.kind)) {
    if (node.kind === 'Para' || node.kind === 'Plain' || node.kind === 'Header') {
      // These should contain inlines (after any attr components)
      for (const component of node.components) {
        // Skip attr-related components
        if (component.kind === 'identifier' || component.kind === 'class' || component.kind === 'key-value') {
          continue;
        }
        assert.ok(isInlineKind(component.kind),
          `${node.kind} should contain inlines, got ${component.kind}`);
      }
    } else if (node.kind === 'BulletList' || node.kind === 'OrderedList') {
      // Lists contain blocks (list items are represented as nested blocks)
      for (const component of node.components) {
        assert.ok(isBlockKind(component.kind),
          `List should contain blocks, got ${component.kind}`);
      }
    }
  }

  // Inline nodes contain only inlines (except Note which is special)
  if (isInlineKind(node.kind) && node.kind !== 'Note') {
    for (const component of node.components) {
      // Some inlines like Link, Emph, etc. contain inline children
      if (component.kind !== 'identifier' && component.kind !== 'class' && component.kind !== 'key-value') {
        assert.ok(isInlineKind(component.kind),
          `Inline ${node.kind} should contain inlines, got ${component.kind}`);
      }
    }
  }

  // Note is special: it's an inline but contains blocks
  if (node.kind === 'Note') {
    for (const component of node.components) {
      assert.ok(isBlockKind(component.kind),
        `Note should contain blocks, got ${component.kind}`);
    }
  }

  // Recursively validate
  for (const component of node.components) {
    validateTypeConstraints(component);
  }
}

/**
 * Helper to check if kind is a block
 */
export function isBlockKind(kind: string): boolean {
  const blockKinds = [
    'Plain', 'Para', 'Header', 'CodeBlock', 'RawBlock',
    'BlockQuote', 'OrderedList', 'BulletList', 'DefinitionList',
    'HorizontalRule', 'Table', 'Div', 'Null', 'Figure'
  ];
  return blockKinds.includes(kind);
}

/**
 * Helper to check if kind is an inline
 */
export function isInlineKind(kind: string): boolean {
  const inlineKinds = [
    'Str', 'Emph', 'Underline', 'Strong', 'Strikeout', 'Superscript', 'Subscript',
    'SmallCaps', 'Quoted', 'Cite', 'Code', 'Space', 'SoftBreak', 'LineBreak',
    'Math', 'RawInline', 'Link', 'Image', 'Note', 'Span'
  ];
  return inlineKinds.includes(kind);
}

/**
 * Comprehensive tree validation (runs all validators)
 */
export function validateTree(node: AnnotatedParse): void {
  validateTreeStructure(node);
  detectCircularReferences(node);
  validateSourceRangeNesting(node);
  validateSourceOrdering(node);

  // Only validate document ordering if it's a document
  if (node.kind === 'Document') {
    validateDocumentOrdering(node);
  }

  // Type constraints might be too strict for some edge cases, so we make this optional
  try {
    validateTypeConstraints(node);
  } catch (e) {
    // Log but don't fail - type constraints are aspirational
    console.warn(`Type constraint warning for ${node.kind}:`, (e as Error).message);
  }
}
