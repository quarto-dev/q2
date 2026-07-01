/**
 * Capture-consumption test for the default `format: html` Preview (bd-uy4uygha).
 *
 * The sibling `ReactPreview.capture.integration.test.tsx` covers the q2-preview
 * (AST) renderer. hub-client's *default* renderer for a plain document (and
 * every website page) is `Preview`, which renders `format: html` via
 * `renderToHtml`. This test pins that `Preview` fetches the active document's
 * capture (`getBinaryDocById`) and threads its bytes into the render call as
 * `captureGzJson`, so a document executed by a connected `q2 provide-hub` shows
 * its output. The actual splicing is covered by `captureSpliceHtml.wasm.test.ts`.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import React from 'react';

const CAPTURE_BYTES = new Uint8Array([9, 8, 7, 6]);

const { renderToHtml, getBinaryDocById } = vi.hoisted(() => ({
  renderToHtml: vi.fn(async () => ({
    success: true,
    html: '<div></div>',
    diagnostics: [],
    warnings: [],
    pass1_failures: [],
  })),
  getBinaryDocById: vi.fn(async () => ({
    content: new Uint8Array([9, 8, 7, 6]),
    mimeType: 'application/x-engine-capture+gzip',
  })),
}));

vi.mock('@quarto/preview-runtime', () => ({
  renderToHtml,
  getBinaryDocById,
  isWasmReady: () => true,
  setScrollSyncEnabled: vi.fn(),
  getFileContent: vi.fn(() => null),
  getBinaryFileContent: vi.fn(() => null),
}));

vi.mock('../../hooks/usePreference', () => ({ usePreference: () => [false, vi.fn()] }));
vi.mock('../../hooks/useScrollSync', () => ({
  useScrollSync: () => ({ handlePreviewScroll: vi.fn(), handlePreviewClick: vi.fn() }),
}));
vi.mock('../../hooks/useSelectionSync', () => ({
  useSelectionSync: () => ({ handlePreviewSelection: vi.fn() }),
}));
vi.mock('@quarto/preview-renderer/iframe/MorphIframe', () => ({
  default: React.forwardRef(() => <div data-testid="morph-iframe" />),
}));
vi.mock('@quarto/preview-renderer/overlays/PreviewErrorOverlay', () => ({
  PreviewErrorOverlay: () => null,
}));
vi.mock('@quarto/preview-renderer/overlays/PreviewStaticInfoViews', () => ({
  ErrorView: () => null,
}));

import Preview from './Preview';
import type { CaptureRef } from '@quarto/preview-runtime';

function baseProps(captures?: Record<string, CaptureRef>) {
  return {
    content: '---\nformat: html\nengine: knitr\n---\n\n```{r}\n1 + 1\n```\n',
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
    captures,
  };
}

describe('Preview capture consumption (bd-uy4uygha)', () => {
  beforeEach(() => {
    renderToHtml.mockClear();
    getBinaryDocById.mockClear();
  });

  it('fetches the active doc capture and threads its bytes into the html render call', async () => {
    const captures: Record<string, CaptureRef> = {
      'doc.qmd': { captureDocId: 'cap-1', state: 'idle' },
    };

    render(<Preview {...baseProps(captures)} />);

    await waitFor(() => expect(getBinaryDocById).toHaveBeenCalledWith('cap-1'));

    await waitFor(() => {
      const withCapture = renderToHtml.mock.calls.find(
        (c) => c[0]?.captureGzJson !== undefined,
      );
      expect(withCapture, 'a render call must carry the capture bytes').toBeTruthy();
      expect(withCapture![0].captureGzJson).toEqual(CAPTURE_BYTES);
    });
  });

  it('passes no capture bytes when the active doc has no capture entry', async () => {
    render(<Preview {...baseProps(undefined)} />);

    await waitFor(() => expect(renderToHtml).toHaveBeenCalled());
    expect(getBinaryDocById).not.toHaveBeenCalled();
    for (const call of renderToHtml.mock.calls) {
      expect(call[0].captureGzJson).toBeUndefined();
    }
  });
});
