/**
 * P3.2 tests for the SPA:
 *
 * A. Unit tests for computeNestedEditBuffers (the exported pure helper in
 *    PreviewApp.tsx). Tests the gating logic directly with a mocked `regen`
 *    argument. Genuine fail-on-revert: remove `!unlock` from the guard →
 *    the "flag off" test calls regen and turns RED.
 *
 * B. Integration tests: SPA query param `?nestingCursor=1` → nestingCursor: true
 *    in state → iframe receives unlockNestingCursor: true AND regenerateNestedBuffers
 *    is called with the rendered content + AST (on-path assertion).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

// ── A. Unit tests for the exported pure helper ────────────────────────────

// Import the helper directly. No mocking of @quarto/preview-runtime needed
// because we supply the `regen` argument explicitly.
import { computeNestedEditBuffers } from './PreviewApp';

const CONTENT = '# Hello\n';
const AST = '{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}';
const REGEN_RESULT = { '0:0-10:0': '- item\n' };

describe('computeNestedEditBuffers (P3.2 SPA helper)', () => {
  it('does NOT call regen when unlock is false', () => {
    const mockRegen = vi.fn(() => REGEN_RESULT);
    const result = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
    expect(result).toEqual({});
  });

  it('calls regen with both inputs when unlock is true', () => {
    const mockRegen = vi.fn(() => REGEN_RESULT);
    const result = computeNestedEditBuffers(true, CONTENT, AST, mockRegen);
    expect(mockRegen).toHaveBeenCalledWith(CONTENT, AST);
    expect(result).toEqual(REGEN_RESULT);
  });

  it('does NOT call regen when unlock is true but content is empty', () => {
    const mockRegen = vi.fn(() => REGEN_RESULT);
    computeNestedEditBuffers(true, '', AST, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
  });

  it('does NOT call regen when unlock is true but ast is null', () => {
    const mockRegen = vi.fn(() => REGEN_RESULT);
    computeNestedEditBuffers(true, CONTENT, null, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
  });

  it('returns the same stable reference on off-path calls', () => {
    const mockRegen = vi.fn(() => REGEN_RESULT);
    const r1 = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    const r2 = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    expect(r1).toBe(r2);
  });
});

// ── B. Integration tests via PreviewApp ──────────────────────────────────

// Capture props on every <Q2PreviewIframe> render.
const capturedIframeProps: Array<Record<string, unknown>> = [];
vi.mock('@quarto/preview-renderer/iframe/Q2PreviewIframe', () => ({
  Q2PreviewIframe: (props: Record<string, unknown>) => {
    capturedIframeProps.push(props);
    return <div data-testid="q2-preview-iframe-mock" />;
  },
}));

// Mock the runtime so the test doesn't need WASM or samod.
type RuntimeMockState = {
  files: FileEntry[];
  renderResult: Record<string, unknown>;
};
let runtimeMockState: RuntimeMockState;
let mockRegenerateNestedBuffers: ReturnType<typeof vi.fn>;

vi.mock('@quarto/preview-runtime', () => {
  const mockRegen = vi.fn(() => ({ '0:0-10:0': '- item\n' }));
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).__mockRegenerateNestedBuffers__ = mockRegen;
  return {
    initWasm: vi.fn().mockResolvedValue(undefined),
    isWasmReady: vi.fn(() => true),
    connect: vi.fn(async (..._args: unknown[]) => runtimeMockState.files),
    disconnect: vi.fn(async () => undefined),
    setSyncHandlers: vi.fn(),
    renderPageForPreview: vi.fn(
      async (_path: string, _grammars?: unknown, _capture?: Uint8Array) =>
        runtimeMockState.renderResult,
    ),
    getBinaryDocById: vi.fn(async (_docId: string) => null),
    getFilePaths: vi.fn(() => runtimeMockState.files.map((f: FileEntry) => f.path)),
    vfsReadFile: vi.fn(() => ({ success: true, content: CONTENT })),
    vfsAddFile: vi.fn(() => ({ success: true })),
    parseQmdContentSync: vi.fn(() => ({ success: true, ast: '{"blocks":[]}' })),
    applyNodeEdit: vi.fn(() => 'updated qmd content\n'),
    applyEditorOperations: vi.fn(),
    diffToEditorChanges: vi.fn(() => []),
    getFileContent: vi.fn(),
    regenerateNestedBuffers: mockRegen,
  };
});

// Imported after vi.mock so the mocks are in place.
import PreviewApp from './PreviewApp';

beforeEach(() => {
  vi.clearAllMocks();
  capturedIframeProps.length = 0;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  mockRegenerateNestedBuffers = (globalThis as any).__mockRegenerateNestedBuffers__;
  runtimeMockState = {
    files: [{ path: 'index.qmd', docId: 'automerge:doc-index' }],
    renderResult: {
      success: true,
      ast_json: AST,
      // Include untransformed_ast_json so the gating helper receives a
      // non-empty ast argument and execution reaches the flag check.
      untransformed_ast_json: AST,
    },
  };

  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.includes('/health')) {
        return new Response(
          JSON.stringify({
            status: 'ok',
            index_document_id: 'automerge:test-index-doc',
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        );
      }
      if (url.includes('/api/preview/config')) {
        return new Response(
          JSON.stringify({ allow_edit: false }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        );
      }
      if (url.includes('/api/preview/deps')) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        });
      }
      if (url.includes('/api/preview/diagnostics')) {
        return new Response(JSON.stringify([]), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        });
      }
      return new Response('not found', { status: 404 });
    }),
  );
});

describe('P3.2 SPA query param: ?nestingCursor=1 → iframe receives unlockNestingCursor', () => {
  it('passes unlockNestingCursor: true to Q2PreviewIframe when ?nestingCursor=1 in the URL', async () => {
    // Seed window.location.search before PreviewApp reads it at load.
    Object.defineProperty(window, 'location', {
      value: {
        ...window.location,
        search: '?nestingCursor=1',
        hash: '',
      },
      writable: true,
    });

    render(<PreviewApp />);

    await waitFor(
      () => {
        const iframeEl = document.querySelector('[data-testid="q2-preview-iframe-mock"]');
        expect(iframeEl).not.toBeNull();
      },
      { timeout: 3000 },
    );

    const props = capturedIframeProps[capturedIframeProps.length - 1];
    // After boot with ?nestingCursor=1, the iframe should receive unlockNestingCursor: true.
    expect(props?.unlockNestingCursor).toBe(true);
    // On-path assertion: regenerateNestedBuffers must have been called because
    // nestingCursor is on AND the render produced non-empty content + untransformedAstJson.
    expect(mockRegenerateNestedBuffers).toHaveBeenCalledWith(CONTENT, AST);
  });

  it('passes unlockNestingCursor: false (or absent) when no ?nestingCursor param', async () => {
    Object.defineProperty(window, 'location', {
      value: {
        ...window.location,
        search: '',
        hash: '',
      },
      writable: true,
    });

    render(<PreviewApp />);

    await waitFor(
      () => {
        const iframeEl = document.querySelector('[data-testid="q2-preview-iframe-mock"]');
        expect(iframeEl).not.toBeNull();
      },
      { timeout: 3000 },
    );

    const props = capturedIframeProps[capturedIframeProps.length - 1];
    // No param → nestingCursor defaults to false → unlockNestingCursor should be false/absent.
    expect(props?.unlockNestingCursor).toBeFalsy();
  });

  it('passes unlockNestingCursor: false when ?nestingCursor=0', async () => {
    Object.defineProperty(window, 'location', {
      value: {
        ...window.location,
        search: '?nestingCursor=0',
        hash: '',
      },
      writable: true,
    });

    render(<PreviewApp />);

    await waitFor(
      () => {
        const iframeEl = document.querySelector('[data-testid="q2-preview-iframe-mock"]');
        expect(iframeEl).not.toBeNull();
      },
      { timeout: 3000 },
    );

    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props?.unlockNestingCursor).toBeFalsy();
  });
});
