/**
 * Pool kinds 3 (Legacy) and 4 (Generated) — bd-htza1cii.
 *
 * pampa's JSON writer emits code 4 for content a pipeline transform produced
 * (`crates/pampa/src/writers/json.rs`, `SerializableSourceMapping::Generated`);
 * code 3 is read-only legacy (`crates/pampa/src/readers/json.rs`). The
 * semantics mirrored here are the Rust ones (`quarto-source-map`,
 * `SourceInfo::resolve_byte_range` / `invocation_anchor`):
 *
 * - 4 resolves through its `invocation` anchor; with no invocation anchor
 *   it has no source location (a `value-source` anchor alone does not count).
 * - 3 with a numeric head `[parent_id, ...]` is a Substring of that parent.
 * - 3 with a string head `[filter_path, line]` is a Generated-by-filter with
 *   no anchors, hence no source location.
 *
 * "No source location" is not an error: the error handler is not called.
 */

import { test } from 'node:test';
import { strict as assert } from 'node:assert';
import { SourceInfoReconstructor, SerializableSourceInfo, SourceContext } from '../src/source-map.js';
import { MetadataConverter } from '../src/meta-converter.js';
import type { JsonMetaValue } from '../src/types.js';

const content = 'Hello World';
const ctx: SourceContext = { files: [{ id: 0, path: 'test.qmd', content }] };

function make(pool: SerializableSourceInfo[]) {
  const errors: string[] = [];
  const r = new SourceInfoReconstructor(pool, ctx, (msg) => {
    errors.push(msg);
  });
  return { r, errors };
}

test('Generated with an invocation anchor resolves to the anchor', () => {
  const { r, errors } = make([
    { r: [6, 11], t: 0, d: 0 }, // "World"
    { r: [0, 0], t: 4, d: { by: { kind: 'shortcode' }, from: [{ role: 'invocation', si_id: 0 }] } },
  ]);
  assert.deepEqual(r.getSourceLocation(1), { fileId: 0, start: 6, end: 11 });
  const fields = r.getAnnotatedParseSourceFields(1);
  assert.equal(fields.source.value.substring(fields.start, fields.end), 'World');
  assert.equal(r.toMappedString(1).value, 'World');
  assert.equal(r.hasSourceLocation(1), true);
  assert.deepEqual(errors, []);
});

test('Generated prefers the invocation anchor over a value-source anchor', () => {
  const { r } = make([
    { r: [0, 5], t: 0, d: 0 }, // "Hello"
    { r: [6, 11], t: 0, d: 0 }, // "World"
    {
      r: [0, 0],
      t: 4,
      d: {
        by: { kind: 'meta-shortcode' },
        from: [
          { role: 'value-source', si_id: 0 },
          { role: 'invocation', si_id: 1 },
        ],
      },
    },
  ]);
  assert.deepEqual(r.getSourceLocation(2), { fileId: 0, start: 6, end: 11 });
});

test('Generated without anchors has no source location and reports no error', () => {
  const { r, errors } = make([{ r: [0, 0], t: 4, d: { by: { kind: 'config-default' } } }]);
  assert.equal(r.hasSourceLocation(0), false);
  assert.deepEqual(r.getSourceLocation(0), { fileId: -1, start: 0, end: 0 });
  const fields = r.getAnnotatedParseSourceFields(0);
  assert.equal(fields.source.value, '');
  assert.equal(fields.start, 0);
  assert.equal(fields.end, 0);
  assert.equal(r.toMappedString(0).value, '');
  assert.deepEqual(errors, []);
});

test('Generated with only a value-source anchor has no source location (Rust parity)', () => {
  const { r, errors } = make([
    { r: [0, 5], t: 0, d: 0 },
    { r: [0, 0], t: 4, d: { by: { kind: 'x' }, from: [{ role: 'value-source', si_id: 0 }] } },
  ]);
  assert.equal(r.hasSourceLocation(1), false);
  assert.deepEqual(errors, []);
});

