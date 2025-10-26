/**
 * Tree Validation Tests
 *
 * Comprehensive tests for validating AnnotatedParse tree structure,
 * ordering, nesting, metrics, navigation, and type constraints.
 */

import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parseRustQmdDocument } from '../src/index.js';
import type { RustQmdJson, AnnotatedParse } from '../src/types.js';
import {
  validateTreeStructure,
  detectCircularReferences,
  validateDocumentOrdering,
  validateSourceRangeNesting,
  validateSourceOrdering,
  getTreeDepth,
  countTreeNodes,
  extractTextContent,
  findNodesByKind,
  collectBreadthFirst,
  collectDepthFirst,
  validateTypeConstraints,
  validateTree,
  isBlockKind,
  isInlineKind
} from './helpers/tree-validators.js';

/**
 * Load example JSON fixture and populate file content
 */
function loadExample(name: string): RustQmdJson {
  const jsonPath = resolve(process.cwd(), `examples/${name}.json`);
  const json = JSON.parse(readFileSync(jsonPath, 'utf-8')) as RustQmdJson;

  // Load corresponding QMD file content
  const qmdPath = resolve(process.cwd(), `examples/${name}.qmd`);
  const qmdContent = readFileSync(qmdPath, 'utf-8');

  // Populate content for all files
  for (const file of json.astContext.files) {
    file.content = qmdContent;
  }

  return json;
}

