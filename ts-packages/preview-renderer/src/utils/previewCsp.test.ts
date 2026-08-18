/**
 * Unit tests for `injectPreviewCsp` (quarto-dev/q2#128, bd-sxx1az83).
 *
 * The injection-point contract is the highest-risk part of the Safari
 * preview-link fix: post-fix the injected CSP meta is the *only* script
 * mitigation in the preview iframe (the sandbox gains `allow-scripts`),
 * so an injection miss is a same-origin script-execution escape. These
 * tests pin the contract at string level; parse-level checks ("the meta
 * lands as the first element in document order") live in
 * `MorphIframe.integration.test.tsx` (jsdom) and the behavioral
 * no-scripts guarantee in `hub-client/e2e/preview-script-blocking.spec.ts`
 * (real browsers — jsdom enforces neither sandbox nor CSP).
 */

import { describe, it, expect } from 'vitest';
import { injectPreviewCsp, PREVIEW_CSP_META } from './previewCsp';

describe('injectPreviewCsp', () => {
  it('injects immediately after the DOCTYPE when present', () => {
    const html = '<!DOCTYPE html>\n<html><head></head><body>hi</body></html>';
    const out = injectPreviewCsp(html);
    expect(out.startsWith('<!DOCTYPE html>' + PREVIEW_CSP_META)).toBe(true);
  });

  it('matches the DOCTYPE case-insensitively', () => {
    const out = injectPreviewCsp('<!doctype html><html><body>x</body></html>');
    expect(out.startsWith('<!doctype html>' + PREVIEW_CSP_META)).toBe(true);
  });

  it('keeps the DOCTYPE first when leading whitespace/comments precede it', () => {
    // Per HTML5's initial insertion mode, whitespace and comments before
    // the DOCTYPE are ignored, so this DOCTYPE still takes effect — but
    // only if we don't insert anything before it.
    const html = '  <!-- banner -->\n<!DOCTYPE html><html><body>x</body></html>';
    const out = injectPreviewCsp(html);
    const doctypeEnd = out.indexOf('<!DOCTYPE html>') + '<!DOCTYPE html>'.length;
    expect(out.indexOf(PREVIEW_CSP_META)).toBe(doctypeEnd);
  });

  it('inserts at byte 0 when there is no DOCTYPE', () => {
    const out = injectPreviewCsp('<html><body>x</body></html>');
    expect(out.startsWith(PREVIEW_CSP_META)).toBe(true);
  });

  // Not Quirks Mode insurance: the HTML spec forces no-quirks for iframe
  // srcdoc documents regardless of what precedes the DOCTYPE. DOCTYPE-first
  // keeps the payload valid if it's ever served as a standalone document.
  it('never inserts before the DOCTYPE', () => {
    const out = injectPreviewCsp('<!DOCTYPE html><html><body>x</body></html>');
    expect(out.indexOf('<!DOCTYPE html')).toBeGreaterThanOrEqual(0);
    expect(out.indexOf('<!DOCTYPE html')).toBeLessThan(out.indexOf(PREVIEW_CSP_META));
  });

  it('is idempotent', () => {
    const once = injectPreviewCsp('<!DOCTYPE html><html><body>x</body></html>');
    expect(injectPreviewCsp(once)).toBe(once);
    // Also for the no-DOCTYPE (byte 0) path.
    const onceNoDoctype = injectPreviewCsp('<p>fragment</p>');
    expect(injectPreviewCsp(onceNoDoctype)).toBe(onceNoDoctype);
  });

  it('leaves an existing user meta CSP intact (it can only restrict further)', () => {
    const userMeta =
      '<meta http-equiv="Content-Security-Policy" content="default-src \'self\'">';
    const html = `<!DOCTYPE html><html><head>${userMeta}</head><body>x</body></html>`;
    const out = injectPreviewCsp(html);
    expect(out).toContain(userMeta);
    // Ours comes first in document order.
    expect(out.indexOf(PREVIEW_CSP_META)).toBeLessThan(out.indexOf(userMeta));
  });

  it('does not treat the meta string inside user content as already-injected', () => {
    // A code sample in the document body quoting our meta must not
    // suppress injection at the top — that would make the idempotency
    // check a spoofable fail-open hole.
    const html = `<!DOCTYPE html><html><body><code>${PREVIEW_CSP_META}</code></body></html>`;
    const out = injectPreviewCsp(html);
    expect(out.startsWith('<!DOCTYPE html>' + PREVIEW_CSP_META)).toBe(true);
  });

  it('puts the meta before any <script> in the payload', () => {
    const html =
      '<!DOCTYPE html><html><head><script src="libs/x.js"></script></head><body>x</body></html>';
    const out = injectPreviewCsp(html);
    expect(out.indexOf(PREVIEW_CSP_META)).toBeLessThan(out.indexOf('<script'));
  });

  // A "first child of <head>" string search is spoofable by markup that
  // merely *looks* like a head in a naive scan. The contract instead:
  // after the DOCTYPE if present, else byte 0 — so in every case the meta
  // is the first element the parser sees.
  describe('adversarial head-like markup (meta must be first in document order)', () => {
    const cases: Array<[string, string]> = [
      [
        'head-like markup inside a comment',
        '<!-- <head><script>alert(1)</script></head> --><html><body>x</body></html>',
      ],
      [
        'head-like markup inside <title>',
        '<html><head><title><head><script></title></head><body>x</body></html>',
      ],
      [
        'head-like markup inside <script>',
        '<html><head><script>var s = "<head>";</script></head><body>x</body></html>',
      ],
      [
        'head-like markup inside <textarea>',
        '<html><body><textarea><head></head></textarea></body></html>',
      ],
      ['uppercase <HEAD>', '<HTML><HEAD></HEAD><BODY>x</BODY></HTML>'],
      [
        'no-<head> fragment starting with <script>',
        '<script>alert(1)</script><p>x</p>',
      ],
    ];
    for (const [name, html] of cases) {
      it(name, () => {
        const out = injectPreviewCsp(html);
        // None of these payloads have a DOCTYPE, so the meta must be at
        // byte 0 — ahead of every tag, including the decoys.
        expect(out.startsWith(PREVIEW_CSP_META)).toBe(true);
      });
    }
  });
});
