/**
 * Test suite for example files
 *
 * Loads each example JSON file from examples/ directory and performs
 * basic conversions and validations to ensure:
 * 1. The examples are valid and loadable
 * 2. The conversion API works on real documents
 * 3. The examples serve as living documentation
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  parseRustQmdDocument,
  parseRustQmdMetadata,
  parseRustQmdBlocks,
  parseRustQmdBlock,
  type RustQmdJson
} from '../src/index.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const examplesDir = path.join(__dirname, '..', 'examples');

/**
 * Helper to load an example JSON file
 */
function loadExample(name: string): RustQmdJson {
  const filePath = path.join(examplesDir, `${name}.json`);
  const content = fs.readFileSync(filePath, 'utf-8');
  return JSON.parse(content);
}

test('simple.json - complete document conversion', () => {
  const json = loadExample('simple');
  const doc = parseRustQmdDocument(json);

  // Should have Document kind
  assert.strictEqual(doc.kind, 'Document');

  // Should have components (metadata + blocks)
  assert.ok(doc.components.length > 0, 'Document should have components');

  // First component should be metadata
  const metaComponent = doc.components[0];
  assert.strictEqual(metaComponent.kind, 'mapping', 'First component should be metadata mapping');

  // Should have blocks after metadata
  assert.ok(doc.components.length > 1, 'Document should have blocks after metadata');

  // Verify we can access block kinds
  const blockComponents = doc.components.slice(1); // Skip metadata
  const blockKinds = blockComponents.map(c => c.kind);

  // Simple.qmd has: Header, Para, Header, CodeBlock, BulletList
  assert.ok(blockKinds.includes('Header'), 'Should have Header blocks');
  assert.ok(blockKinds.includes('Para'), 'Should have Para blocks');
  assert.ok(blockKinds.includes('CodeBlock'), 'Should have CodeBlock');
  assert.ok(blockKinds.includes('BulletList'), 'Should have BulletList');
});

test('simple.json - metadata extraction', () => {
  const json = loadExample('simple');
  const metadata = parseRustQmdMetadata(json);

  assert.strictEqual(metadata.kind, 'mapping');
  assert.ok(metadata.components.length > 0, 'Metadata should have components');

  // Should have title and author keys
  const keys = metadata.components
    .filter(c => c.kind === 'key')
    .map(c => c.result as string);

  assert.ok(keys.includes('title'), 'Should have title key');
  assert.ok(keys.includes('author'), 'Should have author key');

  // Should have MetaInlines values
  const metaInlines = metadata.components.filter(c => c.kind === 'MetaInlines');
  assert.strictEqual(metaInlines.length, 2, 'Should have two MetaInlines values');

  // MetaInlines result should contain the inline array with Str objects
  // (Note: components are empty when converting metadata alone - nested conversion
  //  happens when converting the full document)
  const results = metaInlines.map(m => JSON.stringify(m.result));
  assert.ok(results.some(r => r.includes('Simple')), 'Should have MetaInlines result with "Simple"');
  assert.ok(results.some(r => r.includes('Test')), 'Should have MetaInlines result with "Test"');
});

test('simple.json - individual block conversion', () => {
  const json = loadExample('simple');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  assert.ok(blocks.length > 0, 'Should have blocks');

  // Convert first block individually
  const firstBlock = parseRustQmdBlock(json.blocks[0], json);

  // Should match first element from blocks array
  assert.strictEqual(firstBlock.kind, blocks[0].kind);
  assert.strictEqual(firstBlock.source.value, blocks[0].source.value);
});

test('simple.json - inline element extraction', () => {
  const json = loadExample('simple');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find a Para block with inline content
  const paraBlock = blocks.find(b => b.kind === 'Para');
  assert.ok(paraBlock, 'Should have at least one Para block');

  // Para should have inline components
  assert.ok(paraBlock!.components.length > 0, 'Para should have inline components');

  // Check for different inline types
  const inlineKinds = paraBlock!.components.map(c => c.kind);

  // The para has "This is a simple Quarto document with some **bold** and *italic* text."
  assert.ok(inlineKinds.includes('Str'), 'Should have Str inlines');
  assert.ok(inlineKinds.includes('Space'), 'Should have Space inlines');
  assert.ok(inlineKinds.includes('Strong'), 'Should have Strong (bold) inlines');
  assert.ok(inlineKinds.includes('Emph'), 'Should have Emph (italic) inlines');
});

test('table.json - table structure conversion', () => {
  const json = loadExample('table');
  const doc = parseRustQmdDocument(json);

  // Find the Table block
  const tableBlock = doc.components.find(c => c.kind === 'Table');
  assert.ok(tableBlock, 'Document should contain a Table block');

  // Table should have components (attr, caption, rows, cells)
  assert.ok(tableBlock!.components.length > 0, 'Table should have components');

  // Should have attr-id for the table (tbl-example)
  const attrId = tableBlock!.components.find(c => c.kind === 'attr-id');
  assert.ok(attrId, 'Table should have an ID attribute');
  assert.strictEqual(attrId!.result, 'tbl-example');

  // Should have cell content (Plain blocks with Str inlines)
  // Navigate: Table -> table-head/table-body/table-foot -> table-row -> table-cell -> Plain
  const structuralNodes = tableBlock!.components.filter(
    c => c.kind === 'table-head' || c.kind === 'table-body' || c.kind === 'table-foot'
  );
  const cells = structuralNodes.flatMap(node =>
    node.components.flatMap(row =>
      row.components.filter(c => c.kind === 'table-cell')
    )
  );
  const plainBlocks = cells.flatMap(cell => cell.components.filter(c => c.kind === 'Plain'));
  assert.ok(plainBlocks.length > 0, 'Table cells should have Plain content');
});

