/**
 * Comprehensive Substring Invariant Tests
 *
 * Test Philosophy:
 * 1. Establish ground truth by finding strings in .qmd files using indexOf()
 * 2. Navigate to corresponding AnnotatedParse nodes in parsed structure
 * 3. Verify: node.source.value.substring(node.start, node.end) == expected text
 * 4. Verify: node.start and node.end match known-good positions from source file
 *
 * This approach catches bugs where start/end offsets don't match the actual
 * source positions (like the bug in convertMeta that used local instead of
 * resolved coordinates).
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRustQmdDocument, type RustQmdJson, type AnnotatedParse } from '../src/index.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const examplesDir = path.join(__dirname, '..', 'examples');

/**
 * Helper: Verify substring invariant for a node
 */
function verifyNode(
  qmdContent: string,
  node: AnnotatedParse,
  expectedText: string,
  description: string
): void {
  // Verify substring extraction works
  const extracted = node.source.value.substring(node.start, node.end);

  assert.strictEqual(
    extracted,
    expectedText,
    `${description}: substring(${node.start}, ${node.end}) should extract "${expectedText}", got "${extracted}"`
  );

  // Verify positions match source file
  const expectedStart = qmdContent.indexOf(expectedText);
  if (expectedStart !== -1) {
    // For unique strings, we can verify exact position
    const occurrences = qmdContent.split(expectedText).length - 1;
    if (occurrences === 1) {
      assert.strictEqual(
        node.start,
        expectedStart,
        `${description}: start should be ${expectedStart} (found via indexOf)`
      );
      assert.strictEqual(
        node.end,
        expectedStart + expectedText.length,
        `${description}: end should be ${expectedStart + expectedText.length}`
      );
    }
  }

  console.log(`  ✓ ${description}: [${node.start}, ${node.end}] = "${expectedText.substring(0, 30)}${expectedText.length > 30 ? '...' : ''}"`);
}

/**
 * Helper: Find inline Str node with specific text
 */
function findInlineStr(components: AnnotatedParse[], text: string): AnnotatedParse | null {
  for (const comp of components) {
    if (comp.kind === 'Str' && comp.result === text) {
      return comp;
    }
    if (comp.components && comp.components.length > 0) {
      const found = findInlineStr(comp.components, text);
      if (found) return found;
    }
  }
  return null;
}

/**
 * Helper: Find inline node by kind
 */
function findInlineByKind(components: AnnotatedParse[], kind: string): AnnotatedParse | null {
  for (const comp of components) {
    if (comp.kind === kind) {
      return comp;
    }
    if (comp.components && comp.components.length > 0) {
      const found = findInlineByKind(comp.components, kind);
      if (found) return found;
    }
  }
  return null;
}

/**
 * Helper: Load and parse example
 */
function loadExample(name: string): { content: string; doc: AnnotatedParse } {
  const jsonPath = path.join(examplesDir, `${name}.json`);
  const qmdPath = path.join(examplesDir, `${name}.qmd`);

  const json: RustQmdJson = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));
  const content = fs.readFileSync(qmdPath, 'utf-8');
  json.astContext.files[0].content = content;

  const doc = parseRustQmdDocument(json);
  return { content, doc };
}

// ============================================================================
// Test Suite: links.qmd
// ============================================================================

test('substring invariant - links.qmd: metadata', () => {
  const { content, doc } = loadExample('links');

  console.log('\n--- links.qmd: metadata ---');

  // The first component is the metadata mapping
  const metadata = doc.components[0];
  assert.strictEqual(metadata.kind, 'mapping', 'First component should be metadata mapping');

  // Verify the entire metadata section: "title: Links and Images"
  verifyNode(content, metadata, 'title: Links and Images', 'Metadata mapping');

  // Navigate to title key and value
  // metadata.components = [key, value, ...]
  const titleKey = metadata.components[0];
  const titleValue = metadata.components[1];

  assert.strictEqual(titleKey.kind, 'key', 'First component should be key');
  verifyNode(content, titleKey, 'title', 'Title key');

  // Title value is MetaInlines containing inline elements
  // NOTE: MetaInlines doesn't convert child inlines to AnnotatedParse,
  // it keeps them as raw JSON in the result field
  assert.strictEqual(titleValue.kind, 'MetaInlines', 'Second component should be MetaInlines');

  // Verify the MetaInlines node itself has correct source mapping
  verifyNode(content, titleValue, 'Links and Images', 'Title value');
});

