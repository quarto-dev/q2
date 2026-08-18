/**
 * Integration tests for MorphIframe's Safari link-interception fix
 * (quarto-dev/q2#128, bd-sxx1az83).
 *
 * Pins the mechanism, which is all jsdom can see (jsdom enforces neither
 * the sandbox attribute nor CSP):
 *
 *  1. The iframe sandbox carries `allow-scripts` alongside
 *     `allow-same-origin`/`allow-popups`, so WebKit runs parent-attached
 *     event listeners on the frame (WebKit bug 218086 blocks them
 *     otherwise — that was the Safari link/scroll-sync failure).
 *  2. Every payload the component emits (initial `srcdoc` and every
 *     morphdom update) carries the CSP meta as the first element in
 *     document order, so no script inside the preview document executes
 *     even with `allow-scripts` set.
 *
 * Behavioral coverage (link navigation in WebKit, no script execution)
 * lives in `hub-client/e2e/preview-link-navigation.spec.ts` and
 * `preview-script-blocking.spec.ts` — real browsers only.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, act } from '@testing-library/react';
import { createRef } from 'react';

// Stub the WASM-backed VFS reads; postProcessIframe's resource-rewriting
// passes call them but they're irrelevant to these assertions.
vi.mock('@quarto/preview-runtime', () => ({
  vfsReadFile: vi.fn().mockReturnValue({ success: false }),
  vfsReadBinaryFile: vi.fn().mockReturnValue({ success: false }),
}));

import MorphIframe, { type MorphIframeHandle } from './MorphIframe';

// The expected meta tag, specified independently of the implementation's
// own constant so this test fails if either drifts.
const CSP_META = '<meta http-equiv="Content-Security-Policy" content="script-src \'none\'">';

function previewElement(html: string) {
  return (
    <MorphIframe
      ref={createRef<MorphIframeHandle>()}
      html={html}
      currentFilePath="index.qmd"
      projectFilePaths={['index.qmd', 'other.qmd']}
      qmdContent=""
      onNavigateToDocument={() => {}}
    />
  );
}

describe('MorphIframe sandbox + CSP (q2#128)', () => {
  it('sets sandbox with allow-scripts, allow-same-origin, and allow-popups', () => {
    const { container } = render(previewElement('<!DOCTYPE html><html><body>hi</body></html>'));
    const iframe = container.querySelector('iframe');
    expect(iframe).not.toBeNull();
    const tokens = (iframe!.getAttribute('sandbox') ?? '').split(/\s+/).filter(Boolean);
    expect(tokens).toContain('allow-scripts');
    expect(tokens).toContain('allow-same-origin');
    expect(tokens).toContain('allow-popups');
  });

  it('emits a srcdoc payload whose first element is the CSP meta, before any <script>', () => {
    const { container } = render(
      previewElement(
        '<!DOCTYPE html><html><head><script src="libs/x.js"></script></head><body>hi</body></html>',
      ),
    );
    const iframe = container.querySelector('iframe')!;
    const srcdoc = iframe.getAttribute('srcdoc') ?? '';
    // Immediately after the DOCTYPE — never before it (Quirks Mode).
    expect(srcdoc.startsWith('<!DOCTYPE html>' + CSP_META)).toBe(true);
    expect(srcdoc.indexOf(CSP_META)).toBeLessThan(srcdoc.indexOf('<script'));
  });

  it('keeps the CSP meta in <head> across a morphdom content update', () => {
    const html1 = '<!DOCTYPE html><html><head><title>one</title></head><body>first</body></html>';
    const html2 =
      '<!DOCTYPE html><html><head><title>two</title><script src="libs/y.js"></script></head><body>second</body></html>';

    const utils = render(previewElement(html1));
    const iframe = utils.container.querySelector('iframe')!;

    // jsdom neither parses `srcdoc` into the contentDocument nor fires
    // its load event. Simulate the settled document exactly as the
    // browser would have parsed the component's emitted payload, then
    // fire the load the component waits for.
    const settled = iframe.getAttribute('srcdoc')!;
    act(() => {
      iframe.contentDocument!.open();
      iframe.contentDocument!.write(settled);
      iframe.contentDocument!.close();
      iframe.dispatchEvent(new Event('load'));
    });

    // Sanity: the settled document carries the meta in <head>.
    expect(
      iframe.contentDocument!.head.querySelector('meta[http-equiv="Content-Security-Policy"]'),
    ).not.toBeNull();

    // Morphdom update path (second payload).
    act(() => {
      utils.rerender(previewElement(html2));
    });

    const doc = iframe.contentDocument!;
    const meta = doc.head.querySelector('meta[http-equiv="Content-Security-Policy"]');
    expect(meta).not.toBeNull();
    expect(meta!.getAttribute('content')).toBe("script-src 'none'");
    // The meta still precedes every script in document order.
    expect(doc.head.innerHTML.indexOf('Content-Security-Policy')).toBeLessThan(
      doc.head.innerHTML.indexOf('<script'),
    );
    // And the morph actually applied the new content.
    expect(doc.body.textContent).toContain('second');
  });
});
