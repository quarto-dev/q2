/**
 * Channel routing tests for Plan 2b's two-channel edit API.
 *
 * The PreviewApp.handleSetAst callback routes on PreviewNodeEditPayload.channel:
 *  - 'text'    → parseQmdContentSync(edit.newText) → ast → applyNodeEdit
 *  - 'subtree' → edit.modifiedSubtreeJson → applyNodeEdit directly (no parse)
 *
 * Strategy: mount PreviewApp with a full mock of @quarto/preview-runtime,
 * wait for the render to complete (Q2PreviewIframe appears), capture the
 * `setAst` function passed to the iframe, call it with each channel's
 * payload, and assert which mock functions were/weren't called.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

// ── Mocks ────────────────────────────────────────────────────────────────────

const capturedIframeProps: Array<Record<string, unknown>> = [];
vi.mock('@quarto/preview-renderer/iframe/Q2PreviewIframe', () => ({
  Q2PreviewIframe: (props: Record<string, unknown>) => {
    capturedIframeProps.push(props);
    return <div data-testid="q2-preview-iframe-mock" />;
  },
}));

// Shared mock state mutated per test.
type MockState = {
  files: FileEntry[];
  renderResult: Record<string, unknown>;
  connectError?: string;
};
let mockState: MockState;

vi.mock('@quarto/preview-runtime', () => ({
  initWasm: vi.fn().mockResolvedValue(undefined),
  isWasmReady: vi.fn(() => true),
  connect: vi.fn(async (..._args: unknown[]) => {
    if (mockState.connectError) throw new Error(mockState.connectError);
    return mockState.files;
  }),
  setSyncHandlers: vi.fn(),
  renderPageForPreview: vi.fn(async () => mockState.renderResult),
  getBinaryDocById: vi.fn(async () => null),
  getFilePaths: vi.fn(() => mockState.files.map((f) => f.path)),
  vfsReadFile: vi.fn(() => ({ success: true, content: 'hello world\n' })),
  vfsAddFile: vi.fn(() => ({ success: true })),
  parseQmdContentSync: vi.fn(() => ({
    success: true,
    ast: '{"blocks":[{"t":"Para","c":[{"t":"Str","c":"replacement"}]}]}',
  })),
  applyNodeEdit: vi.fn(() => 'updated qmd\n'),
}));

import PreviewApp from './PreviewApp';

beforeEach(async () => {
  vi.clearAllMocks();
  capturedIframeProps.length = 0;
  mockState = {
    files: [{ path: 'index.qmd', docId: 'automerge:doc-index' }],
    renderResult: {
      success: true,
      ast_json: '{"blocks":[{"t":"Para","c":[{"t":"Str","c":"hello"}]}]}',
      untransformed_ast_json:
        '{"blocks":[{"t":"Para","s":0,"c":[]}],"astContext":{"p":[{"t":0,"r":[0,12],"d":0}]}}',
    },
  };
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.endsWith('/health')) {
        return new Response(
          JSON.stringify({ status: 'ok', index_document_id: 'automerge:test-doc' }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        );
      }
      return new Response('not found', { status: 404 });
    }),
  );
});

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Mount PreviewApp and wait for the first successful render to complete.
 * Returns the `setAst` function captured from the mocked Q2PreviewIframe.
 */
async function mountAndGetSetAst(): Promise<(payload: unknown) => void> {
  render(<PreviewApp />);
  await waitFor(() => {
    expect(screen.queryByTestId('q2-preview-iframe-mock')).not.toBeNull();
  });
  const props = capturedIframeProps[capturedIframeProps.length - 1];
  return props.setAst as (payload: unknown) => void;
}

// ── Channel routing tests ─────────────────────────────────────────────────────

describe('PreviewApp channel routing (Plan 2b)', () => {
  it('text channel: calls parseQmdContentSync then applyNodeEdit', async () => {
    const { parseQmdContentSync, applyNodeEdit } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    const payload = {
      __isPreviewNodeEdit: true,
      channel: 'text' as const,
      destinationSourceInfoJson: '{"t":0,"r":[0,12],"d":0}',
      newText: 'Updated paragraph text.\n',
    };

    setAst(payload);

    expect(parseQmdContentSync).toHaveBeenCalledOnce();
    expect(parseQmdContentSync).toHaveBeenCalledWith('Updated paragraph text.\n');
    expect(applyNodeEdit).toHaveBeenCalledOnce();
  });

  it('subtree channel: calls applyNodeEdit directly WITHOUT parseQmdContentSync', async () => {
    const { parseQmdContentSync, applyNodeEdit } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    const modifiedJson = '{"blocks":[{"t":"Para","c":[{"t":"Str","c":"subtree edit"}]}]}';
    const payload = {
      __isPreviewNodeEdit: true,
      channel: 'subtree' as const,
      destinationSourceInfoJson: '{"t":0,"r":[0,12],"d":0}',
      modifiedSubtreeJson: modifiedJson,
    };

    setAst(payload);

    expect(parseQmdContentSync).not.toHaveBeenCalled();
    expect(applyNodeEdit).toHaveBeenCalledOnce();
    // The modifiedSubtreeJson is passed directly to applyNodeEdit (4th arg).
    const applyArgs = (applyNodeEdit as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(applyArgs[3]).toBe(modifiedJson);
  });

  it('text channel: applyNodeEdit receives the parsed ast (not the raw text)', async () => {
    const { applyNodeEdit, parseQmdContentSync } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    const expectedAst = '{"blocks":[{"t":"Para","c":[{"t":"Str","c":"replacement"}]}]}';
    (parseQmdContentSync as ReturnType<typeof vi.fn>).mockReturnValueOnce({
      success: true,
      ast: expectedAst,
    });

    setAst({
      __isPreviewNodeEdit: true,
      channel: 'text' as const,
      destinationSourceInfoJson: '{"t":0,"r":[0,12],"d":0}',
      newText: 'anything',
    });

    const applyArgs = (applyNodeEdit as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(applyArgs[3]).toBe(expectedAst);
  });

  it('ignores payloads without __isPreviewNodeEdit flag', async () => {
    const { applyNodeEdit } = await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    setAst({ channel: 'text', newText: 'should be ignored' });

    expect(applyNodeEdit).not.toHaveBeenCalled();
  });
});