test('substring invariant - links.qmd: first paragraph with link', () => {
  const { content, doc } = loadExample('links');

  console.log('\n--- links.qmd: first paragraph ---');

  // First block (after metadata) is a Para
  const firstPara = doc.components[1];
  assert.strictEqual(firstPara.kind, 'Para', 'First block should be Para');

  // Find "Check" Str
  const checkStr = findInlineStr(firstPara.components, 'Check');
  assert.ok(checkStr, 'Should find "Check" Str');
  verifyNode(content, checkStr, 'Check', 'Para "Check" word');

  // Find the Link
  const link = findInlineByKind(firstPara.components, 'Link');
  assert.ok(link, 'Should find Link node');

  // Link contains "Quarto" text
  const quartoStr = findInlineStr(link.components, 'Quarto');
  assert.ok(quartoStr, 'Should find "Quarto" in link text');
  verifyNode(content, quartoStr, 'Quarto', 'Link text "Quarto"');

  // Find "information." after link
  // This Str uses a Concat SourceInfo (two pieces: "information" + ".")
  // This tests that Concat resolution correctly spans all pieces
  const infoStr = findInlineStr(firstPara.components, 'information.');
  assert.ok(infoStr, 'Should find "information." Str');
  verifyNode(content, infoStr, 'information.', 'Para "information." word (Concat test)');
});

test('substring invariant - links.qmd: inline code', () => {
  const { content, doc } = loadExample('links');

  console.log('\n--- links.qmd: inline code ---');

  // Second block is para with inline code
  const secondPara = doc.components[2];
  assert.strictEqual(secondPara.kind, 'Para', 'Second block should be Para');

  // Find the Code inline
  const code = findInlineByKind(secondPara.components, 'Code');
  assert.ok(code, 'Should find Code node');

  // Code source range includes backticks: `x = 5`
  // This is correct - the source includes the syntax characters
  verifyNode(content, code, '`x = 5`', 'Inline code with backticks');

  // Verify the result field contains just the code without backticks
  assert.strictEqual(code.result[1], 'x = 5', 'Code result should be without backticks');
});

test('substring invariant - links.qmd: blockquote with nested link', () => {
  const { content, doc } = loadExample('links');

  console.log('\n--- links.qmd: blockquote ---');

  // Third block is BlockQuote
  const blockquote = doc.components[3];
  assert.strictEqual(blockquote.kind, 'BlockQuote', 'Third block should be BlockQuote');

  // BlockQuote contains a Para
  const para = blockquote.components[0];
  assert.strictEqual(para.kind, 'Para', 'BlockQuote should contain Para');

  // Find "blockquote" text
  const blockquoteStr = findInlineStr(para.components, 'blockquote');
  assert.ok(blockquoteStr, 'Should find "blockquote" Str');
  verifyNode(content, blockquoteStr, 'blockquote', 'Blockquote "blockquote" word');

  // Find nested link
  const link = findInlineByKind(para.components, 'Link');
  assert.ok(link, 'Should find Link in blockquote');

  // Link text is "a link"
  const linkTextA = findInlineStr(link.components, 'a');
  const linkTextLink = findInlineStr(link.components, 'link');
  assert.ok(linkTextA, 'Should find "a" in link text');
  assert.ok(linkTextLink, 'Should find "link" in link text');
  verifyNode(content, linkTextA, 'a', 'Blockquote link text "a"');
  verifyNode(content, linkTextLink, 'link', 'Blockquote link text "link"');
});

// ============================================================================
// Test Suite: ordered-list.qmd
// ============================================================================

test('substring invariant - ordered-list.qmd: list items', () => {
  const { content, doc } = loadExample('ordered-list');

  console.log('\n--- ordered-list.qmd: list items ---');

  // Skip metadata, find OrderedList
  const orderedList = doc.components.find(c => c.kind === 'OrderedList');
  assert.ok(orderedList, 'Should find OrderedList');

  // First list item IS a Plain block (not wrapped)
  const firstItemPlain = orderedList.components[0];
  assert.strictEqual(firstItemPlain.kind, 'Plain', 'First list item should be Plain');

  const firstStr = findInlineStr(firstItemPlain.components, 'First');
  assert.ok(firstStr, 'Should find "First" in first item');
  verifyNode(content, firstStr, 'First', 'First list item "First" word');

  const itemStr = findInlineStr(firstItemPlain.components, 'item');
  assert.ok(itemStr, 'Should find "item" in first item');
  verifyNode(content, itemStr, 'item', 'First list item "item" word');
});

