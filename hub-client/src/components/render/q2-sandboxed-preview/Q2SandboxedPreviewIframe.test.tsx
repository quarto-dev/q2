/**
 * @vitest-environment jsdom
 */
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, it, expect, vi, beforeEach } from 'vitest';
import { Q2SandboxedPreviewIframe } from './Q2SandboxedPreviewIframe';

// The parent reads compiled theme CSS out of the WASM VFS to ship it to
// the cross-origin iframe as text (blob URLs are origin-scoped and
// unreachable from the sandbox — see the port plan).
vi.mock('@quarto/preview-runtime', () => ({
  vfsReadFile: vi.fn((path: string) => ({
    success: true,
    content: `/* css for ${path} */`,
  })),
}));

// The service-worker asset proxy resolves through the parent's WASM VFS.
// Paths starting with 'orphan' miss at the VFS root (exercising the
// currentFilePath-relative fallback for non-manifest fetches).
vi.mock('wasm-quarto-hub-client', () => ({
  vfs_read_file: vi.fn((path: string) =>
    JSON.stringify(
      path.startsWith('orphan')
        ? { success: false, error: 'not found' }
        : { success: true, content: `text of ${path}` },
    ),
  ),
  vfs_read_binary_file: vi.fn((path: string) =>
    JSON.stringify(
      path.startsWith('orphan')
        ? { success: false, error: 'not found' }
        : { success: true, content: `base64-of-${path}` },
    ),
  ),
}));

function renderIframe(props: Partial<Parameters<typeof Q2SandboxedPreviewIframe>[0]> = {}) {
  render(
    <Q2SandboxedPreviewIframe
      astJson='{"blocks":[]}'
      currentFilePath="docs/page.qmd"
      setAst={props.setAst ?? (() => {})}
      {...props}
    />,
  );
  const iframe = screen.getByTitle('q2-sandboxed-preview Renderer') as HTMLIFrameElement;
  const postMessage = vi.spyOn(iframe.contentWindow!, 'postMessage');
  return { iframe, postMessage };
}

/** Post a message as the iframe would — with `source` set to its window. */
function postFromIframe(iframe: HTMLIFrameElement, data: unknown) {
  window.dispatchEvent(
    new MessageEvent('message', { data, source: iframe.contentWindow }),
  );
}

function signalIframeReady(iframe: HTMLIFrameElement) {
  postFromIframe(iframe, { type: 'IFRAME_READY' });
}

