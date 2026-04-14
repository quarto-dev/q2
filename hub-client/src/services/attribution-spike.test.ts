/**
 * Phase 0.5: diff() spike test — validate Automerge patch shapes for text operations.
 *
 * This test validates the actual patch format returned by Automerge's diff() function
 * for this project's document model (TextDocumentContent with a `text` field).
 *
 * KEY FINDINGS (documented for attribution service implementation):
 * - Patches use `insert` and `del` actions (NOT `splice` as some docs suggest)
 * - `insert`: { action: 'insert', path: [fieldName, index], values: string[] }
 *   - `values` is an array of individual characters (not a single string)
 *   - Character count = values.length
 * - `del`: { action: 'del', path: [fieldName, index], length: number }
 * - Replace = `del` + `insert` at the same position in the same diff
 * - path[0] = field name (e.g., "text"), path[1] = character index
 *
 * @vitest-environment node
 */

import { describe, it, expect } from 'vitest';
import { from, change, diff, getHeads, Text } from '@automerge/automerge';

/** Helper to create a doc matching the project's TextDocumentContent schema */
function createTextDoc(initialText = '') {
  const doc = from({ text: new Text(initialText) });
  return doc;
}

/** Helper to insert text via collaborative splice */
function insertText(doc: ReturnType<typeof createTextDoc>, index: number, text: string) {
  return change(doc, d => {
    (d.text as unknown as { insertAt(idx: number, ...chars: string[]): void }).insertAt(index, ...text.split(''));
  });
}

/** Helper to delete text via collaborative splice */
function deleteText(doc: ReturnType<typeof createTextDoc>, index: number, length: number) {
  return change(doc, d => {
    (d.text as unknown as { deleteAt(idx: number, len: number): void }).deleteAt(index, length);
  });
}

describe('Automerge diff() patch shapes for TextDocumentContent', () => {
  it('text insertion generates insert patch with values array', () => {
    let doc = createTextDoc();
    const headsBefore = getHeads(doc);
    doc = insertText(doc, 0, 'hello');
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    expect(patches).toHaveLength(1);

    const patch = patches[0];
    expect(patch.action).toBe('insert');
    expect(patch.path).toEqual(['text', 0]);
    expect((patch as { values: string[] }).values).toEqual(['h', 'e', 'l', 'l', 'o']);
  });

  it('text insertion at non-zero index has correct path', () => {
    let doc = createTextDoc();
    doc = insertText(doc, 0, 'hello');
    const headsBefore = getHeads(doc);
    doc = insertText(doc, 5, ' world');
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    expect(patches).toHaveLength(1);

    const patch = patches[0];
    expect(patch.action).toBe('insert');
    expect(patch.path).toEqual(['text', 5]);
    expect((patch as { values: string[] }).values).toEqual([' ', 'w', 'o', 'r', 'l', 'd']);
  });

  it('text deletion generates del patch with length', () => {
    let doc = createTextDoc();
    doc = insertText(doc, 0, 'hello world');
    const headsBefore = getHeads(doc);
    doc = deleteText(doc, 6, 5); // delete "world"
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    expect(patches).toHaveLength(1);

    const patch = patches[0];
    expect(patch.action).toBe('del');
    expect(patch.path).toEqual(['text', 6]);
    expect((patch as { length: number }).length).toBe(5);
  });

  it('mixed replace generates del + insert patches in order', () => {
    let doc = createTextDoc();
    doc = insertText(doc, 0, 'hello world');
    const headsBefore = getHeads(doc);

    // Replace "world" (index 6, len 5) with "there"
    doc = change(doc, d => {
      const text = d.text as unknown as {
        deleteAt(idx: number, len: number): void;
        insertAt(idx: number, ...chars: string[]): void;
      };
      text.deleteAt(6, 5);
      text.insertAt(6, ...'there'.split(''));
    });
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    expect(patches).toHaveLength(2);

    // First patch: deletion
    expect(patches[0].action).toBe('del');
    expect(patches[0].path).toEqual(['text', 6]);
    expect((patches[0] as { length: number }).length).toBe(5);

    // Second patch: insertion at same position
    expect(patches[1].action).toBe('insert');
    expect(patches[1].path).toEqual(['text', 6]);
    expect((patches[1] as { values: string[] }).values).toEqual(['t', 'h', 'e', 'r', 'e']);
  });

  it('path[0] is the field name, path[1] is the character index', () => {
    let doc = createTextDoc();
    const headsBefore = getHeads(doc);
    doc = insertText(doc, 0, 'a');
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    const patch = patches[0];

    // path structure: [fieldName, charIndex]
    expect(typeof patch.path[0]).toBe('string');
    expect(patch.path[0]).toBe('text');
    expect(typeof patch.path[1]).toBe('number');
    expect(patch.path[1]).toBe(0);
  });

  it('non-text patches are filtered by checking path[0]', () => {
    // If we had a doc with multiple fields, patches for other fields
    // would have a different path[0]. Our attribution service should
    // filter to only process patches where path[0] === textFieldName.
    let doc = createTextDoc();
    const headsBefore = getHeads(doc);
    doc = insertText(doc, 0, 'hello');
    const headsAfter = getHeads(doc);

    const patches = diff(doc, headsBefore, headsAfter);
    // All patches should have path[0] === 'text'
    for (const patch of patches) {
      expect(patch.path[0]).toBe('text');
    }
  });
});
