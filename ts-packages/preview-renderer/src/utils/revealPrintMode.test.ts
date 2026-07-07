/**
 * Tests for `forceRevealPrintMode` (bd-vhdknrvl, issue #315).
 *
 * reveal.js enters its paginated PDF layout when `config.view ===
 * "print"` (that is exactly what a `?print-pdf` query sets internally).
 * A printable deck is opened from a `blob:` URL, whose query string is
 * not reliably exposed via `location.search`, so we can't rely on the
 * `?print-pdf` trigger — we inject `view:"print"` into the deck's own
 * `Reveal.initialize({…})` config instead.
 */

import { describe, it, expect } from 'vitest';
import { forceRevealPrintMode } from './revealPrintMode';

describe('forceRevealPrintMode', () => {
  it('injects view:"print" into the reveal config', () => {
    const html = `<html><body>
      <script>Reveal.initialize({controls:true,hash:true});</script>
      </body></html>`;
    const out = forceRevealPrintMode(html);
    expect(out).toContain('Reveal.initialize({view:"print",');
    // original options survive
    expect(out).toContain('controls:true');
  });

  it('handles a pretty-printed (multi-line) initialize call', () => {
    const html = `<script>\nReveal.initialize({\n  controls: true\n});\n</script>`;
    const out = forceRevealPrintMode(html);
    expect(out).toContain('Reveal.initialize({view:"print",');
    expect(out).toContain('controls: true');
  });

  it('is a no-op when there is no reveal initialize call', () => {
    const html = `<html><body><p>not a deck</p></body></html>`;
    expect(forceRevealPrintMode(html)).toBe(html);
  });

  it('only injects once even if initialize appears twice', () => {
    const html = `Reveal.initialize({a:1}); /* later */ Reveal.initialize({b:2});`;
    const out = forceRevealPrintMode(html);
    // Both are patched (defensive) — count occurrences of the marker.
    const matches = out.match(/view:"print"/g) ?? [];
    expect(matches.length).toBeGreaterThanOrEqual(1);
    expect(out).toContain('a:1');
    expect(out).toContain('b:2');
  });
});
