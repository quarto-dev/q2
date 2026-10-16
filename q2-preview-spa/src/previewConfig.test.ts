import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  fetchPreviewConfig,
  parsePreviewConfig,
  PreviewConfigError,
  pdfMetadata,
  wasmRenderOptions,
} from './previewConfig';

const loc = { protocol: 'http:', host: 'localhost:1' };
const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const fast = { attempts: 3, delayMs: 0 };

afterEach(() => vi.unstubAllGlobals());

describe('parsePreviewConfig', () => {
  it('reads allowEdit and an absent render block as no flags', () => {
    expect(parsePreviewConfig({ allowEdit: true })).toEqual({ allowEdit: true });
    expect(parsePreviewConfig({})).toEqual({ allowEdit: false });
    expect(parsePreviewConfig({ allowEdit: 'yes' })).toEqual({ allowEdit: false });
  });

  it('reads the render block', () => {
    expect(
      parsePreviewConfig({ allowEdit: false, render: { to: 'typst', metadata: { toc: true } } }),
    ).toEqual({ allowEdit: false, render: { to: 'typst', metadata: { toc: true } } });
    expect(parsePreviewConfig({ render: { metadata: { a: 1 } } }).render).toEqual({ metadata: { a: 1 } });
    expect(parsePreviewConfig({ render: {} }).render).toBeUndefined();
  });

  it('rejects a malformed render block instead of dropping the flags', () => {
    for (const render of [5, 'x', [], { to: 5 }, { to: '' }, { metadata: [] }, { metadata: 'a' }]) {
      expect(() => parsePreviewConfig({ render }), JSON.stringify(render)).toThrow(PreviewConfigError);
    }
    expect(() => parsePreviewConfig('nope')).toThrow(PreviewConfigError);
  });
});

describe('fetchPreviewConfig', () => {
  it('returns the config of a successful answer', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json(200, { allowEdit: true, render: { to: 'typst' } })));
    expect(await fetchPreviewConfig(loc, fast)).toEqual({ allowEdit: true, render: { to: 'typst' } });
  });

  it('a server that answers with no render block means no flags', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => json(200, { allowEdit: false })));
    expect(await fetchPreviewConfig(loc, fast)).toEqual({ allowEdit: false });
  });

  it('a 404 is an older server: read-only, no flags, no retry', async () => {
    const fetchMock = vi.fn(async () => new Response('nope', { status: 404 }));
    vi.stubGlobal('fetch', fetchMock);
    expect(await fetchPreviewConfig(loc, fast)).toEqual({ allowEdit: false });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('retries a transient failure and then succeeds with the flags', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new TypeError('network'))
      .mockResolvedValueOnce(new Response('boom', { status: 503 }))
      .mockResolvedValueOnce(json(200, { render: { to: 'typst' } }));
    vi.stubGlobal('fetch', fetchMock);
    expect(await fetchPreviewConfig(loc, fast)).toEqual({ allowEdit: false, render: { to: 'typst' } });
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('keeps failing: rejects, so the flagged session does not render as unflagged html', async () => {
    for (const make of [
      () => Promise.reject(new TypeError('network')),
      async () => new Response('boom', { status: 500 }),
      async () => new Response('<html>', { status: 200 }),
    ]) {
      const fetchMock = vi.fn(make);
      vi.stubGlobal('fetch', fetchMock);
      await expect(fetchPreviewConfig(loc, fast)).rejects.toBeInstanceOf(PreviewConfigError);
      expect(fetchMock).toHaveBeenCalledTimes(3);
    }
  });
});

describe('wasmRenderOptions and pdfMetadata', () => {
  it('no flags changes nothing', () => {
    expect(wasmRenderOptions(undefined)).toBeUndefined();
    expect(pdfMetadata(undefined)).toEqual({ 'book-scope': 'chapter' });
  });

  it('passes to and metadata through', () => {
    expect(wasmRenderOptions({ to: 'typst', metadata: { toc: true } })).toEqual({
      to: 'typst',
      metadata: { toc: true },
    });
    expect(wasmRenderOptions({ metadata: { toc: true } })).toEqual({ metadata: { toc: true } });
  });

  it("the pane's chapter scope wins over a CLI book-scope", () => {
    expect(pdfMetadata({ metadata: { 'book-scope': 'book', toc: true } })).toEqual({
      toc: true,
      'book-scope': 'chapter',
    });
  });
});
