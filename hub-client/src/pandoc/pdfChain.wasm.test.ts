/**
 * Real-wasm tests for the PDF chain (host phase H8): the production `DownloadController` with
 * the Rust request builder (the built hub wasm), pandoc.wasm in a real worker thread and the
 * typst compiler in a real worker thread. Needs `node scripts/fetch-pandoc-wasm.mjs`, and
 * `npm run build:wasm`.
 */
import { existsSync, readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { countPdfPages } from '@quarto/typst-host';
import type { ShareTree } from '@quarto/pandoc-host';
import { FakeCache } from '../test-utils/fakeCache';
import { nodePandocWorker } from '../test-utils/nodeWorker';
import { nodeTypstWorker } from '../test-utils/nodeTypstWorker';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { PandocLoader } from './pandocLoader';
import { PandocRunner } from './pandocRunner';
import { DownloadController, type DownloadFormat, type RequestEnvelope } from './downloadController';
import { createTypstLoader, TypstFontsLoader, TYPST_FONTS_PATH, TYPST_WASM_PATH } from '../typst/typstAssets';
import { TypstRunner } from '../typst/typstRunner';
import { splitTypstAssets } from '../typst/typstAssetSplit';

const here = path.dirname(fileURLToPath(import.meta.url));
const PUBLIC = path.resolve(here, '../../public');
const CI = !!process.env.CI;
const haveAssets = pandocWasmAvailable() && existsSync(path.join(PUBLIC, TYPST_WASM_PATH)) && existsSync(path.join(PUBLIC, TYPST_FONTS_PATH));
const BASE = 'https://app.test/';
const PDF: DownloadFormat = { key: 'typst-pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
const SDE = 1_700_000_000;

it('has the pandoc and typst assets (required in CI)', () => {
  if (!haveAssets && CI) throw new Error('pandoc/typst assets missing: run node scripts/fetch-pandoc-wasm.mjs --require');
  if (!haveAssets) console.warn('SKIPPING the real-wasm PDF chain tests: run node scripts/fetch-pandoc-wasm.mjs');
});

interface Wasm {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_pandoc_request: (p: string, f: string, sde?: number, cap?: Uint8Array, fonts?: string[], signal?: AbortSignal, options?: { scope?: 'auto' | 'chapter' }) => Promise<RequestEnvelope>;
  get_pandoc_share_tree: () => ShareTree;
  classify_pandoc_completion: (stage: string, success: boolean, status: string, stderr: string, json: string) => string;
  get_typst_assets: () => { files: { path: string; bytes: Uint8Array }[] };
  typst_date_prelude: (epoch: number) => string;
}

// A 1x1 opaque PNG.
const PNG = new Uint8Array(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==', 'base64'));

describe.skipIf(!haveAssets)('PDF chain against the real wasm', () => {
  let wasm: Wasm;
  const served: Record<string, Uint8Array> = {};
  beforeAll(async () => {
    wasm = (await import('wasm-quarto-hub-client')) as unknown as Wasm;
    await wasm.default(await readFile(path.resolve(here, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm')));
    served[`${BASE}pandoc/pandoc.wasm.gz`] = new Uint8Array(gzipSync(readFileSync(WASM_PATH), { level: 1 }));
    for (const p of [TYPST_WASM_PATH, TYPST_FONTS_PATH]) served[`${BASE}${p}`] = new Uint8Array(readFileSync(path.join(PUBLIC, p)));
  }, 120_000);
  beforeEach(() => {
    wasm.vfs_clear();
    wasm.vfs_set_runtime_metadata('');
  });

  function make() {
    const fetched: string[] = [];
    const env = (caches: CacheStorage) => ({
      caches,
      baseURI: BASE,
      fetch: (async (url: string) => {
        fetched.push(url);
        const key = url.split('?')[0];
        const body = served[key];
        return body ? new Response(body.slice()) : new Response('', { status: 404 });
      }) as unknown as typeof fetch,
    });
    const storage = () => {
      const caches = new Map<string, FakeCache>();
      return { open: async (name: string) => caches.get(name) ?? caches.set(name, new FakeCache()).get(name) } as unknown as CacheStorage;
    };
    const pandocLoader = new PandocLoader({ env: env(storage()) });
    const typstEnv = env(storage());
    const typstRunner = new TypstRunner({ loader: createTypstLoader({ env: typstEnv }), fonts: new TypstFontsLoader({ env: typstEnv }), createWorker: nodeTypstWorker });
    const save = vi.fn();
    const buildRequest = vi.fn((p: string, f: string, sde: number, signal: AbortSignal, fonts?: string[]) => wasm.render_pandoc_request(p, f, sde, undefined, fonts, signal));
    const controller = new DownloadController({
      buildRequest,
      getShareTree: () => wasm.get_pandoc_share_tree(),
      runner: new PandocRunner({ loader: pandocLoader, createWorker: nodePandocWorker }),
      classify: (stage, success, status, stderr, json) => JSON.parse(wasm.classify_pandoc_completion(stage, success, status, stderr, json)),
      typst: { runner: typstRunner, assets: () => splitTypstAssets(wasm.get_typst_assets().files), datePrelude: wasm.typst_date_prelude },
      save,
      nowSeconds: () => SDE,
    });
    return { controller, save, buildRequest, fetched };
  }

  const savedPdf = async (save: ReturnType<typeof vi.fn>): Promise<Uint8Array> => new Uint8Array(await (save.mock.calls[0][0] as Blob).arrayBuffer());
  const failedText = (c: DownloadController) => JSON.stringify(c.getSnapshot());

  it('a typst document goes through pandoc then typst to a valid PDF with the expected page count', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Chain\n---\n\nPage one.\n\n```{=typst}\n#pagebreak()\n```\n\nPage two.\n');
    const { controller, save, buildRequest } = make();
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    expect(controller.getSnapshot(), failedText(controller)).toMatchObject({ phase: 'done', fileName: 'doc.pdf' });
    // The compiler was loaded first: the request was built with its font families.
    expect(buildRequest.mock.calls[0][4]).toEqual(expect.arrayContaining(['Libertinus Serif']));
    const pdf = await savedPdf(save);
    expect(new TextDecoder().decode(pdf.slice(0, 5))).toBe('%PDF-');
    expect(countPdfPages(pdf)).toBe(2);
  }, 180_000);

  it('an image and the template import compile: the compile sees the tree pandoc did', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: With image\n---\n\n# Heading\n\n![A pixel](pixel.png)\n');
    wasm.vfs_add_binary_file('/project/pixel.png', PNG);
    const { controller, save } = make();
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    const s = controller.getSnapshot();
    expect(s, failedText(controller)).toMatchObject({ phase: 'done' });
    const pdf = await savedPdf(save);
    expect(countPdfPages(pdf)).toBe(1);
    // An image pandoc named but typst could not read would be a typst-error, not a download.
    if (s.phase === 'done') expect(s.warnings.filter((w) => w.kind === 'error')).toEqual([]);
  }, 180_000);

  it('a callout uses the vendored packages and Font Awesome without any registry fetch', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Callout\n---\n\n::: {.callout-note}\n## Note\nBody text.\n:::\n');
    const { controller, save, fetched } = make();
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    expect(controller.getSnapshot(), failedText(controller)).toMatchObject({ phase: 'done' });
    expect(countPdfPages(await savedPdf(save))).toBe(1);
    expect(fetched.filter((u) => u.includes('packages.typst.org'))).toEqual([]);
  }, 180_000);

  it('a user template partial is compiled (the partial is part of the request pandoc ran)', async () => {
    wasm.vfs_add_file('/project/p/typst-show.typ', '#set text(fill: rgb("#112233"))\n');
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Partial\nformat:\n  typst:\n    template-partials:\n      - p/typst-show.typ\n---\n\nHello.\n');
    const { controller } = make();
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    expect(controller.getSnapshot(), failedText(controller)).toMatchObject({ phase: 'done' });
  }, 180_000);

  it('the same document twice gives byte-identical PDFs (the date prelude pins the creation date)', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Same\n---\n\nHello.\n');
    const a = make();
    await a.controller.start({ path: '/project/doc.qmd', format: PDF });
    const b = make();
    await b.controller.start({ path: '/project/doc.qmd', format: PDF });
    expect(Buffer.from(await savedPdf(a.save)).equals(Buffer.from(await savedPdf(b.save)))).toBe(true);
  }, 240_000);

  it('a typst compile error blocks the download and is tagged stage typst', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Broken\n---\n\n```{=typst}\n#undefined-function()\n```\n');
    const { controller, save } = make();
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    const s = controller.getSnapshot();
    expect(save).not.toHaveBeenCalled();
    expect(s).toMatchObject({ phase: 'failed', state: 'typst-error' });
    if (s.phase === 'failed') expect(s.diagnostics.some((d) => d.stage === 'typst' && d.kind === 'error')).toBe(true);
  }, 180_000);

  it('a cancel during the compile terminates the worker and reports once', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Cancel\n---\n\nHello.\n');
    const { controller, save } = make();
    const phases: string[] = [];
    controller.subscribe(() => {
      const s = controller.getSnapshot();
      phases.push(s.phase === 'working' ? `working:${s.stage}` : s.phase);
      if (s.phase === 'working' && s.stage === 'typst-compiling') controller.cancel();
    });
    await controller.start({ path: '/project/doc.qmd', format: PDF });
    expect(save).not.toHaveBeenCalled();
    expect(phases.filter((p) => p === 'cancelled')).toHaveLength(1);
    expect(controller.getSnapshot().phase).toBe('cancelled');
  }, 180_000);
});
