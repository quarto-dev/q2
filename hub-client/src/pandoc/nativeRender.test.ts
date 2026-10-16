import { describe, expect, it } from 'vitest';
import { fileNameFromDisposition, renderNatively, renderPdfNatively } from './nativeRender';

const BASE = 'http://127.0.0.1:8080/app/';
const req = { path: 'doc.qmd', format: 'docx', content: 'hi' };

function fakeFetch(res: Response | Error) {
  const calls: { url: string; init: RequestInit }[] = [];
  const fn = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    if (res instanceof Error) throw res;
    return res;
  }) as unknown as typeof fetch;
  return { fn, calls };
}

describe('renderNatively', () => {
  it('posts the request to the route under the page base and returns the file', async () => {
    const warn = [{ title: 'w' }];
    const { fn, calls } = fakeFetch(
      new Response(new Uint8Array([80, 75]), {
        status: 200,
        headers: {
          'content-disposition': `attachment; filename="caf__.docx"; filename*=UTF-8''caf%C3%A9.docx`,
          'x-q2-diagnostics': encodeURIComponent(JSON.stringify(warn)),
        },
      }),
    );
    const out = await renderNatively(req, { fetchFn: fn, baseUrl: BASE });
    expect(calls[0].url).toBe('http://127.0.0.1:8080/app/api/preview/render');
    expect(calls[0].init.method).toBe('POST');
    expect(JSON.parse(calls[0].init.body as string)).toEqual(req);
    expect(out.kind).toBe('ok');
    if (out.kind !== 'ok') return;
    expect(out.fileName).toBe('café.docx');
    expect(out.warnings).toEqual(warn);
    expect(out.blob.size).toBe(2);
  });

  it('reports a 422 as failed with the diagnostics, and downloads nothing', async () => {
    const { fn } = fakeFetch(new Response(JSON.stringify({ error: 'boom', diagnostics: [{ a: 1 }] }), { status: 422 }));
    expect(await renderNatively(req, { fetchFn: fn, baseUrl: BASE })).toEqual({
      kind: 'failed',
      error: 'boom',
      diagnostics: [{ a: 1 }],
    });
  });

  it('reports other statuses and network errors as errors', async () => {
    const bad = fakeFetch(new Response('nope', { status: 400 }));
    expect(await renderNatively(req, { fetchFn: bad.fn, baseUrl: BASE })).toEqual({ kind: 'error', message: 'nope' });
    const down = fakeFetch(new Error('refused'));
    const out = await renderNatively(req, { fetchFn: down.fn, baseUrl: BASE });
    expect(out.kind).toBe('error');
  });

  it('rethrows when the caller aborted', async () => {
    const ac = new AbortController();
    ac.abort();
    const { fn } = fakeFetch(new DOMException('aborted', 'AbortError'));
    await expect(renderNatively(req, { fetchFn: fn, baseUrl: BASE, signal: ac.signal })).rejects.toThrow();
  });

  it('a malformed diagnostics header does not lose the download', async () => {
    const { fn } = fakeFetch(new Response('x', { status: 200, headers: { 'x-q2-diagnostics': '%E0%A4%A' } }));
    const out = await renderNatively(req, { fetchFn: fn, baseUrl: BASE });
    expect(out.kind === 'ok' && out.warnings).toEqual([]);
  });
});

describe('renderNatively contract', () => {
  it('sends scope and attributionJson and reads the unexecuted-cells stat', async () => {
    const { fn, calls } = fakeFetch(
      new Response('x', { status: 200, headers: { 'x-q2-stats': encodeURIComponent(JSON.stringify({ unexecuted_cells: 3 })) } }),
    );
    const request = { ...req, metadata: { 'book-scope': 'chapter' }, attributionJson: '{"a":1}' };
    const out = await renderNatively(request, { fetchFn: fn, baseUrl: BASE });
    expect(JSON.parse(calls[0].init.body as string)).toEqual(request);
    expect(out.kind === 'ok' && out.unexecutedCells).toBe(3);
  });

  it('a malformed or missing stats header counts no unexecuted cells', async () => {
    const bad = fakeFetch(new Response('x', { status: 200, headers: { 'x-q2-stats': '%E0%A4%A' } }));
    const out = await renderNatively(req, { fetchFn: bad.fn, baseUrl: BASE });
    expect(out.kind === 'ok' && out.unexecutedCells).toBe(0);
    const none = await renderNatively(req, { fetchFn: fakeFetch(new Response('x')).fn, baseUrl: BASE });
    expect(none.kind === 'ok' && none.unexecutedCells).toBe(0);
  });

  it('reads the book stat only for a whole-book file, and the file name from Content-Disposition', async () => {
    const stats = (book: unknown) => encodeURIComponent(JSON.stringify({ unexecuted_cells: 0, book }));
    const respond = (book: unknown) =>
      renderNatively(req, {
        fetchFn: fakeFetch(new Response('x', { status: 200, headers: { 'x-q2-stats': stats(book), 'content-disposition': 'attachment; filename="Small-Book.typ"' } })).fn,
        baseUrl: BASE,
      });
    const whole = await respond({ scope: 'book', chapters: 3 });
    expect(whole.kind === 'ok' && whole.book).toEqual({ chapters: 3 });
    expect(whole.kind === 'ok' && whole.fileName).toBe('Small-Book.typ');
    for (const book of [{ scope: 'chapter', chapters: 3 }, { scope: 'none', chapters: 3 }, null]) {
      const out = await respond(book);
      expect(out.kind === 'ok' && out.book).toBeUndefined();
    }
  });

  it('discriminates a 409 by its body: superseded, busy, anything else is an error', async () => {
    const outcome = (body: string) => renderNatively(req, { fetchFn: fakeFetch(new Response(body, { status: 409 })).fn, baseUrl: BASE });
    expect(await outcome('{"superseded":true}')).toEqual({ kind: 'superseded' });
    expect(await outcome('{"busy":true}')).toEqual({ kind: 'busy' });
    expect(await outcome('conflict')).toEqual({ kind: 'error', message: 'conflict' });
  });
});

describe('renderPdfNatively', () => {
  it('posts to the inline pdf route and leaves the file name empty when the server names none', async () => {
    const { fn, calls } = fakeFetch(new Response(new Uint8Array([37, 80]), { status: 200 }));
    const out = await renderPdfNatively({ ...req, format: 'typst-pdf', metadata: { 'book-scope': 'chapter' } }, { fetchFn: fn, baseUrl: BASE });
    expect(calls[0].url).toBe('http://127.0.0.1:8080/app/api/preview/pdf');
    expect(out.kind === 'ok' && out.fileName).toBe('');
  });

  it('rethrows when the caller aborted', async () => {
    const ac = new AbortController();
    ac.abort();
    const { fn } = fakeFetch(new DOMException('aborted', 'AbortError'));
    await expect(renderPdfNatively(req, { fetchFn: fn, baseUrl: BASE, signal: ac.signal })).rejects.toThrow();
  });
});

describe('fileNameFromDisposition', () => {
  it('prefers filename*, falls back to filename, then the default', () => {
    expect(fileNameFromDisposition(`attachment; filename="a.docx"; filename*=UTF-8''b%20c.docx`, 'd')).toBe('b c.docx');
    expect(fileNameFromDisposition('attachment; filename="a.docx"', 'd')).toBe('a.docx');
    expect(fileNameFromDisposition(null, 'd')).toBe('d');
  });
});
