/**
 * @vitest-environment jsdom
 *
 * "Download as" in the `q2 preview` embed (pandoc-host H5, D7): the app-wide controller is built
 * with the native executor, so a click POSTs the editor's current text to the preview server's
 * render route and saves the returned file; pandoc.wasm is not touched.
 */
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';

const save = vi.fn();
vi.mock('./saveBlob', () => ({ saveBlob: (...args: unknown[]) => save(...args) }));
vi.mock('./pandocService', () => ({
  getPandoc: () => {
    throw new Error('pandoc.wasm must not be used in the embed');
  },
}));

beforeEach(() => {
  vi.stubEnv('VITE_PREVIEW_EMBED', '1');
  vi.resetModules();
  save.mockClear();
});
afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

const DOCX = { key: 'docx', label: 'Word', extension: 'docx', mime: 'application/docx' };

describe('download controller in the preview embed', () => {
  it('is offered with pandoc.wasm off, and posts the editor text to the native route', async () => {
    const fetchMock = vi.fn(
      async () =>
        new Response(new Uint8Array([80, 75]), {
          status: 200,
          headers: { 'content-disposition': 'attachment; filename="doc.docx"', 'x-q2-diagnostics': encodeURIComponent(JSON.stringify([{ kind: 'warning', title: 'w' }])) },
        }),
    );
    vi.stubGlobal('fetch', fetchMock);
    const { downloadAvailable, getDownloadController } = await import('./downloadService');
    expect(downloadAvailable()).toBe(true);

    const controller = getDownloadController();
    await controller.start({ path: 'doc.qmd', format: DOCX, content: '# edited but unsaved' });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toMatch(/\/api\/preview\/render$/);
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ path: 'doc.qmd', format: 'docx', content: '# edited but unsaved' });
    // (The Blob comes from Node's Response, not jsdom's Blob class, so check it by shape.)
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0][0]).toMatchObject({ size: 2 });
    expect(save.mock.calls[0][1]).toBe('doc.docx');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'doc.docx', warnings: [{ kind: 'warning' }] });
  });

  it('a 422 from the server downloads nothing and shows the diagnostics', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: 'render failed', diagnostics: [{ kind: 'error', title: 'bad' }] }), { status: 422 })),
    );
    const { getDownloadController } = await import('./downloadService');
    const controller = getDownloadController();
    await controller.start({ path: 'doc.qmd', format: DOCX, content: '' });
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'native-failed', diagnostics: [{ kind: 'error' }] });
  });
});
