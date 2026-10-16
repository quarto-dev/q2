import { describe, it, expect, vi, beforeEach } from 'vitest';

let ready = true;
let response: unknown;
vi.mock('@quarto/preview-runtime', () => ({
  isWasmReady: () => ready,
  resolvePandocFormats: () => response,
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
