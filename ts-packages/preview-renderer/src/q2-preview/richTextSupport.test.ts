// Unit tests for the rich-text availability gate (bd-7pxub583).
//
// `richTextSupport.ts` is the single source of truth for "which block types the
// rich-text editor handles". It had no direct test before; these lock the
// membership set and the flag/mode interactions. The load-bearing case for
// bd-7pxub583 is `Plain` — tight bullet/ordered list items are stored as `Plain`
// blocks, and omitting `Plain` from the set left their content non-rich-editable.

import { describe, it, expect } from 'vitest';
import {
  RICHTEXT_SUPPORTED_TYPES,
  richTextAvailable,
  richEditorActiveForType,
} from './richTextSupport';
import type { PreviewContextValue } from './PreviewContext';

/** Minimal ctx — the predicates only read `richText` and `editorMode`. */
function ctx(over: Partial<PreviewContextValue>): PreviewContextValue {
  return over as PreviewContextValue;
}

describe('RICHTEXT_SUPPORTED_TYPES', () => {
  it('includes the leaf-text block types the editor can seed', () => {
    // Para + Header were the original set; Plain (tight list items, table cells)
    // is the bd-7pxub583 addition.
    expect(RICHTEXT_SUPPORTED_TYPES.has('Para')).toBe(true);
    expect(RICHTEXT_SUPPORTED_TYPES.has('Header')).toBe(true);
    expect(RICHTEXT_SUPPORTED_TYPES.has('Plain')).toBe(true);
  });

  it('excludes container / non-leaf-text block types', () => {
    for (const t of ['BulletList', 'OrderedList', 'CodeBlock', 'BlockQuote', 'Div', 'Table']) {
      expect(RICHTEXT_SUPPORTED_TYPES.has(t)).toBe(false);
    }
  });
});

describe('richTextAvailable', () => {
  it('is true for a supported type when the flag is on', () => {
    expect(richTextAvailable(ctx({ richText: true }), 'Para')).toBe(true);
    expect(richTextAvailable(ctx({ richText: true }), 'Header')).toBe(true);
    // The bug: a tight-list item resolves a `Plain` sourceNode and must be
    // rich-editable.
    expect(richTextAvailable(ctx({ richText: true }), 'Plain')).toBe(true);
  });

  it('is false for an unsupported type even when the flag is on', () => {
    expect(richTextAvailable(ctx({ richText: true }), 'CodeBlock')).toBe(false);
    expect(richTextAvailable(ctx({ richText: true }), 'BulletList')).toBe(false);
  });

  it('is false whenever the flag is off', () => {
    expect(richTextAvailable(ctx({ richText: false }), 'Plain')).toBe(false);
    expect(richTextAvailable(ctx({ richText: false }), 'Para')).toBe(false);
    expect(richTextAvailable(ctx({}), 'Plain')).toBe(false);
  });
});

describe('richEditorActiveForType', () => {
  it('is true for a Plain target with the flag on and mode rich (the default)', () => {
    expect(richEditorActiveForType(ctx({ richText: true }), 'Plain')).toBe(true);
    expect(richEditorActiveForType(ctx({ richText: true, editorMode: 'rich' }), 'Plain')).toBe(true);
  });

  it('is false for a Plain target when the user toggled to plain mode', () => {
    expect(richEditorActiveForType(ctx({ richText: true, editorMode: 'plain' }), 'Plain')).toBe(
      false,
    );
  });

  it('is false for a Plain target when the flag is off', () => {
    expect(richEditorActiveForType(ctx({ richText: false }), 'Plain')).toBe(false);
  });
});
