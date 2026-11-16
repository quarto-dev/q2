/**
 * bd-ov4jydyf — comment text → inlines, and appending a comment to a
 * block's source node.
 *
 * The comment input is plain text. A new line (Shift+Enter, or a paste)
 * must become a Pandoc `LineBreak` (qmd `\`+newline), which stays
 * inside the `[>> …]` span. It must never reach the AST as a raw `\n`
 * inside a `Str`: the writer prints that verbatim, and a blank line
 * then ends the paragraph mid-comment (Q-2-21). A heading is one source
 * line, so there new lines become spaces.
 *
 * Plan: claude-notes/plans/2026-11-16-comment-shift-enter-linebreak.md
 */

import { describe, it, expect } from 'vitest';
import type { BlockNode, InlineNode } from '../framework';
import {
    COMMENT_CONTAINER_CLASS,
    appendCommentToBlock,
    commentLineBreaksAllowed,
    commentTextToInlines,
    makeCommentSpan,
} from './commentEdits';

const S = (c: string) => ({ t: 'Str', c });
const SP = { t: 'Space' };
const LB = { t: 'LineBreak' };
const lines = (text: string) => commentTextToInlines(text, { allowLineBreaks: true });
const oneLine = (text: string) => commentTextToInlines(text, { allowLineBreaks: false });

describe('commentTextToInlines', () => {
    it('splits words into Str and Space, the way the qmd reader does', () => {
        expect(lines('hello world')).toEqual([S('hello'), SP, S('world')]);
    });

    it('collapses runs of spaces and tabs to one Space', () => {
        expect(lines('a   \t b')).toEqual([S('a'), SP, S('b')]);
    });

    it('keeps a non-breaking space inside the word (the reader does too)', () => {
        expect(lines('a b')).toEqual([S('a b')]);
    });

    it('turns a new line into a LineBreak', () => {
        expect(lines('line one\nline two')).toEqual([
            S('line'), SP, S('one'), LB, S('line'), SP, S('two'),
        ]);
    });

    it('treats \\r\\n and \\r as a new line', () => {
        expect(lines('a\r\nb')).toEqual([S('a'), LB, S('b')]);
        expect(lines('a\rb')).toEqual([S('a'), LB, S('b')]);
    });

    it('keeps an empty line in the middle as two LineBreaks', () => {
        expect(lines('a\n\nb')).toEqual([S('a'), LB, LB, S('b')]);
    });

    it('never starts or ends with a LineBreak', () => {
        expect(lines('\n\na\n\n')).toEqual([S('a')]);
        expect(lines('  \n a \n  ')).toEqual([S('a')]);
    });

    it('drops whitespace at the start and end of each line', () => {
        expect(lines('a\n   b')).toEqual([S('a'), LB, S('b')]);
        expect(lines('a  \nb')).toEqual([S('a'), LB, S('b')]);
    });

    it('returns [] for whitespace-only text', () => {
        expect(lines('')).toEqual([]);
        expect(lines('  \n \t\n')).toEqual([]);
    });

    it('keeps markdown-looking text literal (the writer escapes it)', () => {
        expect(lines('a\n# b')).toEqual([S('a'), LB, S('#'), SP, S('b')]);
        expect(lines('*x*')).toEqual([S('*x*')]);
    });

    it('joins lines with a Space when line breaks are not allowed', () => {
        expect(oneLine('a\nb')).toEqual([S('a'), SP, S('b')]);
        expect(oneLine('a\n\n  b')).toEqual([S('a'), SP, S('b')]);
    });

    it('never produces whitespace in a Str, nor a leading/trailing break or Space', () => {
        const inputs = [
            'x', ' x ', 'a b\nc', '\n\n', 'a\n\n\nb ', '\t a\t\n\tb\t', 'a \n \n b',
            'one\r\n\r\ntwo', '  # h\n- i\n1. j  ',
        ];
        for (const input of inputs) {
            for (const allowLineBreaks of [true, false]) {
                const out = commentTextToInlines(input, { allowLineBreaks });
                const edge = (n: InlineNode | undefined) =>
                    n !== undefined && (n.t === 'LineBreak' || n.t === 'Space');
                expect(edge(out[0]), JSON.stringify(input)).toBe(false);
                expect(edge(out[out.length - 1]), JSON.stringify(input)).toBe(false);
                for (const n of out) {
                    if (n.t === 'Str') expect((n as { c: string }).c).not.toMatch(/[ \t\r\n]/);
                    if (!allowLineBreaks) expect(n.t).not.toBe('LineBreak');
                }
                // Never two Spaces in a row, and never Space next to LineBreak.
                for (let i = 1; i < out.length; i++) {
                    const pair = `${out[i - 1].t},${out[i].t}`;
                    expect(['Space,Space', 'Space,LineBreak', 'LineBreak,Space']).not.toContain(pair);
                }
            }
        }
    });
});

