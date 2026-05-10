/**
 * WASM safety net for q2-preview's CustomNode wire format in project
 * mode (Plan 2C Phase 5.3).
 *
 * Renders a `_quarto.yml`-rooted project doc containing a callout and
 * asserts the response's `ast_json` contains a `Div` with
 * `__quarto_custom_node` in its classes and a `data-custom-type=Callout`
 * kv. This catches drift between Gordon's deny-list refactor
 * (`Q2_PREVIEW_TRANSFORM_EXCLUDED` at `pipeline.rs:1049`) and what
 * `unwrapCustomNodes` (in `framework/customNode.ts`) will see — if
 * `callout-resolve` ever falls out of the exclusion list, the callout
 * becomes plain HTML and unwrap finds nothing.
 *
 * Pattern: same `initWasm` + project-mode setup as
 * `assetManifestProject.wasm.test.ts`.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsClear } from './wasmRenderer';

interface RenderResponse {
    success: boolean;
    error?: string;
    ast_json?: string;
}

let renderInProject: (path: string) => Promise<string>;

beforeAll(async () => {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
        default: (input?: BufferSource) => Promise<unknown>;
        render_page_in_project: (path: string) => Promise<string>;
    };
    await wasm.default(wasmBytes);
    await initWasm();
    renderInProject = wasm.render_page_in_project.bind(wasm);
});

beforeEach(() => {
    vfsClear();
});

describe('q2-preview CustomNode wire format in project mode (Plan 2C)', () => {
    it('callout survives as a CustomNode wrapper in the project pass-2 ast_json', async () => {
        // The callout-resolve transform is excluded from q2-preview's
        // pipeline (see Q2_PREVIEW_TRANSFORM_EXCLUDED at pipeline.rs:
        // 1050). Without that exclusion, the callout AST is rewritten
        // to plain Bootstrap HTML and the CustomNode wrapper is lost
        // before the iframe sees it.
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'index.qmd',
            '---\nformat: q2-preview\n---\n\n' +
                '::: {.callout-note}\n' +
                'Body text.\n' +
                ':::\n',
        );

        const json = await renderInProject('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;

        expect(result.success, `Render failed: ${result.error}`).toBe(true);
        expect(result.ast_json).toBeTruthy();

        // The wire format wraps the CustomNode in a Pandoc Div with
        // `__quarto_custom_node` in the class list and a kv pair
        // carrying the type_name. See pampa/src/writers/json.rs:
        // 1297 (write_custom_block).
        expect(result.ast_json!).toContain('__quarto_custom_node');
        // The type_name is emitted as a kv pair `data-custom-type` →
        // `Callout`. JSON-stringified kvs are nested arrays; just
        // grep for the substring.
        expect(result.ast_json!).toContain('data-custom-type');
        expect(result.ast_json!).toContain('Callout');
    });

    it('theorem survives as a CustomNode wrapper too', async () => {
        // Sanity check that the wire-format catch isn't callout-only —
        // the same exclusion-list rule keeps theorem wrappers intact
        // (CrossrefRenderTransform at pipeline.rs:1071 is excluded so
        // render_theorem doesn't run).
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'index.qmd',
            '---\nformat: q2-preview\n---\n\n' +
                '::: {#thm-1 .theorem}\n' +
                'Body text.\n' +
                ':::\n',
        );

        const json = await renderInProject('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;
        expect(result.success).toBe(true);
        expect(result.ast_json!).toContain('__quarto_custom_node');
        expect(result.ast_json!).toContain('Theorem');
    });
});
