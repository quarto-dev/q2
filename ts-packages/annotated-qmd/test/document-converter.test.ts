/**
 * DocumentConverter Tests
 *
 * Comprehensive tests for the DocumentConverter class that orchestrates
 * InlineConverter, BlockConverter, and MetadataConverter.
 */

import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { SourceInfoReconstructor } from '../src/source-map.js';
import { DocumentConverter } from '../src/document-converter.js';
import type { RustQmdJson } from '../src/types.js';

/**
 * Load example JSON fixture and populate file content
 */
function loadExample(name: string): RustQmdJson {
  const jsonPath = resolve(process.cwd(), `examples/${name}.json`);
  const json = JSON.parse(readFileSync(jsonPath, 'utf-8')) as RustQmdJson;

  // Load corresponding QMD file content
  const qmdPath = resolve(process.cwd(), `examples/${name}.qmd`);
  const qmdContent = readFileSync(qmdPath, 'utf-8');

  // Populate content for all files (typically just one)
  for (const file of json.astContext.files) {
    file.content = qmdContent;
  }

  return json;
}

/**
 * Create SourceContext from RustQmdJson astContext
 */
function createSourceContext(json: RustQmdJson) {
  return {
    files: json.astContext.files.map((f, idx) => ({
      id: idx,
      path: f.name,
      content: f.content || ''
    }))
  };
}

