/**
 * Complex Document Tests
 *
 * End-to-end tests for realistic complex documents with mixed content,
 * nested structures, and multiple formatting elements.
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
 * Count nodes by kind recursively
 */
function countNodesByKind(node: AnnotatedParse, counts: Map<string, number> = new Map()): Map<string, number> {
  // Count this node
  const current = counts.get(node.kind) || 0;
  counts.set(node.kind, current + 1);

  // Recursively count components
  for (const component of node.components) {
    countNodesByKind(component, counts);
  }

  return counts;
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
 * Get maximum nesting depth
 */
function getMaxDepth(node: AnnotatedParse, currentDepth: number = 0): number {
  if (node.components.length === 0) {
    return currentDepth;
  }

  let maxChildDepth = currentDepth;
  for (const component of node.components) {
    const childDepth = getMaxDepth(component, currentDepth + 1);
    maxChildDepth = Math.max(maxChildDepth, childDepth);
  }

  return maxChildDepth;
}

describe('Complex Documents', () => {

  test('blog-post.qmd - Complete document structure', () => {
    const json = loadExample('blog-post');
    const doc = parseRustQmdDocument(json);

    // Should be a Document
    assert.equal(doc.kind, 'Document');

    // Should have metadata and blocks
    assert.ok(doc.components.length > 1, 'Should have metadata and multiple blocks');

    // First component should be metadata
    assert.equal(doc.components[0].kind, 'mapping');

    // Count different node types
    const counts = countNodesByKind(doc);

    // Should have headers (sections)
    const headerCount = counts.get('Header') || 0;
    assert.ok(headerCount >= 3, `Should have at least 3 headers, found ${headerCount}`);

    // Should have paragraphs
    const paraCount = counts.get('Para') || 0;
    assert.ok(paraCount >= 5, `Should have at least 5 paragraphs, found ${paraCount}`);

    // Should have code blocks
    const codeBlockCount = counts.get('CodeBlock') || 0;
    assert.ok(codeBlockCount >= 2, `Should have at least 2 code blocks, found ${codeBlockCount}`);

    // Should have links
    const linkNodes = findNodesByKind(doc, 'Link');
    assert.ok(linkNodes.length >= 2, `Should have at least 2 links, found ${linkNodes.length}`);

    // Should have lists
    const orderedListCount = counts.get('OrderedList') || 0;
    const bulletListCount = counts.get('BulletList') || 0;
    assert.ok(orderedListCount + bulletListCount >= 2,
      `Should have at least 2 lists, found ${orderedListCount + bulletListCount}`);

    console.log('\n  Blog post structure:');
    console.log(`    - ${headerCount} headers`);
    console.log(`    - ${paraCount} paragraphs`);
    console.log(`    - ${codeBlockCount} code blocks`);
    console.log(`    - ${linkNodes.length} links`);
    console.log(`    - ${orderedListCount + bulletListCount} lists`);
  });

  test('blog-post.qmd - Source mapping accuracy', () => {
    const json = loadExample('blog-post');
    const qmdContent = json.astContext.files[0].content;
    const doc = parseRustQmdDocument(json);

    // Verify document source is correct
    assert.equal(doc.source.value, qmdContent);

    // Verify start/end offsets are valid
    assert.equal(doc.start, 0);
    assert.equal(doc.end, qmdContent.length);

    // Test a few specific strings
    const headers = findNodesByKind(doc, 'Header');
    assert.ok(headers.length > 0, 'Should have headers');

    // Find "Introduction" header
    const introHeader = headers.find(h => {
      const str = h.source.value.substring(h.start, h.end);
      return str.includes('Introduction');
    });
    assert.ok(introHeader, 'Should find Introduction header');

    // Verify substring extraction works
    const extracted = introHeader!.source.value.substring(introHeader!.start, introHeader!.end);
    assert.ok(extracted.includes('Introduction'), 'Extracted text should contain Introduction');
  });

  test('academic-paper.qmd - Complete document structure', () => {
    const json = loadExample('academic-paper');
    const doc = parseRustQmdDocument(json);

    assert.equal(doc.kind, 'Document');

    const counts = countNodesByKind(doc);

    // Should have headers (sections)
    const headerCount = counts.get('Header') || 0;
    assert.ok(headerCount >= 5, `Should have at least 5 headers (sections), found ${headerCount}`);

    // Should have tables
    const tableCount = counts.get('Table') || 0;
    assert.ok(tableCount >= 1, `Should have at least 1 table, found ${tableCount}`);

    // Should have math (DisplayMath or InlineMath)
    const mathNodes = findNodesByKind(doc, 'Math');
    assert.ok(mathNodes.length >= 2, `Should have at least 2 math elements, found ${mathNodes.length}`);

    // Should have footnotes (Note inlines)
    const noteNodes = findNodesByKind(doc, 'Note');
    assert.ok(noteNodes.length >= 1, `Should have at least 1 footnote, found ${noteNodes.length}`);

    // Should have bullet/ordered lists
    const listCount = (counts.get('BulletList') || 0) + (counts.get('OrderedList') || 0);
    assert.ok(listCount >= 1, `Should have at least 1 list, found ${listCount}`);

    console.log('\n  Academic paper structure:');
    console.log(`    - ${headerCount} headers (sections)`);
    console.log(`    - ${tableCount} tables`);
    console.log(`    - ${mathNodes.length} math elements`);
    console.log(`    - ${noteNodes.length} footnotes`);
    console.log(`    - ${listCount} lists`);
  });

  test('academic-paper.qmd - Nested structures', () => {
    const json = loadExample('academic-paper');
    const doc = parseRustQmdDocument(json);

    // Check nesting depth
    const maxDepth = getMaxDepth(doc);
    assert.ok(maxDepth >= 5, `Should have nesting depth of at least 5, found ${maxDepth}`);

    // Tables should have nested cell content
    const tables = findNodesByKind(doc, 'Table');
    assert.ok(tables.length > 0, 'Should have tables');

    // Lists should have nested items
    const lists = [...findNodesByKind(doc, 'BulletList'), ...findNodesByKind(doc, 'OrderedList')];
    assert.ok(lists.length > 0, 'Should have lists');

    console.log(`\n  Maximum nesting depth: ${maxDepth}`);
  });

  test('tutorial.qmd - Complete document structure', () => {
    const json = loadExample('tutorial');
    const doc = parseRustQmdDocument(json);

    assert.equal(doc.kind, 'Document');

    const counts = countNodesByKind(doc);

    // Should have headers (sections/steps)
    const headerCount = counts.get('Header') || 0;
    assert.ok(headerCount >= 6, `Should have at least 6 headers (steps), found ${headerCount}`);

    // Should have many code blocks (tutorial with examples)
    const codeBlockCount = counts.get('CodeBlock') || 0;
    assert.ok(codeBlockCount >= 5, `Should have at least 5 code blocks, found ${codeBlockCount}`);

    // Should have divs (callouts)
    const divCount = counts.get('Div') || 0;
    assert.ok(divCount >= 4, `Should have at least 4 divs (callouts), found ${divCount}`);

    // Should have ordered lists (steps)
    const orderedListCount = counts.get('OrderedList') || 0;
    assert.ok(orderedListCount >= 2, `Should have at least 2 ordered lists, found ${orderedListCount}`);

    console.log('\n  Tutorial structure:');
    console.log(`    - ${headerCount} headers (sections/steps)`);
    console.log(`    - ${codeBlockCount} code blocks`);
    console.log(`    - ${divCount} divs (callouts)`);
    console.log(`    - ${orderedListCount} ordered lists`);
  });

  test('tutorial.qmd - Deeply nested divs (callouts)', () => {
    const json = loadExample('tutorial');
    const doc = parseRustQmdDocument(json);

    // Find divs
    const divs = findNodesByKind(doc, 'Div');
    assert.ok(divs.length >= 4, 'Should have multiple divs');

    // Check for nested divs (callout within callout)
    let foundNestedDiv = false;
    for (const div of divs) {
      const nestedDivs = findNodesByKind(div, 'Div');
      if (nestedDivs.length > 0) {
        foundNestedDiv = true;
        console.log(`\n  Found nested div with ${nestedDivs.length} child div(s)`);
        break;
      }
    }

    assert.ok(foundNestedDiv, 'Should have at least one div nested within another');
  });

  test('tutorial.qmd - Component ordering and navigation', () => {
    const json = loadExample('tutorial');
    const doc = parseRustQmdDocument(json);

    // First component should be metadata
    assert.equal(doc.components[0].kind, 'mapping');

    // Subsequent components should be blocks in order
    let lastHeaderText = '';
    for (let i = 1; i < doc.components.length; i++) {
      const component = doc.components[i];

      // Headers should appear in sequence
      if (component.kind === 'Header') {
        const headerText = component.source.value.substring(component.start, component.end);

        // Just verify we can extract text
        assert.ok(headerText.length > 0, `Header ${i} should have text`);

        // Track progression (optional - headers should generally increase in file order)
        if (lastHeaderText) {
          assert.ok(component.start > 0, 'Header should have valid start position');
        }
        lastHeaderText = headerText;
      }
    }
  });

  test('All complex documents - Metadata preservation', () => {
    const documents = ['blog-post', 'academic-paper', 'tutorial'];

    for (const docName of documents) {
      const json = loadExample(docName);
      const doc = parseRustQmdDocument(json);

      // Should have metadata as first component
      assert.equal(doc.components[0].kind, 'mapping',
        `${docName}: First component should be metadata`);

      const metadata = doc.components[0];

      // Metadata should have title
      assert.ok('title' in metadata.result,
        `${docName}: Metadata should have title`);

      // Metadata should have author
      assert.ok('author' in metadata.result,
        `${docName}: Metadata should have author`);

      console.log(`\n  ${docName} metadata: title="${(metadata.result as any).title}"`);
    }
  });

  test('All complex documents - Performance check', () => {
    const documents = ['blog-post', 'academic-paper', 'tutorial'];

    for (const docName of documents) {
      const json = loadExample(docName);

      const startTime = performance.now();
      const doc = parseRustQmdDocument(json);
      const endTime = performance.now();

      const duration = endTime - startTime;

      // Conversion should be reasonably fast (< 100ms for these documents)
      assert.ok(duration < 100,
        `${docName}: Conversion took ${duration.toFixed(2)}ms, should be < 100ms`);

      const nodeCount = countNodesByKind(doc);
      const totalNodes = Array.from(nodeCount.values()).reduce((sum, count) => sum + count, 0);

      console.log(`\n  ${docName}: ${totalNodes} total nodes in ${duration.toFixed(2)}ms`);
    }
  });
});

console.log('\nComplex document tests complete\n');
