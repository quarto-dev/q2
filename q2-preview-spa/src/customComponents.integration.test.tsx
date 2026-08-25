/**
 * Integration tests for the SPA's render-components wiring (GH #402 /
 * bd-ue80chl0 Phase 2): PreviewApp reads `render-components` from the
 * rendered AST meta, builds `customComponentsCode` via the shared
 * helpers, and hands it to <Q2PreviewIframe>.
 *
 * Same seam-pinning approach as `PreviewApp.integration.test.tsx`
 * (runtime + iframe + transpiler mocked). The cadence tests pin the
 * plan's Q1 decision:
 *   - `.qmd` keystrokes must NOT re-transpile (per-keystroke babel runs
 *     would accumulate);
 *   - `.tsx` touches and path-list changes MUST re-transpile;
 *   - documents without the key never load the transpiler and keep a
 *     referentially-stable empty `customComponentsCode` (so the iframe
 *     never re-posts LOAD_CUSTOM_COMPONENTS).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, act } from '@testing-library/react';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

// ─── Mocks ───────────────────────────────────────────────────────────────────

const capturedIframeProps: Array<Record<string, unknown>> = [];
vi.mock('@quarto/preview-renderer/iframe/Q2PreviewIframe', () => ({
  Q2PreviewIframe: (props: Record<string, unknown>) => {
    capturedIframeProps.push(props);
    return <div data-testid="q2-preview-iframe-mock" />;
  },
}));

const transpileSpy = vi.hoisted(() =>
  vi.fn((code: string) => `JS:${code}`),
);
vi.mock('@quarto/preview-renderer/utils/tsxTranspiler', () => ({
  transpileTSX: transpileSpy,
}));

type RuntimeMockState = {
  files: FileEntry[];
  renderResult: Record<string, unknown>;
  /** Text-file contents served by getFileContent (mutable per test). */
  textContents: Map<string, string>;
};
let runtimeMockState: RuntimeMockState;

vi.mock('@quarto/preview-runtime', () => ({
  initWasm: vi.fn().mockResolvedValue(undefined),
  isWasmReady: vi.fn(() => true),
  connect: vi.fn(async () => runtimeMockState.files),
  disconnect: vi.fn(async () => undefined),
  setSyncHandlers: vi.fn(),
  renderPageForPreview: vi.fn(async () => runtimeMockState.renderResult),
  getBinaryDocById: vi.fn(async () => null),
  getFilePaths: vi.fn(() => runtimeMockState.files.map((f) => f.path)),
  getFileContent: vi.fn(
    (path: string) => runtimeMockState.textContents.get(path) ?? null,
  ),
  vfsReadFile: vi.fn(() => ({ success: true, content: 'test qmd content\n' })),
  vfsAddFile: vi.fn(() => ({ success: true })),
  parseQmdContentSync: vi.fn(() => ({ success: true, ast: '{"blocks":[]}' })),
  applyNodeEdit: vi.fn(() => 'updated qmd content\n'),
  regenerateNestedBuffers: vi.fn(() => ({})),
}));

import PreviewApp from './PreviewApp';
import { EMPTY_CUSTOM_COMPONENTS } from './customComponents';

// ─── Fixtures ────────────────────────────────────────────────────────────────

function astJsonWith(paths: string[] | null): string {
  const meta =
    paths === null
      ? {}
      : {
          'render-components': {
            t: 'MetaList',
            c: paths.map((p) => ({
              t: 'MetaInlines',
              c: [{ t: 'Str', c: p }],
            })),
          },
        };
  return JSON.stringify({ 'pandoc-api-version': [1, 23, 0], meta, blocks: [] });
}

async function lastSyncHandlers() {
  const runtime = await import('@quarto/preview-runtime');
  const calls = (runtime.setSyncHandlers as ReturnType<typeof vi.fn>).mock
    .calls;
  expect(calls.length).toBeGreaterThan(0);
  return calls[calls.length - 1][0];
}

