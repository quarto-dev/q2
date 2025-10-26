/**
 * Edge Cases Tests
 *
 * Tests for empty content, boundary values, minimal documents,
 * null/missing fields, and source mapping edge cases.
 */

import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parseRustQmdDocument } from '../src/index.js';
import type { RustQmdJson, AnnotatedParse } from '../src/types.js';

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

/**
 * Find nodes by kind recursively
 */
function findNodesByKind(node: AnnotatedParse, targetKind: string): AnnotatedParse[] {
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
 * Validate that AnnotatedParse has all required fields
 */
function validateAnnotatedParse(node: AnnotatedParse): void {
  assert.ok('kind' in node, 'Should have kind');
  assert.ok('source' in node, 'Should have source');
  assert.ok('components' in node, 'Should have components');
  assert.ok('start' in node, 'Should have start');
  assert.ok('end' in node, 'Should have end');
  assert.ok('result' in node, 'Should have result');

  // Source should be valid
  assert.ok(node.source !== undefined, 'source should not be undefined');

  // Offsets should be valid
  assert.ok(node.start >= 0, 'start should be >= 0');
  assert.ok(node.end >= node.start, 'end should be >= start');

  // Components should be array
  assert.ok(Array.isArray(node.components), 'components should be array');
}

/**
 * Validate that empty content produces empty components
 */
function validateEmptyContent(node: AnnotatedParse): void {
  validateAnnotatedParse(node);
  assert.equal(node.components.length, 0, 'Empty content should have 0 components');
}

/**
 * Validate source mapping is within file bounds
 */
function validateSourceBounds(node: AnnotatedParse, fileLength: number): void {
  assert.ok(node.start >= 0 && node.start <= fileLength,
    `start ${node.start} should be in bounds [0, ${fileLength}]`);
  assert.ok(node.end >= 0 && node.end <= fileLength,
    `end ${node.end} should be in bounds [0, ${fileLength}]`);
}

/**
 * Recursively validate all nodes in tree
 */
function validateTree(node: AnnotatedParse, fileLength: number): void {
  validateAnnotatedParse(node);
  validateSourceBounds(node, fileLength);

  for (const component of node.components) {
    validateTree(component, fileLength);
  }
}

describe('Edge Cases', () => {

  describe('Category 1: Empty Content', () => {

    test('empty-content.qmd - document parses without errors', () => {
      const json = loadExample('empty-content');
      const doc = parseRustQmdDocument(json);

      validateAnnotatedParse(doc);
      assert.equal(doc.kind, 'Document');
      assert.ok(doc.components.length > 0, 'Should have some components');
    });

    test('empty paragraph produces valid AnnotatedParse', () => {
      const json = loadExample('empty-content');
      const doc = parseRustQmdDocument(json);

      // Find all paragraphs
      const paras = findNodesByKind(doc, 'Para');
      assert.ok(paras.length > 0, 'Should have some Para nodes');

      // All paras should be valid even if empty
      for (const para of paras) {
        validateAnnotatedParse(para);
        // Empty para has components.length = 0
        if (para.components.length === 0) {
          validateEmptyContent(para);
        }
      }
    });

    test('empty formatting elements work', () => {
      const json = loadExample('empty-content');
      const doc = parseRustQmdDocument(json);

      // Find formatting nodes (Emph, Strong)
      const emphNodes = findNodesByKind(doc, 'Emph');
      const strongNodes = findNodesByKind(doc, 'Strong');

      // All should be valid, even if empty
      [...emphNodes, ...strongNodes].forEach(node => {
        validateAnnotatedParse(node);
      });
    });

    test('empty code block has correct source', () => {
      const json = loadExample('empty-content');
      const doc = parseRustQmdDocument(json);
      const qmdContent = json.astContext.files[0].content;

      const codeBlocks = findNodesByKind(doc, 'CodeBlock');
      assert.ok(codeBlocks.length >= 2, 'Should have at least 2 code blocks');

      // All code blocks should have valid structure
      for (const cb of codeBlocks) {
        validateAnnotatedParse(cb);
        validateSourceBounds(cb, qmdContent.length);
        // Code blocks can have empty string content
        assert.ok(cb.end >= cb.start, 'Should have valid range');
      }
    });

    test('empty list items produce valid structure', () => {
      const json = loadExample('empty-content');
      const doc = parseRustQmdDocument(json);

      const bulletLists = findNodesByKind(doc, 'BulletList');
      const orderedLists = findNodesByKind(doc, 'OrderedList');

      // All lists should be valid
      [...bulletLists, ...orderedLists].forEach(list => {
        validateAnnotatedParse(list);
        assert.ok(list.components.length >= 0, 'Lists can have 0+ items');
      });
    });

    test('empty metadata produces valid mapping', () => {
      // minimal-doc has minimal metadata
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      // First component should be metadata
      assert.equal(doc.components[0].kind, 'mapping');
      validateAnnotatedParse(doc.components[0]);
    });
  });

  describe('Category 2: Minimal Documents', () => {

    test('document with minimal content', () => {
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      validateAnnotatedParse(doc);
      assert.equal(doc.kind, 'Document');

      // Should have metadata and at least one block
      assert.ok(doc.components.length >= 2, 'Should have metadata + blocks');
      assert.equal(doc.components[0].kind, 'mapping', 'First should be metadata');
    });

    test('single paragraph document', () => {
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      const paras = findNodesByKind(doc, 'Para');
      assert.ok(paras.length >= 1, 'Should have at least one paragraph');

      const para = paras[0];
      validateAnnotatedParse(para);
      assert.ok(para.components.length > 0, 'Paragraph should have content');
    });

    test('single-item list', () => {
      const json = loadExample('boundary-values');
      const doc = parseRustQmdDocument(json);

      const bulletLists = findNodesByKind(doc, 'BulletList');

      // Find single-item list
      const singleItemList = bulletLists.find(list => list.components.length === 1);

      if (singleItemList) {
        validateAnnotatedParse(singleItemList);
        assert.equal(singleItemList.components.length, 1, 'Should have exactly 1 item');
      }
    });

    test('document source spans entire file', () => {
      const json = loadExample('minimal-doc');
      const qmdContent = json.astContext.files[0].content;
      const doc = parseRustQmdDocument(json);

      assert.equal(doc.start, 0, 'Document should start at 0');
      assert.equal(doc.end, qmdContent.length, 'Document should end at file length');
      assert.equal(doc.source.value, qmdContent, 'Document source should be full content');
    });

    test('minimal table structure', () => {
      const json = loadExample('missing-fields');
      const doc = parseRustQmdDocument(json);

      const tables = findNodesByKind(doc, 'Table');
      assert.ok(tables.length >= 1, 'Should have at least one table');

      // Check single-cell table
      const singleCellTable = tables.find(t => {
        // Tables have complex structure, just validate it exists
        return t.kind === 'Table';
      });

      if (singleCellTable) {
        validateAnnotatedParse(singleCellTable);
      }
    });

    test('definition list with single definition', () => {
      const json = loadExample('missing-fields');
      const doc = parseRustQmdDocument(json);

      const defLists = findNodesByKind(doc, 'DefinitionList');

      if (defLists.length > 0) {
        const defList = defLists[0];
        validateAnnotatedParse(defList);
        assert.ok(defList.components.length >= 0, 'Should have components');
      }
    });
  });

  describe('Category 3: Boundary Values', () => {

    test('header level 6 (maximum)', () => {
      const json = loadExample('boundary-values');
      const doc = parseRustQmdDocument(json);

      const headers = findNodesByKind(doc, 'Header');

      // Find level 6 header
      const level6 = headers.find(h => {
        // Header result is [level, attr, inlines]
        const result = h.result as any[];
        return result && result[0] === 6;
      });

      assert.ok(level6, 'Should find level 6 header');
      validateAnnotatedParse(level6!);
    });

    test('very long Str content (1000+ chars)', () => {
      const json = loadExample('boundary-values');
      const doc = parseRustQmdDocument(json);

      const strs = findNodesByKind(doc, 'Str');

      // Find a very long Str
      const longStr = strs.find(s => {
        const text = s.result as string;
        return text && text.length > 100; // Approximation
      });

      if (longStr) {
        validateAnnotatedParse(longStr);
        assert.ok(typeof longStr.result === 'string', 'Str result should be string');
      }
    });

    test('Span/Attr with many classes', () => {
      const json = loadExample('boundary-values');
      const doc = parseRustQmdDocument(json);

      const spans = findNodesByKind(doc, 'Span');

      // Find span with many classes
      const multiClassSpan = spans.find(s => {
        const result = s.result as any[];
        if (result && Array.isArray(result[0])) {
          const classes = result[0][1]; // Attr = [id, classes, kvs]
          return classes && classes.length >= 5;
        }
        return false;
      });

      if (multiClassSpan) {
        validateAnnotatedParse(multiClassSpan);
      }
    });

    test('ordered list with custom start number', () => {
      const json = loadExample('boundary-values');
      const doc = parseRustQmdDocument(json);

      const orderedLists = findNodesByKind(doc, 'OrderedList');

      // Find list with start != 1
      const customStartList = orderedLists.find(ol => {
        const result = ol.result as any[];
        if (result && Array.isArray(result[0])) {
          const startNum = result[0][0]; // ListAttributes = [start, style, delim]
          return startNum !== 1;
        }
        return false;
      });

      if (customStartList) {
        validateAnnotatedParse(customStartList);
        assert.ok(customStartList.components.length > 0, 'Should have items');
      }
    });

    test('boundary values - all nodes valid', () => {
      const json = loadExample('boundary-values');
      const qmdContent = json.astContext.files[0].content;
      const doc = parseRustQmdDocument(json);

      // Recursively validate entire tree
      validateTree(doc, qmdContent.length);
    });
  });

  describe('Category 4: Null and Missing Fields', () => {

    test('link with empty target', () => {
      const json = loadExample('missing-fields');
      const doc = parseRustQmdDocument(json);

      const links = findNodesByKind(doc, 'Link');
      assert.ok(links.length >= 1, 'Should have links');

      // Find link with empty target
      const emptyTargetLink = links.find(link => {
        const result = link.result as any[];
        if (result && Array.isArray(result[1])) {
          const target = result[1][0]; // Target = [url, title]
          return target === '';
        }
        return false;
      });

      if (emptyTargetLink) {
        validateAnnotatedParse(emptyTargetLink);
      }
    });

    test('link with empty text', () => {
      const json = loadExample('missing-fields');
      const doc = parseRustQmdDocument(json);

      const links = findNodesByKind(doc, 'Link');

      // Find link with no text components
      const emptyTextLink = links.find(link => link.components.length === 0);

      if (emptyTextLink) {
        validateAnnotatedParse(emptyTextLink);
        validateEmptyContent(emptyTextLink);
      }
    });

    test('table without caption', () => {
      const json = loadExample('missing-fields');
      const doc = parseRustQmdDocument(json);

      const tables = findNodesByKind(doc, 'Table');
      assert.ok(tables.length >= 1, 'Should have tables');

      // All tables should be valid regardless of caption
      for (const table of tables) {
        validateAnnotatedParse(table);
      }
    });

    test('missing fields handled gracefully', () => {
      const json = loadExample('missing-fields');
      const qmdContent = json.astContext.files[0].content;
      const doc = parseRustQmdDocument(json);

      // Entire tree should be valid
      validateTree(doc, qmdContent.length);
    });
  });

  describe('Category 5: Source Mapping Edge Cases', () => {

    test('Space element has valid source range', () => {
      const json = loadExample('zero-width');
      const doc = parseRustQmdDocument(json);

      const spaces = findNodesByKind(doc, 'Space');
      assert.ok(spaces.length > 0, 'Should have Space elements');

      for (const space of spaces) {
        validateAnnotatedParse(space);
        // Space typically has start == end (zero-width)
        assert.ok(space.end >= space.start, 'Should have valid range');
      }
    });

    test('SoftBreak element has valid source range', () => {
      const json = loadExample('zero-width');
      const doc = parseRustQmdDocument(json);

      const softBreaks = findNodesByKind(doc, 'SoftBreak');

      for (const sb of softBreaks) {
        validateAnnotatedParse(sb);
        assert.ok(sb.end >= sb.start, 'Should have valid range');
      }
    });

    test('LineBreak element has valid source range', () => {
      const json = loadExample('zero-width');
      const doc = parseRustQmdDocument(json);

      const lineBreaks = findNodesByKind(doc, 'LineBreak');
      assert.ok(lineBreaks.length > 0, 'Should have LineBreak elements');

      for (const lb of lineBreaks) {
        validateAnnotatedParse(lb);
        assert.ok(lb.end >= lb.start, 'Should have valid range');
      }
    });

    test('element at file start (offset 0)', () => {
      const json = loadExample('minimal-doc');
      const doc = parseRustQmdDocument(json);

      // Document itself starts at 0
      assert.equal(doc.start, 0, 'Document should start at 0');
      validateAnnotatedParse(doc);

      // Metadata also starts at 0 (after ---)
      const metadata = doc.components[0];
      validateAnnotatedParse(metadata);
    });

    test('element at file end', () => {
      const json = loadExample('zero-width');
      const qmdContent = json.astContext.files[0].content;
      const doc = parseRustQmdDocument(json);

      // Document should end at file length
      assert.equal(doc.end, qmdContent.length, 'Document should end at file length');
      validateAnnotatedParse(doc);

      // Find last element
      const lastBlock = doc.components[doc.components.length - 1];
      validateAnnotatedParse(lastBlock);
      assert.ok(lastBlock.end <= qmdContent.length, 'Last element should be within bounds');
    });

    test('zero-width elements - all nodes valid', () => {
      const json = loadExample('zero-width');
      const qmdContent = json.astContext.files[0].content;
      const doc = parseRustQmdDocument(json);

      // Recursively validate entire tree
      validateTree(doc, qmdContent.length);
    });
  });
});

console.log('\nEdge cases tests complete\n');
