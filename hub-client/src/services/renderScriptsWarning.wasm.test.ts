/**
 * WASM safety net for the host-dependent Q-5-12 render-scripts
 * warning (bd-pq72bplh).
 *
 * A project that configures `project.pre-render` / `project.post-render`
 * scripts must warn in the hub preview (the browser cannot run the
 * scripts) but NOT in `q2 preview` (its native server runs pre-render
 * scripts once at boot — see D7 in
 * `claude-notes/plans/2026-07-29-pre-post-render-scripts.md`). The two
 * hosts enter the WASM through different entry points:
 * `render_page_in_project` (hub-client) vs `render_page_for_preview`
 * (q2-preview SPA).
 *
 * Call order inside the single test matters: the warning is
 * once-per-session (AtomicBool in the WASM module, one instance per
 * test file). Rendering through the preview entry point FIRST also
 * proves the suppressed path does not consume the once-gate.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsClear } from '@quarto/preview-runtime';

interface JsonDiagnosticLike {
    kind: string;
    code?: string;
}

interface RenderResponse {
    success: boolean;
    error?: string;
    warnings?: JsonDiagnosticLike[];
}

let renderInProject: (path: string) => Promise<string>;
let renderForPreview: (path: string) => Promise<string>;

beforeAll(async () => {
    // Pre-load the WASM module with explicit bytes — node has no
    // `fetch`, so an argless `wasm.default()` would fail.
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
        default: (input?: BufferSource) => Promise<unknown>;
        render_page_in_project: (path: string) => Promise<string>;
        render_page_for_preview: (path: string) => Promise<string>;
    };
    await wasm.default(wasmBytes);

    // Initialize the wasmRenderer singleton so the VFS helpers
    // (`vfsAddFile`, `vfsClear`) talk to the already-loaded module.
    // wasm-bindgen's `__wbg_init` is idempotent.
    await initWasm();

    renderInProject = wasm.render_page_in_project.bind(wasm);
    renderForPreview = wasm.render_page_for_preview.bind(wasm);
});

beforeEach(() => {
    vfsClear();
});

function q512Warnings(response: RenderResponse): JsonDiagnosticLike[] {
    return (response.warnings ?? []).filter((w) => w.code === 'Q-5-12');
}

describe('host-dependent Q-5-12 render-scripts warning (bd-pq72bplh)', () => {
    it('warns in the hub entry point but not the q2-preview entry point', async () => {
        vfsAddFile(
            '_quarto.yml',
            'project:\n  type: default\n  pre-render: pre.sh\n',
        );
        vfsAddFile('index.qmd', '---\ntitle: Repro\n---\n\nHello.\n');

        // 1. q2-preview host: no warning — the native server already
        //    ran the pre-render scripts at boot.
        const preview = JSON.parse(
            await renderForPreview('/project/index.qmd'),
        ) as RenderResponse;
        expect(preview.success, `preview render failed: ${preview.error}`).toBe(
            true,
        );
        expect(q512Warnings(preview)).toEqual([]);

        // 2. Hub host: the warning fires — and the suppressed preview
        //    render above must not have consumed the once-per-session
        //    gate.
        const hub = JSON.parse(
            await renderInProject('/project/index.qmd'),
        ) as RenderResponse;
        expect(hub.success, `hub render failed: ${hub.error}`).toBe(true);
        const warnings = q512Warnings(hub);
        expect(warnings).toHaveLength(1);
        expect(warnings[0].kind).toBe('warning');

        // 3. Hub host again: once-per-session — no repeat.
        const hubAgain = JSON.parse(
            await renderInProject('/project/index.qmd'),
        ) as RenderResponse;
        expect(hubAgain.success).toBe(true);
        expect(q512Warnings(hubAgain)).toEqual([]);
    });
});
