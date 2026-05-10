/**
 * WASM safety net for q2-preview's asset manifest in project mode
 * (Plan 2B Phase 5.3).
 *
 * Mirrors `themeFingerprint.wasm.test.ts` at the WASM-bridge layer.
 * Renders a `_quarto.yml`-rooted project doc with `![](hero.png)`,
 * adds real PNG bytes via `vfs_add_binary_file`, parses the response's
 * `ast_json`, and exercises `buildAssetManifest` to confirm the parent
 * walker resolves the path correctly against the project's
 * `currentFilePath`.
 *
 * Catches default-project `currentFilePath` resolution bugs analogous
 * to the Plan 2A theme path mismatch (commit `e6381abd`).
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsAddBinaryFile, vfsClear } from './wasmRenderer';
import { buildAssetManifest, type ManifestCacheEntry } from '../components/render/q2-preview/assetWalker';

interface RenderResponse {
    success: boolean;
    error?: string;
    ast_json?: string;
    theme_fingerprint?: string;
}

// Access the WASM module's `render_page_in_project` directly. Calling
// it through wasmRenderer.ts would force the iframe-side renderer
// (different code path); we want the raw WASM render here.
let renderInProject: (path: string) => Promise<string>;

// Minimal valid 1×1 red PNG. Use Buffer (a Uint8Array subclass) — node's
// wasm-bindgen interop treats Uint8Array literals constructed from a
// numeric-array argument as zero-length on some node versions; Buffer
// allocates with the correct backing store.
const PNG_BYTES = new Uint8Array(Buffer.from([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
    0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41,
    0x54, 0x08, 0x99, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x01, 0x5b, 0xa9, 0x6b,
    0xa3, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
]));

beforeAll(async () => {
    // Pre-load the WASM module with explicit bytes — node has no
    // `fetch`, so `wasm.default()` (no args) inside `initWasm` would
    // fail. wasm-bindgen's `__wbg_init` is idempotent, so the second
    // call inside `initWasm` returns the already-loaded instance.
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
        default: (input?: BufferSource) => Promise<unknown>;
        render_page_in_project: (path: string) => Promise<string>;
    };
    await wasm.default(wasmBytes);

    // Now initialize the wasmRenderer singleton; it picks up the
    // already-initialized module so the asset walker's
    // `vfsReadBinaryFile` works against the same module our
    // VFS-add helpers populate.
    await initWasm();

    renderInProject = wasm.render_page_in_project.bind(wasm);

    // Stub URL.createObjectURL / revokeObjectURL — node has no DOM APIs.
    let counter = 0;
    (globalThis.URL.createObjectURL as unknown) = (() => `blob:test-${counter++}`);
    (globalThis.URL.revokeObjectURL as unknown) = (() => {});
    if (typeof globalThis.atob !== 'function') {
        (globalThis as { atob?: (s: string) => string }).atob = (s: string) =>
            Buffer.from(s, 'base64').toString('binary');
    }
});

beforeEach(() => {
    vfsClear();
});

/**
 * VFS path conventions (verified against
 * `crates/quarto-system-runtime/src/wasm.rs::VirtualFileSystem`):
 *
 *  - `project_root` defaults to `/project`. Relative VFS paths are
 *    resolved against this root; absolute paths are used as-is.
 *  - The render entry `render_page_in_project` accepts absolute paths
 *    like `/project/doc.qmd`.
 *  - The asset walker's `vfsReadBinaryFile` call strips the leading
 *    slash (matches the production iframePostProcessor pattern), so
 *    `/project/hero.png` becomes `project/hero.png`, which the VFS
 *    then re-roots to `/project/project/hero.png` — wrong.
 *
 *  The fix that makes both the render and the walker land on the same
 *  VFS key: store assets at a relative path (`hero.png`) — VFS
 *  normalizes it to `/project/hero.png` — and pass `currentFilePath`
 *  to the walker as a project-relative path (matching production,
 *  where Automerge's `file.path` doesn't carry the `/project/`
 *  prefix). Render input still uses the absolute form.
 */
describe('q2-preview asset manifest in project mode (Plan 2B)', () => {
    it('preserves Image target.0 string through the q2-preview pipeline', async () => {
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'index.qmd',
            '---\nformat: q2-preview\n---\n\n![alt](hero.png)\n',
        );
        vfsAddBinaryFile('hero.png', PNG_BYTES);

        const json = await renderInProject('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;

        expect(result.success, `Render failed: ${result.error}`).toBe(true);
        expect(result.ast_json).toBeTruthy();

        // The Image's target.0 should be the user-written URL,
        // unchanged. The asset walker (parent-side) resolves it
        // against currentFilePath; the iframe never sees a rewrite.
        expect(result.ast_json!).toContain('"hero.png"');
    });

    it('parent walker mints a blob URL for the project-relative Image path', async () => {
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'index.qmd',
            '---\nformat: q2-preview\n---\n\n![alt](hero.png){width=400}\n',
        );
        // Add the binary BEFORE render — the natural lifecycle: an
        // upload precedes the first render. Asserts bd-3gtn fix:
        // ResourceCollectorTransform's empty-content artifact must
        // not overwrite the user's bytes during the WASM flush loop.
        vfsAddBinaryFile('hero.png', PNG_BYTES);

        const json = await renderInProject('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;
        expect(result.success).toBe(true);
        expect(result.ast_json).toBeTruthy();

        const cache = new Map<string, ManifestCacheEntry>();
        // currentFilePath is project-relative (matches production: the
        // Automerge file.path doesn't carry the /project/ prefix).
        const { manifest } = buildAssetManifest(
            result.ast_json!,
            'index.qmd',
            cache,
        );

        expect(manifest['hero.png']).toBeDefined();
        expect(manifest['hero.png']).toMatch(/^blob:/);
    });

    it('walker resolves a relative path with `..` traversal in a subdir doc', async () => {
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'sub/page.qmd',
            '---\nformat: q2-preview\n---\n\n![alt](../hero.png)\n',
        );
        // Pre-render binary upload (bd-3gtn).
        vfsAddBinaryFile('hero.png', PNG_BYTES);

        const json = await renderInProject('/project/sub/page.qmd');
        const result = JSON.parse(json) as RenderResponse;
        expect(result.success, `Render failed: ${result.error}`).toBe(true);

        const cache = new Map<string, ManifestCacheEntry>();
        const { manifest } = buildAssetManifest(
            result.ast_json!,
            'sub/page.qmd',
            cache,
        );

        // origPath stays as user-written; blob URL is keyed by it.
        expect(manifest['../hero.png']).toBeDefined();
        expect(manifest['../hero.png']).toMatch(/^blob:/);
    });

    it('walker skips external URLs (no VFS read, not in manifest)', async () => {
        vfsAddFile('_quarto.yml', 'project:\n  type: default\n');
        vfsAddFile(
            'index.qmd',
            '---\nformat: q2-preview\n---\n\n![](https://cdn.example.com/hero.png)\n',
        );

        const json = await renderInProject('/project/index.qmd');
        const result = JSON.parse(json) as RenderResponse;
        expect(result.success).toBe(true);

        const cache = new Map<string, ManifestCacheEntry>();
        const { manifest } = buildAssetManifest(
            result.ast_json!,
            'index.qmd',
            cache,
        );

        expect(manifest).toEqual({});
    });
});