test('Generated chains through another Generated invocation', () => {
  const { r } = make([
    { r: [0, 5], t: 0, d: 0 },
    { r: [0, 0], t: 4, d: { by: { kind: 'a' }, from: [{ role: 'invocation', si_id: 0 }] } },
    { r: [0, 0], t: 4, d: { by: { kind: 'b' }, from: [{ role: 'invocation', si_id: 1 }] } },
  ]);
  assert.deepEqual(r.getSourceLocation(2), { fileId: 0, start: 0, end: 5 });
});

test('Malformed Generated data goes to the error handler', () => {
  const { r, errors } = make([{ r: [0, 0], t: 4, d: 7 as unknown as never }]);
  r.getSourceLocation(0);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /Generated/);
});

test('Legacy numeric-headed entry is a Substring of its parent', () => {
  const { r, errors } = make([
    { r: [0, 11], t: 0, d: 0 },
    { r: [6, 11], t: 3, d: [0] },
  ]);
  assert.deepEqual(r.getSourceLocation(1), { fileId: 0, start: 6, end: 11 });
  assert.equal(r.toMappedString(1).value, 'World');
  assert.deepEqual(errors, []);
});

test('Legacy string-headed entry (filter provenance) has no source location', () => {
  const { r, errors } = make([{ r: [0, 0], t: 3, d: ['filters/x.lua', 12] }]);
  assert.equal(r.hasSourceLocation(0), false);
  assert.deepEqual(r.getSourceLocation(0), { fileId: -1, start: 0, end: 0 });
  assert.deepEqual(errors, []);
});

test('Concat skips a location-less Generated piece and keeps the others', () => {
  const { r, errors } = make([
    { r: [0, 5], t: 0, d: 0 }, // "Hello"
    { r: [0, 0], t: 4, d: { by: { kind: 'config-default' } } },
    { r: [0, 8], t: 2, d: [[0, 0, 5], [1, 5, 3]] },
  ]);
  assert.deepEqual(r.getSourceLocation(2), { fileId: 0, start: 0, end: 5 });
  assert.deepEqual(errors, []);
});

test('Concat of only location-less pieces has no source location', () => {
  const { r, errors } = make([
    { r: [0, 0], t: 4, d: { by: { kind: 'config-default' } } },
    { r: [0, 3], t: 2, d: [[0, 0, 3]] },
  ]);
  assert.equal(r.hasSourceLocation(1), false);
  assert.deepEqual(errors, []);
});

test('Location-less top-level metadata entries stay in result but not in components', () => {
  // "title: A" at [0, 8]; key source [0, 5], value [7, 8]; plus an injected key.
  const metaCtx: SourceContext = { files: [{ id: 0, path: 'doc.qmd', content: 'title: A\n\nBody' }] };
  const pool: SerializableSourceInfo[] = [
    { r: [0, 5], t: 0, d: 0 }, // key "title"
    { r: [7, 8], t: 0, d: 0 }, // value "A"
    { r: [0, 0], t: 4, d: { by: { kind: 'config-default' } } },
  ];
  const r = new SourceInfoReconstructor(pool, metaCtx);
  const meta: Record<string, JsonMetaValue> = {
    injected: { t: 'MetaMap', c: [], s: 2 } as unknown as JsonMetaValue,
    title: { t: 'MetaString', c: 'A', s: 1 } as unknown as JsonMetaValue,
  };
  const ap = new MetadataConverter(r, { title: 0, injected: 2 }).convertMeta(meta);
  assert.equal(ap.start, 0);
  assert.equal(ap.end, 8);
  // An AnnotatedParse tree describes source text: a key no source produced
  // has no place among the components, but the JSON value keeps it.
  const keys = ap.components.filter((c) => c.kind === 'key').map((c) => c.result);
  assert.deepEqual(keys, ['title']);
  assert.equal(ap.components.length, 2);
  assert.deepEqual(Object.keys(ap.result as object).sort(), ['injected', 'title']);
});
