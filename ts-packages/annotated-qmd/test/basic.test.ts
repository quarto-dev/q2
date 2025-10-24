/**
 * Basic test to verify @quarto/mapped-string integration
 */

import { test } from 'node:test';
import assert from 'node:assert';
import { MappedString, asMappedString, mappedSubstring } from '@quarto/mapped-string';

test('can import and use @quarto/mapped-string', () => {
  const str = asMappedString("Hello, World!", "test.txt");

  assert.strictEqual(str.value, "Hello, World!");
  assert.strictEqual(str.fileName, "test.txt");

  // Test that map function works
  const result = str.map(0);
  assert.ok(result !== undefined);
  assert.strictEqual(result.index, 0);
  assert.strictEqual(result.originalString, str);
});

test('can create mapped substrings', () => {
  const original = asMappedString("Hello, World!", "test.txt");
  const sub = mappedSubstring(original, 7, 12);

  assert.strictEqual(sub.value, "World");
  assert.strictEqual(sub.fileName, "test.txt");

  // Map through the substring
  const result = sub.map(0);
  assert.ok(result !== undefined);
  assert.strictEqual(result.index, 7); // Should map to offset 7 in original
});

test('can convert complete JSON to AnnotatedParse', async () => {
  const { parseRustQmdMetadata } = await import('../src/index.js');

  const json = {
    meta: {
      title: { t: 'MetaString', c: 'Hello World', s: 0 },
      author: { t: 'MetaString', c: 'Alice', s: 1 }
    },
    blocks: [],
    astContext: {
      sourceInfoPool: [
        { r: [11, 22], t: 0, d: 0 },  // "Hello World"
        { r: [31, 36], t: 0, d: 0 }   // "Alice"
      ],
      files: [
        { name: 'test.qmd', content: '---\ntitle: Hello World\nauthor: Alice\n---' }
      ]
    },
    'pandoc-api-version': [1, 23, 1]
  };

  const result = parseRustQmdMetadata(json);

  assert.strictEqual(result.kind, 'mapping');
  assert.strictEqual(typeof result.result, 'object');
  assert.strictEqual((result.result as any).title, 'Hello World');
  assert.strictEqual((result.result as any).author, 'Alice');
  assert.strictEqual(result.components.length, 4);  // title key, title value, author key, author value
});

test('can parse math-with-attr.json', async () => {
  const { parseRustQmdMetadata } = await import('../src/index.js');
  const fs = await import('fs/promises');
  const path = await import('path');
  const { fileURLToPath } = await import('url');

  // Get the directory of this test file
  const __dirname = path.dirname(fileURLToPath(import.meta.url));

  // Load JSON fixture from test/fixtures
  const jsonPath = path.join(__dirname, 'fixtures', 'math-with-attr.json');
  const jsonText = await fs.readFile(jsonPath, 'utf-8');
  const json = JSON.parse(jsonText);

  // Read the QMD file content from test/fixtures
  const qmdPath = path.join(__dirname, 'fixtures', 'math-with-attr.qmd');
  const qmdContent = await fs.readFile(qmdPath, 'utf-8');

  // Populate file content (simulating what user would do)
  for (const file of json.astContext.files) {
    file.content = qmdContent;
  }

  const result = parseRustQmdMetadata(json);

  // Basic validation that it didn't throw
  assert.strictEqual(result.kind, 'mapping');
  assert.ok(result.result);
  assert.ok((result.result as any).title);
});
