/**
 * Content-provenance range tests
 *
 * These pin the ranges the reader reports for spans whose *decoded content*
 * differs from the *source bytes* that produced it, which is what
 * `AttrSourceInfo.attributes[i].1` and YAML block-scalar spans now carry.
 *
 * Two producers emit such spans today, and they take different wire shapes:
 *
 *   - **Attribute values** (`{key="a\*b"}`) arrive as a top-level `Concat`
 *     of `Original` leaves — or, when the value has no escapes at all and the
 *     tiling collapses to a single verbatim piece, as a plain quote-trimmed
 *     `Original`. There is no `Substring` wrapper.
 *   - **YAML block-scalar inlines** (`title: |`) arrive as
 *     `Substring { parent: Concat }`: the block scalar's decoded text is a
 *     `Concat`, and each inline inside it is a `Substring` of that content.
 *
 * The contract in both cases: `start`/`end` are the **source hull of the bytes
 * that produced the decoded content**, so `source.substring(start, end)` yields
 * the *raw* text (escapes and line-continuation indentation included) while
 * `result` holds the *decoded* text. The two are equal only when the decode was
 * a no-op.
 *
 * The cases that actually bind:
 *
 *   - a value whose escape is *last* (`"y\*"`) — an exclusive end computed by
 *     mapping the last content character and adding one lands *inside* the
 *     two-byte escape;
 *   - an inline *after* a collapsed piece in a block scalar — an offset composed
 *     affinely over the parent's hull drifts by the bytes the collapse removed.
 *
 * A test using only unescaped values, or only the first line of a block scalar,
 * passes under both the correct and the broken implementations.
 *
 * Regenerate the fixtures from `test/fixtures` with:
 *
 *     for q in attr-value-provenance.qmd yaml-block-scalar.qmd; do
 *       cargo run --quiet --bin pampa -- -t json -i "$q" > "${q%.qmd}.json"
 *     done
 */

import { test } from 'node:test';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseRustQmdDocument, type RustQmdJson, type AnnotatedParse } from '../src/index.js';
import { SourceInfoReconstructor } from '../src/source-map.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.join(__dirname, 'fixtures');

/** Load a fixture pair, populating file content the way a consumer must. */
function loadFixture(name: string): { json: RustQmdJson; source: string } {
  const json = JSON.parse(
    fs.readFileSync(path.join(fixturesDir, `${name}.json`), 'utf-8')
  ) as RustQmdJson;
  const source = fs.readFileSync(path.join(fixturesDir, `${name}.qmd`), 'utf-8');
  for (const file of json.astContext.files) {
    (file as { content?: string }).content = source;
  }
  return { json, source };
}

function collect(node: AnnotatedParse, kind: string, out: AnnotatedParse[] = []): AnnotatedParse[] {
  if (node.kind === kind) out.push(node);
  for (const child of node.components ?? []) collect(child, kind, out);
  return out;
}

test('attribute values: range is the source hull of the decoded content', () => {
  const { json, source } = loadFixture('attr-value-provenance');
  const doc = parseRustQmdDocument(json);
  const values = collect(doc, 'attr-value');

  // `::: {plain="hello" esc="a\*b" both="\[x\]" tail="y\*"}`
  const expected: Array<{
    result: string;
    range: [number, number];
    raw: string;
    why: string;
  }> = [
    {
      result: 'hello',
      range: [12, 17],
      raw: 'hello',
      why: 'no escapes: a plain quote-trimmed Original, so raw === decoded'
    },
    {
      result: 'a*b',
      range: [24, 28],
      raw: 'a\\*b',
      why: 'escape in the middle: hull spans the 2-byte escape'
    },
    {
      result: '[x]',
      range: [36, 41],
      raw: '\\[x\\]',
      why: 'escapes at both ends: 5 source bytes for 3 content bytes'
    },
    {
      result: 'y*',
      range: [49, 52],
      raw: 'y\\*',
      why: 'trailing escape: the exclusive end must clear the whole escape'
    }
  ];

  assert.strictEqual(values.length, expected.length, 'fixture should yield 4 attr-values');

  expected.forEach((want, i) => {
    const got = values[i];
    assert.strictEqual(got.result, want.result, `value ${i} decoded text (${want.why})`);
    assert.deepStrictEqual(
      [got.start, got.end],
      want.range,
      `value ${i} source range (${want.why})`
    );
    assert.strictEqual(
      source.substring(got.start, got.end),
      want.raw,
      `value ${i} raw source text (${want.why})`
    );
    assert.strictEqual(
      got.source.value.substring(got.start, got.end),
      want.raw,
      `value ${i} raw text via node.source (${want.why})`
    );
  });

  // The quoted value's range must exclude its quotes on both sides — the old
  // contract included them.
  const plain = values[0];
  assert.strictEqual(source[plain.start - 1], '"', 'opening quote sits just before start');
  assert.strictEqual(source[plain.end], '"', 'closing quote sits at end');
});

test('YAML block-scalar inlines: Substring over Concat maps through the parent, not affinely', () => {
  const { json, source } = loadFixture('yaml-block-scalar');
  const reconstructor = new SourceInfoReconstructor(
    json.astContext.p,
    {
      files: json.astContext.files.map((f, i) => ({
        id: i,
        path: f.name,
        content: (f as { content?: string }).content ?? ''
      }))
    }
  );

  // `title: |` over two indented lines. The decoded scalar is
  // "line one\nline two\n"; the newline+indent between the lines is 3 source
  // bytes collapsed to 1, and the trailing newline is 1 content byte with no
  // source bytes at all. Everything after the first collapse drifts under
  // affine composition.
  const inlines = (json.meta as any).title.c as Array<{ t: string; c?: string; s: number }>;
  const expected: Array<[string, string, [number, number], string]> = [
    ['Str', 'line', [15, 19], 'line'],
    ['Space', '', [19, 20], ' '],
    ['Str', 'one', [20, 23], 'one'],
    ['SoftBreak', '', [23, 26], '\n  '],
    ['Str', 'line', [26, 30], 'line'],
    ['Space', '', [30, 31], ' '],
    ['Str', 'two', [31, 34], 'two']
  ];

  assert.strictEqual(inlines.length, expected.length, 'fixture should yield 7 inlines');

  expected.forEach(([kind, text, range, raw], i) => {
    const inline = inlines[i];
    assert.strictEqual(inline.t, kind, `inline ${i} kind`);
    if (text) assert.strictEqual(inline.c, text, `inline ${i} decoded text`);
    const loc = reconstructor.getSourceLocation(inline.s);
    assert.deepStrictEqual(
      [loc.start, loc.end],
      range,
      `inline ${i} (${kind} ${JSON.stringify(text)}) source range`
    );
    assert.strictEqual(
      source.substring(loc.start, loc.end),
      raw,
      `inline ${i} (${kind} ${JSON.stringify(text)}) raw source text`
    );
  });

  // The whole scalar's hull, for good measure: it must stop at the last
  // content-bearing byte rather than running to the file's end.
  const scalar = reconstructor.getSourceLocation((json.meta as any).title.s);
  assert.deepStrictEqual([scalar.start, scalar.end], [15, 34], 'block scalar hull');
  assert.strictEqual(source.substring(scalar.start, scalar.end), 'line one\n  line two');
});