describe('commentLineBreaksAllowed', () => {
    it('is false only for headings', () => {
        expect(commentLineBreaksAllowed({ t: 'Header', c: [1, ['', [], []], []] })).toBe(false);
        expect(commentLineBreaksAllowed({ t: 'Para', c: [] })).toBe(true);
        expect(commentLineBreaksAllowed({ t: 'Plain', c: [] })).toBe(true);
        expect(commentLineBreaksAllowed({ t: 'CodeBlock', c: [['', [], []], 'x'] })).toBe(true);
        expect(commentLineBreaksAllowed(null)).toBe(true);
    });
});

describe('appendCommentToBlock', () => {
    const comment = makeCommentSpan([S('a'), LB, S('b')] as InlineNode[]);

    it('appends to a paragraph without mutating the input', () => {
        const para: BlockNode = { t: 'Para', c: [S('Text.')] as InlineNode[] };
        const before = structuredClone(para);
        const out = appendCommentToBlock(para, comment);
        expect(para).toEqual(before);
        expect(out).toEqual({ t: 'Para', c: [S('Text.'), comment] });
    });

    it('appends to a heading inline slot', () => {
        const h: BlockNode = { t: 'Header', c: [2, ['h', [], []], [S('T')] as InlineNode[]] };
        expect(appendCommentToBlock(h, comment)).toEqual({
            t: 'Header', c: [2, ['h', [], []], [S('T'), comment]],
        });
    });

    it('wraps a code block in a comment container', () => {
        const code: BlockNode = { t: 'CodeBlock', c: [['', [], []], 'x = 1'] };
        expect(appendCommentToBlock(code, comment)).toEqual({
            t: 'Div',
            c: [['', [COMMENT_CONTAINER_CLASS], []], [code, { t: 'Para', c: [comment] }]],
        });
    });

    it('appends to the last comment paragraph of an existing container', () => {
        const old = makeCommentSpan([S('old')] as InlineNode[]);
        const container: BlockNode = {
            t: 'Div',
            c: [['', [COMMENT_CONTAINER_CLASS], []], [
                { t: 'CodeBlock', c: [['', [], []], 'x'] },
                { t: 'Para', c: [old] },
            ]],
        };
        const out = appendCommentToBlock(container, comment) as { c: [unknown, BlockNode[]] };
        expect(out.c[1][1]).toEqual({ t: 'Para', c: [old, comment] });
    });

    it('adds a comment paragraph to a container that has none left', () => {
        const container: BlockNode = {
            t: 'Div',
            c: [['', [COMMENT_CONTAINER_CLASS], []], [{ t: 'CodeBlock', c: [['', [], []], 'x'] }]],
        };
        const out = appendCommentToBlock(container, comment) as { c: [unknown, BlockNode[]] };
        expect(out.c[1]).toHaveLength(2);
        expect(out.c[1][1]).toEqual({ t: 'Para', c: [comment] });
    });

    it('returns null for a block that cannot hold a comment', () => {
        expect(appendCommentToBlock({ t: 'HorizontalRule' }, comment)).toBeNull();
        expect(appendCommentToBlock({ t: 'Div', c: [['', ['note'], []], []] }, comment)).toBeNull();
    });
});
