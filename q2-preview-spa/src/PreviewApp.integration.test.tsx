/**
 * Integration test for the SPA's boot path.
 *
 * Mocks `@quarto/preview-runtime` (the WASM/automerge side), the
 * Q2PreviewIframe (the rendering side), and `fetch` (for `/health`) so
 * we can assert the wiring end-to-end: PreviewApp fetches
 * `index_document_id` from `/health`, calls initWasm + connect, picks
 * the first .qmd, calls renderPageInProject, and hands the returned
 * astJson + currentFilePath to <Q2PreviewIframe>.
 *
 * No real WASM, no real samod, no real HTTP — those layers are covered
 * by their own tests in @quarto/preview-runtime and quarto-preview's
 * Rust-side smoke. This test pins the *seam* the SPA owns: which
 * methods get called, in which order, with which arguments, and that
 * the iframe ends up with the right props.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

// ─── Mocks ───────────────────────────────────────────────────────────────────

// Capture props on every <Q2PreviewIframe> render so we can assert on them
// after async boot completes.
const capturedIframeProps: Array<Record<string, unknown>> = [];
vi.mock('@quarto/preview-renderer/iframe/Q2PreviewIframe', () => ({
  Q2PreviewIframe: (props: Record<string, unknown>) => {
    capturedIframeProps.push(props);
    return <div data-testid="q2-preview-iframe-mock" />;
  },
}));

// Mock the runtime so the test doesn't need WASM or a sync server.
// Each test fills in fixtures via the `runtimeMockState` below.
type RuntimeMockState = {
  files: FileEntry[];
  /** What `renderPageInProject(path)` returns. */
  renderResult: Record<string, unknown>;
  /** Whether `connect()` should throw. */
  connectError?: string;
};
let runtimeMockState: RuntimeMockState;

vi.mock('@quarto/preview-runtime', () => ({
  initWasm: vi.fn().mockResolvedValue(undefined),
  isWasmReady: vi.fn(() => true),
  connect: vi.fn(async (..._args: unknown[]) => {
    if (runtimeMockState.connectError) {
      throw new Error(runtimeMockState.connectError);
    }
    return runtimeMockState.files;
  }),
  setSyncHandlers: vi.fn(),
  renderPageForPreview: vi.fn(
    async (_path: string, _grammars?: unknown, _capture?: Uint8Array) =>
      runtimeMockState.renderResult,
  ),
  getBinaryDocById: vi.fn(async (_docId: string) => null),
  getFilePaths: vi.fn(() => runtimeMockState.files.map((f) => f.path)),
}));

// Imported after vi.mock so the mocks are in place.
import PreviewApp from './PreviewApp';

beforeEach(() => {
  vi.clearAllMocks();
  capturedIframeProps.length = 0;
  runtimeMockState = {
    files: [{ path: 'index.qmd', docId: 'automerge:doc-index' }],
    renderResult: { success: true, ast_json: '{"blocks":[]}' },
  };
  // `GET /health` returns the project's index document id — see plan
  // §A.5 + Q-A3. PreviewApp uses this on boot instead of a URL
  // fragment because the CLI binds + serves before any docId is known
  // browser-side.
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.endsWith('/health')) {
        return new Response(
          JSON.stringify({
            status: 'ok',
            index_document_id: 'automerge:test-index-doc',
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        );
      }
      return new Response('not found', { status: 404 });
    }),
  );
});

