/**
 * WASM smoke test for `RenderResponse.theme_fingerprint` (Plan 2A item 11).
 *
 * Drives the q2-preview WASM render path end-to-end and verifies the
 * `theme_fingerprint` field is populated on the JSON response. This
 * isolates "is the Rust side correct" from "is the dev server's WASM
 * bundle stale" — if this test passes but the user sees no theme
 * `<link>` in the iframe, the dev server is serving an old WASM.
 *
 * Covers:
 *  - Single-doc q2-preview render produces a non-empty fingerprint
 *    even with no `theme:` YAML key (compile-default Bootstrap+Quarto
 *    CSS still gets fingerprinted).
 *  - The CSS artifact lands at `/.quarto/project-artifacts/styles.css`
 *    in single-doc mode, matching `DEFAULT_CSS_ARTIFACT_PATH`.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';

interface WasmModule {
    default: (input?: BufferSource) => Promise<void>;
    vfs_add_file: (path: string, content: string) => string;
    vfs_clear: () => string;
    vfs_read_file: (path: string) => string;
    render_page_in_project: (path: string) => Promise<string>;
}

interface RenderResponse {
    success: boolean;
    error?: string;
    ast_json?: string;
    theme_fingerprint?: string;
}

let wasm: WasmModule;

beforeAll(async () => {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);

    wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(wasmBytes);

    setVfsCallbacks(
        (path: string): string | null => {
            try {
                const result = JSON.parse(wasm.vfs_read_file(path)) as {
                    success: boolean;
                    content?: string;
                };
                return result.success && result.content !== undefined
                    ? result.content
                    : null;
            } catch {
                return null;
            }
        },
        (path: string): boolean => {
            try {
                const result = JSON.parse(wasm.vfs_read_file(path)) as {
                    success: boolean;
                    content?: string;
                };
                return result.success && result.content !== undefined;
            } catch {
                return false;
            }
        },
    );
});

beforeEach(() => {
    wasm.vfs_clear();
});

describe('RenderResponse.theme_fingerprint (Plan 2A item 11)', () => {
    it('q2-preview single-doc render emits a non-empty fingerprint', async () => {
        wasm.vfs_add_file(
            '/project/doc.qmd',
            '---\nformat: q2-preview\n---\n\n# Hello\n',
        );

        const json = await wasm.render_page_in_project('/project/doc.qmd');
        const result = JSON.parse(json) as RenderResponse;

        expect(result.success, `Render failed: ${result.error}`).toBe(true);
        expect(result.ast_json).toBeTruthy();
        expect(result.theme_fingerprint).toBeTruthy();
        expect(typeof result.theme_fingerprint).toBe('string');
        expect(result.theme_fingerprint!.length).toBeGreaterThan(0);
    });

    it('CSS artifact is readable at DEFAULT_CSS_ARTIFACT_PATH for single-doc q2-preview', async () => {
        wasm.vfs_add_file(
            '/project/doc.qmd',
            '---\nformat: q2-preview\n---\n\n# Hello\n',
        );

        await wasm.render_page_in_project('/project/doc.qmd');

        // The Q2PreviewIframe wrapper reads from this exact path.
        const cssRead = JSON.parse(
            wasm.vfs_read_file('/.quarto/project-artifacts/styles.css'),
        ) as { success: boolean; content?: string };

        expect(
            cssRead.success,
            'theme CSS should be at DEFAULT_CSS_ARTIFACT_PATH after q2-preview render',
        ).toBe(true);
        expect(cssRead.content).toBeTruthy();
        expect(cssRead.content!.length).toBeGreaterThan(0);
    });

    it('q2-preview default-project (with _quarto.yml) emits a fingerprint', async () => {
        // Regression: previously the orchestrator drained Project-scoped
        // artifacts via `flush_site_libs` for non-website projects, never
        // merging them into `project_artifacts`. The summary's
        // `theme_fingerprint` lookup missed and the WASM response lacked
        // the field. The fix captures the fingerprint at the renderer
        // level (before the drain) and stashes it on `WasmPassTwoOutput`.
        wasm.vfs_add_file('/project/_quarto.yml', 'project:\n  type: default\n');
        wasm.vfs_add_file(
            '/project/index.qmd',
            '---\nformat: q2-preview\n---\n\n# Hello\n',
        );

        const json = await wasm.render_page_in_project('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;

        expect(result.success, `Render failed: ${result.error}`).toBe(true);
        expect(
            result.theme_fingerprint,
            'default-project q2-preview must surface theme_fingerprint (regression for the project-mode bug surfaced 2026-05-09)',
        ).toBeTruthy();
    });

    it('CSS artifact is readable at DEFAULT_CSS_ARTIFACT_PATH for project-mode q2-preview', async () => {
        // Regression: in project mode, `compile_theme_css` writes the
        // artifact at the fingerprinted path
        // `quarto/quarto-theme-<fp>.css`, not at `styles.css`. The
        // iframe wrapper reads from `/.quarto/project-artifacts/styles.css`
        // unconditionally — so without this dual-write fix, project-mode
        // q2-preview never gets a theme `<link>` even when the
        // fingerprint plumbing is intact.
        //
        // Fix: q2-preview's pass-2 renderer also writes the theme bytes
        // to the stable `styles.css` location regardless of project mode,
        // honoring Plan 1's contract that "RenderToPreviewAstRenderer
        // writes the compiled theme CSS to /.quarto/project-artifacts/styles.css
        // on every q2-preview render."
        wasm.vfs_add_file('/project/_quarto.yml', 'project:\n  type: default\n');
        wasm.vfs_add_file(
            '/project/index.qmd',
            '---\nformat: q2-preview\n---\n\n# Hello\n',
        );

        await wasm.render_page_in_project('/project/index.qmd');

        const cssRead = JSON.parse(
            wasm.vfs_read_file('/.quarto/project-artifacts/styles.css'),
        ) as { success: boolean; content?: string };
        expect(
            cssRead.success,
            'project-mode q2-preview must place theme CSS at DEFAULT_CSS_ARTIFACT_PATH for the iframe',
        ).toBe(true);
        expect(cssRead.content).toBeTruthy();
        expect(cssRead.content!.length).toBeGreaterThan(0);
    });
});
