/**
 * Pure helpers for editing `[>> …]` comments in a block's source node
 * (bd-ov4jydyf). Shared by the q2-preview comment chrome
 * (`q2-preview/custom/CommentBlock.tsx`, `CommentSpan.tsx`) and by the
 * hub-client WASM round-trip test, so the test commits exactly what the
 * UI commits.
 *
 * Plan: claude-notes/plans/2026-11-16-comment-shift-enter-linebreak.md
 */
import type {
    BlockNode,
    DivBlock,
    HeaderBlock,
    InlineNode,
    ParaBlock,
    PlainBlock,
    SpanInline,
} from '../framework';

/** Class of a comment editorial-mark span (`[>> …]`). */
export const COMMENT_CLASS = 'quarto-edit-comment';

/**
 * A block that can't hold an inline comment span (a code block) gets
 * wrapped in a Div with this class; the comment lives as a `[>> ...]`
 * paragraph inside the Div.
 */
export const COMMENT_CONTAINER_CLASS = 'quarto-edit-comment-container';

export function isComment(inline: InlineNode): boolean {
    if (inline.t === 'Span' && 'c' in inline) {
        const classes = (inline as SpanInline).c[0][1];
        return classes.includes(COMMENT_CLASS);
    }
    return false;
}

export function isCommentContainer(block: BlockNode): boolean {
    return (
        block.t === 'Div' &&
        ((block as DivBlock).c[0][1] ?? []).includes(COMMENT_CONTAINER_CLASS)
    );
}

/**
 * The mutable inline array where a block's comment spans live, or null
 * when the block type has no inline slot. (Casts because the BlockNode
 * union includes `UnknownBlock { t: string }`, which defeats
 * discriminant narrowing.)
 */
export function inlineSlot(block: BlockNode): InlineNode[] | null {
    if (block.t === 'Para' || block.t === 'Plain') {
        return (block as ParaBlock | PlainBlock).c;
    }
    if (block.t === 'Header') return (block as HeaderBlock).c[2];
    return null;
}

/**
 * Whether a comment stored in (or under) `block` may contain line
 * breaks. A heading is one source line (`# Title [>> …]`): a `\`+newline
 * inside it ends the heading and breaks the document. Everything else
 * that can hold a comment (paragraphs, list-item `Plain`s, comment
 * containers) can span lines.
 */
export function commentLineBreaksAllowed(block: BlockNode | null | undefined): boolean {
    return block?.t !== 'Header';
}

// The qmd reader splits text on ASCII spaces and tabs only; other
// whitespace (e.g. U+00A0) stays inside the word.
const SPACE_RUN = /[ \t]+/;

/**
 * Convert the comment input's plain text to inlines, in the shape the
 * qmd reader produces for the same text:
 *
 *  - words → `Str`, runs of spaces/tabs → one `Space`;
 *  - a new line (`\n`, `\r\n` or `\r`) → `LineBreak` (qmd `\`+newline),
 *    or `Space` when `allowLineBreaks` is false (headings);
 *  - an empty line in the middle → two `LineBreak`s (round-trips as
 *    `a\`, `\`, `b`);
 *  - no `LineBreak` or `Space` at either end: a span ending in a
 *    `LineBreak` writes `\` + newline + `]`, which does not parse.
 *
 * Text never reaches a `Str` with a raw new line: the writer prints it
 * verbatim, and a blank line then ends the paragraph mid-comment.
 * Markdown-looking text stays literal (`Str "*x*"`); the writer escapes it.
 */
export function commentTextToInlines(
    text: string,
    opts: { allowLineBreaks: boolean },
): InlineNode[] {
    const lines = text
        .replace(/\r\n?/g, '\n')
        .split('\n')
        .map((line) => line.split(SPACE_RUN).filter((w) => w.length > 0));
    // Drop empty lines at both ends.
    let start = 0;
    let end = lines.length;
    while (start < end && lines[start].length === 0) start++;
    while (end > start && lines[end - 1].length === 0) end--;

    const out: InlineNode[] = [];
    for (let i = start; i < end; i++) {
        const words = lines[i];
        if (i > start) {
            if (opts.allowLineBreaks) {
                out.push({ t: 'LineBreak' });
            } else if (words.length > 0 && out[out.length - 1]?.t !== 'Space') {
                out.push({ t: 'Space' });
            }
        }
        words.forEach((w, j) => {
            if (j > 0) out.push({ t: 'Space' });
            out.push({ t: 'Str', c: w });
        });
    }
    return out;
}

/** A new comment span holding `inlines`. */
export function makeCommentSpan(inlines: InlineNode[]): SpanInline {
    return { t: 'Span', c: [['', [COMMENT_CLASS], []], inlines] };
}

/**
 * Return a copy of `source` (a block's source node) with `comment`
 * appended where comments on that block live, or null when the block
 * can't hold a comment:
 *
 *  - Para / Plain / Header: appended to the block's inlines;
 *  - CodeBlock: wrapped in a comment-container Div whose second child is
 *    a paragraph holding the comment;
 *  - comment container: appended to its last comment paragraph, or to a
 *    new paragraph at the end when none is left.
 *
 * The input is not mutated.
 */
export function appendCommentToBlock(source: BlockNode, comment: SpanInline): BlockNode | null {
    const modified = structuredClone(source);
    const slot = inlineSlot(modified);
    if (slot) {
        slot.push(comment);
        return modified;
    }
    if (modified.t === 'CodeBlock') {
        const wrapper: DivBlock = {
            t: 'Div',
            c: [
                ['', [COMMENT_CONTAINER_CLASS], []],
                [modified, { t: 'Para', c: [comment] } as ParaBlock],
            ],
        };
        return wrapper as BlockNode;
    }
    if (isCommentContainer(modified)) {
        const children = (modified as DivBlock).c[1];
        const lastCommentPara = [...children].reverse().find((ch) =>
            (inlineSlot(ch) ?? []).some(isComment),
        );
        if (lastCommentPara) {
            inlineSlot(lastCommentPara)!.push(comment);
        } else {
            children.push({ t: 'Para', c: [comment] } as ParaBlock);
        }
        return modified;
    }
    return null;
}