describe('DocumentConverter', () => {

  test('convertDocument() with complete document (meta + blocks)', () => {
    // Load simple.json which has both metadata and blocks
    const json = loadExample('simple');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(
      reconstructor,
      json.astContext.metaTopLevelKeySources
    );

    const result = converter.convertDocument(json);

    // Should be a Document
    assert.equal(result.kind, 'Document');

    // Should have the full JSON as result
    assert.ok(result.result);
    assert.equal(typeof result.result, 'object');

    // Should have source mapping to entire document
    assert.ok(result.source);
    assert.ok(result.source.value.length > 0);

    // Should start at beginning of document
    assert.equal(result.start, 0);

    // Should end at end of document
    assert.equal(result.end, result.source.value.length);

    // Should have components: first metadata, then blocks
    assert.ok(Array.isArray(result.components));
    assert.ok(result.components.length > 0);

    // First component should be metadata (kind: 'mapping')
    assert.equal(result.components[0].kind, 'mapping');

    // Remaining components should be blocks
    for (let i = 1; i < result.components.length; i++) {
      assert.ok(result.components[i].kind !== 'mapping',
        `Component ${i} should be a block, not mapping`);
    }
  });

  test('convertDocument() with metadata only', () => {
    // Create a minimal document with just metadata
    const json: RustQmdJson = {
      "pandoc-api-version": [1, 23, 1],
      meta: {
        title: {
          t: "MetaString",
          c: "Test Title",
          s: 0
        }
      },
      blocks: [],
      astContext: {
        sourceInfoPool: [
          { t: 0, d: 0, r: [0, 100] }
        ],
        files: [
          { name: "test.qmd", total_length: 100, line_breaks: [], content: "---\ntitle: Test Title\n---\n" }
        ],
        metaTopLevelKeySources: { title: 0 }
      }
    };

    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(
      reconstructor,
      json.astContext.metaTopLevelKeySources
    );

    const result = converter.convertDocument(json);

    assert.equal(result.kind, 'Document');
    assert.equal(result.components.length, 1, 'Should have only metadata component');
    assert.equal(result.components[0].kind, 'mapping');
  });

  test('convertDocument() with blocks only (no metadata)', () => {
    // Load a document without metadata (or create one)
    const json: RustQmdJson = {
      "pandoc-api-version": [1, 23, 1],
      meta: {},
      blocks: [
        {
          t: "Para",
          c: [
            { t: "Str", c: "Hello", s: 0 },
            { t: "Space", s: 1 },
            { t: "Str", c: "World", s: 2 }
          ],
          s: 3
        }
      ],
      astContext: {
        sourceInfoPool: [
          { t: 0, d: 0, r: [0, 5] },    // "Hello"
          { t: 0, d: 0, r: [5, 6] },    // space
          { t: 0, d: 0, r: [6, 11] },   // "World"
          { t: 0, d: 0, r: [0, 11] }    // Para
        ],
        files: [
          { name: "test.qmd", total_length: 11, line_breaks: [], content: "Hello World" }
        ]
      }
    };

    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(reconstructor);

    const result = converter.convertDocument(json);

    assert.equal(result.kind, 'Document');
    assert.equal(result.components.length, 1, 'Should have only block component');
    assert.equal(result.components[0].kind, 'Para');
  });

  test('convertBlocks() converts array of blocks', () => {
    const json = loadExample('simple');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(reconstructor);

    const results = converter.convertBlocks(json.blocks);

    assert.ok(Array.isArray(results));
    assert.equal(results.length, json.blocks.length);

    // Each result should be an AnnotatedParse
    for (const result of results) {
      assert.ok(result.kind);
      assert.ok(result.source);
      assert.ok(Array.isArray(result.components));
      assert.equal(typeof result.start, 'number');
      assert.equal(typeof result.end, 'number');
    }
  });

  test('convertBlock() converts individual blocks', () => {
    const json = loadExample('simple');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(reconstructor);

    // Test converting the first block
    const firstBlock = json.blocks[0];
    const result = converter.convertBlock(firstBlock);

    assert.ok(result.kind);
    assert.ok(result.source);
    assert.ok(Array.isArray(result.components));
    assert.equal(typeof result.start, 'number');
    assert.equal(typeof result.end, 'number');

    // Result should match the block type
    assert.equal(result.kind, firstBlock.t);
  });

  test('convertInline() converts individual inlines', () => {
    const json = loadExample('links');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(reconstructor);

    // Get first inline from first Para block
    const firstPara = json.blocks.find(b => b.t === 'Para');
    assert.ok(firstPara, 'Should have a Para block');
    assert.ok('c' in firstPara && Array.isArray(firstPara.c));

    const firstInline = firstPara.c[0];
    const result = converter.convertInline(firstInline);

    assert.ok(result.kind);
    assert.ok(result.source);
    assert.ok(Array.isArray(result.components));
    assert.equal(typeof result.start, 'number');
    assert.equal(typeof result.end, 'number');

    // Result should match the inline type
    assert.equal(result.kind, firstInline.t);
  });

  test('component ordering: metadata before blocks', () => {
    const json = loadExample('simple');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(
      reconstructor,
      json.astContext.metaTopLevelKeySources
    );

    const result = converter.convertDocument(json);

    // Verify metadata comes first
    const metadataComponent = result.components[0];
    assert.equal(metadataComponent.kind, 'mapping', 'First component should be metadata');

    // Verify blocks come after
    for (let i = 1; i < result.components.length; i++) {
      const component = result.components[i];
      // Should be a block type (Para, Header, CodeBlock, etc.)
      assert.ok(component.kind !== 'mapping',
        `Component ${i} should be a block, not metadata`);
      assert.ok(component.kind !== 'key',
        `Component ${i} should be a block, not a key`);
    }
  });

  test('document source spans entire file', () => {
    const json = loadExample('simple');
    const sourceContext = createSourceContext(json);
    const reconstructor = new SourceInfoReconstructor(
      json.astContext.sourceInfoPool,
      sourceContext,
      () => {}
    );
    const converter = new DocumentConverter(
      reconstructor,
      json.astContext.metaTopLevelKeySources
    );

    const result = converter.convertDocument(json);

    // Document should span from start to end of file
    assert.equal(result.start, 0, 'Document should start at position 0');

    // Document end should match source length
    assert.equal(result.end, result.source.value.length,
      'Document should end at source length');

    // Source should contain actual file content
    assert.ok(result.source.value.length > 0, 'Source should have content');

    // For simple.json, we know the content
    const qmdContent = readFileSync(resolve(process.cwd(), 'examples/simple.qmd'), 'utf-8');
    assert.equal(result.source.value, qmdContent,
      'Source should match original QMD content');
  });
});

console.log('\nDocumentConverter tests complete\n');
