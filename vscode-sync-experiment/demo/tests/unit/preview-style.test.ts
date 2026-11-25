import { describe, expect, it } from 'vitest';
import { judgePreviewStyle, type PreviewFacts } from '../../src/preview-style';

// What a themed (cosmo) preview reads out in the browser, and what the same page reads with no CSS at all.
const styled: PreviewFacts = {
  hasContent: true,
  bodyFontFamily: '"Source Sans Pro", -apple-system, "Segoe UI", Roboto, sans-serif',
  bodyFontSizePx: 16,
  bodyMarginPx: 0,
  h1FontSizePx: 40,
  sheets: [{ href: 'blob:x', rules: 1200 }],
};
const bare: PreviewFacts = {
  hasContent: true,
  bodyFontFamily: '"Times New Roman"',
  bodyFontSizePx: 16,
  bodyMarginPx: 8,
  h1FontSizePx: 32,
  sheets: [],
};

describe('judgePreviewStyle: is a rendered preview styled?', () => {
  it('accepts a themed page', () => {
    expect(judgePreviewStyle(styled)).toEqual({ styled: true, reasons: [] });
  });

  it('rejects a page with no stylesheet and the browser defaults, saying why', () => {
    const v = judgePreviewStyle(bare);
    expect(v.styled).toBe(false);
    expect(v.reasons).toEqual(expect.arrayContaining(['no-stylesheet-rules', 'default-font']));
  });

  it('rejects a page whose stylesheets are present but empty (a failed or half-loaded theme)', () => {
    const v = judgePreviewStyle({ ...bare, sheets: [{ href: 'blob:x', rules: 0 }, { href: 'a.css', rules: null }] });
    expect(v.styled).toBe(false);
    expect(v.reasons).toContain('no-stylesheet-rules');
  });

  it('rejects when sheets have rules yet the text still has the default font and the default heading size', () => {
    const v = judgePreviewStyle({ ...bare, sheets: [{ href: 'x', rules: 3 }] });
    expect(v.styled).toBe(false);
    expect(v.reasons).toEqual(['default-font']);
  });

  it('does not judge a preview that has no content yet (nothing mounted is not unstyled)', () => {
    expect(judgePreviewStyle({ ...bare, hasContent: false })).toEqual({ styled: true, reasons: [] });
  });
});

import { worthRecording } from '../../src/preview-probe';

describe('worthRecording: which network traffic is kept as evidence', () => {
  it('keeps failures, non-2xx answers, and every stylesheet', () => {
    expect(worthRecording(null, 'fetch', 'net::ERR_ABORTED')).toBe(true);
    expect(worthRecording(404, 'fetch')).toBe(true);
    expect(worthRecording(500, 'document')).toBe(true);
    expect(worthRecording(200, 'stylesheet')).toBe(true);
  });
  it('drops ordinary successes and cache hits', () => {
    expect(worthRecording(200, 'script')).toBe(false);
    expect(worthRecording(304, 'fetch')).toBe(false);
  });
});