describe('Q2SandboxedPreviewIframe', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('delegates clipboard-write permission to the cross-origin frame', () => {
    // codeCopy.ts (unmodified q2-preview code running inside the iframe)
    // calls navigator.clipboard.writeText; a cross-origin frame only gets
    // that permission when the embedding iframe delegates it.
    const { iframe } = renderIframe();
    expect(iframe.getAttribute('allow')).toContain('clipboard-write');
  });

  it('pins a light canvas behind the transparent sandboxed document', () => {
    // The sandboxed document paints no background; the editor's preview pane
    // follows the chrome theme (dark in dark mode). The iframe's own
    // background must stay light or the document's default dark text becomes
    // unreadable in dark mode.
    const { iframe } = renderIframe();
    expect(iframe.style.background).toBe('rgb(255, 255, 255)');
  });

  it('ships currentFilePath alongside astJson in the UPDATE_AST payload', async () => {
    // The real renderer resolves relative asset paths and source slices
    // against the active document path; astJson alone is not enough.
    const { iframe, postMessage } = renderIframe();
    signalIframeReady(iframe);

    await waitFor(() => {
      const updateAst = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
      );
      expect(updateAst).toBeDefined();
      const payload = (updateAst![0] as { payload: Record<string, unknown> }).payload;
      expect(payload.astJson).toBe('{"blocks":[]}');
      expect(payload.currentFilePath).toBe('docs/page.qmd');
    });
  });

  it('ships a proxy-URL asset manifest in the UPDATE_AST payload', async () => {
    const astJson = JSON.stringify({
      blocks: [
        { t: 'Para', c: [{ t: 'Image', c: [['', [], []], [], ['images/pic.png', '']] }] },
      ],
    });
    const { iframe, postMessage } = renderIframe({ astJson, currentFilePath: '/project/sub/doc.qmd' });
    signalIframeReady(iframe);

    await waitFor(() => {
      const updateAst = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
      );
      expect(updateAst).toBeDefined();
      const payload = (updateAst![0] as { payload: Record<string, unknown> }).payload;
      expect(payload.assetManifest).toEqual({
        'images/pic.png': 'project/sub/images/pic.png',
      });
    });
  });

  it('answers a url request with a url_response carrying the same request id', async () => {
    const { iframe, postMessage } = renderIframe();
    signalIframeReady(iframe);

    postFromIframe(iframe, { type: 'url', id: 'req-42', path: 'project/sub/images/pic.png' });

    await waitFor(() => {
      const response = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'url_response',
      );
      expect(response).toBeDefined();
      const msg = response![0] as Record<string, unknown>;
      expect(msg.id).toBe('req-42');
      expect(msg.success).toBe(true);
      expect(msg.isBinary).toBe(true);
      expect(msg.content).toBe('base64-of-project/sub/images/pic.png');
    });
  });

  it('retries a missed url request against the current document directory (non-manifest fetch fallback, bd-00bgt5cy)', async () => {
    // An <img> from raw HTML in docs/page.qmd resolves its URL against
    // the page base, so the SW forwards 'orphan.png' — which only exists
    // at docs/orphan.png on the VFS.
    const { iframe, postMessage } = renderIframe();
    signalIframeReady(iframe);

    postFromIframe(iframe, { type: 'url', id: 'req-77', path: 'orphan.png' });

    await waitFor(() => {
      const response = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'url_response',
      );
      expect(response).toBeDefined();
      const msg = response![0] as Record<string, unknown>;
      expect(msg.id).toBe('req-77');
      expect(msg.success).toBe(true);
      expect(msg.content).toBe('base64-of-docs/orphan.png');
    });
  });

  it('posts UPDATE_THEME with the CSS text when themeFingerprint is a string', async () => {
    const { iframe, postMessage } = renderIframe({ themeFingerprint: 'fp-1' });
    signalIframeReady(iframe);

    await waitFor(() => {
      const updateTheme = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_THEME',
      );
      expect(updateTheme).toBeDefined();
      const msg = updateTheme![0] as { cssText: string | null; fingerprint: string | null };
      expect(msg.cssText).toContain('/* css for ');
      expect(msg.fingerprint).toBe('fp-1');
    });
  });

  it('posts an explicit UPDATE_THEME clear when themeFingerprint is null', async () => {
    const { iframe, postMessage } = renderIframe({ themeFingerprint: null });
    signalIframeReady(iframe);

    await waitFor(() => {
      const updateTheme = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_THEME',
      );
      expect(updateTheme).toBeDefined();
      const msg = updateTheme![0] as { cssText: string | null; fingerprint: string | null };
      expect(msg.cssText).toBeNull();
      expect(msg.fingerprint).toBeNull();
    });
  });

  it('exposes a scroll handle: scrollToLine posts SCROLL_TO_LINE, getScrollRatio returns the last reported ratio', async () => {
    const handleRef: { current: { scrollToLine: (l: number) => void; getScrollRatio: () => number | null } | null } = { current: null };
    const { iframe, postMessage } = renderIframe({ scrollHandleRef: handleRef });
    signalIframeReady(iframe);

    await waitFor(() => expect(handleRef.current).not.toBeNull());
    expect(handleRef.current!.getScrollRatio()).toBeNull();

    handleRef.current!.scrollToLine(42);
    expect(
      postMessage.mock.calls.some(
        ([msg]) => (msg as { type?: string; line?: number }).type === 'SCROLL_TO_LINE'
          && (msg as { line?: number }).line === 42,
      ),
    ).toBe(true);

    postFromIframe(iframe, { type: 'PREVIEW_SCROLLED', ratio: 0.37 });
    await waitFor(() => expect(handleRef.current!.getScrollRatio()).toBe(0.37));
  });

  it('forwards PREVIEW_SCROLLED to onScroll and CLICK_AT_LINE to onClickAtLine with hostY = iframeY + iframe top', async () => {
    const onScroll = vi.fn();
    const onClickAtLine = vi.fn();
    const { iframe } = renderIframe({ onScroll, onClickAtLine });
    vi.spyOn(iframe, 'getBoundingClientRect').mockReturnValue({ top: 100 } as DOMRect);
    signalIframeReady(iframe);

    postFromIframe(iframe, { type: 'PREVIEW_SCROLLED', ratio: 0.5 });
    await waitFor(() => expect(onScroll).toHaveBeenCalled());

    postFromIframe(iframe, { type: 'CLICK_AT_LINE', line: 7, iframeY: 23 });
    await waitFor(() => expect(onClickAtLine).toHaveBeenCalledWith(7, 123));
  });

  it('forwards NAVIGATE_TO_DOCUMENT, SET_AST, SLIDE_CHANGED, and AST_RENDERED to their callbacks', async () => {
    const onNavigateToDocument = vi.fn();
    const setAst = vi.fn();
    const onSlideChange = vi.fn();
    const onAstRendered = vi.fn();
    const { iframe } = renderIframe({ onNavigateToDocument, setAst, onSlideChange, onAstRendered });
    signalIframeReady(iframe);

    postFromIframe(iframe, { type: 'NAVIGATE_TO_DOCUMENT', path: 'other.qmd', anchor: 'sec' });
    postFromIframe(iframe, { type: 'SET_AST', ast: { blocks: [] } });
    postFromIframe(iframe, { type: 'SLIDE_CHANGED', index: 3 });
    postFromIframe(iframe, { type: 'AST_RENDERED' });

    await waitFor(() => {
      expect(onNavigateToDocument).toHaveBeenCalledWith('other.qmd', 'sec');
      expect(setAst).toHaveBeenCalledWith({ blocks: [] });
      expect(onSlideChange).toHaveBeenCalledWith(3);
      expect(onAstRendered).toHaveBeenCalled();
    });
  });

  it('posts LOAD_CUSTOM_COMPONENTS and a deduped SET_SLIDE when ready', async () => {
    const { iframe, postMessage } = renderIframe({
      customComponentsCode: { 'comp.tsx': 'export default 1' },
      currentSlideIndex: 2,
    });
    signalIframeReady(iframe);

    await waitFor(() => {
      expect(
        postMessage.mock.calls.some(
          ([msg]) => (msg as { type?: string }).type === 'LOAD_CUSTOM_COMPONENTS',
        ),
      ).toBe(true);
      expect(
        postMessage.mock.calls.filter(
          ([msg]) => (msg as { type?: string }).type === 'SET_SLIDE',
        ),
      ).toHaveLength(1);
    });

    // An in-deck SLIDE_CHANGED echoing back as the same index must not re-post.
    postFromIframe(iframe, { type: 'SLIDE_CHANGED', index: 4 });
    await waitFor(() => {
      expect(
        postMessage.mock.calls.filter(
          ([msg]) => (msg as { type?: string }).type === 'SET_SLIDE',
        ),
      ).toHaveLength(1);
    });
  });

  it('ships the full feature payload in UPDATE_AST', async () => {
    const { iframe, postMessage } = renderIframe({
      projectFilePaths: ['docs/page.qmd', 'other.qmd'],
      pendingAnchor: 'sec-2',
      pendingAnchorEpoch: 3,
      renderedContent: '# src',
      untransformedAstJson: '{"blocks":[],"pre":true}',
      currentActor: 'actor-1',
      commentsMode: 'hide',
      unlockNestingCursor: true,
      richText: true,
      nestedEditBuffers: { k: 'v' },
    });
    signalIframeReady(iframe);

    await waitFor(() => {
      const updateAst = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
      );
      expect(updateAst).toBeDefined();
      const payload = (updateAst![0] as { payload: Record<string, unknown> }).payload;
      expect(payload).toMatchObject({
        currentFilePath: 'docs/page.qmd',
        projectFilePaths: ['docs/page.qmd', 'other.qmd'],
        pendingAnchor: 'sec-2',
        pendingAnchorEpoch: 3,
        renderedContent: '# src',
        untransformedAstJson: '{"blocks":[],"pre":true}',
        currentActor: 'actor-1',
        commentsMode: 'hide',
        unlockNestingCursor: true,
        richText: true,
        nestedEditBuffers: { k: 'v' },
      });
    });
  });

  it('ignores messages whose source is not the sandboxed iframe window', async () => {
    // Any window can postMessage the parent; only the embedded frame's
    // own contentWindow may drive this component. A forged SET_AST from
    // another frame must not reach the document state.
    const setAst = vi.fn();
    const { iframe, postMessage } = renderIframe({ setAst });

    // No source (jsdom default: null) — must not mark the iframe ready...
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'IFRAME_READY' } }));
    // ...and must not forward a forged SET_AST.
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'SET_AST', ast: { forged: true } } }));

    await new Promise((r) => setTimeout(r, 50));
    expect(setAst).not.toHaveBeenCalled();
    expect(
      postMessage.mock.calls.some(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
      ),
    ).toBe(false);

    // The genuine frame still works.
    signalIframeReady(iframe);
    await waitFor(() => {
      expect(
        postMessage.mock.calls.some(
          ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
        ),
      ).toBe(true);
    });
  });

  it('skips the UPDATE_THEME post entirely when themeFingerprint is undefined', async () => {
    // Transient render failures must not strip the iframe's last-good
    // styling — same three-way semantics as Q2PreviewIframe.
    const { iframe, postMessage } = renderIframe({ themeFingerprint: undefined });
    signalIframeReady(iframe);

    // Wait for the UPDATE_AST that accompanies readiness, then confirm no
    // UPDATE_THEME rode along.
    await waitFor(() => {
      expect(
        postMessage.mock.calls.some(
          ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
        ),
      ).toBe(true);
    });
    expect(
      postMessage.mock.calls.some(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_THEME',
      ),
    ).toBe(false);
  });
});
