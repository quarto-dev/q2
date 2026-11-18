/**
 * WASM safety net for the Quarto-version declaration warnings
 * (bd-gw8fmaz3).
 *
 * A project whose `_quarto.yml` lacks `quarto-required: "^2"` draws one
 * Q-5-53 per render on every host: the hub (`render_page_in_project`)
 * and the q2-preview SPA (`render_page_for_preview`) both go through the
 * orchestrator's `finish_project_diagnostics`. Unlike Q-5-12 there is no
 * once-per-session gate: the warning describes the project's
 * configuration, so it rides every render response and stays visible in
 * the diagnostics panel. Plan:
 * `claude-notes/plans/2026-11-17-project-quarto-version-marker.md`.
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
    await initWasm();

    renderInProject = wasm.render_page_in_project.bind(wasm);
    renderForPreview = wasm.render_page_for_preview.bind(wasm);
});

beforeEach(() => {
    vfsClear();
});

function codes(response: RenderResponse, prefix: string): string[] {
    return (response.warnings ?? [])
        .map((w) => w.code ?? '')
        .filter((c) => c.startsWith(prefix));
}

async function render(
    entry: (path: string) => Promise<string>,
): Promise<RenderResponse> {
    const response = JSON.parse(
        await entry('/project/index.qmd'),
    ) as RenderResponse;
    expect(response.success, `render failed: ${response.error}`).toBe(true);
    return response;
}

describe('Quarto-version declaration warnings (bd-gw8fmaz3)', () => {
    it('warns Q-5-53 on every hub and preview render of an unmarked project', async () => {
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile('index.qmd', '---\ntitle: Repro\n---\n\nHello.\n');

        const hub = await render(renderInProject);
        expect(codes(hub, 'Q-5-5')).toEqual(['Q-5-53']);
        expect(
            (hub.warnings ?? []).find((w) => w.code === 'Q-5-53')?.kind,
        ).toBe('warning');

        // No once-per-session gate: the next render still reports it.
        const hubAgain = await render(renderInProject);
        expect(codes(hubAgain, 'Q-5-5')).toEqual(['Q-5-53']);

        const preview = await render(renderForPreview);
        expect(codes(preview, 'Q-5-5')).toEqual(['Q-5-53']);
    });

    it('is silent for a project declaring quarto-required: "^2"', async () => {
        vfsAddFile(
            '_quarto.yml',
            'project:\n  type: default\nquarto-required: "^2"\n',
        );
        vfsAddFile('index.qmd', '---\ntitle: Repro\n---\n\nHello.\n');

        expect(codes(await render(renderInProject), 'Q-5-5')).toEqual([]);
        expect(codes(await render(renderForPreview), 'Q-5-5')).toEqual([]);
    });

    it('warns Q-5-54 for an open range', async () => {
        vfsAddFile(
            '_quarto.yml',
            'project:\n  type: default\nquarto-required: ">=1.4"\n',
        );
        vfsAddFile('index.qmd', '---\ntitle: Repro\n---\n\nHello.\n');

        expect(codes(await render(renderInProject), 'Q-5-5')).toEqual([
            'Q-5-54',
        ]);
    });

    it('is silenced by a diagnostics: entry in _quarto.yml', async () => {
        vfsAddFile(
            '_quarto.yml',
            'project:\n  type: default\ndiagnostics:\n  Q-5-53: off\n',
        );
        vfsAddFile('index.qmd', '---\ntitle: Repro\n---\n\nHello.\n');

        expect(codes(await render(renderInProject), 'Q-5-5')).toEqual([]);
    });
});