test('substring invariant - ordered-list.qmd: header', () => {
  const { content, doc } = loadExample('ordered-list');

  console.log('\n--- ordered-list.qmd: header ---');

  // Find Header "Ordered Lists"
  const header = doc.components.find(c => c.kind === 'Header');
  assert.ok(header, 'Should find Header');

  const orderedStr = findInlineStr(header.components, 'Ordered');
  assert.ok(orderedStr, 'Should find "Ordered" in header');
  verifyNode(content, orderedStr, 'Ordered', 'Header "Ordered" word');

  const listsStr = findInlineStr(header.components, 'Lists');
  assert.ok(listsStr, 'Should find "Lists" in header');
  verifyNode(content, listsStr, 'Lists', 'Header "Lists" word');
});

// ============================================================================
// Test Suite: figure.qmd
// ============================================================================

test('substring invariant - figure.qmd: caption text', () => {
  const { content, doc } = loadExample('figure');

  console.log('\n--- figure.qmd: figure caption ---');

  // Find Figure block with caption
  const figure = doc.components.find(c => c.kind === 'Figure');
  assert.ok(figure, 'Should find Figure block');

  // Caption is in a Plain block inside the Figure (second component, after attr-id)
  const captionPlain = figure.components.find(c => c.kind === 'Plain');
  assert.ok(captionPlain, 'Should find Plain with caption');

  const simpleStr = findInlineStr(captionPlain.components, 'Simple');
  assert.ok(simpleStr, 'Should find "Simple" in caption');
  verifyNode(content, simpleStr, 'Simple', 'Caption "Simple" word');

  const figureStr = findInlineStr(captionPlain.components, 'figure');
  assert.ok(figureStr, 'Should find "figure" in caption');
  verifyNode(content, figureStr, 'figure', 'Caption "figure" word');
});

// ============================================================================
// Test Suite: div-attrs.qmd
// ============================================================================

test('substring invariant - div-attrs.qmd: div content', () => {
  const { content, doc } = loadExample('div-attrs');

  console.log('\n--- div-attrs.qmd: div content ---');

  // Find Div
  const div = doc.components.find(c => c.kind === 'Div');
  assert.ok(div, 'Should find Div');

  // Div contains Para with "This is a note callout."
  const para = div.components.find(c => c.kind === 'Para');
  assert.ok(para, 'Div should contain Para');

  // Test "note" word in the div content
  const noteStr = findInlineStr(para.components, 'note');
  assert.ok(noteStr, 'Should find "note" in div');
  verifyNode(content, noteStr, 'note', 'Div "note" word');

  // Test "This" word
  const thisStr = findInlineStr(para.components, 'This');
  assert.ok(thisStr, 'Should find "This" in div');
  verifyNode(content, thisStr, 'This', 'Div "This" word');
});

// ============================================================================
// Test Suite: horizontal-rule.qmd
// ============================================================================

test('substring invariant - horizontal-rule.qmd: text around rules', () => {
  const { content, doc } = loadExample('horizontal-rule');

  console.log('\n--- horizontal-rule.qmd: horizontal rules ---');

  // Find Para with "Above"
  const abovePara = doc.components.find(c => {
    if (c.kind !== 'Para') return false;
    return findInlineStr(c.components, 'Above') !== null;
  });

  assert.ok(abovePara, 'Should find Para with "Above"');

  const aboveStr = findInlineStr(abovePara.components, 'Above');
  assert.ok(aboveStr, 'Should find "Above" str');
  verifyNode(content, aboveStr, 'Above', '"Above" word before rule');

  // Find Para with "Below"
  const belowPara = doc.components.find(c => {
    if (c.kind !== 'Para') return false;
    return findInlineStr(c.components, 'Below') !== null;
  });

  assert.ok(belowPara, 'Should find Para with "Below"');

  const belowStr = findInlineStr(belowPara.components, 'Below');
  assert.ok(belowStr, 'Should find "Below" str');
  verifyNode(content, belowStr, 'Below', '"Below" word after rule');

  // Find Para with "Final"
  const finalPara = doc.components.find(c => {
    if (c.kind !== 'Para') return false;
    return findInlineStr(c.components, 'Final') !== null;
  });

  assert.ok(finalPara, 'Should find Para with "Final"');

  const finalStr = findInlineStr(finalPara.components, 'Final');
  assert.ok(finalStr, 'Should find "Final" str');
  verifyNode(content, finalStr, 'Final', '"Final" word');
});

// ============================================================================
// Summary
// ============================================================================

console.log('\n' + '='.repeat(60));
console.log('Substring Invariant Test Suite Complete');
console.log('='.repeat(60));
console.log('All tests verify: source.value.substring(start, end) == expected text');
console.log('Ground truth established via indexOf() on source .qmd files');
console.log('='.repeat(60) + '\n');
