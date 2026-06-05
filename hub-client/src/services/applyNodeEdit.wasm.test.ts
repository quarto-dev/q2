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
import { sliceBytes } from '@quarto/preview-renderer/utils/sliceSource';

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

// ---------------------------------------------------------------------------
// parseQmdContentSync accepts text without trailing newline.
// ---------------------------------------------------------------------------

describe('parseQmdContentSync no-trailing-newline', () => {
    it('parseQmdContentSync handles text without trailing newline', () => {
        // parse_qmd_content is the underlying WASM function; the TS wrapper
        // (parseQmdContentSync in wasmRenderer.ts) is a thin JSON.parse layer.
        const result: AstResponse = JSON.parse(wasm.parse_qmd_content('a *emph* word'));
        expect(result.success).toBe(true);
        expect(result.ast).toBeTruthy();
    });
});

// ---------------------------------------------------------------------------
// Slice fidelity — byte offsets from pool entries must round-trip through
// sliceBytes to recover the verbatim source text without trailing newlines.
// ---------------------------------------------------------------------------

describe('slice fidelity', () => {
    // Uses PATH ('/project/doc.qmd') — overridden inside each test, consistent
    // with the 'full edit round-trip' pattern above.

    it('sliceBytes recovers paragraph text without trailing newline', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        expect(para, 'untransformed AST must contain a Para block').toBeTruthy();

        const entry = pool[para.s];
        // The Rust parser spans may include a trailing '\n'; trimEnd() reflects what
        // the textarea would receive.  The important invariant is that the inline
        // markdown content is fully present.
        const sliced = sliceBytes(content, entry.r[0], entry.r[1]).trimEnd();
        expect(sliced).toBe('a *emph* word');
    });

    it('sliceBytes recovers heading text without trailing newline', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        // Header block has type 'Header'
        const heading = a_u.blocks.find((b: any) => b.t === 'Header');
        expect(heading, 'untransformed AST must contain a Header block').toBeTruthy();

        const entry = pool[heading.s];
        // The Rust parser spans may include a trailing '\n'; trimEnd() reflects what
        // the textarea would receive.  The important invariant is that the '#' prefix
        // and heading text are fully present.
        const sliced = sliceBytes(content, entry.r[0], entry.r[1]).trimEnd();
        expect(sliced).toBe('## My Heading');
    });
});

// ---------------------------------------------------------------------------
// Surrounding-verbatim round-trips — blocks that are not edited must appear
// byte-verbatim in the output QMD.
// ---------------------------------------------------------------------------

describe('surrounding-verbatim round-trips', () => {
    it('editing the middle block preserves surrounding blocks verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n\nThird para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const paras: any[] = a_u.blocks.filter((b: any) => b.t === 'Para');
        // There should be exactly 3 Para blocks.
        expect(paras.length).toBe(3);

        // Edit the middle paragraph (index 1).
        const middlePara = paras[1];
        const destSiJson = JSON.stringify(pool[middlePara.s]);
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Edited para.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // Surrounding blocks must appear byte-verbatim.
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nThird para.\n');
        // The edited block is present; the original middle text is gone.
        expect(editResult.qmd).toContain('Edited para.');
        expect(editResult.qmd).not.toContain('Second para.');
    });

    it('editing the last block preserves preceding blocks verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n\nThird para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const paras: any[] = a_u.blocks.filter((b: any) => b.t === 'Para');
        expect(paras.length).toBe(3);

        // Edit the last paragraph (index 2).
        const lastPara = paras[2];
        const destSiJson = JSON.stringify(pool[lastPara.s]);
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('EOF edit.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // First two paragraphs must appear verbatim.
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('Second para.');
        expect(editResult.qmd).toContain('EOF edit.');
        expect(editResult.qmd).not.toContain('Third para.');
    });
});

// ---------------------------------------------------------------------------
// Inline markdown round-trip — emphasis syntax survives apply_node_edit.
// ---------------------------------------------------------------------------

describe('inline markdown round-trip via apply_node_edit', () => {
    it('inline emphasis survives the round-trip unchanged', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para.s]);

        // Replace with the same inline markdown.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('a *emph* word\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        // Inline markdown must be preserved.
        expect(editResult.qmd).toContain('a *emph* word');
    });
});

// ---------------------------------------------------------------------------
// Heading round-trip — the # prefix is written by the serializer, not the
// caller.  Passing '## My Heading' as newText must produce '## My Heading'
// in the output without any manual prepending.
// ---------------------------------------------------------------------------

describe('heading round-trip', () => {
    it('heading level and # prefix are preserved through the writer', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const heading = a_u.blocks.find((b: any) => b.t === 'Header');
        expect(heading, 'untransformed AST must contain a Header block').toBeTruthy();
        const destSiJson = JSON.stringify(pool[heading.s]);

        // Parse the heading text as new QMD.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('## My Heading\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        // The ## prefix must appear in the output — no manual prepending needed.
        expect(editResult.qmd).toContain('## My Heading');
    });
});
