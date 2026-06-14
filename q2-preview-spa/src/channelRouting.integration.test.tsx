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
import { render, screen, waitFor, cleanup } from '@testing-library/react';
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
  /**
   * What `GET /api/preview/config` answers (bd-ov4gqk3m). `undefined`
   * makes the endpoint 404 — the SPA must fail closed (read-only).
   */
  allowEdit?: boolean;
};
let mockState: MockState;

// Sentinel splice ops returned by the mocked diffToEditorChanges so the
// write-back test can assert applyEditorOperations receives them verbatim.
const SENTINEL_CHANGES = [{ rangeOffset: 0, rangeLength: 5, text: 'updated' }];

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
  // P3.2: PreviewApp passes this import to its gated nested-buffer memo on
  // every render (even nesting-cursor off), so the strict mock must define it.
  regenerateNestedBuffers: vi.fn(() => ({})),
  // bd-ov4gqk3m: Automerge write-back path.
  getFileContent: vi.fn(() => 'old qmd content\n'),
  diffToEditorChanges: vi.fn(() => SENTINEL_CHANGES),
  applyEditorOperations: vi.fn(),
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
    // Channel-routing tests exercise the edit path, which since
    // bd-ov4gqk3m only runs when the server allows editing.
    allowEdit: true,
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
      if (url.endsWith('/api/preview/config')) {
        if (mockState.allowEdit === undefined) {
          return new Response('not found', { status: 404 });
        }
        return new Response(JSON.stringify({ allowEdit: mockState.allowEdit }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        });
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

// ── Automerge write-back + read-only gating (bd-ov4gqk3m) ────────────────────

const TEXT_PAYLOAD = {
  __isPreviewNodeEdit: true,
  channel: 'text' as const,
  destinationSourceInfoJson: '{"t":0,"r":[0,12],"d":0}',
  newText: 'Updated paragraph text.\n',
};

describe('PreviewApp edit write-back (bd-ov4gqk3m)', () => {
  it('with allowEdit: routes the new QMD into Automerge via applyEditorOperations', async () => {
    const { applyEditorOperations, diffToEditorChanges, getFileContent, vfsAddFile } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    setAst(TEXT_PAYLOAD);

    // Diff base is the Automerge-side content; target is applyNodeEdit's output.
    expect(getFileContent).toHaveBeenCalledWith('index.qmd');
    expect(diffToEditorChanges).toHaveBeenCalledWith('old qmd content\n', 'updated qmd\n');
    // The splice ops go into the Automerge doc for the active file...
    expect(applyEditorOperations).toHaveBeenCalledOnce();
    expect(applyEditorOperations).toHaveBeenCalledWith('index.qmd', SENTINEL_CHANGES);
    // ...and the optimistic local VFS update still happens.
    expect(vfsAddFile).toHaveBeenCalledWith('index.qmd', 'updated qmd\n');
  });

  it('without allowEdit: drops the edit entirely (no applyNodeEdit, no Automerge, no VFS)', async () => {
    mockState.allowEdit = false;
    const { applyNodeEdit, applyEditorOperations, vfsAddFile } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();
    (vfsAddFile as ReturnType<typeof vi.fn>).mockClear();

    setAst(TEXT_PAYLOAD);

    expect(applyNodeEdit).not.toHaveBeenCalled();
    expect(applyEditorOperations).not.toHaveBeenCalled();
    expect(vfsAddFile).not.toHaveBeenCalled();
  });

  it('config endpoint missing (404): fails closed to read-only', async () => {
    mockState.allowEdit = undefined;
    const { applyNodeEdit, applyEditorOperations } =
      await import('@quarto/preview-runtime');
    const setAst = await mountAndGetSetAst();

    setAst(TEXT_PAYLOAD);

    expect(applyNodeEdit).not.toHaveBeenCalled();
    expect(applyEditorOperations).not.toHaveBeenCalled();
  });

  it('forwards editingDisabled: false to the iframe when the server allows edits', async () => {
    await mountAndGetSetAst();
    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props.editingDisabled).toBe(false);
  });

  it('forwards editingDisabled: true to the iframe when the server is read-only', async () => {
    cleanup();
    capturedIframeProps.length = 0;
    mockState.allowEdit = false;
    await mountAndGetSetAst();
    const props = capturedIframeProps[capturedIframeProps.length - 1];
    expect(props.editingDisabled).toBe(true);
  });
});
