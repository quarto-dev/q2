import { describe, it, expect } from 'vitest';
import { Schema } from '@tiptap/pm/model';
import { richTextSchema as S } from './schema';
import { editorialAvailability } from './editorialSelection';

const span = (kind: string) => S.marks.span.create({ attr: ['', [], []], comments: [], kind });
// Paragraph "aa BBB cc DD": B's carry insert, D's carry delete (adjacent runs differ in kind).
const doc = S.nodes.doc.create(null, [
  S.nodes.paragraph.create({ comments: [] }, [
    S.text('aa '),
    S.text('BBB', [span('insert')]),
    S.text(' cc '),
    S.text('DD', [span('delete')]),
    S.text('EE', [span('insert')]),
  ]),
  S.nodes.paragraph.create({ comments: [] }, [S.text('second')]),
]);
// positions: para1 content starts at 1. "aa " 1-4, BBB 4-7, " cc " 7-11, DD 11-13, EE 13-15.

describe('editorialAvailability', () => {
  it('none when empty or blank', () => {
    expect(editorialAvailability(doc, 2, 2).mode).toBe('none');
    expect(editorialAvailability(doc, 3, 4).mode).toBe('none'); // just a space
  });
  it('add for plain text', () => {
    expect(editorialAvailability(doc, 1, 3).mode).toBe('add');
  });
  it('none across paragraphs', () => {
    expect(editorialAvailability(doc, 2, 20).mode).toBe('none');
  });
  it('remove, only that kind, for the exact span', () => {
    const a = editorialAvailability(doc, 4, 7);
    expect(a.mode).toBe('remove');
    if (a.mode === 'remove') expect(a.kind).toBe('insert');
    const d = editorialAvailability(doc, 11, 13); // adjacent insert must not merge in
    expect(d.mode === 'remove' && d.kind).toBe('delete');
  });
  it('none for partial or enlarged span selections', () => {
    expect(editorialAvailability(doc, 4, 6).mode).toBe('none');
    expect(editorialAvailability(doc, 3, 7).mode).toBe('none');
    expect(editorialAvailability(doc, 4, 9).mode).toBe('none');
  });
});

describe('editorialAvailability with a different (name-identical) schema', () => {
  it('uses the document schema, as the live tiptap editor does', () => {
    const S2 = new Schema({ nodes: S.spec.nodes, marks: S.spec.marks });
    const m = S2.marks.span.create({ attr: ['', [], []], comments: [], kind: 'highlight' });
    const d = S2.nodes.doc.create(null, [
      S2.nodes.paragraph.create({ comments: [] }, [S2.text('x '), S2.text('YY', [m])]),
    ]);
    const a = editorialAvailability(d, 3, 5);
    expect(a.mode === 'remove' && a.kind).toBe('highlight');
  });
});