describe('Tree Validation', () => {

  describe('Section 1: Basic Structure', () => {

    test('tree structure validation - simple document', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      // No errors should be thrown
      validateTreeStructure(doc);
    });

    test('tree structure validation - complex documents', () => {
      const documents = ['blog-post', 'academic-paper', 'tutorial'];

      for (const docName of documents) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        // Should validate without errors
        validateTreeStructure(doc);
      }
    });

    test('circular reference detection - should pass (no circular refs)', () => {
      const json = loadExample('tutorial'); // Complex nested document
      const doc = parseRustQmdDocument(json);

      // Should not throw
      detectCircularReferences(doc);
    });

    test('all nodes are valid AnnotatedParse objects', () => {
      const json = loadExample('blog-post');
      const doc = parseRustQmdDocument(json);

      // Collect all nodes
      const allNodes = collectDepthFirst(doc);

      for (const node of allNodes) {
        assert.ok(node !== undefined, 'Node should not be undefined');
        assert.ok(node !== null, 'Node should not be null');
        assert.ok('kind' in node, 'Node should have kind');
        assert.ok('components' in node, 'Node should have components');
        assert.ok('source' in node, 'Node should have source');
        assert.ok('start' in node, 'Node should have start');
        assert.ok('end' in node, 'Node should have end');
      }
    });
  });

  describe('Section 2: Ordering Rules', () => {

    test('Document ordering - metadata first if present', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      validateDocumentOrdering(doc);

      // Metadata should be first
      assert.equal(doc.components[0].kind, 'mapping', 'First component should be metadata');

      // Subsequent components should be blocks
      for (let i = 1; i < doc.components.length; i++) {
        const component = doc.components[i];
        assert.ok(isBlockKind(component.kind), `Component ${i} should be a block`);
      }
    });

    test('Document ordering - no metadata case', () => {
      // minimal-doc has metadata, so let's just verify the validator works
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      // Should not throw
      validateDocumentOrdering(doc);
    });

    test('list items in source order', () => {
      const json = loadExample('ordered-list');
      const doc = parseRustQmdDocument(json);

      const lists = findNodesByKind(doc, 'OrderedList');
      assert.ok(lists.length > 0, 'Should have ordered lists');

      for (const list of lists) {
        // List items should be in source order
        for (let i = 0; i < list.components.length - 1; i++) {
          const current = list.components[i];
          const next = list.components[i + 1];

          assert.ok(next.start >= current.start,
            `List items should be in source order`);
        }
      }
    });

    test('children in source order - all documents', () => {
      const documents = ['simple', 'blog-post', 'minimal-doc'];

      for (const docName of documents) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        // Should not throw
        validateSourceOrdering(doc);
      }
    });
  });

  describe('Section 3: Source Range Nesting', () => {

    test('children within parent bounds', () => {
      const json = loadExample('blog-post');
      const doc = parseRustQmdDocument(json);

      // Should not throw
      validateSourceRangeNesting(doc);
    });

    test('source range nesting - deeply nested structures', () => {
      const json = loadExample('tutorial'); // Has nested callouts
      const doc = parseRustQmdDocument(json);

      // Should not throw
      validateSourceRangeNesting(doc);
    });

    test('source range nesting - edge cases', () => {
      const documents = ['empty-content', 'zero-width', 'boundary-values'];

      for (const docName of documents) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        // Should handle edge cases
        validateSourceRangeNesting(doc);
      }
    });

    test('children source ranges checked recursively', () => {
      const json = loadExample('academic-paper');
      const doc = parseRustQmdDocument(json);

      // Validate entire tree recursively
      const allNodes = collectDepthFirst(doc);

      for (const node of allNodes) {
        for (const component of node.components) {
          if (component.start < node.start) {
            console.log(`\nFound violation:`);
            console.log(`  Parent: ${node.kind} [${node.start}, ${node.end}]`);
            console.log(`  Child:  ${component.kind} [${component.start}, ${component.end}]`);
            if (node.kind === 'Para' || node.kind === 'Plain') {
              console.log(`  Parent text: "${node.source.value.substring(node.start, Math.min(node.end, node.start + 50))}..."`);
            }
          }
          assert.ok(component.start >= node.start,
            `Child ${component.kind}@${component.start} start should be >= parent ${node.kind}@${node.start} start`);
          assert.ok(component.end <= node.end,
            `Child ${component.kind}@${component.end} end should be <= parent ${node.kind}@${node.end} end`);
        }
      }
    });
  });

  describe('Section 4: Tree Metrics', () => {

    test('maximum depth calculation', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      const depth = getTreeDepth(doc);
      assert.ok(depth >= 1, 'Depth should be at least 1');
      assert.ok(depth < 50, 'Depth should be reasonable');
    });

    test('depth is reasonable for all test documents', () => {
      const documents = ['simple', 'blog-post', 'academic-paper', 'tutorial'];

      for (const docName of documents) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        const depth = getTreeDepth(doc);
        assert.ok(depth < 50, `${docName}: depth ${depth} should be < 50`);
        console.log(`  ${docName}: depth = ${depth}`);
      }
    });

    test('total node count', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      const count = countTreeNodes(doc);
      assert.ok(count > 0, 'Should have at least 1 node');
      assert.ok(count > 10, 'Simple document should have > 10 nodes');
    });

    test('node count matches expectations for complex documents', () => {
      const expectations = {
        'blog-post': 300,      // ~301 nodes
        'academic-paper': 450, // ~458 nodes
        'tutorial': 500        // ~511 nodes
      };

      for (const [docName, expectedMin] of Object.entries(expectations)) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        const count = countTreeNodes(doc);
        assert.ok(count >= expectedMin,
          `${docName}: count ${count} should be >= ${expectedMin}`);
        console.log(`  ${docName}: ${count} nodes`);
      }
    });
  });

  describe('Section 5: Navigation Patterns', () => {

    test('find all nodes of specific kind', () => {
      const json = loadExample('blog-post');
      const doc = parseRustQmdDocument(json);

      const headers = findNodesByKind(doc, 'Header');
      assert.ok(headers.length > 0, 'Should find Header nodes');

      const paras = findNodesByKind(doc, 'Para');
      assert.ok(paras.length > 0, 'Should find Para nodes');

      const codeBlocks = findNodesByKind(doc, 'CodeBlock');
      assert.ok(codeBlocks.length > 0, 'Should find CodeBlock nodes');
    });

    test('extract text content from paragraph', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      const paras = findNodesByKind(doc, 'Para');
      assert.ok(paras.length > 0, 'Should have paragraphs');

      const firstPara = paras[0];
      const text = extractTextContent(firstPara);

      assert.ok(text.length > 0, 'Should extract text');
      assert.ok(typeof text === 'string', 'Text should be string');
    });

    test('breadth-first traversal', () => {
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      const nodes = collectBreadthFirst(doc);

      // First node should be Document
      assert.equal(nodes[0].kind, 'Document');

      // Should have multiple nodes
      assert.ok(nodes.length > 1, 'Should have multiple nodes');

      // All nodes should be valid
      for (const node of nodes) {
        assert.ok('kind' in node, 'Node should have kind');
      }
    });

    test('depth-first traversal', () => {
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      const nodes = collectDepthFirst(doc);

      // First node should be Document
      assert.equal(nodes[0].kind, 'Document');

      // Should have multiple nodes
      assert.ok(nodes.length > 1, 'Should have multiple nodes');
    });

    test('traversal order comparison', () => {
      const json = loadExample('blog-post');
      const doc = parseRustQmdDocument(json);

      const bfs = collectBreadthFirst(doc);
      const dfs = collectDepthFirst(doc);

      // Both should visit all nodes
      assert.equal(bfs.length, dfs.length, 'Should visit same number of nodes');

      // First and last nodes might differ due to traversal order
      assert.equal(bfs[0], dfs[0], 'Both should start at root');
    });
  });

  describe('Section 6: Type Constraints', () => {

    test('Document contains metadata and/or blocks', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      assert.equal(doc.kind, 'Document');

      for (const component of doc.components) {
        const isMetadata = component.kind === 'mapping';
        const isBlock = isBlockKind(component.kind);

        assert.ok(isMetadata || isBlock,
          `Document child should be metadata or block, got ${component.kind}`);
      }
    });

    test('Para contains inline elements', () => {
      const json = loadExample('simple');
      const doc = parseRustQmdDocument(json);

      const paras = findNodesByKind(doc, 'Para');
      assert.ok(paras.length > 0, 'Should have Para nodes');

      for (const para of paras) {
        for (const component of para.components) {
          // Para should contain inlines
          assert.ok(isInlineKind(component.kind),
            `Para should contain inlines, got ${component.kind}`);
        }
      }
    });

    test('lists contain blocks', () => {
      const json = loadExample('ordered-list');
      const doc = parseRustQmdDocument(json);

      const lists = [
        ...findNodesByKind(doc, 'BulletList'),
        ...findNodesByKind(doc, 'OrderedList')
      ];

      assert.ok(lists.length > 0, 'Should have lists');

      for (const list of lists) {
        for (const component of list.components) {
          // Lists contain blocks (list items)
          assert.ok(isBlockKind(component.kind),
            `List should contain blocks, got ${component.kind}`);
        }
      }
    });

    test('inline formatting contains inlines', () => {
      const json = loadExample('inline-types');
      const doc = parseRustQmdDocument(json);

      const emphNodes = findNodesByKind(doc, 'Emph');
      const strongNodes = findNodesByKind(doc, 'Strong');

      for (const node of [...emphNodes, ...strongNodes]) {
        for (const component of node.components) {
          // Should contain inlines (skip attr components)
          if (component.kind !== 'identifier' && component.kind !== 'class' && component.kind !== 'key-value') {
            assert.ok(isInlineKind(component.kind),
              `Formatting should contain inlines, got ${component.kind}`);
          }
        }
      }
    });

    test('Note contains blocks (special case)', () => {
      const json = loadExample('inline-types');
      const doc = parseRustQmdDocument(json);

      const notes = findNodesByKind(doc, 'Note');

      if (notes.length > 0) {
        for (const note of notes) {
          // Note is an inline but contains blocks
          for (const component of note.components) {
            assert.ok(isBlockKind(component.kind),
              `Note should contain blocks, got ${component.kind}`);
          }
        }
      }
    });
  });

  describe('Comprehensive Validation', () => {

    test('comprehensive validation - all test documents', () => {
      const documents = [
        'simple', 'links', 'table',
        'blog-post', 'academic-paper', 'tutorial',
        'empty-content', 'minimal-doc', 'boundary-values',
        'missing-fields', 'zero-width'
      ];

      for (const docName of documents) {
        const json = loadExample(docName);
        const doc = parseRustQmdDocument(json);

        try {
          // Run comprehensive validation (should not throw)
          validateTree(doc);
        } catch (e) {
          console.error(`Validation failed for document: ${docName}`);
          throw e;
        }
      }
    });
  });
});

console.log('\nTree validation tests complete\n');
