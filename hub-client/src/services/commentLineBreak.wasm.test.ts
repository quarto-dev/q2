/**
 * bd-ov4jydyf — WASM end-to-end: a comment typed with Shift+Enter
 * round-trips through the real write-back path.
 *
 * Each case renders a q2-preview document, adds a comment to one block
 * the way the comment chrome does (`commentTextToInlines` +
 * `appendCommentToBlock` from preview-renderer, then
 * `stripSourceInfoFields` as `commitSubtreeEdit` does), and runs
 * `apply_node_edit` — the same incremental writer the hub uses. Then:
 *
 *  - the new qmd holds the expected `[>> …\`⏎`…]` text;
 *  - every byte outside the edited block is unchanged;
 *  - the new qmd parses, and the comment read back is EXACTLY the
 *    inlines the client sent (the payload is already in the reader's
 *    shape, so the round trip changes nothing).
 *
 * Plan: claude-notes/plans/2026-11-16-comment-shift-enter-linebreak.md
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsClear } from '@quarto/preview-runtime';
import {
    appendCommentToBlock,
    commentLineBreaksAllowed,
    commentTextToInlines,
    isComment,
    makeCommentSpan,
} from '@quarto/preview-renderer/utils/commentEdits';
import { stripSourceInfoFields } from '@quarto/preview-renderer/q2-preview/stripSourceInfoFields';

const __dirname = dirname(fileURLToPath(import.meta.url));

const PATH = '/project/doc.qmd';
const FRONT = '---\nformat: q2-preview\n---\n\nBefore.\n\n';
const BACK = '\n\nAfter.\n';

let wasm: any;

beforeAll(async () => {
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
    wasm = await import('wasm-quarto-hub-client');
    await wasm.default(wasmBytes);
    await initWasm();
});

beforeEach(() => {
    vfsClear();
    vfsAddFile('/project/_quarto.yml', 'project:\n  type: default\n');
});

type Node = any;

/** Every comment span's inlines under `root`, in document order. */
function commentsIn(root: unknown): Node[][] {
    const found: Node[][] = [];
    const walk = (x: any) => {
        if (Array.isArray(x)) x.forEach(walk);
        else if (x && typeof x === 'object') {
            if (x.t === 'Span' && isComment(x)) found.push(x.c[1]);
            else walk(x.c);
        }
    };
    walk(root);
    return found;
}

/** First span with no classes under `root` (a `[span]` the user commented on). */
function plainSpanIn(root: any): Node | null {
    if (Array.isArray(root)) {
        for (const x of root) {
            const hit = plainSpanIn(x);
            if (hit) return hit;
        }
        return null;
    }
    if (root && typeof root === 'object') {
        if (root.t === 'Span' && (root.c?.[0]?.[1] ?? []).length === 0) return root;
        return plainSpanIn(root.c);
    }
    return null;
}

interface Case {
    name: string;
    /** The block's qmd, placed between "Before." and "After." paragraphs. */
    qmd: string;
    /** Picks the block that `resolveSource` would hand the chrome (the source node). */
    target: (blocks: Node[]) => Node;
    /** What the user typed (Shift+Enter → "\n"). */
    typed: string;
    /** The edited block's expected qmd (front/back stay byte-identical). */
    expected: string;
    /** Comment on a `[span]` inside the block (CommentSpan path). */
    onSpan?: boolean;
    /** Line breaks allowed by the chrome; defaults to the target block's kind. */
    enclosing?: (blocks: Node[]) => Node;
}

const CASES: Case[] = [
    {
        name: 'paragraph',
        qmd: 'Para text.',
        target: (b) => b[1],
        typed: 'one\ntwo',
        expected: 'Para text.[>> one\\\ntwo]',
    },
    {
        name: 'paragraph that already has a comment',
        qmd: 'Text.[>> old]',
        target: (b) => b[1],
        typed: 'one\ntwo',
        expected: 'Text.[>> old][>> one\\\ntwo]',
    },
    {
        name: 'tight list item (Plain)',
        qmd: '* item\n* other',
        target: (b) => b[1].c[0][0],
        typed: 'one\ntwo',
        expected: '* item[>> one\\\n  two]\n* other',
    },
    {
        name: 'loose list item (Para)',
        qmd: '* a\n\n* b',
        target: (b) => b[1].c[0][0],
        typed: 'one\ntwo',
        expected: '* a[>> one\\\n  two]\n\n* b',
    },
    {
        name: 'block quote',
        qmd: '> quote',
        target: (b) => b[1].c[0],
        typed: 'one\ntwo',
        expected: '> quote[>> one\\\n> two]',
    },
    {
        name: 'block quote inside a list item',
        qmd: '* > q',
        target: (b) => b[1].c[0][0].c[0],
        typed: 'one\ntwo',
        expected: '* > q[>> one\\\n  > two]',
    },
    {
        name: 'code block (wrapped in a comment container)',
        qmd: '```\nx = 1\n```',
        target: (b) => b[1],
        typed: 'one\ntwo',
        expected:
            '::: {.quarto-edit-comment-container}\n\n```\nx = 1\n```\n\n[>> one\\\ntwo]\n\n:::',
    },
    {
        name: 'comment on a [span]',
        qmd: 'Some [text] here.',
        target: (b) => b[1],
        onSpan: true,
        typed: 'one\ntwo',
        expected: 'Some [text[>> one\\\ntwo]] here.',
    },
    {
        name: 'heading (one line: new lines become spaces)',
        qmd: '# Title',
        target: (b) => b[1],
        typed: 'one\ntwo',
        expected: '# Title[>> one two]',
    },
    {
        name: 'comment on a [span] in a heading',
        qmd: '# Some [text] here',
        target: (b) => b[1],
        onSpan: true,
        typed: 'one\ntwo',
        expected: '# Some [text[>> one two]] here',
    },
    {
        name: 'empty line in the middle of the comment',
        qmd: 'Para.',
        target: (b) => b[1],
        typed: 'one\n\ntwo',
        expected: 'Para.[>> one\\\n\\\ntwo]',
    },
    {
        name: 'pasted text with CRLF, blank edges and indentation',
        qmd: 'Para.',
        target: (b) => b[1],
        typed: '\r\n  one  \r\n   two\r\n\r\n',
        expected: 'Para.[>> one\\\ntwo]',
    },
];