function lastCapturedCode(): Record<string, string> | undefined {
  return capturedIframeProps.at(-1)?.customComponentsCode as
    | Record<string, string>
    | undefined;
}

beforeEach(() => {
  vi.clearAllMocks();
  capturedIframeProps.length = 0;
  runtimeMockState = {
    files: [
      { path: 'index.qmd', docId: 'automerge:doc-index' },
      { path: 'overrides.tsx', docId: 'automerge:doc-tsx' },
    ],
    renderResult: {
      success: true,
      ast_json: astJsonWith(['overrides.tsx']),
    },
    textContents: new Map([['overrides.tsx', 'export const Para = 1;']]),
  };
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

async function bootAndWaitForComponents(): Promise<void> {
  render(<PreviewApp />);
  await waitFor(() => {
    expect(lastCapturedCode()).toEqual({
      'overrides.tsx': 'JS:export const Para = 1;',
    });
  });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('PreviewApp render-components wiring', () => {
  it('transpiles listed components and passes them to the iframe', async () => {
    await bootAndWaitForComponents();
    expect(transpileSpy).toHaveBeenCalledTimes(1);
  });

  it('does NOT re-transpile on a .qmd content change (Q1 cadence)', async () => {
    await bootAndWaitForComponents();
    const codeBefore = lastCapturedCode();
    const handlers = await lastSyncHandlers();

    // A .qmd keystroke: contentTick bumps, a re-render happens, but the
    // path list is unchanged and no .tsx was touched — babel must not run.
    await act(async () => {
      handlers.onFileContent('index.qmd');
    });
    await waitFor(() => {
      // The re-render reached the iframe (a fresh props capture)…
      expect(capturedIframeProps.length).toBeGreaterThan(0);
    });
    expect(transpileSpy).toHaveBeenCalledTimes(1);
    // …and customComponentsCode kept its identity, so the iframe never
    // re-posts LOAD_CUSTOM_COMPONENTS.
    expect(lastCapturedCode()).toBe(codeBefore);
  });

  it('re-transpiles when a .tsx file is touched', async () => {
    await bootAndWaitForComponents();
    runtimeMockState.textContents.set(
      'overrides.tsx',
      'export const Para = 2;',
    );
    const handlers = await lastSyncHandlers();
    await act(async () => {
      handlers.onFileContent('overrides.tsx');
    });
    await waitFor(() => {
      expect(lastCapturedCode()).toEqual({
        'overrides.tsx': 'JS:export const Para = 2;',
      });
    });
    expect(transpileSpy).toHaveBeenCalledTimes(2);
  });

  it('keeps a referentially-stable empty code map for documents without the key', async () => {
    runtimeMockState.renderResult = {
      success: true,
      ast_json: astJsonWith(null),
    };
    render(<PreviewApp />);
    await waitFor(() => {
      expect(lastCapturedCode()).toBeDefined();
    });
    expect(lastCapturedCode()).toBe(EMPTY_CUSTOM_COMPONENTS.code);
    expect(transpileSpy).not.toHaveBeenCalled();

    // A .qmd edit re-renders; the empty map must keep its identity.
    const handlers = await lastSyncHandlers();
    const before = capturedIframeProps.length;
    await act(async () => {
      handlers.onFileContent('index.qmd');
    });
    await waitFor(() => {
      expect(capturedIframeProps.length).toBeGreaterThan(before);
    });
    expect(lastCapturedCode()).toBe(EMPTY_CUSTOM_COMPONENTS.code);
    expect(transpileSpy).not.toHaveBeenCalled();
  });

  it('surfaces a missing component file in the diagnostics overlay', async () => {
    runtimeMockState.renderResult = {
      success: true,
      ast_json: astJsonWith(['nope.tsx']),
    };
    const { container } = render(<PreviewApp />);
    await waitFor(() => {
      expect(container.querySelector('.preview-error-overlay')).not.toBeNull();
    });
    // The component never loads; the built-ins render.
    expect(lastCapturedCode()).toEqual({});
  });
});
