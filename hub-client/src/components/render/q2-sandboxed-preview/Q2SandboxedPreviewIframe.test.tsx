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

function renderIframe(props: Partial<Parameters<typeof Q2SandboxedPreviewIframe>[0]> = {}) {
  render(
    <Q2SandboxedPreviewIframe
      astJson='{"blocks":[]}'
      currentFilePath="docs/page.qmd"
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
