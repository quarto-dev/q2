/**
 * @vitest-environment jsdom
 *
 * "Download as" in the `q2 preview` embed (pandoc-host H5, D7): the app-wide controller is built
 * with the native executor, so a click POSTs the editor's current text to the preview server's
 * render route and saves the returned file; pandoc.wasm is not touched.
 */
import { Blob as NodeBlob } from 'node:buffer';
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
    // No scope for docx (it has no whole-book output); no attribution without an editor history to read.
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

  it('forwards the click scope and shows the unexecuted-cells count from X-Q2-Stats', async () => {
    const fetchMock = vi.fn(
      async () => new Response(new Uint8Array([1]), { status: 200, headers: { 'x-q2-stats': encodeURIComponent(JSON.stringify({ unexecuted_cells: 2 })) } }),
    );
    vi.stubGlobal('fetch', fetchMock);
    const { getDownloadController } = await import('./downloadService');
    const controller = getDownloadController();
    const epub = { key: 'epub', label: 'EPUB', extension: 'epub', mime: 'application/epub+zip' };
    await controller.start({ path: 'doc.qmd', format: epub, content: 'x', metadata: { 'book-scope': 'chapter' } });
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toMatchObject({ format: 'epub', metadata: { 'book-scope': 'chapter' } });
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', unexecutedCells: 2 });
  });

  it('a busy 409 is a visible error, not a silent drop', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{"busy":true}', { status: 409 })));
    const { getDownloadController } = await import('./downloadService');
    const controller = getDownloadController();
    await controller.start({ path: 'doc.qmd', format: DOCX, content: '' });
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'native-error' });
  });

  it('Download as PDF saves the file from the render route and hands the bytes to the pdf listeners', async () => {
    // The controller reads the PDF with `blob.arrayBuffer()`. Newer Node (24.21) builds `Response.blob()` with the global Blob,
    // which under jsdom has no `arrayBuffer()`, so hand back Node's own Blob (browsers have the method either way).
    const bytes = new Uint8Array([37, 80, 68, 70]);
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        const res = new Response(bytes, { status: 200, headers: { 'content-disposition': 'attachment; filename="doc.pdf"' } });
        res.blob = async () => new NodeBlob([bytes]) as unknown as Blob;
        return res;
      }),
    );
    const { getDownloadController, onPdfCompiled } = await import('./downloadService');
    const seen: number[] = [];
    onPdfCompiled((pdf) => seen.push(pdf.length));
    const pdf = { key: 'typst-pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
    await getDownloadController().start({ path: 'doc.qmd', format: pdf, content: 'x', metadata: { 'book-scope': 'chapter' } });
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0][1]).toBe('doc.pdf');
    expect(seen).toEqual([4]);
  });

  it('the PDF preview is available, so a typst document classifies as the pdf mode', async () => {
    const { pdfPreviewAvailable } = await import('./downloadService');
    const { classifyPreviewMode } = await import('../components/render/getQ2Format');
    expect(pdfPreviewAvailable()).toBe(true);
    const resolved = { success: true, key: 'typst-pdf', class: 'download', pseudoFormat: null, scope: 'chapter', formats: [{ key: 'typst-pdf', class: 'download' }] } as never;
    expect(classifyPreviewMode('a.qmd', { resolve: () => resolved, canDownload: () => true, canPreviewPdf: pdfPreviewAvailable })).toEqual({ mode: 'pdf', formatKey: 'typst-pdf' });
  });
});
