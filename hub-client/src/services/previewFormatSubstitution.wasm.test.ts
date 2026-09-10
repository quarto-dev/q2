/**
 * WASM contract for hub-client's `prefer_preview_format` knob
 * (bd-kltzdhle, D3 in
 * `claude-notes/plans/2026-09-09-hub-client-default-q2-preview.md`).
 *
 * hub-client renders every document through
 * `render_page_in_project_with_attribution`. With
 * `prefer_preview_format = true` that entry point applies the same
 * default-format substitution `q2 preview` uses (`html → q2-preview`,
 * `revealjs → q2-slides`) so a plain document renders through the
 * q2-preview pipeline and returns `ast_json`. Unlike
 * `render_page_for_preview`, it keeps `RenderHost::HubClient`, so the
 * host-dependent Q-5-12 render-scripts warning still fires — that is
 * the reason hub-client gets a knob on its own entry point instead of
 * switching to the SPA's.
 *
 * Call order matters for the Q-5-12 case: the warning is once per WASM
 * module instance (one per test file), so it runs first.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import {
    initWasm,
    renderPageInProjectWithAttribution,
    vfsAddFile,
    vfsClear,
} from '@quarto/preview-runtime';

interface JsonDiagnosticLike {
    kind: string;
    code?: string;
}

interface RenderResponseLike {
    success: boolean;
    error?: string;
    html?: string;
    ast_json?: string;
    is_slides?: boolean;
    warnings?: JsonDiagnosticLike[];
}

beforeAll(async () => {
    // Pre-load the WASM module with explicit bytes — node has no
    // `fetch`, so an argless `wasm.default()` would fail.
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
        default: (input?: BufferSource) => Promise<unknown>;
    };
    await wasm.default(wasmBytes);
    // Initialize the wasmRenderer singleton so the VFS helpers and the
    // typed wrapper talk to the already-loaded module (`__wbg_init` is
    // idempotent).
    await initWasm();
});

beforeEach(() => {
    vfsClear();
});

const PLAIN_DOC = '---\ntitle: Plain\n---\n\nHello *world*.\n';

async function render(
    path: string,
    preferPreviewFormat?: boolean,
): Promise<RenderResponseLike> {
    return (await renderPageInProjectWithAttribution(
        path,
        undefined,
        null,
        undefined,
        preferPreviewFormat,
    )) as RenderResponseLike;
}

function q512Warnings(response: RenderResponseLike): JsonDiagnosticLike[] {
    return (response.warnings ?? []).filter((w) => w.code === 'Q-5-12');
}

describe('prefer_preview_format on the hub-client entry point (bd-kltzdhle)', () => {
    it('(f) keeps RenderHost::HubClient: Q-5-12 still fires with the knob on', async () => {
        vfsAddFile(
            '_quarto.yml',
            'project:\n  type: default\n  pre-render: pre.sh\n',
        );
        vfsAddFile('index.qmd', PLAIN_DOC);

        const resp = await render('/project/index.qmd', true);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        // The substitution took effect (project path) …
        expect(resp.ast_json).toBeDefined();
        expect(resp.html).toBeUndefined();
        // … and the hub host still warns about the scripts it cannot run.
        const warnings = q512Warnings(resp);
        expect(warnings).toHaveLength(1);
        expect(warnings[0].kind).toBe('warning');
    });

    it('(a) no format: key + knob on → q2-preview pipeline (ast_json)', async () => {
        vfsAddFile('doc.qmd', PLAIN_DOC);
        const resp = await render('/project/doc.qmd', true);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.ast_json).toBeDefined();
        expect(resp.html).toBeUndefined();
        // The response names the format the render ran with …
        expect(resp.format).toBe('q2-preview');
        // … and the AST carries the substituted format too, which is what
        // the router echoes to the Editor chrome.
        const ast = JSON.parse(resp.ast_json!);
        expect(ast.meta.format).toMatchObject({ t: 'MetaString', c: 'q2-preview' });
    });

    it('(b) no format: key + knob omitted → HTML pipeline (today\'s contract)', async () => {
        vfsAddFile('doc.qmd', PLAIN_DOC);
        const resp = await render('/project/doc.qmd');
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.html).toBeDefined();
        expect(resp.html).toContain('<p>Hello <em>world</em>.</p>');
        expect(resp.ast_json).toBeUndefined();
    });

    it('(b\') explicit knob off is byte-for-byte the omitted case', async () => {
        vfsAddFile('doc.qmd', PLAIN_DOC);
        const omitted = await render('/project/doc.qmd');
        const off = await render('/project/doc.qmd', false);
        expect(off).toEqual(omitted);
    });

    it('(c) format: q2-html-render + knob on → HTML pipeline', async () => {
        vfsAddFile(
            'doc.qmd',
            '---\ntitle: Opt-out\nformat: q2-html-render\n---\n\nHello *world*.\n',
        );
        const resp = await render('/project/doc.qmd', true);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.html).toBeDefined();
        expect(resp.html).toContain('<p>Hello <em>world</em>.</p>');
        expect(resp.ast_json).toBeUndefined();
        // The response says which format produced the `html`, so a host
        // that expected `ast_json` can explain why (D5).
        expect(resp.format).toBe('q2-html-render');
    });

    it('(d) format: q2-preview → ast_json regardless of the knob', async () => {
        vfsAddFile(
            'doc.qmd',
            '---\ntitle: Explicit\nformat: q2-preview\n---\n\nHello *world*.\n',
        );
        const on = await render('/project/doc.qmd', true);
        const off = await render('/project/doc.qmd', false);
        for (const resp of [on, off]) {
            expect(resp.success, `render failed: ${resp.error}`).toBe(true);
            expect(resp.ast_json).toBeDefined();
            expect(resp.html).toBeUndefined();
        }
    });

    it('(e) format: revealjs + knob on → q2-slides AST with is_slides', async () => {
        vfsAddFile(
            'deck.qmd',
            '---\ntitle: Deck\nformat: revealjs\n---\n\n## One\n\nSlide one.\n\n## Two\n\nSlide two.\n',
        );
        const resp = await render('/project/deck.qmd', true);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.ast_json).toBeDefined();
        expect(resp.html).toBeUndefined();
        expect(resp.is_slides).toBe(true);
        expect(resp.format).toBe('q2-slides');
    });
});
