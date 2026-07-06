// bd-7pxub583 — the single-`Plain` seed path used by RichTextEditor.
//
// When the rich editor opens on a tight-list item (or a table cell), its
// `resolved.sourceNode` is a `Plain` block, and it seeds the editor with
// `astToDoc([Plain], ...)` (RichTextEditor.tsx). This guards that exact call:
// a lone `Plain` maps to a single paragraph node and serializes back to the bare
// inline text — no wrapper block, no dropped inlines, no list markers. Tightness
// itself is preserved by the backend on commit (pampa `preserve_leaf_variant`,
// covered by node_edit_tests.rs); here we only assert the TS seed→serialize.
//
// Pure (no pampa oracle) so it runs in the default `vitest run` suite.

import { describe, it, expect } from 'vitest';
import { astToDoc } from './astToProseMirror';
import { docToMarkdown } from './serializer';
import type { AstNode } from './ast';

const STR = (c: string): AstNode => ({ t: 'Str', c } as unknown as AstNode);
const SPACE: AstNode = { t: 'Space' } as unknown as AstNode;
const STRONG = (...c: AstNode[]): AstNode => ({ t: 'Strong', c } as unknown as AstNode);
const EMPH = (...c: AstNode[]): AstNode => ({ t: 'Emph', c } as unknown as AstNode);
const PLAIN = (...c: AstNode[]): AstNode => ({ t: 'Plain', c } as unknown as AstNode);

describe('bd-7pxub583 — astToDoc seeds a lone Plain as one paragraph', () => {
    it('maps a plain-text Plain to a single paragraph with no unknown nodes', () => {
        const { doc, unknown } = astToDoc([PLAIN(STR('apple'))], [], '- apple\n');
        expect(unknown).toEqual([]);
        expect(doc.childCount).toBe(1);
        expect(doc.firstChild?.type.name).toBe('paragraph');
        expect(doc.textContent).toBe('apple');
    });

    it('serializes the seeded paragraph back to the bare inline text (no marker/block)', () => {
        const { doc } = astToDoc([PLAIN(STR('apple'))], [], '- apple\n');
        const md = docToMarkdown(doc);
        // No leading "- ", no wrapping — just the item's inline content.
        expect(md.trim()).toBe('apple');
    });

    it('round-trips inline marks inside a tight-list item', () => {
        // "some **bold** and _italic_ text"
        const plain = PLAIN(
            STR('some'), SPACE, STRONG(STR('bold')), SPACE, STR('and'), SPACE,
            EMPH(STR('italic')), SPACE, STR('text'),
        );
        const { doc, unknown } = astToDoc([plain], [], '');
        expect(unknown).toEqual([]);
        const md = docToMarkdown(doc).trim();
        expect(md).toContain('**bold**');
        // qmd italic serializes with `_` (serializer.ts), not `*`.
        expect(md).toContain('_italic_');
        expect(md).toBe('some **bold** and _italic_ text');
    });
});
