/**
 * Probe fixture for `@automerge/automerge` cursor semantics.
 *
 * Pins the cursor behaviours that `presenceService.ts` + `usePresence.ts`
 * rely on to the currently-installed Automerge version (2.2.9 at the time
 * of writing). If any of these behaviours change in a future upgrade,
 * CI will fail here and we can revisit the presence design explicitly
 * rather than discover the regression via user reports of misplaced
 * remote cursors.
 *
 * Covers the edge cases enumerated in
 * `claude-notes/plans/2026-04-21-automerge-cursors-for-presence.md`
 * under "Design sketch → Edge cases".
 */

import { describe, it, expect } from 'vitest';
import { next as A } from '@automerge/automerge';

describe('automerge cursor semantics probe (pins @automerge/automerge 2.2.9 behaviour)', () => {
  it('EOF cursor: offset === length returns the "e" sentinel and tracks through appends', () => {
    let doc = A.from({ text: 'hello' });
    const eof = A.getCursor(doc, ['text'], 5);
    expect(eof).toBe('e');
    expect(A.getCursorPosition(doc, ['text'], eof)).toBe(5);
    doc = A.change(doc, (d) => A.splice(d, ['text'], 5, 0, 'X'));
    expect(A.getCursorPosition(doc, ['text'], eof)).toBe(6);
  });

  it('empty doc: cursor at offset 0 is the "e" sentinel and tracks through inserts', () => {
    let doc = A.from({ text: '' });
    const c = A.getCursor(doc, ['text'], 0);
    expect(c).toBe('e');
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(0);
    doc = A.change(doc, (d) => A.splice(d, ['text'], 0, 0, 'hello'));
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(5);
  });

  it('past-EOF offset clamps to the end sentinel (does not throw)', () => {
    const doc = A.from({ text: 'hello' });
    const c = A.getCursor(doc, ['text'], 999);
    expect(c).toBe('e');
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(5);
  });

  it('mid-doc cursor with default bias: inserts at the cursor offset push the cursor forward', () => {
    let doc = A.from({ text: 'hello' });
    const c = A.getCursor(doc, ['text'], 3);
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(3);
    doc = A.change(doc, (d) => A.splice(d, ['text'], 3, 0, 'X'));
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(4);
  });

  it('deletion across a cursor collapses it to editStart + replacement length', () => {
    let doc = A.from({ text: 'abcdefgh' });
    const c = A.getCursor(doc, ['text'], 5); // 'f'
    doc = A.change(doc, (d) => A.splice(d, ['text'], 2, 5, 'X'));
    expect(doc.text).toBe('abXh');
    expect(A.getCursorPosition(doc, ['text'], c)).toBe(3);
  });

  it('cursor anchored to a char not present in the receiver doc throws RangeError', () => {
    const base = A.from({ text: 'hello' });
    // Sender forks the doc and inserts a new char; cursor anchors to that
    // char. The receiver's doc doesn't have the sender's op yet.
    const sender = A.change(A.clone(base), (d) => A.splice(d, ['text'], 2, 0, 'XY'));
    const unsyncedCursor = A.getCursor(sender, ['text'], 3); // anchors to 'Y'
    expect(() => A.getCursorPosition(base, ['text'], unsyncedCursor)).toThrow(RangeError);
  });

  it('move bias: "before" and "after" produce different cursor strings but resolve identically for insertions in the current Automerge version', () => {
    // NOTE: the plan originally expected `move: 'before'` to keep a
    // selection-end cursor from being swept forward by inserts at the
    // boundary. In 2.2.9 this is a no-op — both biases resolve to the
    // same position after mid-doc and EOF inserts. If a future Automerge
    // upgrade fixes this, this test will fail and we should revisit the
    // broadcast path in `presenceService.ts`.
    let doc = A.from({ text: 'hello' });
    const after = A.getCursor(doc, ['text'], 4, 'after');
    const before = A.getCursor(doc, ['text'], 4, 'before');
    expect(after).not.toBe(before); // strings differ
    doc = A.change(doc, (d) => A.splice(d, ['text'], 4, 0, 'X'));
    expect(A.getCursorPosition(doc, ['text'], after)).toBe(
      A.getCursorPosition(doc, ['text'], before),
    );
  });
});
