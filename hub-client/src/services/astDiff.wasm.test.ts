/**
 * WASM end-to-end tests for the diff_asts_to_qmd export and its
 * `diffAstsToQmd` preview-runtime wrapper (AST diff annotation,
 * claude-notes/plans/2026-07-22-ast-diff-annotation.md).
 *
 * Parses two qmd states via `parse_qmd_content`, diffs them, and checks
 * that the annotated qmd contains `[++ ...]` / `[-- ...]` editorial marks
 * for inline changes and `::: {.added}` / `::: {.removed}` divs for block
 * changes. This is the same call sequence the Editor's Snapshot/Compare
 * debug buttons perform.
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, diffAsts } from '@quarto/preview-runtime';

const __dirname = dirname(fileURLToPath(import.meta.url));

interface AstResponse {
    success: boolean;
    error?: string;
    ast?: string;
    qmd?: string;
}

let wasm: any;

beforeAll(async () => {
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
    wasm = await import('wasm-quarto-hub-client');
    await wasm.default(wasmBytes);
    await initWasm();
});

function parseToAstJson(qmd: string): string {
    const response: AstResponse = JSON.parse(wasm.parse_qmd_content(qmd));
    expect(response.success, response.error).toBe(true);
    return response.ast!;
}

describe('diff_asts_to_qmd', () => {
    it('annotates inline and block changes in qmd output', () => {
        // The third block changes TYPE (paragraph -> code block), forcing a
        // block-level removed+added pair; the second block only changes one
        // word, producing inline marks. (Two same-position paragraphs that
        // share any inline are diffed at inline granularity instead.)
        const before = parseToAstJson('# Title\n\nThe cat sat on the mat.\n\nDoomed paragraph.\n');
        const after = parseToAstJson(
            '# Title\n\nThe dog sat on the mat.\n\n```js\nshiny();\n```\n',
        );

        const response: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(before, after));
        expect(response.success, response.error).toBe(true);
        const qmd = response.qmd!;

        expect(qmd).toContain('[-- cat]');
        expect(qmd).toContain('[++ dog]');
        expect(qmd).toContain('::: {.removed}');
        expect(qmd).toContain('Doomed paragraph.');
        expect(qmd).toContain('::: {.added}');
        expect(qmd).toContain('shiny();');
    });

    it('diffs same-position paragraphs at inline granularity', () => {
        const before = parseToAstJson('Doomed paragraph.\n');
        const after = parseToAstJson('Brand new paragraph.\n');

        const response: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(before, after));
        expect(response.success, response.error).toBe(true);
        const qmd = response.qmd!;

        expect(qmd).toContain('[-- Doomed]');
        expect(qmd).toContain('[++ Brand]');
        expect(qmd).toContain('paragraph.');
        expect(qmd).not.toContain('::: {.added}');
        expect(qmd).not.toContain('::: {.removed}');
    });

    it('produces no annotations for identical ASTs', () => {
        const ast = parseToAstJson('# Title\n\nSome *emphasised* text.\n');
        const response: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(ast, ast));
        expect(response.success, response.error).toBe(true);

        for (const marker of ['[++', '[--', '::: {.added}', '::: {.removed}']) {
            expect(response.qmd!).not.toContain(marker);
        }
        expect(response.qmd!).toContain('Some *emphasised* text.');
    });

    it('wraps a newly added list whole (bug report: 2026-07-22)', () => {
        const before = parseToAstJson('# hi\n');
        const after = parseToAstJson('# hi\n\n- my bullet point\n');

        const response: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(before, after));
        expect(response.success, response.error).toBe(true);
        const qmd = response.qmd!;

        // The entire BulletList is new: the .added div must wrap the list,
        // not appear inside a kept list's item.
        expect(qmd).not.toContain('* :::');
        expect(qmd).not.toContain('- :::');
        expect(qmd).toContain('::: {.added}');
        expect(qmd).toContain('my bullet point');
    });

    it('returns a renderable AST with marks desugared to Spans', () => {
        const before = parseToAstJson('The cat sat.\n');
        const after = parseToAstJson('The dog sat.\n');
        const diff: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(before, after));
        expect(diff.success, diff.error).toBe(true);

        // The modal renders diff.ast directly — no qmd round-trip.
        expect(diff.ast!).toContain('quarto-insert');
        expect(diff.ast!).toContain('quarto-delete');
    });

    it('preserves boundary spaces inside insert marks in the AST output', () => {
        // Appending " And more." produces an added run that STARTS with a
        // Space inline. The AST output must keep it inside the
        // quarto-insert Span (a qmd round-trip would collapse it).
        const before = parseToAstJson('Hello world.\n');
        const after = parseToAstJson('Hello world. And more.\n');
        const diff: AstResponse = JSON.parse(wasm.diff_asts_to_qmd(before, after));
        expect(diff.success, diff.error).toBe(true);

        const ast = JSON.parse(diff.ast!);
        const spans: any[] = [];
        const walk = (node: any) => {
            if (Array.isArray(node)) return node.forEach(walk);
            if (node && typeof node === 'object') {
                if (node.t === 'Span' && node.c?.[0]?.[1]?.includes('quarto-insert')) {
                    spans.push(node);
                }
                Object.values(node).forEach(walk);
            }
        };
        walk(ast.blocks);
        expect(spans.length).toBe(1);
        const spanContent = spans[0].c[1];
        expect(spanContent[0]?.t, 'insert content must start with the added Space').toBe('Space');
    });

    it('reports an error for malformed AST JSON', () => {
        const response: AstResponse = JSON.parse(wasm.diff_asts_to_qmd('not json', 'not json'));
        expect(response.success).toBe(false);
        expect(response.error).toContain('Failed to parse before JSON AST');
    });
});

describe('diffAsts wrapper', () => {
    it('returns the annotated qmd and AST', () => {
        const before = parseToAstJson('Alpha.\n');
        const after = parseToAstJson('Alpha.\n\nBeta.\n');

        const { qmd, astJson } = diffAsts(before, after);
        expect(qmd).toContain('::: {.added}');
        expect(qmd).toContain('Beta.');
        expect(JSON.parse(astJson).blocks.length).toBeGreaterThan(0);
    });

    it('throws on malformed input', () => {
        expect(() => diffAsts('nope', 'nope')).toThrow(/AST diff failed/);
    });
});
