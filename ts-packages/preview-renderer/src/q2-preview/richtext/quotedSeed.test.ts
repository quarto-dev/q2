// bd-iwv3708i — Quoted inline renders as editable plaintext quotes, not a chip.
//
// A Pandoc `Quoted` node (double/single) must seed the rich-text editor as
// literal straight quote characters (`"…"` / `'…'`) wrapping still-WYSIWYG
// content, NOT as an opaque `chip` atom. This mirrors how Emph/Strong recurse
// into their content; the only difference is that the delimiter characters are
// emitted as literal text so the user edits them directly.
//
// Pure (no pampa oracle) so it runs in the default `vitest run` suite. The
// serialize→reparse round-trip (straight quotes -> pampa `Quoted`) is guarded
// separately by roundtrip.test.ts under QUARTO_RUN_PAMPA_ROUNDTRIP=1.

import { describe, it, expect } from 'vitest';
import type { Node as PMNode } from '@tiptap/pm/model';
import { astToDoc } from './astToProseMirror';
import { docToMarkdown } from './serializer';
import { richTextSchema } from './schema';
import type { AstNode } from './ast';

const STR = (c: string): AstNode => ({ t: 'Str', c } as unknown as AstNode);
const SPACE: AstNode = { t: 'Space' } as unknown as AstNode;
const STRONG = (...c: AstNode[]): AstNode => ({ t: 'Strong', c } as unknown as AstNode);
const EMPH = (...c: AstNode[]): AstNode => ({ t: 'Emph', c } as unknown as AstNode);
const PARA = (...c: AstNode[]): AstNode => ({ t: 'Para', c } as unknown as AstNode);
// Pandoc Quoted content is [QuoteType, Inline[]].
const QUOTED = (qt: 'SingleQuote' | 'DoubleQuote', ...c: AstNode[]): AstNode =>
  ({ t: 'Quoted', c: [{ t: qt }, c] } as unknown as AstNode);

/** Count `chip` atoms anywhere in a ProseMirror doc. */
function chipCount(doc: PMNode): number {
  let n = 0;
  doc.descendants((node) => {
    if (node.type.name === 'chip') n += 1;
  });
  return n;
}

describe('bd-iwv3708i — astToDoc seeds Quoted as editable plaintext quotes', () => {
  it('double-quoted text becomes literal "…" text, not a chip', () => {
    const para = PARA(STR('He'), SPACE, STR('said'), SPACE, QUOTED('DoubleQuote', STR('smart'), SPACE, STR('quotes')));
    const { doc, unknown } = astToDoc([para], [], '');
    expect(unknown).toEqual([]);
    expect(chipCount(doc)).toBe(0);
    expect(doc.textContent).toBe('He said "smart quotes"');
  });

  it('single-quoted text becomes literal \'…\' text, not a chip', () => {
    const para = PARA(STR('A'), SPACE, QUOTED('SingleQuote', STR('single'), SPACE, STR('quoted')), SPACE, STR('phrase'));
    const { doc, unknown } = astToDoc([para], [], '');
    expect(unknown).toEqual([]);
    expect(chipCount(doc)).toBe(0);
    expect(doc.textContent).toBe("A 'single quoted' phrase");
  });

  it('serializes back to straight quotes (double)', () => {
    const para = PARA(QUOTED('DoubleQuote', STR('smart'), SPACE, STR('quotes')));
    const { doc } = astToDoc([para], [], '');
    expect(docToMarkdown(doc).trim()).toBe('"smart quotes"');
  });

  it('serializes back to straight quotes (single)', () => {
    const para = PARA(QUOTED('SingleQuote', STR('x')));
    const { doc } = astToDoc([para], [], '');
    expect(docToMarkdown(doc).trim()).toBe("'x'");
  });

  it('marks inside a quote stay WYSIWYG (bold + italic recurse)', () => {
    // "very *important* text" with bold on "very"
    const para = PARA(
      QUOTED('DoubleQuote', STR('very'), SPACE, EMPH(STR('important')), SPACE, STR('text')),
    );
    const { doc, unknown } = astToDoc([para], [], '');
    expect(unknown).toEqual([]);
    expect(chipCount(doc)).toBe(0);
    // The quote chars are literal text; the inner Emph serializes as `_…_` (qmd italic).
    expect(docToMarkdown(doc).trim()).toBe('"very _important_ text"');
  });

  it('nested quotes recurse to nested literal quotes', () => {
    // "outer 'inner' done"
    const para = PARA(
      QUOTED('DoubleQuote', STR('outer'), SPACE, QUOTED('SingleQuote', STR('inner')), SPACE, STR('done')),
    );
    const { doc, unknown } = astToDoc([para], [], '');
    expect(unknown).toEqual([]);
    expect(chipCount(doc)).toBe(0);
    expect(doc.textContent).toBe(`"outer 'inner' done"`);
    expect(docToMarkdown(doc).trim()).toBe(`"outer 'inner' done"`);
  });

  it('empty quotes emit just the delimiter pair', () => {
    const para = PARA(QUOTED('DoubleQuote'));
    const { doc, unknown } = astToDoc([para], [], '');
    expect(unknown).toEqual([]);
    expect(chipCount(doc)).toBe(0);
    expect(doc.textContent).toBe('""');
  });
});

describe('bd-iwv3708i — serializer passes straight quotes through unescaped', () => {
  const S = richTextSchema;

  // Guards the assumption the Quoted->plaintext conversion relies on:
  // prosemirror-markdown's esc() escapes only ` * \ ~ [ ] _ — never `"`/`'`.
  // Built directly from the schema (not via astToDoc) so it isolates the
  // serializer from the AST-conversion change.
  it('literal "…" and \'…\' text serialize verbatim (no backslash escapes)', () => {
    const doc = S.node('doc', null, [
      S.node('paragraph', null, [S.text(`say "hi" and 'yo' now`)]),
    ]);
    expect(docToMarkdown(doc).trim()).toBe(`say "hi" and 'yo' now`);
  });
});
