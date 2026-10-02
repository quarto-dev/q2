import { describe, expect, it } from 'vitest';
import type { PandocFormatInfo } from '@quarto/preview-runtime';
import { MENU_FORMATS, selectMenuFormats } from './downloadService';

const row = (key: string, extra: Partial<PandocFormatInfo> = {}): PandocFormatInfo => ({
  key,
  label: key.toUpperCase(),
  extension: key === 'typst' ? 'typ' : key,
  mime: 'x/y',
  available: true,
  hidden: false,
  ...extra,
});
const TABLE = [row('docx'), row('pptx'), row('epub'), row('typst'), row('pdf', { hidden: true })];

describe('selectMenuFormats', () => {
  it('offers only the reviewed entries until the later request phases are wired in', () => {
    expect(MENU_FORMATS).toEqual(['docx']);
    expect(selectMenuFormats(TABLE, false).map((f) => f.key)).toEqual(['docx']);
  });

  it('keeps the Rust table order and drops hidden and unavailable rows', () => {
    const all = ['docx', 'pptx', 'epub', 'typst', 'pdf'];
    expect(selectMenuFormats(TABLE, false, all).map((f) => f.key)).toEqual(['docx', 'pptx', 'epub', 'typst']);
    expect(selectMenuFormats([row('docx', { available: false })], false, all)).toEqual([]);
  });

  it('hides typst in the embed: its native render compiles to PDF', () => {
    const all = ['docx', 'pptx', 'epub', 'typst'];
    expect(selectMenuFormats(TABLE, true, all).map((f) => f.key)).toEqual(['docx', 'pptx', 'epub']);
  });

  it('carries the extension and mime the download uses', () => {
    expect(selectMenuFormats([row('typst')], false, ['typst'])[0]).toEqual({ key: 'typst', label: 'TYPST', extension: 'typ', mime: 'x/y' });
  });
});
