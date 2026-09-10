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
vi.mock('wasm-quarto-hub-client', () => ({
  vfs_read_file: vi.fn((path: string) =>
    JSON.stringify({ success: true, content: `text of ${path}` }),
  ),
  vfs_read_binary_file: vi.fn((path: string) =>
    JSON.stringify({ success: true, content: `base64-of-${path}` }),
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

function signalIframeReady() {
  window.dispatchEvent(new MessageEvent('message', { data: { type: 'IFRAME_READY' } }));
}

describe('Q2SandboxedPreviewIframe', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
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
    const { postMessage } = renderIframe();
    signalIframeReady();

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
    const { postMessage } = renderIframe({ astJson, currentFilePath: '/project/sub/doc.qmd' });
    signalIframeReady();

    await waitFor(() => {
      const updateAst = postMessage.mock.calls.find(
        ([msg]) => (msg as { type?: string }).type === 'UPDATE_AST',
      );
      expect(updateAst).toBeDefined();
      const payload = (updateAst![0] as { payload: Record<string, unknown> }).payload;
      expect(payload.assetManifest).toEqual({
        'images/pic.png': '__q2_vfs__/project/sub/images/pic.png',
      });
    });
  });

  it('answers a url request with a url_response carrying the same request id', async () => {
    const { postMessage } = renderIframe();
    signalIframeReady();

    window.dispatchEvent(
      new MessageEvent('message', {
        data: { type: 'url', id: 'req-42', path: 'project/sub/images/pic.png' },
      }),
    );

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

  it('posts UPDATE_THEME with the CSS text when themeFingerprint is a string', async () => {
    const { postMessage } = renderIframe({ themeFingerprint: 'fp-1' });
    signalIframeReady();

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
    const { postMessage } = renderIframe({ themeFingerprint: null });
    signalIframeReady();

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
    const { postMessage } = renderIframe({ scrollHandleRef: handleRef });
    signalIframeReady();

    await waitFor(() => expect(handleRef.current).not.toBeNull());
    expect(handleRef.current!.getScrollRatio()).toBeNull();

    handleRef.current!.scrollToLine(42);
    expect(
      postMessage.mock.calls.some(
        ([msg]) => (msg as { type?: string; line?: number }).type === 'SCROLL_TO_LINE'
          && (msg as { line?: number }).line === 42,
      ),
    ).toBe(true);

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'PREVIEW_SCROLLED', ratio: 0.37 } }));
    await waitFor(() => expect(handleRef.current!.getScrollRatio()).toBe(0.37));
  });

  it('forwards PREVIEW_SCROLLED to onScroll and CLICK_AT_LINE to onClickAtLine with hostY = iframeY + iframe top', async () => {
    const onScroll = vi.fn();
    const onClickAtLine = vi.fn();
    const { iframe } = renderIframe({ onScroll, onClickAtLine });
    vi.spyOn(iframe, 'getBoundingClientRect').mockReturnValue({ top: 100 } as DOMRect);
    signalIframeReady();

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'PREVIEW_SCROLLED', ratio: 0.5 } }));
    await waitFor(() => expect(onScroll).toHaveBeenCalled());

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'CLICK_AT_LINE', line: 7, iframeY: 23 } }));
    await waitFor(() => expect(onClickAtLine).toHaveBeenCalledWith(7, 123));
  });

  it('forwards NAVIGATE_TO_DOCUMENT, SET_AST, SLIDE_CHANGED, and AST_RENDERED to their callbacks', async () => {
    const onNavigateToDocument = vi.fn();
    const setAst = vi.fn();
    const onSlideChange = vi.fn();
    const onAstRendered = vi.fn();
    renderIframe({ onNavigateToDocument, setAst, onSlideChange, onAstRendered });
    signalIframeReady();

    window.dispatchEvent(new MessageEvent('message', { data: { type: 'NAVIGATE_TO_DOCUMENT', path: 'other.qmd', anchor: 'sec' } }));
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'SET_AST', ast: { blocks: [] } } }));
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'SLIDE_CHANGED', index: 3 } }));
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'AST_RENDERED' } }));

    await waitFor(() => {
      expect(onNavigateToDocument).toHaveBeenCalledWith('other.qmd', 'sec');
      expect(setAst).toHaveBeenCalledWith({ blocks: [] });
      expect(onSlideChange).toHaveBeenCalledWith(3);
      expect(onAstRendered).toHaveBeenCalled();
    });
  });

  it('posts LOAD_CUSTOM_COMPONENTS and a deduped SET_SLIDE when ready', async () => {
    const { postMessage } = renderIframe({
      customComponentsCode: { 'comp.tsx': 'export default 1' },
      currentSlideIndex: 2,
    });
    signalIframeReady();

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
    window.dispatchEvent(new MessageEvent('message', { data: { type: 'SLIDE_CHANGED', index: 4 } }));
    await waitFor(() => {
      expect(
        postMessage.mock.calls.filter(
          ([msg]) => (msg as { type?: string }).type === 'SET_SLIDE',
        ),
      ).toHaveLength(1);
    });
  });

  it('ships the full feature payload in UPDATE_AST', async () => {
    const { postMessage } = renderIframe({
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
    signalIframeReady();

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

  it('skips the UPDATE_THEME post entirely when themeFingerprint is undefined', async () => {
    // Transient render failures must not strip the iframe's last-good
    // styling — same three-way semantics as Q2PreviewIframe.
    const { postMessage } = renderIframe({ themeFingerprint: undefined });
    signalIframeReady();

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