// Second lines that would start a block if the writer didn't escape them.
const AWKWARD = ['# x', '- x', '+ x', '1. x', '> x', '::: x', '| x |', '```', '***', ']', '[>> y', '\\'];

function run(c: Case) {
    const content = FRONT + c.qmd + BACK;
    vfsAddFile(PATH, content);
    return wasm.render_page_in_project_with_attribution(PATH, undefined, null).then((raw: string) => {
        const result = JSON.parse(raw);
        expect(result.success, result.error).toBe(true);
        const a_u = JSON.parse(result.untransformed_ast_json);
        const pool: any[] = a_u.astContext.p;
        const block = c.target(a_u.blocks);
        expect(block, 'target block not found').toBeTruthy();
        const destSiJson = JSON.stringify(pool[block.s]);

        // What the chrome does (CommentBlock.addComment / CommentSpan.addComment).
        const enclosing = c.enclosing ? c.enclosing(a_u.blocks) : block;
        const inlines = commentTextToInlines(c.typed, {
            allowLineBreaks: commentLineBreaksAllowed(enclosing),
        });
        let modified: Node;
        if (c.onSpan) {
            modified = structuredClone(block);
            const span = plainSpanIn(modified);
            expect(span, 'no [span] in the block').toBeTruthy();
            span.c[1].push(makeCommentSpan(inlines));
        } else {
            modified = appendCommentToBlock(block, makeCommentSpan(inlines));
            expect(modified).not.toBeNull();
        }
        // What commitSubtreeEdit sends.
        const subtree = JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [stripSourceInfoFields(modified)],
        });

        const edit = JSON.parse(wasm.apply_node_edit(content, result.untransformed_ast_json, destSiJson, subtree));
        expect(edit.success, `apply_node_edit failed: ${edit.error}`).toBe(true);
        const qmd: string = edit.qmd;

        // The new qmd parses, and the comment comes back exactly as sent.
        const reparsed = wasm.parse_qmd_content(qmd);
        expect(reparsed.success, `re-parse failed: ${reparsed.error}\n--- qmd ---\n${qmd}`).toBe(true);
        const back = commentsIn(stripSourceInfoFields(JSON.parse(reparsed.ast).blocks));
        expect(back[back.length - 1], `--- qmd ---\n${qmd}`).toEqual(inlines);

        return { qmd, content, inlines };
    });
}

describe('comment line breaks round-trip through apply_node_edit (bd-ov4jydyf)', () => {
    for (const c of CASES) {
        it(c.name, async () => {
            const { qmd } = await run(c);
            // Only the edited block changed; the rest is byte-identical.
            expect(qmd).toBe(FRONT + c.expected + BACK);
        });
    }

    it('heading comments never contain a LineBreak', async () => {
        const { inlines } = await run(CASES.find((c) => c.name.startsWith('heading'))!);
        expect(inlines.some((n: Node) => n.t === 'LineBreak')).toBe(false);
    });

    for (const second of AWKWARD) {
        it(`a second line starting with ${JSON.stringify(second)} stays inside the comment`, async () => {
            const { qmd } = await run({
                name: second,
                qmd: 'Para.',
                target: (b) => b[1],
                typed: `first\n${second}`,
                expected: '',
            });
            // Still exactly three paragraphs: Before, the commented one, After.
            const blocks = JSON.parse(wasm.parse_qmd_content(qmd).ast).blocks;
            expect(blocks.map((b: Node) => b.t), qmd).toEqual(['Para', 'Para', 'Para']);
        });
    }
});
