/**
 * Re-render-trigger test for `ReactPreview` (bd-4jjckvwt).
 *
 * The AST/slides preview path must re-render the ACTIVE document when a
 * *sibling* file changes — most importantly `_brand.yml`, whose edits
 * (arriving via Automerge sync) must recompile the deck's theme CSS
 * without a page reload. The non-React `Preview.tsx` (HTML path) already
 * does this by depending on the `fileContents` Map, whose identity
 * changes on every Automerge edit (App.tsx). This test locks the same
 * contract for `ReactPreview`.
 *
 * Mechanism: a sibling edit produces a fresh `fileContents` Map with the
 * SAME active-document `content`. If the re-render effect depends on
 * `fileContents`, changing only the Map identity must re-invoke the WASM
 * render (`renderPageInProjectWithAttribution` with the preview-format
 * substitution requested — the entry every preview-pipeline format uses,
 * revealjs included, since bd-kltzdhle). Before the fix the effect omitted
 * `fileContents`, so the second render never fired — the regression this
 * test guards.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import React from 'react';

// --- WASM renderer: spies. `vi.hoisted` so the (hoisted) `vi.mock`
//     factory below can reference them. `renderPageForPreview` is the
//     q2-preview SPA's entry point and must NOT be called from hub-client
//     (it hard-codes the native-preview render host). ---
const { renderPageInProjectWithAttribution, renderPageForPreview } = vi.hoisted(() => ({
  renderPageForPreview: vi.fn(),
  renderPageInProjectWithAttribution: vi.fn(async () => ({
    success: true,
    ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    untransformed_ast_json: '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[]}',
    theme_fingerprint: 'fp-1',
    diagnostics: [],
    warnings: [],
  })),
}));

vi.mock('@quarto/preview-runtime', () => ({
  renderPageForPreview,
  renderPageInProjectWithAttribution,
  parseQmdToAstWithAttribution: vi.fn(async () => ({ success: true, ast: '{}', diagnostics: [] })),
  isWasmReady: () => true,
  incrementalWriteQmd: vi.fn(),
  applyNodeEdit: vi.fn(),
  parseQmdContentSync: vi.fn(() => ({ success: true, ast: '{}' })),
  getActorId: () => 'actor-1',
  regenerateNestedBuffers: vi.fn(() => ({})),
  // revealjs takes doRender's isSlidesPreview branch regardless, but the
  // dispatch still calls this helper, so return the real-ish mapping.
  pipelineKindForFormat: (f: string) => (f === 'q2-preview' ? 'preview' : undefined),
}));

// Hooks: keep them inert so the test isolates the render-trigger effect.
vi.mock('../../hooks/useAttribution', () => ({
  useAttribution: () => ({ payload: null, generating: false }),
}));
vi.mock('../../hooks/usePreference', () => ({
  usePreference: () => [false, vi.fn()],
}));

// Don't mount the real iframe renderer — this test is about the render
// trigger, not the downstream display.
vi.mock('./ReactRenderer', () => ({
  default: () => <div data-testid="react-renderer" />,
}));

import ReactPreview from './ReactPreview';

function baseProps(fileContents: Map<string, string>) {
  return {
    content: '---\nformat: revealjs\n---\n\n## A slide\n',
    currentFile: { path: 'slides.qmd', name: 'slides.qmd' } as any,
    files: [],
    fileContents,
    scrollSyncEnabled: false,
    editorRef: { current: null } as any,
    editorReady: true,
    editorHasFocusRef: { current: false } as any,
    onFileChange: () => {},
    onOpenNewFileDialog: () => {},
    onDiagnosticsChange: () => {},
    onContentRewrite: () => {},
    format: 'revealjs',
    attributionOn: false,
  };
}

describe('ReactPreview re-render on sibling change (bd-4jjckvwt)', () => {
  beforeEach(() => {
    renderPageInProjectWithAttribution.mockClear();
    renderPageForPreview.mockClear();
  });

  it('re-renders the active deck when a sibling file (e.g. _brand.yml) changes', async () => {
    // Initial fileContents: active doc + a sibling _brand.yml.
    const props = baseProps(
      new Map([
        ['slides.qmd', '---\nformat: revealjs\n---\n\n## A slide\n'],
        ['_brand.yml', 'color:\n  primary: "#ff0000"\n'],
      ]),
    );

    const { rerender } = render(<ReactPreview {...props} />);

    // Mount render fires once — through the hub-client entry point with
    // the preview-format substitution requested (5th arg), never through
    // the SPA-only `renderPageForPreview` (bd-kltzdhle, D3).
    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(1));
    expect(renderPageInProjectWithAttribution.mock.calls[0][4]).toBe(true);
    expect(renderPageForPreview).not.toHaveBeenCalled();

    // A sibling edit: `_brand.yml` changed → App.tsx mints a NEW Map
    // identity. The active document's `content` is unchanged; only the
    // Map identity differs.
    const afterBrandEdit = new Map([
      ['slides.qmd', props.content],
      ['_brand.yml', 'color:\n  primary: "#00ff00"\n'],
    ]);

    rerender(<ReactPreview {...props} fileContents={afterBrandEdit} />);

    // The deck must re-render (→ recompile theme CSS with the new brand).
    // Pre-fix the effect omitted `fileContents`, so this never fired.
    await waitFor(() => expect(renderPageInProjectWithAttribution).toHaveBeenCalledTimes(2));
  });
});
