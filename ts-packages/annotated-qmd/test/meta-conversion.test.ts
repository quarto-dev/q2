/**
 * Tests for Metadata Conversion
 */

import { strict as assert } from 'assert';
import { SourceInfoReconstructor, SourceContext, SerializableSourceInfo } from '../src/source-map.js';
import { MetadataConverter } from '../src/meta-converter.js';
import type { JsonMetaValue } from '../src/types.js';

console.log('Running Metadata conversion tests...');

// Test 1: Simple string metadata
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntitle: Hello\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [11, 16], t: 0, d: 0 }  // "Hello"
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaString',
    c: 'Hello',
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.result, 'Hello');
  assert.equal(result.kind, 'MetaString');
  // In new architecture: source is top-level, use substring to extract
  assert.equal(result.source.value.substring(result.start, result.end), 'Hello', 'MetaString source substring is value content');
  assert.equal(result.components.length, 0);
  assert.equal(result.start, 11);
  assert.equal(result.end, 16);

  console.log('✔ Simple string metadata works');
}

// Test 2: Boolean metadata
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntoc: true\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [9, 13], t: 0, d: 0 }  // "true"
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaBool',
    c: true,
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.result, true);
  assert.equal(result.kind, 'MetaBool');
  assert.equal(result.components.length, 0);

  console.log('✔ Boolean metadata works');
}

// Test 3: Markdown in metadata (MetaInlines)
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntitle: My **Document**\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [0, 5], t: 0, d: 0 },    // "My"
    { r: [6, 7], t: 0, d: 0 },    // " "
    { r: [8, 16], t: 0, d: 0 },   // "Document"
    { r: [0, 30], t: 0, d: 0 }    // Whole value
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaInlines',
    c: [
      { t: 'Str', c: 'My', s: 0 },
      { t: 'Space', s: 1 },
      { t: 'Strong', c: [{ t: 'Str', c: 'Document', s: 2 }], s: 3 }
    ],
    s: 3
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaInlines');
  assert.ok(Array.isArray(result.result));
  assert.equal((result.result as any).length, 3);
  assert.equal(result.components.length, 0);  // Empty - cannot track internal locations yet

  console.log('✔ Markdown in metadata (MetaInlines) works');
}

// Test 4: MetaList
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\nauthor: [Alice, Bob]\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [17, 22], t: 0, d: 0 },  // "Alice"
    { r: [24, 27], t: 0, d: 0 },  // "Bob"
    { r: [16, 28], t: 0, d: 0 }   // Whole list
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaList',
    c: [
      { t: 'MetaString', c: 'Alice', s: 0 },
      { t: 'MetaString', c: 'Bob', s: 1 }
    ],
    s: 2
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaList');
  assert.ok(Array.isArray(result.result));
  assert.equal((result.result as any[]).length, 2);
  assert.equal((result.result as any[])[0], 'Alice');
  assert.equal((result.result as any[])[1], 'Bob');
  assert.equal(result.components.length, 2);  // Non-empty - contains child AnnotatedParse
  assert.equal(result.components[0].result, 'Alice');
  assert.equal(result.components[1].result, 'Bob');

  console.log('✔ MetaList works');
}

// Test 5: MetaMap
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\nauthor:\n  name: Alice\n  email: alice@example.com\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [18, 23], t: 0, d: 0 },  // "name" key
    { r: [25, 30], t: 0, d: 0 },  // "Alice"
    { r: [33, 38], t: 0, d: 0 },  // "email" key
    { r: [40, 58], t: 0, d: 0 },  // "alice@example.com"
    { r: [16, 59], t: 0, d: 0 }   // Whole map
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaMap',
    c: {
      entries: [
        { key: 'name', key_source: 0, value: { t: 'MetaString', c: 'Alice', s: 1 } },
        { key: 'email', key_source: 2, value: { t: 'MetaString', c: 'alice@example.com', s: 3 } }
      ]
    },
    s: 4
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaMap');
  assert.equal(typeof result.result, 'object');
  assert.equal((result.result as any).name, 'Alice');
  assert.equal((result.result as any).email, 'alice@example.com');
  assert.equal(result.components.length, 4);  // Interleaved key/value pairs
  assert.equal(result.components[0].kind, 'key');
  assert.equal(result.components[0].result, 'name');
  assert.equal(result.components[1].result, 'Alice');
  assert.equal(result.components[2].kind, 'key');
  assert.equal(result.components[2].result, 'email');
  assert.equal(result.components[3].result, 'alice@example.com');

  console.log('✔ MetaMap works');
}

// Test 6: Tagged YAML value (!expr)
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ncompute: !expr x + 1\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [17, 22], t: 0, d: 0 },  // "x + 1"
    { r: [17, 22], t: 0, d: 0 }   // Whole value
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaInlines',
    c: [
      {
        t: 'Span',
        c: [
          { t: '', c: ['yaml-tagged-string'], kv: [['tag', 'expr']] },
          [{ t: 'Str', c: 'x + 1', s: 0 }]
        ],
        s: 1
      }
    ],
    s: 1
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaInlines:tagged:expr');
  assert.ok(Array.isArray(result.result));

  console.log('✔ Tagged YAML value (!expr) works');
}

// Test 7: Top-level metadata conversion
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntitle: Hello\nauthor: Alice\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [11, 16], t: 0, d: 0 },  // "Hello"
    { r: [25, 30], t: 0, d: 0 }   // "Alice"
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const jsonMeta: Record<string, JsonMetaValue> = {
    title: { t: 'MetaString', c: 'Hello', s: 0 },
    author: { t: 'MetaString', c: 'Alice', s: 1 }
  };

  const result = converter.convertMeta(jsonMeta);

  assert.equal(result.kind, 'mapping');
  assert.equal(typeof result.result, 'object');
  assert.equal((result.result as any).title, 'Hello');
  assert.equal((result.result as any).author, 'Alice');
  assert.equal(result.components.length, 4);  // Interleaved: title key, title value, author key, author value

  console.log('✔ Top-level metadata conversion works');
}

// Test 8: Nested structures
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\nauthor:\n  - name: Alice\n    email: alice@example.com\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [20, 25], t: 0, d: 0 },   // "name" key
    { r: [27, 32], t: 0, d: 0 },   // "Alice"
    { r: [37, 42], t: 0, d: 0 },   // "email" key
    { r: [44, 62], t: 0, d: 0 },   // "alice@example.com"
    { r: [18, 63], t: 0, d: 0 },   // Whole map
    { r: [16, 64], t: 0, d: 0 }    // Whole list
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaList',
    c: [
      {
        t: 'MetaMap',
        c: {
          entries: [
            { key: 'name', key_source: 0, value: { t: 'MetaString', c: 'Alice', s: 1 } },
            { key: 'email', key_source: 2, value: { t: 'MetaString', c: 'alice@example.com', s: 3 } }
          ]
        },
        s: 4
      }
    ],
    s: 5
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaList');
  assert.ok(Array.isArray(result.result));
  assert.equal((result.result as any[]).length, 1);
  assert.equal(typeof (result.result as any[])[0], 'object');
  assert.equal((result.result as any[])[0].name, 'Alice');
  assert.equal((result.result as any[])[0].email, 'alice@example.com');

  console.log('✔ Nested structures work');
}

console.log('\nAll Metadata conversion tests passed! ✨');
