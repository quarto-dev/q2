import { describe, it, expect, vi, beforeEach } from 'vitest';

let ready = true;
let response: unknown;
const resolve = vi.fn((..._args: unknown[]) => response);
vi.mock('@quarto/preview-runtime', () => ({
  isWasmReady: () => ready,
  resolvePandocFormatsAsync: async (...args: unknown[]) => resolve(...args),
}));

import { isTypstPdf } from './pdfPage';

const ok = (key: string, cls: string, formats = [{ key, class: cls }]) => ({ success: true, key, class: cls, pseudoFormat: null, scope: 'chapter', formats });

describe('isTypstPdf follows the resolver’s resolved key', () => {
  beforeEach(() => {
    ready = true;
  });

  it('is true for a typst-pdf download (a typst document, or a project-level `format: typst`)', async () => {
    response = ok('typst-pdf', 'download');
    expect(await isTypstPdf('a.qmd')).toBe(true);
  });

  it('reads the resolved key, not formats[0]', async () => {
    response = ok('typst-pdf', 'download', [{ key: 'html', class: 'preview' }, { key: 'typst-pdf', class: 'download' }]);
    expect(await isTypstPdf('a.qmd')).toBe(true);
    response = ok('html', 'preview', [{ key: 'typst-pdf', class: 'download' }, { key: 'html', class: 'preview' }]);
    expect(await isTypstPdf('a.qmd')).toBe(false);
  });

  it('is false for other formats, the typst source key, a failed resolution, or before wasm is ready', async () => {
    response = ok('typst', 'download');
    expect(await isTypstPdf('a.qmd')).toBe(false);
    response = { success: false, error: 'no' };
    expect(await isTypstPdf('a.qmd')).toBe(false);
    response = ok('typst-pdf', 'download');
    ready = false;
    expect(await isTypstPdf('a.qmd')).toBe(false);
  });
});

describe('isTypstPdf takes the session’s render flags (F8)', () => {
  beforeEach(() => {
    ready = true;
    resolve.mockClear();
  });

  it('with no flags the resolver is asked as before', async () => {
    response = ok('html', 'preview');
    expect(await isTypstPdf('a.qmd')).toBe(false);
    expect(resolve).toHaveBeenCalledWith('a.qmd', undefined);
  });

  it('passes to and metadata to the resolver, which decides', async () => {
    response = ok('typst-pdf', 'download');
    expect(await isTypstPdf('a.qmd', { to: 'typst', metadata: { toc: true } })).toBe(true);
    expect(resolve).toHaveBeenCalledWith('a.qmd', { to: 'typst', metadata: { toc: true } });
  });
});
