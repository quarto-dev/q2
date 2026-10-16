import { describe, it, expect, vi, beforeEach } from 'vitest';

let ready = true;
let response: unknown;
const resolve = vi.fn((..._args: unknown[]) => response);
vi.mock('@quarto/preview-runtime', () => ({
  isWasmReady: () => ready,
  resolvePandocFormats: (...args: unknown[]) => resolve(...args),
}));

import { isTypstPdf } from './pdfPage';

const ok = (key: string, cls: string, formats = [{ key, class: cls }]) => ({ success: true, key, class: cls, pseudoFormat: null, scope: 'chapter', formats });

describe('isTypstPdf follows the resolver’s resolved key', () => {
  beforeEach(() => {
    ready = true;
  });

  it('is true for a typst-pdf download (a typst document, or a project-level `format: typst`)', () => {
    response = ok('typst-pdf', 'download');
    expect(isTypstPdf('a.qmd')).toBe(true);
  });

  it('reads the resolved key, not formats[0]', () => {
    response = ok('typst-pdf', 'download', [{ key: 'html', class: 'preview' }, { key: 'typst-pdf', class: 'download' }]);
    expect(isTypstPdf('a.qmd')).toBe(true);
    response = ok('html', 'preview', [{ key: 'typst-pdf', class: 'download' }, { key: 'html', class: 'preview' }]);
    expect(isTypstPdf('a.qmd')).toBe(false);
  });

  it('is false for other formats, the typst source key, a failed resolution, or before wasm is ready', () => {
    response = ok('typst', 'download');
    expect(isTypstPdf('a.qmd')).toBe(false);
    response = { success: false, error: 'no' };
    expect(isTypstPdf('a.qmd')).toBe(false);
    response = ok('typst-pdf', 'download');
    ready = false;
    expect(isTypstPdf('a.qmd')).toBe(false);
  });
});

describe('isTypstPdf takes the session’s render flags (F8)', () => {
  beforeEach(() => {
    ready = true;
    resolve.mockClear();
  });

  it('with no flags the resolver is asked as before', () => {
    response = ok('html', 'preview');
    expect(isTypstPdf('a.qmd')).toBe(false);
    expect(resolve).toHaveBeenCalledWith('a.qmd', undefined);
  });

  it('passes to and metadata to the resolver, which decides', () => {
    response = ok('typst-pdf', 'download');
    expect(isTypstPdf('a.qmd', { to: 'typst', metadata: { toc: true } })).toBe(true);
    expect(resolve).toHaveBeenCalledWith('a.qmd', { to: 'typst', metadata: { toc: true } });
  });
});
