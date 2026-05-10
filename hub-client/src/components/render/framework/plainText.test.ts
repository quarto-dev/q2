/**
 * Vitest unit tests for `framework/plainText.ts` (Plan 2D Phase 6.0f).
 *
 * The walks lived in `q2-preview/utils.tsx` before Phase 6.0a; they
 * had no dedicated unit tests there — every assertion was indirect
 * via Image/Note rendering. These tests lock the walk behavior so a
 * future re-tightening can't silently regress meta-string coercion.
 */

import { describe, expect, it } from 'vitest';
import type { BlockNode, InlineNode } from './types';
import { inlinesToPlainText, blocksToPlainText } from './plainText';

describe('inlinesToPlainText', () => {
    it('concatenates Str / Space / SoftBreak / LineBreak', () => {
        const inlines: InlineNode[] = [
            { t: 'Str', c: 'Hello' },
            { t: 'Space' },
            { t: 'Str', c: 'world' },
            { t: 'SoftBreak' },
            { t: 'Str', c: 'and' },
            { t: 'LineBreak' },
            { t: 'Str', c: 'goodbye' },
        ];
        expect(inlinesToPlainText(inlines)).toBe('Hello world and\ngoodbye');
    });

    it('recurses into Emph / Strong / Underline / Strikeout / Super / Sub / SmallCaps', () => {
        expect(
            inlinesToPlainText([
                { t: 'Emph', c: [{ t: 'Str', c: 'a' }] },
                { t: 'Strong', c: [{ t: 'Str', c: 'b' }] },
                { t: 'Underline', c: [{ t: 'Str', c: 'c' }] },
                { t: 'Strikeout', c: [{ t: 'Str', c: 'd' }] },
                { t: 'Superscript', c: [{ t: 'Str', c: 'e' }] },
                { t: 'Subscript', c: [{ t: 'Str', c: 'f' }] },
                { t: 'SmallCaps', c: [{ t: 'Str', c: 'g' }] },
            ]),
        ).toBe('abcdefg');
    });

    it('reads Code content from c[1]', () => {
        expect(
            inlinesToPlainText([
                { t: 'Code', c: [['', [], []], 'snippet'] },
            ] as InlineNode[]),
        ).toBe('snippet');
    });

    it('walks Link / Image visible inlines (ignores URL/title)', () => {
        expect(
            inlinesToPlainText([
                {
                    t: 'Link',
                    c: [
                        ['', [], []],
                        [{ t: 'Str', c: 'click' }],
                        ['/x', ''],
                    ],
                },
            ] as InlineNode[]),
        ).toBe('click');
    });

    it('walks Span / Quoted inlines', () => {
        expect(
            inlinesToPlainText([
                {
                    t: 'Span',
                    c: [
                        ['', [], []],
                        [{ t: 'Str', c: 'hi' }],
                    ],
                },
                {
                    t: 'Quoted',
                    c: [{ t: 'DoubleQuote' }, [{ t: 'Str', c: 'q' }]],
                },
            ] as InlineNode[]),
        ).toBe('hiq');
    });

    it('emits the LaTeX source for Math', () => {
        expect(
            inlinesToPlainText([
                { t: 'Math', c: [{ t: 'InlineMath' }, 'x^2'] },
            ] as InlineNode[]),
        ).toBe('x^2');
    });

    it('emits RawInline content', () => {
        expect(
            inlinesToPlainText([
                { t: 'RawInline', c: ['html', '<br>'] },
            ] as InlineNode[]),
        ).toBe('<br>');
    });

    it('returns empty string for unknown inlines', () => {
        expect(
            inlinesToPlainText([{ t: 'UnknownVariant', c: 'x' }] as any),
        ).toBe('');
    });
});

describe('blocksToPlainText', () => {
    it('joins block content with a single space, skipping empty parts', () => {
        const blocks: BlockNode[] = [
            { t: 'Para', c: [{ t: 'Str', c: 'first' }] },
            { t: 'Para', c: [{ t: 'Str', c: 'second' }] },
        ];
        expect(blocksToPlainText(blocks)).toBe('first second');
    });

    it('walks Header content from c[2]', () => {
        expect(
            blocksToPlainText([
                { t: 'Header', c: [1, ['', [], []], [{ t: 'Str', c: 'Title' }]] },
            ] as BlockNode[]),
        ).toBe('Title');
    });

    it('emits CodeBlock / RawBlock content from c[1]', () => {
        expect(
            blocksToPlainText([
                {
                    t: 'CodeBlock',
                    c: [['', [], []], 'code body'],
                },
                { t: 'RawBlock', c: ['html', '<p>raw</p>'] },
            ] as BlockNode[]),
        ).toBe('code body <p>raw</p>');
    });

    it('returns empty string for HorizontalRule and CustomBlock', () => {
        expect(
            blocksToPlainText([
                { t: 'HorizontalRule' },
                { t: 'CustomBlock' },
            ] as any),
        ).toBe('');
    });
});
