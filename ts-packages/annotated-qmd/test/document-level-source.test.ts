/**
 * Test document-level source mapping
 *
 * Reproduces issue where top-level AnnotatedParse from parseRustQmdDocument
 * has start=0, end=0, and source.value="" instead of full document content.
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRustQmdDocument, type RustQmdJson } from '../src/index.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const examplesDir = path.join(__dirname, '..', 'examples');

test('document-level source mapping - links.json', () => {
  // Load the JSON
  const jsonPath = path.join(examplesDir, 'links.json');
  const json: RustQmdJson = JSON.parse(fs.readFileSync(jsonPath, 'utf-8'));

  // Load the .qmd source file
  const qmdPath = path.join(examplesDir, 'links.qmd');
  const qmdContent = fs.readFileSync(qmdPath, 'utf-8');

  // Set the file content (as user described)
  json.astContext.files[0].content = qmdContent;

  // Parse the document
  const result = parseRustQmdDocument(json);

  console.log('\n=== Document-level source mapping ===');
  console.log('result.kind:', result.kind);
  console.log('result.start:', result.start);
  console.log('result.end:', result.end);
  console.log('result.source.value.length:', result.source.value.length);
  console.log('result.source.value:', JSON.stringify(result.source.value.substring(0, 50)));
  console.log('qmdContent.length:', qmdContent.length);
  console.log('Expected source to contain document, but got empty string:', result.source.value === '');
  console.log('===\n');

  // Verify document spans entire file
  assert.strictEqual(result.start, 0, 'Document starts at offset 0');
  assert.strictEqual(result.end, qmdContent.length, 'Document ends at content length');
  assert.strictEqual(result.source.value, qmdContent, 'Document source is the entire .qmd content');

  // Verify invariant: source.value.substring(start, end) == full content
  assert.strictEqual(result.source.value.substring(result.start, result.end), qmdContent, 'Substring extraction works correctly');
});

console.log('Document-level source mapping test complete\n');
