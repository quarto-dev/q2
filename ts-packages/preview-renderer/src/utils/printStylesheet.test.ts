/**
 * Tests for `injectPrintStylesheet` (issue #315, bd-vhdknrvl).
 *
 * The printable document render (`render_printable` → HTML pipeline)
 * ships only the theme's `@media print` rules; q2's HTML template does
 * not inline the pandoc default print partial (orphans/widows, heading
 * break-avoidance). We append a small, conservative print stylesheet so
 * the standalone document paginates cleanly. Applied to documents only
 * — reveal decks carry their own precise print CSS.
 */

import { describe, it, expect } from 'vitest';
import { injectPrintStylesheet } from './printStylesheet';

describe('injectPrintStylesheet', () => {
  it('adds a print @media block before </head>', () => {
    const html = '<html><head><title>x</title></head><body>y</body></html>';
    const out = injectPrintStylesheet(html);
    const headClose = out.indexOf('</head>');
    const media = out.indexOf('@media print');
    expect(media).toBeGreaterThan(-1);
    expect(media).toBeLessThan(headClose);
    expect(out).toContain('data-q2-print');
  });

  it('includes heading break-avoidance and orphans/widows', () => {
    const out = injectPrintStylesheet('<head></head>');
    expect(out).toMatch(/h1[^}]*break-after/i);
    expect(out).toMatch(/orphans/i);
    expect(out).toMatch(/widows/i);
  });

  it('falls back to prepending when there is no </head>', () => {
    const html = '<body>only body</body>';
    const out = injectPrintStylesheet(html);
    expect(out).toContain('@media print');
    expect(out).toContain('only body');
    expect(out.indexOf('@media print')).toBeLessThan(out.indexOf('only body'));
  });
});
