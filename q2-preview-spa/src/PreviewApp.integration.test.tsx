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
  renderPageForPreview: vi.fn(async (_path: string) => runtimeMockState.renderResult),
  getFilePaths: vi.fn(() => runtimeMockState.files.map((f) => f.path)),
}));

// Imported after vi.mock so the mocks are in place.
import PreviewApp from './PreviewApp';

beforeEach(() => {
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
});
