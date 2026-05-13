/**
 * Integration test for the SPA's boot path.
 *
 * Mocks `@quarto/preview-runtime` (the WASM/automerge side) and the
 * Q2PreviewIframe (the rendering side) so we can assert the wiring
 * end-to-end: PreviewApp parses the URL fragment, calls initWasm +
 * connect, picks the first .qmd, calls renderPageInProject, and hands
 * the returned astJson + currentFilePath to <Q2PreviewIframe>.
 *
 * No real WASM, no real samod — those layers are covered by their
 * own tests in @quarto/preview-runtime. This test pins the *seam*
 * the SPA owns: which methods get called, in which order, with which
 * arguments, and that the iframe ends up with the right props.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
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
  renderPageInProject: vi.fn(async (_path: string) => runtimeMockState.renderResult),
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
  // URL fragment carries indexDocId — see plan §A.5 + Q-A3.
  window.location.hash = '#/preview/automerge:test-index-doc';
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
});
