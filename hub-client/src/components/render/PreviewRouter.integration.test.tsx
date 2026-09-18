/**
 * Routing test for `PreviewRouter` (bd-kltzdhle).
 *
 * The router probes the document (`parseQmdToAst` → `meta.format` →
 * `getQ2Format`) and mounts either `ReactPreview` (React AST renderer,
 * Q2PreviewIframe) or `Preview` (full-DOM MorphIframe). It also echoes
 * the resolved format up through `onFormatChange`, which `Editor.tsx`
 * uses to enable the Edit / Authors pills and the printable-document
 * affordance. Both halves are pinned here for the three cases the plan
 * cares about:
 *
 *   - a document with no `format:` key (the WASM reports `html`) →
 *     `ReactPreview` with `format='q2-preview'`, `onFormatChange('q2-preview')`;
 *   - `format: q2-html-render` → `Preview`, `onFormatChange(null)`;
 *   - `format: q2-debug` → `ReactPreview` with `format='q2-debug'` (the
 *     pass-through arm, unchanged).
 *
 * Both preview components are mocked to sentinels that capture their
 * props; the WASM probe is mocked to return an AST whose `meta.format`
 * is whatever the normalised detection would have produced.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import React from 'react';

const capturedReactPreviewProps: any[] = [];
const capturedPreviewProps: any[] = [];

// The probe result is set per test. `meta.format` is what
// `MetadataMergeStage` writes: the normalised `Format::target_format`.
let probeFormat = 'html';

vi.mock('@quarto/preview-runtime', () => ({
  parseQmdToAst: vi.fn(async () => ({
    success: true,
    ast: JSON.stringify({
      'pandoc-api-version': [1, 23, 1],
      meta: { format: { t: 'MetaString', c: probeFormat } },
      blocks: [],
    }),
  })),
  isWasmReady: () => true,
  initWasm: vi.fn(async () => {}),
}));

vi.mock('./ReactPreview', () => ({
  default: (props: any) => {
    capturedReactPreviewProps.push(props);
    return <div data-testid="react-preview" />;
  },
}));

vi.mock('./Preview', () => ({
  default: (props: any) => {
    capturedPreviewProps.push(props);
    return <div data-testid="full-dom-preview" />;
  },
}));

import PreviewRouter from './PreviewRouter';

function baseProps(content: string, onFormatChange: (f: string | null) => void) {
  return {
    content,
    currentFile: { path: 'doc.qmd', name: 'doc.qmd' } as any,
    files: [],
    fileContents: new Map([['doc.qmd', content]]),
    scrollSyncEnabled: false,
    editorRef: { current: null } as any,
    editorReady: true,
    editorHasFocusRef: { current: false } as any,
    onFileChange: () => {},
    onOpenNewFileDialog: () => {},
    onDiagnosticsChange: () => {},
    onContentRewrite: () => {},
    onFormatChange,
    attributionOn: false,
  };
}

describe('PreviewRouter renderer dispatch (bd-kltzdhle)', () => {
  beforeEach(() => {
    capturedReactPreviewProps.length = 0;
    capturedPreviewProps.length = 0;
  });

  it('mounts ReactPreview as q2-preview for a document with no format: key', async () => {
    probeFormat = 'html';
    const onFormatChange = vi.fn();
    render(<PreviewRouter {...baseProps('# Hello\n\nPlain document.\n', onFormatChange)} />);

    await waitFor(() => expect(screen.getByTestId('react-preview')).toBeTruthy());
    expect(capturedPreviewProps).toHaveLength(0);
    expect(capturedReactPreviewProps.at(-1).format).toBe('q2-preview');
    expect(onFormatChange).toHaveBeenLastCalledWith('q2-preview');
  });

  it('mounts the full-DOM Preview for format: q2-html-render', async () => {
    probeFormat = 'q2-html-render';
    const onFormatChange = vi.fn();
    render(
      <PreviewRouter
        {...baseProps('---\nformat: q2-html-render\n---\n\nOpt-out.\n', onFormatChange)}
      />,
    );

    await waitFor(() => expect(screen.getByTestId('full-dom-preview')).toBeTruthy());
    expect(capturedReactPreviewProps).toHaveLength(0);
    expect(onFormatChange).toHaveBeenLastCalledWith(null);
  });

  it('passes q2-debug through to ReactPreview unchanged', async () => {
    probeFormat = 'q2-debug';
    const onFormatChange = vi.fn();
    render(
      <PreviewRouter {...baseProps('---\nformat: q2-debug\n---\n\nDebug.\n', onFormatChange)} />,
    );

    await waitFor(() => expect(screen.getByTestId('react-preview')).toBeTruthy());
    expect(capturedReactPreviewProps.at(-1).format).toBe('q2-debug');
    expect(onFormatChange).toHaveBeenLastCalledWith('q2-debug');
  });
});
