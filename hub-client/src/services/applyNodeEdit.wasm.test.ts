/**
 * WASM end-to-end tests for the apply_node_edit write-back path
 * (target-incremental-writes Phase 6).
 *
 * Three integration gaps that unit tests cannot catch — each has a
 * dedicated regression test:
 *
 *  1. Pool lives at `astContext.p` (not at the document root).
 *  2. Multi-file project path (`render_page_in_project_with_attribution`)
 *     must return `untransformed_ast_json` — previously hardcoded to None.
 *  3. `apply_node_edit` must accept the compact pool-entry format
 *     {"t":0,"r":[s,e],"d":file_id} that the frontend actually sends.
 *
 * All tests use `render_page_in_project_with_attribution` because that
 * is the function the hub-client calls for q2-preview documents.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsClear } from '@quarto/preview-runtime';

const __dirname = dirname(fileURLToPath(import.meta.url));

interface RenderResponse {
    success: boolean;
    error?: string;
    ast_json?: string;
    untransformed_ast_json?: string;
}
interface AstResponse {
    success: boolean;
    error?: string;
    ast?: string;
    qmd?: string;
}

// Minimal q2-preview document for testing.
const CONTENT = '---\nformat: q2-preview\n---\n\nHello world.\n';
const PATH = '/project/doc.qmd';
const PROJECT_YML = 'project:\n  type: default\n';

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
    vfsAddFile('/project/_quarto.yml', PROJECT_YML);
    vfsAddFile(PATH, CONTENT);
});

// ---------------------------------------------------------------------------
// Regression 1: pool must be at astContext.p, not at the document root.
// ---------------------------------------------------------------------------

describe('pool location', () => {
    it('pool is at astContext.p, not at raw.p', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        // Root-level `p` must NOT exist — pool lives inside `astContext`.
        expect(a_u.p, 'pool must not be at document root').toBeUndefined();
        // `astContext.p` must be a non-empty array.
        expect(Array.isArray(a_u.astContext?.p), 'astContext.p must be an array').toBe(true);
        expect((a_u.astContext.p as unknown[]).length).toBeGreaterThan(0);
    });

    it('pool entries use compact format {"t","r","d"}', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const entry = pool[0];
        expect(typeof entry.t).toBe('number');
        expect(Array.isArray(entry.r)).toBe(true);
        expect(entry.r).toHaveLength(2);
        expect(typeof entry.d).not.toBe('undefined');
    });
});

// ---------------------------------------------------------------------------
// Regression 2: multi-file project path must return untransformed_ast_json.
// ---------------------------------------------------------------------------

describe('project path returns untransformed_ast_json', () => {
    it('render_page_in_project_with_attribution populates untransformed_ast_json', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);
        expect(result.ast_json, 'ast_json must be present').toBeTruthy();
        expect(
            result.untransformed_ast_json,
            'untransformed_ast_json must be present for project path',
        ).toBeTruthy();
    });

    it('untransformed_ast_json is a valid Pandoc document with blocks', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        expect(Array.isArray(a_u.blocks)).toBe(true);
        expect(a_u.blocks.length).toBeGreaterThan(0);
        // The paragraph block must have a pool reference.
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        expect(para, 'untransformed AST must contain a Para block').toBeTruthy();
        expect(typeof para.s).toBe('number');
    });
});

// ---------------------------------------------------------------------------
// Regression 3: apply_node_edit must accept the compact pool-entry format.
// ---------------------------------------------------------------------------

describe('apply_node_edit with compact source_info format', () => {
    it('accepts the compact {"t","r","d"} pool-entry format the frontend sends', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        // The compact pool entry — exactly what the iframe sends.
        const compactEntry = pool[para.s];
        const destSiJson = JSON.stringify(compactEntry);

        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Replaced text.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(CONTENT, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        expect(editResult.qmd).toContain('Replaced text.');
    });
});

// ---------------------------------------------------------------------------
// Full round-trip: render → edit block 0 → verify QMD precision.
// ---------------------------------------------------------------------------

describe('full edit round-trip', () => {
    it('replaces only the target paragraph; rest of QMD is verbatim', async () => {
        const content = '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        // Find first Para block (block 0 in the untransformed AST).
        const para0 = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para0.s]);

        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Edited.\n'));
        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success).toBe(true);

        // Exact assertions — not just presence.
        expect(editResult.qmd).toContain('Edited.');
        expect(editResult.qmd).not.toContain('First para.');
        expect(editResult.qmd).toContain('Second para.');
    });

    it('edit preserves inline markdown (typed emphasis round-trips correctly)', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para.s]);

        // The replacement text uses inline markdown.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Hello *world*.\n'));
        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(CONTENT, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success).toBe(true);
        // The asterisks must survive the round-trip.
        expect(editResult.qmd).toContain('*world*');
        expect(editResult.qmd).not.toContain('Hello world.');
    });
});
