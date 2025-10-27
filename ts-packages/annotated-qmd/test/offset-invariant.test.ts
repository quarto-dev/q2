/**
 * Test that the substring invariant holds for all AnnotatedParse nodes
 *
 * Invariant: For any AnnotatedParse node with (source, start, end),
 * source.value.substring(start, end) should extract the correct text.
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRustQmdDocument, type RustQmdJson } from '../src/index.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const examplesDir = path.join(__dirname, '..', 'examples');

test('offset invariant - links.json components[0]', () => {
  // Load the JSON
  const jsonPath = path.join(examplesDir, 'links.json');
  const json: RustQmdJson = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));

  // Load the .qmd source file
  const qmdPath = path.join(examplesDir, 'links.qmd');
  const qmdContent = fs.readFileSync(qmdPath, 'utf-8');

  // Set the file content
  json.astContext.files[0].content = qmdContent;

  // Parse the document
  const result = parseRustQmdDocument(json);

  console.log('\n=== Testing offset invariant for components[0] ===');

  // The first component is the metadata mapping
  const comp0 = result.components[0];

  console.log('Component 0:');
  console.log('  kind:', comp0.kind);
  console.log('  start:', comp0.start);
  console.log('  end:', comp0.end);
  console.log('  source.value.length:', comp0.source.value.length);

  // The metadata line "title: Links and Images" is at positions 4-27 in the file
  const expectedStart = 4;
  const expectedEnd = 27;
  const expectedText = 'title: Links and Images';

  console.log('\nExpected:');
  console.log('  start:', expectedStart);
  console.log('  end:', expectedEnd);
  console.log('  text:', JSON.stringify(expectedText));

  console.log('\nActual substring extraction:');
  const extractedText = comp0.source.value.substring(comp0.start, comp0.end);
  console.log('  extracted:', JSON.stringify(extractedText));

  console.log('\nDirect file substring [4, 27]:');
  console.log('  direct:', JSON.stringify(qmdContent.substring(4, 27)));

  // Test the invariant
  assert.strictEqual(comp0.start, expectedStart, 'Start offset should be 4');
  assert.strictEqual(comp0.end, expectedEnd, 'End offset should be 27');
  assert.strictEqual(extractedText, expectedText, 'Substring extraction should yield correct text');

  console.log('\n✓ Invariant holds: source.value.substring(start, end) == expected text');
});

console.log('Offset invariant test complete\n');
