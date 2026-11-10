/**
 * Engine-capture consumption and untransformed-AST capture gating for ReactPreview.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, render, waitFor } from '@testing-library/react';
import React from 'react';

const CAPTURE_BYTES = new Uint8Array([1, 2, 3, 4, 5]);
const AST = '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}';

const { renderPageInProjectWithAttribution, getBinaryDocById, regenerateNestedBuffers } = vi.hoisted(() => ({
  renderPageInProjectWithAttribution: vi.fn(async () => ({
    success: true,
    ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    untransformed_ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    theme_fingerprint: 'fp-1',
    diagnostics: [],
    warnings: [],
  })),
  getBinaryDocById: vi.fn(async () => ({
    content: new Uint8Array([1, 2, 3, 4, 5]),
    mimeType: 'application/x-engine-capture+gzip',
  })),
  regenerateNestedBuffers: vi.fn(() => ({})),
}));

vi.mock('@quarto/preview-runtime', () => ({
  renderPageInProjectWithAttribution,
  getBinaryDocById,
  renderPageForPreview: vi.fn(),
  parseQmdToAstWithAttribution: vi.fn(async () => ({ success: true, ast: '{}', diagnostics: [] })),
  isWasmReady: () => true,
  incrementalWriteQmd: vi.fn(),
  applyNodeEdit: vi.fn(),
  parseQmdContentSync: vi.fn(() => ({ success: true, ast: '{}' })),
  getAuthorId: () => 'author-1',
  regenerateNestedBuffers,
  pipelineKindForFormat: (f: string) => (f === 'q2-preview' ? 'preview' : undefined),
}));

vi.mock('../../hooks/useAttribution', () => ({
  useAttribution: () => ({ payload: null, generating: false }),
}));

const prefs: Record<string, unknown> = {
  previewEditing: false,
  unlockNestingCursor: false,
};
vi.mock('../../hooks/usePreference', () => ({
  usePreference: (key: string) => [prefs[key] ?? false, vi.fn()],
}));
vi.mock('./ReactRenderer', () => ({
  default: () => <div data-testid="react-renderer" />,
}));

import ReactPreview from './ReactPreview';
import type { CaptureRef } from '@quarto/preview-runtime';

function baseProps(
  captures?: Record<string, CaptureRef>,
  commentsMode?: 'show' | 'expand' | 'hide',
) {
  return {
    content: '---\nformat: q2-preview\nengine: knitr\n---\n\n```{r}\n1 + 1\n```\n',
    currentFile: { path: 'doc.qmd', name: 'doc.qmd' } as any,
    files: [],
    fileContents: new Map([['doc.qmd', 'x']]),
    scrollSyncEnabled: false,
    editorRef: { current: null } as any,
    editorReady: true,
    editorHasFocusRef: { current: false } as any,
    onFileChange: () => {},
    onOpenNewFileDialog: () => {},
    onDiagnosticsChange: () => {},
    onContentRewrite: () => {},
    format: 'q2-preview',
    attributionOn: false,
    captures,
    commentsMode,
  };
}

describe('ReactPreview capture consumption (bd-sfet3264, Phase 1D)', () => {
  beforeEach(() => {
    renderPageInProjectWithAttribution.mockClear();
    getBinaryDocById.mockClear();
    regenerateNestedBuffers.mockClear();
    prefs.previewEditing = false;
    prefs.unlockNestingCursor = false;
  });

  it('fetches the active doc capture and threads its bytes into the render call', async () => {
    const captures: Record<string, CaptureRef> = {
      'doc.qmd': { captureDocId: 'cap-1', state: 'idle' },
    };

    render(<ReactPreview {...baseProps(captures)} />);

    await waitFor(() => expect(getBinaryDocById).toHaveBeenCalledWith('cap-1'));
    await waitFor(() => {
      const calls = renderPageInProjectWithAttribution.mock.calls;
      const withCapture = calls.find((c) => c[3] !== undefined);
      expect(withCapture, 'a render call must carry the capture bytes').toBeTruthy();
      expect(withCapture![3]).toEqual(CAPTURE_BYTES);
    });
  });

  it('passes no capture bytes when the active doc has no capture entry', async () => {
    render(<ReactPreview {...baseProps(undefined)} />);

    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalled());
    expect(getBinaryDocById).not.toHaveBeenCalled();
    for (const call of renderPageInProjectWithAttribution.mock.calls) {
      expect(call[3]).toBeUndefined();
    }
  });

  it('requests the q2 preview default-format substitution (5th arg true) — bd-kltzdhle', async () => {
    render(<ReactPreview {...baseProps(undefined)} />);

    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalled());
    for (const call of renderPageInProjectWithAttribution.mock.calls) {
      expect(call[4]).toBe(true);
    }
  });
});

describe('ReactPreview untransformed-AST capture gating (Phase 3B1)', () => {
  beforeEach(() => {
    renderPageInProjectWithAttribution.mockClear();
    getBinaryDocById.mockClear();
    regenerateNestedBuffers.mockClear();
    prefs.previewEditing = false;
    prefs.unlockNestingCursor = false;
  });

  it.each([
    { name: 'editing arms capture even with comments hidden', editing: true, commentsMode: 'hide' as const, capture: true },
    { name: 'show comments arms capture in read mode', editing: false, commentsMode: 'show' as const, capture: true },
    { name: 'expand comments arms capture in read mode', editing: false, commentsMode: 'expand' as const, capture: true },
    { name: 'default show comments arms capture in read mode', editing: false, commentsMode: undefined, capture: true },
    { name: 'read mode with hidden comments leaves capture off', editing: false, commentsMode: 'hide' as const, capture: false },
  ])('$name', async ({ editing, commentsMode, capture }) => {
    prefs.previewEditing = editing;
    render(<ReactPreview {...baseProps(undefined, commentsMode)} />);

    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalled());
    for (const call of renderPageInProjectWithAttribution.mock.calls) {
      expect(call[5]).toEqual({ captureUntransformedAst: capture });
    }
  });

  it('entering editing causes exactly one armed render through the existing effect', async () => {
    const props = baseProps(undefined, 'hide');
    const view = render(<ReactPreview {...props} />);
    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1));
    renderPageInProjectWithAttribution.mockClear();

    prefs.previewEditing = true;
    await act(async () => view.rerender(<ReactPreview {...props} />));

    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1));
    await act(async () => Promise.resolve());
    expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1);
    expect(renderPageInProjectWithAttribution.mock.calls[0][5]).toEqual({ captureUntransformedAst: true });
  });

  it('showing hidden comments causes exactly one armed render through the existing effect', async () => {
    const props = baseProps(undefined, 'hide');
    const view = render(<ReactPreview {...props} />);
    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1));
    renderPageInProjectWithAttribution.mockClear();

    await act(async () => view.rerender(<ReactPreview {...props} commentsMode="show" />));

    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1));
    await act(async () => Promise.resolve());
    expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1);
    expect(renderPageInProjectWithAttribution.mock.calls[0][5]).toEqual({ captureUntransformedAst: true });
  });

  it('regenerates nested buffers only after editing turns on, even with unchanged AST strings', async () => {
    prefs.unlockNestingCursor = true;
    const props = baseProps(undefined, 'show');
    const view = render(<ReactPreview {...props} />);
    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalled());
    await act(async () => Promise.resolve());

    // Visible comments arm capture in read mode, but must not arm editing work.
    expect(regenerateNestedBuffers).not.toHaveBeenCalled();

    prefs.previewEditing = true;
    await act(async () => view.rerender(<ReactPreview {...props} />));

    await waitFor(() => expect(regenerateNestedBuffers).toHaveBeenCalledTimes(1));
    expect(regenerateNestedBuffers).toHaveBeenCalledWith(props.content, AST);
  });
});
