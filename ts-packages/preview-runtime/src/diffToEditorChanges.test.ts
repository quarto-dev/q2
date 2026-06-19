/**
 * Tests for diffToEditorChanges (bd-ov4gqk3m).
 *
 * The contract under test: the returned splice operations, applied **in
 * order against the evolving document** (exactly how
 * `applyEditorOperations` splices them into the Automerge text), must
 * transform `currentContent` into `targetContent` — and must do so
 * *minimally*, expressing offsets against the evolving document rather
 * than rewriting shared content.
 *
 * The round-trips drive the REAL automerge text splice — the same
 * `splice(doc, ['text'], rangeOffset, rangeLength, text)` primitive the
 * sync client's `applyEditorOperations` runs inside `handle.change`. This
 * replaces an earlier hand-rolled string-slice mirror of that consumer (a
 * fail-on-revert pass found the mirror could drift from the real splice,
 * and that the round-trips alone did not bind offset correctness — a
 * degenerate "replace the whole document" producer round-tripped too).
 */

import { describe, it, expect } from 'vitest';
import * as Automerge from '@automerge/automerge';
import { diffToEditorChanges } from './diffToEditorChanges';
import type { EditorContentChange } from '@quarto/quarto-sync-client';

/**
 * Apply the changes through the real automerge text splice (the primitive
 * `applyEditorOperations` uses), against a fresh doc seeded with `content`.
 */
function applyViaAutomerge(content: string, changes: EditorContentChange[]): string {
  let doc = Automerge.from<{ text: string }>({ text: content });
  doc = Automerge.change(doc, (d) => {
    for (const change of changes) {
      Automerge.splice(d, ['text'], change.rangeOffset, change.rangeLength, change.text);
    }
  });
  return doc.text;
}

/** Total characters inserted across all changes (0 for a pure deletion). */
function insertedChars(changes: EditorContentChange[]): number {
  return changes.reduce((n, c) => n + c.text.length, 0);
}

describe('diffToEditorChanges', () => {
  it('returns empty array for identical content', () => {
    expect(diffToEditorChanges('hello', 'hello')).toEqual([]);
    expect(diffToEditorChanges('', '')).toEqual([]);
  });

  it('produces a single insertion with exact offsets', () => {
    const changes = diffToEditorChanges('hello', 'hello world');
    expect(changes).toEqual([{ rangeOffset: 5, rangeLength: 0, text: ' world' }]);
  });

  it('produces a single deletion with exact offsets', () => {
    const changes = diffToEditorChanges('hello world', 'hello');
    expect(changes).toEqual([{ rangeOffset: 5, rangeLength: 6, text: '' }]);
  });

  it('expresses later offsets against the evolving document', () => {
    // 'abc def ghi' → 'abcX def ghiY': after the first insert at offset 3,
    // the second insert lands at offset 12 in the *new* string (not 11).
    const current = 'abc def ghi';
    const target = 'abcX def ghiY';
    const changes = diffToEditorChanges(current, target);
    expect(applyViaAutomerge(current, changes)).toBe(target);
    expect(changes).toEqual([
      { rangeOffset: 3, rangeLength: 0, text: 'X' },
      { rangeOffset: 12, rangeLength: 0, text: 'Y' },
    ]);
  });

  it('localizes a small edit instead of rewriting shared content', () => {
    const current = '---\ntitle: Doc\n---\n\nFirst paragraph.\n\nSecond paragraph.\n';
    const target = '---\ntitle: Doc\n---\n\nFirst paragraph, *edited*.\n\nSecond paragraph.\n';
    const changes = diffToEditorChanges(current, target);

    // Round-trips through the real automerge splice…
    expect(applyViaAutomerge(current, changes)).toBe(target);

    // …but a "delete everything, insert the whole target" producer would
    // round-trip too. Bind offset correctness: the shared prefix is left
    // untouched (first change starts past byte 0) and only the inserted
    // fragment is written — not the entire ~65-char document.
    expect(changes[0].rangeOffset).toBeGreaterThan(0);
    expect(insertedChars(changes)).toBeLessThanOrEqual(', *edited*'.length);
  });

  it('round-trips mixed insert/delete/replace edits through the real splice', () => {
    const cases: Array<[string, string]> = [
      ['hello world', 'world'],
      ['world', 'hello world'],
      ['hello', 'hallo'],
      ['abc def ghi', 'ABC def GHI'],
      ['completely different', 'totally new content'],
      ['', 'new content'],
      ['old content', ''],
      ['line1\nline2\nline3', 'line1\nlineX\nline3\nline4'],
      ['a\tb', 'a\t\tb'],
      ['line1\r\nline2', 'line1\nline2'],
    ];
    for (const [current, target] of cases) {
      const changes = diffToEditorChanges(current, target);
      expect(applyViaAutomerge(current, changes), `'${current}' → '${target}'`).toBe(target);
    }
  });

  it('round-trips unicode and emoji content (UTF-16 offsets) through the real splice', () => {
    const cases: Array<[string, string]> = [
      ['hello 世界', 'hello 世界!'],
      ['hello 👋', 'hello 👋🌍'],
      ['naïve café', 'naïve, café'],
    ];
    for (const [current, target] of cases) {
      const changes = diffToEditorChanges(current, target);
      expect(applyViaAutomerge(current, changes), `'${current}' → '${target}'`).toBe(target);
    }
  });
});
