/**
 * Capture-consumption test for `ReactPreview` (bd-sfet3264, Phase 1D).
 *
 * hub-client must consume a recorded engine capture for the active document:
 * when the `captures` sidecar (threaded down from App.tsx) has an entry for
 * the active file, ReactPreview fetches that capture binary doc's bytes via
 * `getBinaryDocById` and threads them into the q2-preview render call
 * (`renderPageInProjectWithAttribution`, 4th arg) so the recorded engine
 * output is spliced into the AST.
 *
 * This test pins the wiring at the hub-client boundary (props → fetch →
 * render-call argument). The actual splicing is covered by the WASM-level
 * test `captureSplice.wasm.test.ts`.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import React from 'react';

// Distinct capture bytes the getBinaryDocById mock will return, so we can
// assert the EXACT bytes reach the render call (not just "something truthy").
const CAPTURE_BYTES = new Uint8Array([1, 2, 3, 4, 5]);

const { renderPageInProjectWithAttribution, getBinaryDocById } = vi.hoisted(() => ({
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
  getActorId: () => 'actor-1',
  regenerateNestedBuffers: vi.fn(() => ({})),
  pipelineKindForFormat: (f: string) => (f === 'q2-preview' ? 'preview' : undefined),
}));

vi.mock('../../hooks/useAttribution', () => ({
  useAttribution: () => ({ payload: null, generating: false }),
}));
vi.mock('../../hooks/usePreference', () => ({
  usePreference: () => [false, vi.fn()],
}));
vi.mock('./ReactRenderer', () => ({
  default: () => <div data-testid="react-renderer" />,
}));

import ReactPreview from './ReactPreview';
import type { CaptureRef } from '@quarto/preview-runtime';

function baseProps(captures?: Record<string, CaptureRef>) {
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
  };
}

describe('ReactPreview capture consumption (bd-sfet3264, Phase 1D)', () => {
  beforeEach(() => {
    renderPageInProjectWithAttribution.mockClear();
    getBinaryDocById.mockClear();
  });

  it('fetches the active doc capture and threads its bytes into the render call', async () => {
    const captures: Record<string, CaptureRef> = {
      'doc.qmd': { captureDocId: 'cap-1', state: 'idle' },
    };

    render(<ReactPreview {...baseProps(captures)} />);

    // The capture binary doc for the active file must be fetched by id.
    await waitFor(() => expect(getBinaryDocById).toHaveBeenCalledWith('cap-1'));

    // The fetched bytes must reach the render call as the 4th argument.
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
    // No capture sidecar ⇒ no fetch, and every render call's 4th arg is undefined.
    expect(getBinaryDocById).not.toHaveBeenCalled();
    for (const call of renderPageInProjectWithAttribution.mock.calls) {
      expect(call[3]).toBeUndefined();
    }
  });
});
