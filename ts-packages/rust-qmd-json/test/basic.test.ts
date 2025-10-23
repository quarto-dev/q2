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
    source_pool: [
      { r: [11, 22], t: 0, d: 0 },  // "Hello World"
      { r: [31, 36], t: 0, d: 0 }   // "Alice"
    ],
    source_context: {
      files: [
        { id: 0, path: 'test.qmd', content: '---\ntitle: Hello World\nauthor: Alice\n---' }
      ]
    }
  };

  const result = parseRustQmdMetadata(json);

  assert.strictEqual(result.kind, 'mapping');
  assert.strictEqual(typeof result.result, 'object');
  assert.strictEqual((result.result as any).title, 'Hello World');
  assert.strictEqual((result.result as any).author, 'Alice');
  assert.strictEqual(result.components.length, 4);  // title key, title value, author key, author value
});
