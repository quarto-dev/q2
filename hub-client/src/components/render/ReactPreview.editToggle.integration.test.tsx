/**
 * Edit toggle at the hub-client boundary (bd-ew0vak6b).
 *
 * The bottom-bar Edit pill persists as the `previewEditing` preference.
 * `ReactPreview` reads it directly (like `richText`) and must:
 *
 *  1. forward `editingDisabled={!previewEditing}` to `ReactRenderer`, so the
 *     q2-preview iframe drops every edit affordance when editing is off; and
 *  2. drop any `PreviewNodeEditPayload` that still arrives through `setAst`
 *     while editing is off (defense in depth, mirroring the q2-preview SPA's
 *     `handleSetAst` guard) — no `applyNodeEdit`, no `onContentRewrite`.
 *
 * With editing on, the existing commit path runs unchanged.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, act } from '@testing-library/react';
import React from 'react';

const AST = '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}';

const { renderPageInProjectWithAttribution, applyNodeEdit } = vi.hoisted(() => ({
  renderPageInProjectWithAttribution: vi.fn(async () => ({
    success: true,
    ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    untransformed_ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    theme_fingerprint: 'fp-1',
    diagnostics: [],
    warnings: [],
  })),
  applyNodeEdit: vi.fn(() => 'rewritten qmd'),
}));

vi.mock('@quarto/preview-runtime', () => ({
  renderPageInProjectWithAttribution,
  getBinaryDocById: vi.fn(),
  renderPageForPreview: vi.fn(),
  parseQmdToAstWithAttribution: vi.fn(async () => ({ success: true, ast: '{}', diagnostics: [] })),
  isWasmReady: () => true,
  incrementalWriteQmd: vi.fn(),
  applyNodeEdit,
  parseQmdContentSync: vi.fn(() => ({ success: true, ast: '{}' })),
  getActorId: () => 'actor-1',
  regenerateNestedBuffers: vi.fn(() => ({})),
  pipelineKindForFormat: (f: string) => (f === 'q2-preview' ? 'preview' : undefined),
}));

vi.mock('../../hooks/useAttribution', () => ({
  useAttribution: () => ({ payload: null, generating: false }),
}));

// Per-key preference values so `previewEditing` can be varied per test
// while the other persisted flags keep their production defaults.
const prefs: Record<string, unknown> = {
  errorOverlayCollapsed: true,
  unlockNestingCursor: false,
  richText: true,
  previewEditing: true,
};
vi.mock('../../hooks/usePreference', () => ({
  usePreference: (key: string) => [prefs[key], vi.fn()],
}));

// Capture the props ReactPreview hands to ReactRenderer so the test can
// both read `editingDisabled` and drive `setAst` the way the iframe would.
const rendererProps: any[] = [];
vi.mock('./ReactRenderer', () => ({
  default: (props: any) => {
    rendererProps.push(props);
    return <div data-testid="react-renderer" />;
  },
}));

import ReactPreview from './ReactPreview';

function baseProps(onContentRewrite: (qmd: string) => void) {
  return {
    content: '---\nformat: q2-preview\n---\n\nhello\n',
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
    onContentRewrite,
    format: 'q2-preview',
    attributionOn: false,
  };
}

const EDIT_PAYLOAD = {
  __isPreviewNodeEdit: true,
  channel: 'text',
  newText: 'hello edited',
  destinationSourceInfoJson: '{"t":0,"r":[0,5],"d":0}',
};

async function mountAndSettle(onContentRewrite: (qmd: string) => void) {
  render(<ReactPreview {...baseProps(onContentRewrite)} />);
  // The renderer mounts once the first render lands (previewState GOOD).
  await waitFor(() => expect(rendererProps.length).toBeGreaterThan(0));
  return () => rendererProps.at(-1);
}

describe('ReactPreview edit toggle (bd-ew0vak6b)', () => {
  beforeEach(() => {
    rendererProps.length = 0;
    applyNodeEdit.mockClear();
    renderPageInProjectWithAttribution.mockClear();
    prefs.previewEditing = true;
  });

  it('forwards editingDisabled=false to ReactRenderer when previewEditing is on', async () => {
    const latest = await mountAndSettle(() => {});
    expect(latest().editingDisabled).toBe(false);
  });

  it('forwards editingDisabled=true to ReactRenderer when previewEditing is off', async () => {
    prefs.previewEditing = false;
    const latest = await mountAndSettle(() => {});
    expect(latest().editingDisabled).toBe(true);
  });

  it('drops a PreviewNodeEditPayload while editing is off (no applyNodeEdit, no rewrite)', async () => {
    prefs.previewEditing = false;
    const onContentRewrite = vi.fn();
    const latest = await mountAndSettle(onContentRewrite);

    await act(async () => {
      latest().setAst(EDIT_PAYLOAD);
    });

    expect(applyNodeEdit).not.toHaveBeenCalled();
    expect(onContentRewrite).not.toHaveBeenCalled();
  });

  it('applies a PreviewNodeEditPayload while editing is on (regression baseline)', async () => {
    const onContentRewrite = vi.fn();
    const latest = await mountAndSettle(onContentRewrite);

    await act(async () => {
      latest().setAst(EDIT_PAYLOAD);
    });

    expect(applyNodeEdit).toHaveBeenCalledOnce();
    expect(onContentRewrite).toHaveBeenCalledWith('rewritten qmd');
  });
});
