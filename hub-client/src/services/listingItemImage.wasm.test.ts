/**
 * WASM regression test for bd-yqlbfrln: a listing page whose items
 * carry a front-matter `image:` must render in the hub-client.
 *
 * The Blog template's `index.qmd` lists `posts/`, and each post
 * declares `image: "image.jpg"`. `ListingGenerateTransform` used to
 * register a copy intent for every item image with a destination
 * under the native `output_dir` (`/project/_site/...`, bd-qv2lsab0).
 * In the hub the only allowed write root is the synthetic
 * `/.quarto/project-artifacts` VFS root, so `OutputSink` rejected the
 * copy and Pass 2 failed for the whole page:
 *
 *   Pass 2 failed for /project/index.qmd: output destination
 *   /project/_site/posts/post-with-code/image.jpg is not under any
 *   allowed root (/.quarto/project-artifacts)
 *
 * Mirrors the hub's real render path: `render_page_in_project_with_
 * attribution` with `prefer_preview_format = true`, so a document with
 * no `format:` key goes through q2-preview and returns `ast_json`.
 *
 * Run with: npm run test:wasm
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import {
    initWasm,
    renderPageInProjectWithAttribution,
    vfsAddFile,
    vfsAddBinaryFile,
    vfsClear,
} from '@quarto/preview-runtime';

interface RenderResponseLike {
    success: boolean;
    error?: string;
    html?: string;
    ast_json?: string;
}

// Any bytes do: the copy machinery (and its absence) is what is under
// test, not image decoding.
const JPG_BYTES = new Uint8Array(Buffer.from([0xff, 0xd8, 0xff, 0xd9]));

beforeAll(async () => {
    const __dirname = dirname(fileURLToPath(import.meta.url));
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as {
        default: (input?: BufferSource) => Promise<unknown>;
    };
    await wasm.default(wasmBytes);
    await initWasm();
});

beforeEach(() => {
    vfsClear();
});

function addBlogProject() {
    vfsAddFile(
        '_quarto.yml',
        [
            'project:',
            '  type: website',
            '',
            'website:',
            '  title: "Blog"',
            '',
            'format:',
            '  html:',
            '    theme: cosmo',
            '',
        ].join('\n'),
    );
    vfsAddFile(
        'index.qmd',
        [
            '---',
            'title: "Blog"',
            'listing:',
            '  contents: posts',
            '  sort: "date desc"',
            '  type: default',
            'page-layout: full',
            'title-block-banner: true',
            '---',
            '',
        ].join('\n'),
    );
    vfsAddFile(
        'posts/post-with-code/index.qmd',
        [
            '---',
            'title: "Post With Code"',
            'date: "2026-01-15"',
            'categories: [news, code]',
            'image: "image.jpg"',
            '---',
            '',
            'Body.',
            '',
        ].join('\n'),
    );
    vfsAddBinaryFile('posts/post-with-code/image.jpg', JPG_BYTES);
}

async function renderIndex(preferPreviewFormat: boolean): Promise<RenderResponseLike> {
    return (await renderPageInProjectWithAttribution(
        '/project/index.qmd',
        undefined,
        null,
        undefined,
        preferPreviewFormat,
    )) as RenderResponseLike;
}

describe('listing item images in the hub-client (bd-yqlbfrln)', () => {
    it('renders the Blog template index through q2-preview without a copy failure', async () => {
        addBlogProject();
        const resp = await renderIndex(true);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.ast_json).toBeDefined();
        // The thumbnail still points at the post image, host-relative,
        // for the parent-side asset walker to read from the VFS.
        expect(resp.ast_json!).toContain('posts/post-with-code/image.jpg');
    });

    it('renders the same listing through the HTML pipeline too', async () => {
        addBlogProject();
        const resp = await renderIndex(false);
        expect(resp.success, `render failed: ${resp.error}`).toBe(true);
        expect(resp.html).toBeDefined();
        expect(resp.html!).toContain('src="posts/post-with-code/image.jpg"');
    });
});
