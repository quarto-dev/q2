/**
 * Tests for runtime type safety
 */

import { strict as assert } from 'assert';
import { SourceInfoReconstructor, SourceContext, SerializableSourceInfo } from '../src/source-map.js';
import { MetadataConverter } from '../src/meta-converter.js';
import type { JsonMetaValue } from '../src/types.js';

console.log('Running type safety tests...');

// Test 1: Invalid Original SourceInfo data type
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: 'Hello World' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [0, 5], t: 0, d: 'invalid' as unknown }  // Should be number
  ];

  let errorCalled = false;
  const errorHandler = (msg: string) => {
    errorCalled = true;
    assert.ok(msg.includes('must be a number'));
  };

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, errorHandler);
  const mapped = reconstructor.toMappedString(0);

  assert.ok(errorCalled);
  assert.equal(mapped.value, '');  // Fallback
  console.log('✔ Invalid Original data type caught');
}

// Test 2: Invalid Substring SourceInfo data type
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: 'Hello World' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [0, 11], t: 0, d: 0 },
    { r: [6, 11], t: 1, d: { invalid: 'object' } as unknown }  // Should be number
  ];

  let errorCalled = false;
  const errorHandler = (msg: string) => {
    errorCalled = true;
    assert.ok(msg.includes('must be a number'));
  };

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, errorHandler);
  const mapped = reconstructor.toMappedString(1);

  assert.ok(errorCalled);
  assert.equal(mapped.value, '');  // Fallback
  console.log('✔ Invalid Substring data type caught');
}

// Test 3: Invalid Concat SourceInfo data type
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: 'Hello World' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [0, 5], t: 0, d: 0 },
    { r: [0, 10], t: 2, d: 'not-an-object' as unknown }  // Should be {pieces: [...]}
  ];

  let errorCalled = false;
  const errorHandler = (msg: string) => {
    errorCalled = true;
    assert.ok(msg.includes('Invalid Concat data format'));
  };

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, errorHandler);
  const mapped = reconstructor.toMappedString(1);

  assert.ok(errorCalled);
  assert.equal(mapped.value, '');  // Fallback
  console.log('✔ Invalid Concat data type caught');
}

// Test 4: Invalid MetaList content type
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\nauthor: invalid\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [16, 23], t: 0, d: 0 }
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaList',
    c: 'not-an-array' as unknown,  // Should be array
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaList');
  assert.deepEqual(result.result, []);  // Fallback to empty array
  assert.equal(result.components.length, 0);
  console.log('✔ Invalid MetaList content type handled gracefully');
}

// Test 5: Invalid MetaMap content type
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\nauthor: invalid\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [16, 23], t: 0, d: 0 }
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaMap',
    c: 'not-an-object' as unknown,  // Should be {entries: [...]}
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaMap');
  assert.deepEqual(result.result, {});  // Fallback to empty object
  assert.equal(result.components.length, 0);
  console.log('✔ Invalid MetaMap content type handled gracefully');
}

// Test 6: Non-string MetaString content
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntitle: 123\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [11, 14], t: 0, d: 0 }
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaString',
    c: 123 as unknown,  // Number instead of string
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaString');
  assert.equal(result.result, '123');  // Converted to string
  console.log('✔ Non-string MetaString content converted');
}

// Test 7: Non-boolean MetaBool content
{
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'test.qmd', content: '---\ntoc: 1\n---' }
    ]
  };

  const pool: SerializableSourceInfo[] = [
    { r: [9, 10], t: 0, d: 0 }
  ];

  const reconstructor = new SourceInfoReconstructor(pool, sourceContext);
  const converter = new MetadataConverter(reconstructor);

  const metaValue: JsonMetaValue = {
    t: 'MetaBool',
    c: 1 as unknown,  // Number instead of boolean
    s: 0
  };

  const result = converter.convertMetaValue(metaValue);

  assert.equal(result.kind, 'MetaBool');
  assert.equal(result.result, true);  // Converted to boolean
  console.log('✔ Non-boolean MetaBool content converted');
}

console.log('\nAll type safety tests passed! ✨');
