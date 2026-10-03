import { describe, expect, it, vi } from 'vitest';
import type { PandocFormatInfo } from '@quarto/preview-runtime';
import { MENU_FORMATS, createPdfPreviewController, selectMenuFormats } from './downloadService';
import { getPreviewPandocRunner } from './pandocService';

const row = (key: string, extra: Partial<PandocFormatInfo> = {}): PandocFormatInfo => ({
  key,
  label: key.toUpperCase(),
  extension: key === 'typst' ? 'typ' : key,
  mime: 'x/y',
  available: true,
  hidden: false,
  ...extra,
});
const TABLE = [row('docx'), row('pptx'), row('epub'), row('typst'), row('pdf')];

describe('selectMenuFormats', () => {
  it('offers every format the Rust table has, in its order', () => {
    expect(MENU_FORMATS).toEqual(['docx', 'pptx', 'epub', 'typst', 'pdf']);
    expect(selectMenuFormats(TABLE, false).map((f) => f.key)).toEqual(['docx', 'pptx', 'epub', 'typst', 'pdf']);
  });

  it('keeps the Rust table order and drops hidden and unavailable rows', () => {
    const all = ['docx', 'pptx', 'epub', 'typst', 'pdf'];
    expect(selectMenuFormats(TABLE, false, all).map((f) => f.key)).toEqual(['docx', 'pptx', 'epub', 'typst', 'pdf']);
    expect(selectMenuFormats([row('docx', { available: false }), row('pdf', { hidden: true })], false, all)).toEqual([]);
  });

  it('hides typst and pdf in the embed (default list too): its native render compiles typst to PDF, and pdf is browser-only', () => {
    expect(selectMenuFormats(TABLE, true).map((f) => f.key)).toEqual(['docx', 'pptx', 'epub']);
  });

  it('carries the extension and mime the download uses', () => {
    expect(selectMenuFormats([row('typst')], false, ['typst'])[0]).toEqual({ key: 'typst', label: 'TYPST', extension: 'typ', mime: 'x/y' });
  });
});

describe('createPdfPreviewController: the warm option', () => {
  it('warm takes the preview pool from the warm runner; the fresh path never touches it', () => {
    const acquire = vi.spyOn(getPreviewPandocRunner(), 'acquire').mockImplementation(() => {});
    const release = vi.spyOn(getPreviewPandocRunner(), 'release').mockImplementation(() => {});
    const fresh = createPdfPreviewController(() => {}, { warm: false });
    fresh.acquire();
    fresh.release();
    expect(acquire).not.toHaveBeenCalled();
    const warm = createPdfPreviewController(() => {}, { warm: true });
    warm.acquire();
    warm.release();
    expect(acquire).toHaveBeenCalledTimes(1);
    expect(release).toHaveBeenCalledTimes(1);
    acquire.mockRestore();
    release.mockRestore();
  });

  it('the preview\'s runners are not the app-wide ones, so a Download and a preview never share a render slot', async () => {
    const { getPandoc } = await import('./pandocService');
    const { getPreviewTypstRunner, getTypst } = await import('../typst/typstService');
    expect(getPreviewPandocRunner()).not.toBe(getPandoc().runner);
    expect(getPreviewTypstRunner()).not.toBe(getTypst().runner);
    expect(getPreviewPandocRunner()).toBe(getPreviewPandocRunner());
  });
});
