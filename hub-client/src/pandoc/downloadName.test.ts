import { describe, expect, it } from 'vitest';
import { sanitizeDownloadName } from './downloadName';

describe('sanitizeDownloadName', () => {
  it('uses the document stem and the output extension', () => {
    expect(sanitizeDownloadName('report.qmd', 'docx')).toBe('report.docx');
    expect(sanitizeDownloadName('docs/sub/My Report.qmd', 'pptx')).toBe('My Report.pptx');
    expect(sanitizeDownloadName('a.b.qmd', 'epub')).toBe('a.b.epub');
  });

  it('drops directory parts and ../', () => {
    expect(sanitizeDownloadName('../../etc/passwd.qmd', 'docx')).toBe('passwd.docx');
    expect(sanitizeDownloadName('..\\..\\win\\x.qmd', 'docx')).toBe('x.docx');
    expect(sanitizeDownloadName('dir/..', 'docx')).toBe('document.docx');
    expect(sanitizeDownloadName('', 'docx')).toBe('document.docx');
  });

  it('removes control characters', () => {
    expect(sanitizeDownloadName('a\u0000b\u001fc\u007fd\u0085e.qmd', 'docx')).toBe('abcde.docx');
    expect(sanitizeDownloadName('line\nbreak.qmd', 'docx')).toBe('linebreak.docx');
  });

  it('removes bidi controls and invisible characters that can disguise an extension', () => {
    expect(sanitizeDownloadName('evil‮xcod.qmd', 'docx')).toBe('evilxcod.docx');
    expect(sanitizeDownloadName('a⁦b⁩c‎d؜e​f﻿g.qmd', 'docx')).toBe('abcdefg.docx');
  });

  it('replaces Windows-reserved characters and avoids device names, leading dots and trailing dots', () => {
    expect(sanitizeDownloadName('a:b*c?d"e<f>g|h.qmd', 'docx')).toBe('a_b_c_d_e_f_g_h.docx');
    expect(sanitizeDownloadName('con.qmd', 'docx')).toBe('document.docx');
    expect(sanitizeDownloadName('.hidden.qmd', 'docx')).toBe('hidden.docx');
    expect(sanitizeDownloadName('name . .qmd', 'docx')).toBe('name.docx');
  });

  it('truncates long stems by code point and keeps the extension', () => {
    const name = sanitizeDownloadName(`${'😀'.repeat(300)}.qmd`, 'docx');
    expect(Array.from(name.replace(/\.docx$/, '')).length).toBe(120);
    expect(name.endsWith('.docx')).toBe(true);
  });

  it('sanitizes the extension too', () => {
    expect(sanitizeDownloadName('a.qmd', '../docx')).toBe('a.docx');
    expect(sanitizeDownloadName('a.qmd', '')).toBe('a.out');
  });
});