test('table.json - table caption', () => {
  const json = loadExample('table');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  const tableBlock = blocks.find(b => b.kind === 'Table');
  assert.ok(tableBlock, 'Should have Table block');

  // Caption content should be in the components
  // The caption long blocks contain Plain blocks with Str inlines
  // Navigate: Table -> caption-long -> Plain
  const captionLong = tableBlock!.components.find(c => c.kind === 'caption-long');
  assert.ok(captionLong, 'Table should have caption-long structural node');
  const plainComponents = captionLong!.components.filter(c => c.kind === 'Plain');
  assert.ok(plainComponents.length > 0, 'Table caption should have Plain components');

  // Collect all Str from Plain components
  const captionText = plainComponents
    .flatMap(p => p.components)
    .filter(c => c.kind === 'Str')
    .map(c => c.result)
    .join(' ');

  assert.ok(captionText.includes('Example'), 'Caption should include "Example"');
  assert.ok(captionText.includes('table'), 'Caption should include "table"');
});

test('links.json - link and inline code conversion', () => {
  const json = loadExample('links');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Para with link
  const paraWithLink = blocks.find(b => {
    return b.kind === 'Para' && b.components.some(c => c.kind === 'Link');
  });
  assert.ok(paraWithLink, 'Should have Para with Link');

  // Extract link component
  const link = paraWithLink!.components.find(c => c.kind === 'Link');
  assert.ok(link, 'Should have Link component');

  // Link should have inline content
  assert.ok(link!.components.length > 0, 'Link should have content');

  // Link content should be "Quarto"
  const linkText = link!.components
    .filter(c => c.kind === 'Str')
    .map(c => c.result)
    .join('');
  assert.strictEqual(linkText, 'Quarto');
});

test('links.json - inline code', () => {
  const json = loadExample('links');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find Para with Code
  const paraWithCode = blocks.find(b => {
    return b.kind === 'Para' && b.components.some(c => c.kind === 'Code');
  });
  assert.ok(paraWithCode, 'Should have Para with Code');

  // Extract code component
  const code = paraWithCode!.components.find(c => c.kind === 'Code');
  assert.ok(code, 'Should have Code component');

  // Code result is [attr, text] - the text is at index 1
  const codeResult = code!.result as any[];
  assert.strictEqual(codeResult[1], 'x = 5');
});

test('links.json - blockquote with nested content', () => {
  const json = loadExample('links');
  const blocks = parseRustQmdBlocks(json.blocks, json);

  // Find BlockQuote
  const blockquote = blocks.find(b => b.kind === 'BlockQuote');
  assert.ok(blockquote, 'Should have BlockQuote');

  // BlockQuote should contain a Para
  const para = blockquote!.components.find(c => c.kind === 'Para');
  assert.ok(para, 'BlockQuote should contain Para');

  // Para should contain a Link
  const link = para!.components.find(c => c.kind === 'Link');
  assert.ok(link, 'BlockQuote Para should contain Link');

  // Verify link target in result (Link result is [attr, [inlines], [url, title]])
  const linkResult = link!.result as any[];
  assert.strictEqual(linkResult[2][0], 'https://example.com');
});

test('all examples - source mapping preservation', () => {
  const examples = ['simple', 'table', 'links'];

  examples.forEach(name => {
    const json = loadExample(name);
    const doc = parseRustQmdDocument(json);

    // Walk all components and verify they have source info
    function checkSource(component: any, depth = 0): void {
      // All components should have source (MappedString)
      assert.ok('source' in component, `Component at depth ${depth} should have source`);
      assert.ok('value' in component.source, 'Source should be a MappedString');

      // All components should have start/end offsets
      assert.ok(typeof component.start === 'number', 'Should have start offset');
      assert.ok(typeof component.end === 'number', 'Should have end offset');

      // Recursively check nested components
      if (component.components && Array.isArray(component.components)) {
        component.components.forEach((child: any) => checkSource(child, depth + 1));
      }
    }

    checkSource(doc);
  });
});

test('all examples - result field preservation', () => {
  const examples = ['simple', 'table', 'links'];

  examples.forEach(name => {
    const json = loadExample(name);
    const doc = parseRustQmdDocument(json);

    // Document result should preserve the original structure
    assert.ok(doc.result, 'Document should have result');

    const result = doc.result as any;
    assert.ok('pandoc-api-version' in result, 'Result should have pandoc-api-version');
    assert.ok('meta' in result, 'Result should have meta');
    assert.ok('blocks' in result, 'Result should have blocks');
  });
});