describe('PreviewApp boot path', () => {
  it('renders <Q2PreviewIframe> with the first .qmd after connect+render', async () => {
    render(<PreviewApp />);

    // Wait for the async boot chain (initWasm → connect → renderPageInProject
    // → Q2PreviewIframe mount). Failure here means one of those steps
    // didn't return the value our mock provided.
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    // The latest captured props should match our mocks.
    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props.currentFilePath).toBe('index.qmd');
    expect(props.astJson).toBe('{"blocks":[]}');
    // setAst is required by Q2PreviewIframe; Phase A's no-op is fine but
    // it must at least be a function so the iframe doesn't crash on
    // first DOM-stable edit.
    expect(typeof props.setAst).toBe('function');
  });

  it('shows "Initializing" before connect resolves', async () => {
    // Make connect resolve only when we tell it to, so the initial
    // (loading) view is observable.
    let resolveConnect!: (files: FileEntry[]) => void;
    const runtime = await import('@quarto/preview-runtime');
    (runtime.connect as ReturnType<typeof vi.fn>).mockImplementationOnce(
      () => new Promise<FileEntry[]>((res) => { resolveConnect = res; }),
    );

    render(<PreviewApp />);
    // Loading copy. We don't pin the exact wording, just that *some*
    // initializing affordance is visible before the iframe appears.
    await waitFor(() => {
      expect(screen.queryByText(/initializing/i)).not.toBeNull();
    });
    expect(screen.queryByTestId('q2-preview-iframe-mock')).toBeNull();

    // Resolve and confirm the iframe takes over.
    resolveConnect(runtimeMockState.files);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
  });

  it('surfaces a connection error via <PreviewErrorOverlay>', async () => {
    runtimeMockState.connectError = 'kaboom: sync server unreachable';
    render(<PreviewApp />);
    await waitFor(() => {
      // PreviewErrorOverlay shows "Render Error" in the expanded state;
      // we render it with `collapsed={false}` for the error case so
      // users see the message immediately.
      expect(screen.queryByText(/render error/i)).not.toBeNull();
    });
    expect(screen.queryByText(/kaboom/i)).not.toBeNull();
    expect(screen.queryByTestId('q2-preview-iframe-mock')).toBeNull();
  });

  it('forwards theme_fingerprint into Q2PreviewIframe so the iframe styles itself', async () => {
    // The hub render returns `theme_fingerprint` when a compiled
    // theme exists. PreviewApp must thread it into Q2PreviewIframe's
    // `themeFingerprint` prop — otherwise the iframe never knows to
    // mint a blob URL for /.quarto/project-artifacts/styles.css and
    // post UPDATE_THEME, leaving the output unstyled. Two cases:
    // (a) string passed through; (b) field absent → `null` (explicit
    // "no theme intended" rather than "preserve last").
    runtimeMockState.renderResult = {
      success: true,
      ast_json: '{"blocks":[]}',
      theme_fingerprint: 'abc123',
    };
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    const propsWithTheme = capturedIframeProps[capturedIframeProps.length - 1];
    expect(propsWithTheme.themeFingerprint).toBe('abc123');
  });

  it('passes themeFingerprint=null when render succeeds without a theme', async () => {
    runtimeMockState.renderResult = {
      success: true,
      ast_json: '{"blocks":[]}',
      // theme_fingerprint deliberately absent
    };
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props.themeFingerprint).toBeNull();
  });

  it('normalizes a bare /health docId by prefixing automerge:', async () => {
    // /health returns the bare id (samod's storage form); connect()
    // expects automerge:<id> (automerge-repo's DocumentId form). The
    // SPA must bridge the two. This test pins that contract: connect
    // should be called with the prefixed form even though /health
    // returns the bare form.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(
        JSON.stringify({ status: 'ok', index_document_id: '4ByAxLmGYwAEYN5xZEX7Jq1GxTmU' }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )),
    );
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    const runtime = await import('@quarto/preview-runtime');
    const calls = (runtime.connect as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls.length).toBeGreaterThan(0);
    const [, docIdArg] = calls[calls.length - 1];
    expect(docIdArg).toBe('automerge:4ByAxLmGYwAEYN5xZEX7Jq1GxTmU');
  });

  it('re-runs the render when the force-refresh button is clicked', async () => {
    // bd-b5hf / Phase A.6 — the epic's resolution #4 ("force-refresh
    // invariant") promises an always-visible UI affordance that
    // re-runs the render pipeline against current automerge state.
    // The dep-graph won't always know that a cross-doc edit affects
    // the active page; the button is the user's escape hatch.
    //
    // What we pin here: clicking the button calls
    // `renderPageForPreview` at least once more after the initial
    // boot render. We deliberately don't pin the *trigger mechanism*
    // (state-bump vs prop change vs direct call) — only the observable
    // outcome at the runtime seam.
    const runtime = await import('@quarto/preview-runtime');
    const renderMock = runtime.renderPageForPreview as ReturnType<typeof vi.fn>;
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    const initialCallCount = renderMock.mock.calls.length;
    expect(initialCallCount).toBeGreaterThan(0);

    // The button lives in PreviewApp's chrome (outside the sandboxed
    // renderer iframe) so it's always reachable even when the inner
    // render is misbehaving. Match by accessible name rather than
    // by test-id so the UX label is part of the contract.
    const refreshButton = screen.getByRole('button', {
      name: /refresh|re-render|reload/i,
    });
    fireEvent.click(refreshButton);

    await waitFor(() => {
      expect(renderMock.mock.calls.length).toBeGreaterThan(initialCallCount);
    });
  });

  it('threads a capture payload through to renderPageForPreview when the sidecar has one (Phase C.4)', async () => {
    // Phase C.4 (bd-kw93.3): when the IndexDocument's V2 capture
    // sidecar carries a captureDocId for the active page, PreviewApp
    // resolves the binary doc and forwards its gzipped JSON bytes to
    // the WASM renderer. We pin the seam shape: getBinaryDocById is
    // called with the captureDocId from onCapturesChange, and the
    // returned bytes are passed as the third argument to
    // renderPageForPreview.
    const runtime = await import('@quarto/preview-runtime');
    const setSyncHandlersMock = runtime.setSyncHandlers as ReturnType<typeof vi.fn>;
    const getBinaryDocByIdMock = runtime.getBinaryDocById as ReturnType<typeof vi.fn>;
    const renderMock = runtime.renderPageForPreview as ReturnType<typeof vi.fn>;

    const sentinelBytes = new Uint8Array([1, 2, 3, 4]);
    getBinaryDocByIdMock.mockImplementation(async (docId: string) => {
      // Only resolve the exact id we pre-fed into onCapturesChange;
      // anything else returns null so we don't accidentally pass
      // bytes for a different doc.
      if (docId === 'capture-doc-1') {
        return { content: sentinelBytes, mimeType: 'application/x-engine-capture+gzip' };
      }
      return null;
    });

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    // Invoke the onCapturesChange handler PreviewApp registered with
    // setSyncHandlers — simulating a sync-client capture event.
    const handlersArg = setSyncHandlersMock.mock.calls.at(-1)?.[0] as
      | { onCapturesChange?: (captures: Record<string, unknown>) => void }
      | undefined;
    expect(handlersArg?.onCapturesChange).toBeTypeOf('function');

    handlersArg!.onCapturesChange!({
      'index.qmd': { captureDocId: 'capture-doc-1' },
    });

    // The render effect re-fires off the new captures state; once it
    // has had a turn, getBinaryDocById should have been asked for our
    // capture and the bytes should land in renderPageForPreview's
    // third arg.
    await waitFor(() => {
      expect(getBinaryDocByIdMock).toHaveBeenCalledWith('capture-doc-1');
    });
    await waitFor(() => {
      const lastCall = renderMock.mock.calls.at(-1);
      expect(lastCall).toBeDefined();
      // arg 0: path, arg 1: grammars, arg 2: capture bytes
      expect(lastCall![2]).toBe(sentinelBytes);
    });
  });

  it('renders without a capture when the sidecar is empty (Phase C.4 fall-through)', async () => {
    // Default state: no onCapturesChange ever fires, so the active
    // page renders with `captureGzJson === undefined`. Confirms the
    // no-replay path is unchanged from pre-C.4 behaviour.
    const runtime = await import('@quarto/preview-runtime');
    const getBinaryDocByIdMock = runtime.getBinaryDocById as ReturnType<typeof vi.fn>;
    const renderMock = runtime.renderPageForPreview as ReturnType<typeof vi.fn>;

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    expect(getBinaryDocByIdMock).not.toHaveBeenCalled();
    const firstCall = renderMock.mock.calls[0];
    expect(firstCall).toBeDefined();
    // arg 2 is the capture; should be undefined.
    expect(firstCall![2]).toBeUndefined();
  });

  it('surfaces a /health failure with an actionable message', async () => {
    // Replace the default mock with one that 500s on /health. This is
    // the failure mode if the hub crashes before the SPA boots —
    // surfacing it visibly beats a blank "Initializing…" screen.
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('boom', { status: 500, statusText: 'Internal Server Error' })),
    );
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByText(/render error/i)).not.toBeNull();
    });
    expect(screen.queryByText(/\/health/i)).not.toBeNull();
    expect(screen.queryByTestId('q2-preview-iframe-mock')).toBeNull();
  });

  // ──────────────────────────────────────────────────────────────
  // Phase D.2 (bd-kw93.13): boot URL `?page=<rel>` query support
  // ──────────────────────────────────────────────────────────────

  it('seeds activeFile from ?page= when the CLI carries a requested page', async () => {
    // CLI emits `http://127.0.0.1:N/?page=about.qmd` when the user
    // asked for a specific page (or when the project has an
    // `index.qmd` at the root). The SPA's pickInitialPage must
    // honor it rather than falling through to firstQmd.
    runtimeMockState.files = [
      { path: 'intro.qmd', docId: 'automerge:intro' },
      { path: 'about.qmd', docId: 'automerge:about' },
    ];

    const originalLocation = window.location;
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, search: '?page=about.qmd' },
    });

    try {
      render(<PreviewApp />);
      await waitFor(() => {
        expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
      });
      const props = capturedIframeProps[capturedIframeProps.length - 1];
      expect(props.currentFilePath).toBe('about.qmd');
    } finally {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: originalLocation,
      });
    }
  });

  it('falls back to firstQmd when ?page= names a file not in the index', async () => {
    // Hand-crafted URL with a stale/unknown path must not strand
    // the SPA on an empty page — silently fall back to firstQmd.
    runtimeMockState.files = [
      { path: 'intro.qmd', docId: 'automerge:intro' },
      { path: 'about.qmd', docId: 'automerge:about' },
    ];

    const originalLocation = window.location;
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, search: '?page=does-not-exist.qmd' },
    });

    try {
      render(<PreviewApp />);
      await waitFor(() => {
        expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
      });
      const props = capturedIframeProps[capturedIframeProps.length - 1];
      expect(props.currentFilePath).toBe('intro.qmd');
    } finally {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: originalLocation,
      });
    }
  });

  // ──────────────────────────────────────────────────────────────
  // Phase D.4 (bd-kw93.10): non-terminal render errors overlay on
  //                        top of the last-good render
  // ──────────────────────────────────────────────────────────────

  it('keeps the previous render visible when a subsequent render fails', async () => {
    // Boot succeeds with a real render, then trigger another
    // render where the WASM call throws. The iframe (last-good
    // astJson) must stay mounted; PreviewErrorOverlay shows on top.
    runtimeMockState.files = [{ path: 'index.qmd', docId: 'automerge:i' }];

    const runtime = await import('@quarto/preview-runtime');
    let renderCallCount = 0;
    (runtime.renderPageForPreview as ReturnType<typeof vi.fn>).mockImplementation(
      async () => {
        renderCallCount += 1;
        if (renderCallCount === 1) {
          return { success: true, ast_json: '{"blocks":[]}' };
        }
        // Second call fails as if a malformed qmd hit the parser.
        throw new Error('synthetic parse error');
      },
    );

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    expect(capturedIframeProps.length).toBeGreaterThanOrEqual(1);

    // Trigger a re-render. setSyncHandlers' `onFileContent` callback
    // is what would bump contentTick in production; we trigger the
    // same effect via the captured handler.
    const setHandlersCalls = (
      runtime.setSyncHandlers as ReturnType<typeof vi.fn>
    ).mock.calls;
    const lastHandlers = setHandlersCalls[setHandlersCalls.length - 1][0] as {
      onFileContent: (path: string, content: string) => void;
    };

    lastHandlers.onFileContent('index.qmd', 'updated content');

    await waitFor(() => {
      // The overlay must surface (look for the collapsed-mode
      // affordance — "Error" button text).
      expect(screen.queryByText(/error/i)).not.toBeNull();
    });

    // The iframe stays mounted with the LAST GOOD ast_json — i.e. the
    // render failure does NOT take the user back to "Initializing…".
    expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
  });

  it('clears the render-error overlay when the next render succeeds', async () => {
    runtimeMockState.files = [{ path: 'index.qmd', docId: 'automerge:i' }];

    const runtime = await import('@quarto/preview-runtime');
    let renderCallCount = 0;
    (runtime.renderPageForPreview as ReturnType<typeof vi.fn>).mockImplementation(
      async () => {
        renderCallCount += 1;
        if (renderCallCount === 1) {
          return { success: true, ast_json: '{"blocks":[]}' };
        }
        if (renderCallCount === 2) {
          // Second render fails …
          throw new Error('synthetic parse error');
        }
        // … third render succeeds again (user fixed the qmd).
        return { success: true, ast_json: '{"blocks":[{"recovered":true}]}' };
      },
    );

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    const setHandlersCalls = (
      runtime.setSyncHandlers as ReturnType<typeof vi.fn>
    ).mock.calls;
    const lastHandlers = setHandlersCalls[setHandlersCalls.length - 1][0] as {
      onFileContent: (path: string, content: string) => void;
    };

    // Trigger failure render.
    lastHandlers.onFileContent('index.qmd', 'busted');
    await waitFor(() => {
      expect(screen.queryByText(/error/i)).not.toBeNull();
    });

    // Trigger recovery render.
    lastHandlers.onFileContent('index.qmd', 'fixed');
    await waitFor(() => {
      // Overlay clears (no "Error" affordance visible).
      expect(screen.queryByText(/^error$/i)).toBeNull();
    });
    // And the iframe got the new ast_json.
    const propsAfterRecovery = capturedIframeProps[capturedIframeProps.length - 1];
    expect(propsAfterRecovery.astJson).toBe('{"blocks":[{"recovered":true}]}');
  });

  // ──────────────────────────────────────────────────────────────
  // Phase F.1 (bd-kw93.14): cross-page navigation handler
  // ──────────────────────────────────────────────────────────────

  it('forwards projectFilePaths into Q2PreviewIframe so the link handler can recognise artifact-rooted .html', async () => {
    runtimeMockState.files = [
      { path: 'index.qmd', docId: 'automerge:i' },
      { path: 'about.qmd', docId: 'automerge:a' },
      { path: 'posts/first.qmd', docId: 'automerge:p1' },
    ];
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });
    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props.projectFilePaths).toEqual([
      'index.qmd',
      'about.qmd',
      'posts/first.qmd',
    ]);
  });

  it('handleNavigate updates activeFile and pushes a fresh history entry', async () => {
    runtimeMockState.files = [
      { path: 'index.qmd', docId: 'automerge:i' },
      { path: 'about.qmd', docId: 'automerge:a' },
    ];
    const pushSpy = vi.spyOn(window.history, 'pushState');
    try {
      render(<PreviewApp />);
      await waitFor(() => {
        expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
      });
      const initial = capturedIframeProps[capturedIframeProps.length - 1];
      expect(initial.currentFilePath).toBe('index.qmd');

      // Simulate the iframe posting a NAVIGATE_TO_DOCUMENT — extract
      // the latest onNavigateToDocument and call it directly.
      const onNavigate = initial.onNavigateToDocument as
        | ((path: string, anchor: string | null) => void)
        | undefined;
      expect(typeof onNavigate).toBe('function');
      onNavigate!('about.qmd', null);

      await waitFor(() => {
        const latest = capturedIframeProps[capturedIframeProps.length - 1];
        return latest.currentFilePath === 'about.qmd';
      });

      // history.pushState was called with a URL whose `?page=` is the
      // new active file.
      expect(pushSpy).toHaveBeenCalled();
      const lastPush = pushSpy.mock.calls[pushSpy.mock.calls.length - 1];
      expect(lastPush[2]).toContain('page=about.qmd');
    } finally {
      pushSpy.mockRestore();
    }
  });

  it('handleNavigate with anchor bumps pendingAnchorEpoch on each call', async () => {
    runtimeMockState.files = [
      { path: 'index.qmd', docId: 'automerge:i' },
      { path: 'about.qmd', docId: 'automerge:a' },
    ];
    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    const initialEpoch = (
      capturedIframeProps[capturedIframeProps.length - 1].pendingAnchorEpoch as number | undefined
    ) ?? 0;

    const onNavigate = capturedIframeProps[capturedIframeProps.length - 1]
      .onNavigateToDocument as (path: string, anchor: string | null) => void;
    onNavigate('about.qmd', 'intro');

    await waitFor(() => {
      const latest = capturedIframeProps[capturedIframeProps.length - 1];
      expect(latest.pendingAnchor).toBe('intro');
      expect(latest.pendingAnchorEpoch).toBe(initialEpoch + 1);
    });

    // Click the same anchor again — epoch must advance again so the
    // iframe re-scrolls (deliberate "take me back there" gesture).
    onNavigate('about.qmd', 'intro');
    await waitFor(() => {
      const latest = capturedIframeProps[capturedIframeProps.length - 1];
      expect(latest.pendingAnchorEpoch).toBe(initialEpoch + 2);
    });
  });

  it('popstate restores activeFile + pendingAnchor from the URL', async () => {
    runtimeMockState.files = [
      { path: 'index.qmd', docId: 'automerge:i' },
      { path: 'about.qmd', docId: 'automerge:a' },
    ];

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    // Simulate the user clicking back: location is restored to a
    // previous entry, then popstate fires. We mutate location's
    // search/hash via Object.defineProperty (jsdom doesn't have a
    // real history stack), then dispatch the event.
    const originalLocation = window.location;
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, search: '?page=about.qmd', hash: '#section-2' },
    });

    try {
      window.dispatchEvent(new PopStateEvent('popstate'));
      await waitFor(() => {
        const latest = capturedIframeProps[capturedIframeProps.length - 1];
        expect(latest.currentFilePath).toBe('about.qmd');
        expect(latest.pendingAnchor).toBe('section-2');
      });
    } finally {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: originalLocation,
      });
    }
  });

  it('seeds pendingAnchor from a boot URL hash', async () => {
    runtimeMockState.files = [
      { path: 'index.qmd', docId: 'automerge:i' },
      { path: 'about.qmd', docId: 'automerge:a' },
    ];
    const originalLocation = window.location;
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: {
        ...originalLocation,
        search: '?page=about.qmd',
        hash: '#install',
      },
    });

    try {
      render(<PreviewApp />);
      await waitFor(() => {
        expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
      });
      const props = capturedIframeProps[capturedIframeProps.length - 1];
      expect(props.currentFilePath).toBe('about.qmd');
      expect(props.pendingAnchor).toBe('install');
      // Boot hash → epoch 1 (first scroll request); without a hash
      // the epoch stays at 0.
      expect(props.pendingAnchorEpoch).toBe(1);
    } finally {
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: originalLocation,
      });
    }
  });

  it('shows engine errors via the stale-capture overlay (CaptureRef.lastError pass-through)', async () => {
    // Pin the existing pass-through in StaleCaptureOverlay (Phase
    // C.5) so a future refactor doesn't silently regress D.4.
    runtimeMockState.files = [{ path: 'index.qmd', docId: 'automerge:i' }];

    const runtime = await import('@quarto/preview-runtime');
    const setHandlersCallsBefore = (
      runtime.setSyncHandlers as ReturnType<typeof vi.fn>
    ).mock.calls.length;

    render(<PreviewApp />);
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    const setHandlersCalls = (
      runtime.setSyncHandlers as ReturnType<typeof vi.fn>
    ).mock.calls;
    const handlers = setHandlersCalls[setHandlersCallsBefore][0] as {
      onCapturesChange: (
        captures: Record<string, { captureDocId: string; state?: string; lastError?: string }>,
      ) => void;
    };

    // Inject a sidecar entry whose state === 'error' and carries a
    // user-facing message.
    handlers.onCapturesChange({
      'index.qmd': {
        captureDocId: 'automerge:cap',
        state: 'error',
        lastError: 'kernel died: ModuleNotFoundError: pandas',
      },
    });

    await waitFor(() => {
      expect(
        screen.queryByText(/kernel died.*pandas/i),
      ).not.toBeNull();
    });
  });

  // ─── bd-iuzmk: browser-tab title reflects the active doc's title ─────
  //
  // Today the SPA's <title> is hard-coded to "Quarto Preview" in
  // q2-preview.html. Once a render lands and the AST carries a
  // `meta.title`, the SPA should update `document.title` so users with
  // the published page and the preview open side-by-side can tell tabs
  // apart. Format: `<title> (Quarto Preview)` when a title is present;
  // fall back to "Quarto Preview" when it isn't.
  //
  // These tests assert the contract directly on `document.title` (not
  // a DOM node) because that's the value the browser tab reads.
  //
  // ⚠️  Earlier tests in this suite (the render-error-overlay cases
  // around line 415 / 460) call
  // `(runtime.renderPageForPreview).mockImplementation(...)` with
  // hardcoded return shapes. `vi.clearAllMocks()` in `beforeEach`
  // only clears call history, not implementations, so my tests would
  // inherit a stale stub that ignores `runtimeMockState.renderResult`
  // and short-circuits the title-extraction path. Each of the three
  // tests below re-installs the default closure-over-state mock at
  // the top so they observe their own renderResult mutation.

  async function resetRenderPageForPreviewMockToDefault() {
    const runtime = await import('@quarto/preview-runtime');
    (runtime.renderPageForPreview as ReturnType<typeof vi.fn>).mockImplementation(
      async (_p: string, _g?: unknown, _c?: Uint8Array) => runtimeMockState.renderResult,
    );
  }

  it('sets document.title to "<meta.title> (Quarto Preview)" after a render with a title', async () => {
    await resetRenderPageForPreviewMockToDefault();
    // The AST shape mirrors what pampa's JSON writer emits — meta is a
    // plain object at the top level, with each value tagged via
    // `t/c`. Pampa wraps a YAML `title: "Hello, world"` as MetaInlines
    // (the YAML scalar is parsed for inline-formatting opportunities),
    // so we use that shape here. The framework's `extractMetaString`
    // also accepts MetaString for completeness — covered in the
    // sibling test below.
    runtimeMockState.renderResult = {
      success: true,
      ast_json: JSON.stringify({
        blocks: [],
        meta: {
          title: {
            t: 'MetaInlines',
            c: [{ t: 'Str', c: 'Hello, world' }],
          },
        },
      }),
    };

    // Baseline before render. q2-preview.html ships with this literal.
    document.title = 'Quarto Preview';
    render(<PreviewApp />);

    await waitFor(() => {
      expect(document.title).toBe('Hello, world (Quarto Preview)');
    });
  });

  it('keeps document.title at "Quarto Preview" when meta.title is absent', async () => {
    await resetRenderPageForPreviewMockToDefault();
    // A doc with no `title:` in its frontmatter parses to a meta
    // object without that key (or with an undefined value). The SPA
    // must NOT clobber the existing title with "undefined (Quarto
    // Preview)" — it falls back to the bare suffix.
    runtimeMockState.renderResult = {
      success: true,
      ast_json: JSON.stringify({ blocks: [], meta: {} }),
    };

    document.title = 'Quarto Preview';
    render(<PreviewApp />);

    // Wait for the iframe to mount so we know the render effect
    // ran at least once. Title may still be the default, which is the
    // assertion this test wants to lock in.
    await waitFor(() => {
      expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
    });

    expect(document.title).toBe('Quarto Preview');
  });

  it('updates document.title when subsequent renders carry a different title (live edit)', async () => {
    await resetRenderPageForPreviewMockToDefault();
    // Live-edit case: user retitles the doc in the QMD. The next
    // render's AST has the new meta.title, and the tab should
    // update — same render effect, same dependency.
    runtimeMockState.renderResult = {
      success: true,
      ast_json: JSON.stringify({
        blocks: [],
        meta: {
          title: { t: 'MetaString', c: 'Original' },
        },
      }),
    };
    document.title = 'Quarto Preview';
    render(<PreviewApp />);

    await waitFor(() => {
      expect(document.title).toBe('Original (Quarto Preview)');
    });

    // Simulate an edit: re-mock the next render with a different
    // title and trigger a re-render via the force-refresh button
    // (which bumps `contentTick`, the render effect's dep). Using
    // the existing affordance avoids reaching into PreviewApp's
    // internals to fire a fresh render manually.
    runtimeMockState.renderResult = {
      success: true,
      ast_json: JSON.stringify({
        blocks: [],
        meta: {
          title: { t: 'MetaString', c: 'Renamed' },
        },
      }),
    };
    const refreshBtn = screen.getByRole('button', { name: /refresh|reload|re-?render/i });
    fireEvent.click(refreshBtn);

    await waitFor(() => {
      expect(document.title).toBe('Renamed (Quarto Preview)');
    });
  });
});
