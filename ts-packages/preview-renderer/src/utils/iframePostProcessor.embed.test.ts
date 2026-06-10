// @vitest-environment jsdom
/**
 * Tests for `postProcessIframe`'s embedded-resource `<iframe>` inlining
 * (bd-kjrpya2d): `.embed-example-iframe` decks emitted with an
 * artifact-rooted `src` are inlined from the VFS via `srcdoc`, with a
 * fallback to the VFS *source* path when the artifact path misses.
 *
 * Runs in jsdom. We mock `@quarto/preview-runtime` (the module the
 * post-processor imports `vfsReadFile` from) so we control VFS reads.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@quarto/preview-runtime', () => ({
  vfsReadFile: vi.fn(() => ({ success: false })),
  vfsReadBinaryFile: vi.fn(() => ({ success: false })),
}));

import { vfsReadFile } from '@quarto/preview-runtime';
import { postProcessIframe } from './iframePostProcessor';

const mockRead = vi.mocked(vfsReadFile);

/** Build a preview iframe whose body contains `bodyHtml`. */
function makeIframe(bodyHtml: string): HTMLIFrameElement {
  const iframe = document.createElement('iframe');
  document.body.appendChild(iframe);
  iframe.contentDocument!.open();
  iframe.contentDocument!.write(`<!doctype html><html><body>${bodyHtml}</body></html>`);
  iframe.contentDocument!.close();
  return iframe;
}

describe('postProcessIframe: embedded-example iframe inlining (bd-kjrpya2d)', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockRead.mockReset();
  });

  it('inlines from the VFS source path when the artifact path misses', () => {
    const DECK = '<!doctype html><html><body><div class="reveal">deck</div></body></html>';
    mockRead.mockImplementation((path: string) =>
      // Artifact path misses; the deck lives at its source path.
      path === 'examples/p/03/slides.html'
        ? { success: true, content: DECK }
        : { success: false },
    );

    const iframe = makeIframe(
      `<iframe id="f" class="embed-example-iframe" src="/.quarto/project-artifacts/examples/p/03/slides.html"></iframe>`,
    );
    postProcessIframe(iframe, { currentFilePath: 'presentations/revealjs/index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('src')).toBeNull();
    expect(inner.getAttribute('srcdoc')).toBe(DECK);
    // Proves the fallback: artifact path tried first, then the source path.
    expect(mockRead).toHaveBeenCalledWith('/.quarto/project-artifacts/examples/p/03/slides.html');
    expect(mockRead).toHaveBeenCalledWith('examples/p/03/slides.html');
    iframe.remove();
  });

  it('prefers the artifact path when it hits (no source fallback needed)', () => {
    const ARTIFACT = '<html>artifact</html>';
    mockRead.mockImplementation((path: string) =>
      path === '/.quarto/project-artifacts/gen.html'
        ? { success: true, content: ARTIFACT }
        : { success: false },
    );

    const iframe = makeIframe(`<iframe id="f" src="/.quarto/project-artifacts/gen.html"></iframe>`);
    postProcessIframe(iframe, { currentFilePath: 'index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('srcdoc')).toBe(ARTIFACT);
    // Source-path fallback never consulted.
    expect(mockRead).not.toHaveBeenCalledWith('gen.html');
    iframe.remove();
  });

  it('inlines a page-relative `/examples/...` src from the VFS source path', () => {
    // The embed feature (bd-z1smhvuo, commit 867aa7c1 "page-relative
    // iframe src") emits the deck `src` as a site-root-relative path,
    // NOT artifact-rooted. In preview there is no server to answer
    // `/examples/...`, so we strip the leading slash and read the deck
    // from its VFS source path (synced in by bd-kjrpya2d part 2).
    const DECK = '<!doctype html><html><body><div class="reveal">deck</div></body></html>';
    mockRead.mockImplementation((path: string) =>
      path === 'examples/presentations/03-fragments/slides.html'
        ? { success: true, content: DECK }
        : { success: false },
    );

    const iframe = makeIframe(
      `<iframe id="f" class="embed-example-iframe" src="/examples/presentations/03-fragments/slides.html"></iframe>`,
    );
    postProcessIframe(iframe, { currentFilePath: 'presentations/revealjs/index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('src')).toBeNull();
    expect(inner.getAttribute('srcdoc')).toBe(DECK);
    // Literal path tried first (misses), then leading-slash-stripped source.
    expect(mockRead).toHaveBeenCalledWith('/examples/presentations/03-fragments/slides.html');
    expect(mockRead).toHaveBeenCalledWith('examples/presentations/03-fragments/slides.html');
    iframe.remove();
  });

  it('leaves a page-relative src untouched when it is not in the VFS', () => {
    // Read-success is the safety gate: a root-relative iframe that does
    // NOT resolve in the VFS genuinely needs a network load and must
    // not be clobbered.
    mockRead.mockReturnValue({ success: false });
    const iframe = makeIframe(`<iframe id="f" src="/not/in/vfs.html"></iframe>`);
    postProcessIframe(iframe, { currentFilePath: 'index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('srcdoc')).toBeNull();
    expect(inner.getAttribute('src')).toBe('/not/in/vfs.html');
    iframe.remove();
  });

  it('leaves an external (http/https) iframe src untouched', () => {
    mockRead.mockReturnValue({ success: false });
    const iframe = makeIframe(`<iframe id="f" src="https://example.com/x.html"></iframe>`);
    postProcessIframe(iframe, { currentFilePath: 'index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('src')).toBe('https://example.com/x.html');
    expect(inner.getAttribute('srcdoc')).toBeNull();
    // A non-root-relative src is never read from the VFS at all.
    expect(mockRead).not.toHaveBeenCalledWith('https://example.com/x.html');
    iframe.remove();
  });

  it('leaves the iframe alone when neither artifact nor source path resolves', () => {
    mockRead.mockReturnValue({ success: false });
    const iframe = makeIframe(
      `<iframe id="f" src="/.quarto/project-artifacts/examples/missing/slides.html"></iframe>`,
    );
    postProcessIframe(iframe, { currentFilePath: 'index.qmd' });

    const inner = iframe.contentDocument!.querySelector('#f') as HTMLIFrameElement;
    expect(inner.getAttribute('srcdoc')).toBeNull();
    expect(inner.getAttribute('src')).toBe(
      '/.quarto/project-artifacts/examples/missing/slides.html',
    );
    iframe.remove();
  });
});
