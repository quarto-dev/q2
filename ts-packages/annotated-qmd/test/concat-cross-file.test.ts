/**
 * Cross-file `Concat` range tests
 *
 * These pin `SourceInfoReconstructor#mapContentRange`'s `Concat` arm for a
 * `Concat` whose pieces come from *different* files. `content-provenance.test.ts`
 * covers the escaping/collapsing behaviour of `mapContentRange` but its two
 * fixtures (`attr-value-provenance.qmd`, `yaml-block-scalar.qmd`) never
 * produce a cross-file `Concat` — every piece in those fixtures resolves to
 * the same file. None of the 20 examples committed under
 * `test/fixtures/examples/` does either.
 *
 * A **synthetic pool** is therefore the right (and only) way to exercise this
 * arm: we hand-build a `SerializableSourceInfo[]` with two `Original` pieces
 * pointing at two different `file_id`s and drive `SourceInfoReconstructor`
 * directly, the same way `test/source-map.test.ts` and
 * `test/meta-conversion.test.ts` already do for other arms.
 *
 * This is *not* the pattern `quarto-config/src/span_assert.rs` forbids.
 * That module's ban on `SourceInfo::for_test()` is about **producing** a
 * span for a *different* system under test — a synthetic span there would
 * make a wrong span indistinguishable from a right one, defeating span
 * assertions elsewhere in the pipeline. Here the reconstructor **is** the
 * system under test, and its contract is exactly "given this wire shape,
 * produce this range" — the synthetic pool *is* its real input domain, not
 * a stand-in for something else.
 *
 * Fixed by the `lastPieceFileId` split in `mapContentRange`'s `Concat` arm
 * (see the comment there): before that fix, a piece lying entirely *before*
 * the query wrote `file_id` in the "content ran out" fallback branch, so it
 * could pre-empt (case 1) or masquerade as (case 2) the file a later,
 * genuinely-contributing piece resolves to.
 */

import { test } from 'node:test';
import assert from 'node:assert';
import {
  SourceInfoReconstructor,
  type SerializableSourceInfo,
  type SourceContext,
} from '../src/source-map.js';

/**
 * Two-file `Concat`: piece A owns content `[0, 5)` from file 0, piece B owns
 * content `[5, 10)` from file 1. Both pieces are length-preserving
 * (`Original` whose own `r` extent equals its `Concat` contribution length),
 * so both are indexable rather than opaque.
 *
 * Pool layout:
 *   0: Original, file 0, r=[0,5]                  ("piece A")
 *   1: Original, file 1, r=[0,5]                  ("piece B")
 *   2: Concat, r=[0,10], pieces=[[0,0,5],[1,5,5]]  (A then B)
 *
 * Individual tests append a `Substring` wrapper (id 3) around the Concat to
 * pose a specific content-range query through the public API
 * (`getSourceLocation`), the same way a YAML block-scalar inline wraps its
 * parent Concat in real fixtures.
 */
function twoFileConcatPool(wrapperRange: [number, number]): {
  pool: SerializableSourceInfo[];
  sourceContext: SourceContext;
} {
  const pool: SerializableSourceInfo[] = [
    { r: [0, 5], t: 0, d: 0 }, // 0: piece A, file 0
    { r: [0, 5], t: 0, d: 1 }, // 1: piece B, file 1
    { r: [0, 10], t: 2, d: [[0, 0, 5], [1, 5, 5]] }, // 2: Concat(A, B)
    { r: wrapperRange, t: 1, d: 2 }, // 3: Substring wrapping the Concat
  ];
  const sourceContext: SourceContext = {
    files: [
      { id: 0, path: 'file0.qmd', content: 'AAAAA' },
      { id: 1, path: 'file1.qmd', content: 'BBBBB' },
    ],
  };
  return { pool, sourceContext };
}

const WRAPPER_ID = 3;

test('Concat: query landing entirely in the second piece resolves to that piece\'s file', () => {
  // Content range [6, 8) is entirely inside piece B's contribution [5, 10).
  const { pool, sourceContext } = twoFileConcatPool([6, 8]);
  const errors: string[] = [];
  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, (msg) => {
    errors.push(msg);
  });

  const loc = reconstructor.getSourceLocation(WRAPPER_ID);

  assert.deepStrictEqual(loc, { fileId: 1, start: 1, end: 3 });
  assert.deepStrictEqual(errors, [], 'no error should be reported');
});

test('Concat: zero-width query at the very end resolves to the last piece\'s file', () => {
  // Content range [10, 10) is the zero-width point at the concat's end,
  // past every piece's extent -> falls into the "content ran out" fallback.
  const { pool, sourceContext } = twoFileConcatPool([10, 10]);
  const errors: string[] = [];
  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, (msg) => {
    errors.push(msg);
  });

  const loc = reconstructor.getSourceLocation(WRAPPER_ID);

  assert.deepStrictEqual(loc, { fileId: 1, start: 5, end: 5 });
  assert.deepStrictEqual(errors, [], 'no error should be reported');
});

test('Concat: a query genuinely spanning both pieces still reports a conflict', () => {
  // Content range [3, 7) overlaps piece A's contribution [0, 5) (at [3,5))
  // AND piece B's contribution [5, 10) (at [5,7)) -- a real cross-file span.
  const { pool, sourceContext } = twoFileConcatPool([3, 7]);
  const errors: string[] = [];
  const reconstructor = new SourceInfoReconstructor(pool, sourceContext, (msg) => {
    errors.push(msg);
  });

  const loc = reconstructor.getSourceLocation(WRAPPER_ID);

  assert.strictEqual(loc.fileId, -1, 'genuinely cross-file spans have no single file_id');
  assert.strictEqual(errors.length, 1, 'exactly one error should be reported');
  assert.match(errors[0], /span more than one file/);
});
