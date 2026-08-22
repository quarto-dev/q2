/**
 * Tests for useSelectionSync — preview→editor selection sync, focused on the
 * Phase 2 change from `claude-notes/plans/2026-08-22-click-align-editor-y.md`:
 * `handlePreviewSelection` used to reveal with `editor.revealRangeInCenter`;
 * it now ALIGNS via a `revealEditorLine` callback threaded in from
 * `useScrollSync` (A7 — threading the callback, rather than extracting the
 * arithmetic into a shared pure helper, is what keeps this call bracketed by
 * `useScrollSync`'s own `isSyncingRef`, the flag the ratio-sync overwrite race
 * actually reads).
 *
 * There were no tests for this hook before this phase.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';
import type { SourceLocation } from '@quarto/preview-renderer/iframe/DoubleBufferedIframe';
import type { MorphIframeHandle } from '@quarto/preview-renderer/iframe/MorphIframe';
import { useSelectionSync } from './useSelectionSync';
import { useScrollSync } from './useScrollSync';

function loc(startLine: number, startCol: number, endLine: number, endCol: number): SourceLocation {
  return { fileId: 0, startLine, startCol, endLine, endCol };
}

/**
 * Minimal editor fake for the wiring-only rows: `revealEditorLine` is a bare
 * `vi.fn()` here (the alignment arithmetic itself is A1a-A1c's job in
 * `useScrollSync.test.ts`), so this fake only needs what
 * `handlePreviewSelection` touches directly: `setSelection`, `focus`, and
 * `revealRangeInCenter` (to prove it is NOT called — A1e's sibling row for
 * this hook).
 */
function makeWiringEditor() {
  let contentCb: () => void = () => {};
  const editor = {
    setSelection: vi.fn(),
    focus: vi.fn(),
    revealRangeInCenter: vi.fn(),
    onDidChangeModelContent: (cb: () => void) => {
      contentCb = cb;
      return { dispose: vi.fn() };
    },
    onDidChangeCursorSelection: () => ({ dispose: vi.fn() }),
  } as unknown as Monaco.editor.IStandaloneCodeEditor;
  return { editor, fireContentChange: () => contentCb() };
}

function setupWiring(revealEditorLine = vi.fn()) {
  const { editor } = makeWiringEditor();
  const editorRef = { current: editor } as RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  const previewRef = { current: null } as RefObject<MorphIframeHandle | null>;

  const { result } = renderHook(() =>
    useSelectionSync({ editorRef, previewRef, enabled: true, revealEditorLine }),
  );

  return { result, editor, revealEditorLine };
}

describe('useSelectionSync.handlePreviewSelection wiring to revealEditorLine (Phase 2)', () => {
  it('B1a: forwards the selection start line and hostY to revealEditorLine, exactly once', () => {
    const { result, revealEditorLine } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(loc(45, 1, 45, 1), loc(45, 1, 45, 1), 234);
    });
    expect(revealEditorLine).toHaveBeenCalledExactlyOnceWith(45, 234);
  });

  it('B1b: hostY omitted is forwarded as omitted (revealEditorLine falls back to top-align, decision A3)', () => {
    const { result, revealEditorLine } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(loc(12, 1, 12, 1), loc(12, 1, 12, 1));
    });
    expect(revealEditorLine).toHaveBeenCalledExactlyOnceWith(12, undefined);
  });

  it('B2a: still calls editor.setSelection with the same range as before this phase (A4)', () => {
    const { result, editor } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(loc(10, 2, 10, 2), loc(20, 3, 20, 3));
    });
    expect(editor.setSelection).toHaveBeenCalledExactlyOnceWith({
      startLineNumber: 10,
      startColumn: 2,
      endLineNumber: 20,
      endColumn: 3,
    });
  });

  it('B2b: still calls editor.focus() (A4 — the HTML preview keeps its focus steal)', () => {
    const { result, editor } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(loc(1, 1, 1, 1), loc(1, 1, 1, 1));
    });
    expect(editor.focus).toHaveBeenCalledOnce();
  });

  it('B2c: never calls editor.revealRangeInCenter (centring is gone, not just unasserted)', () => {
    const { result, editor } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(loc(1, 1, 1, 1), loc(1, 1, 1, 1), 50);
    });
    expect(editor.revealRangeInCenter).not.toHaveBeenCalled();
  });

  it('does nothing when either position is null (pre-existing guard, unaffected by this phase)', () => {
    const { result, revealEditorLine, editor } = setupWiring();
    act(() => {
      result.current.handlePreviewSelection(null, loc(1, 1, 1, 1));
    });
    expect(revealEditorLine).not.toHaveBeenCalled();
    expect(editor.setSelection).not.toHaveBeenCalled();
  });
});

/**
 * End-to-end alignment rows: `useSelectionSync` composed with a REAL
 * `useScrollSync().revealEditorLine`, sharing `editorRef` the same way
 * `Preview.tsx` composes them. This is what actually proves the alignment
 * arithmetic + both clamps reach `editor.setScrollTop` through the selection
 * path (mirroring A1a/A1b/A1c, not re-testing them, since the arithmetic
 * itself lives in and is already pinned by `useScrollSync.test.ts`), and
 * — the load-bearing row — that the reveal-then-overwrite race (A7) is
 * closed on this path exactly because the SAME `isSyncingRef` is shared.
 */
function makeComposedEditor(opts: { topForLine?: number; editorTop?: number } = {}) {
  let cursorPositionCb: () => void = () => {};
  let modelContentCb: () => void = () => {};
  const editor = {
    getPosition: () => ({ lineNumber: 1, column: 1 }),
    getScrollHeight: () => 1000,
    getLayoutInfo: () => ({ height: 400 }),
    getTopForLineNumber: vi.fn(() => opts.topForLine ?? 0),
    getDomNode: vi.fn(
      () =>
        ({
          getBoundingClientRect: () => ({ top: opts.editorTop ?? 0 }),
        }) as unknown as HTMLElement,
    ),
    setScrollTop: vi.fn(),
    setSelection: vi.fn(),
    focus: vi.fn(),
    revealRangeInCenter: vi.fn(),
    onDidChangeCursorPosition: (cb: () => void) => {
      cursorPositionCb = cb;
      return { dispose: vi.fn() };
    },
    onDidChangeModelContent: (cb: () => void) => {
      modelContentCb = cb;
      return { dispose: vi.fn() };
    },
    onDidChangeCursorSelection: () => ({ dispose: vi.fn() }),
  } as unknown as Monaco.editor.IStandaloneCodeEditor;
  return { editor, fireCursorPositionChange: () => cursorPositionCb(), fireModelContentChange: () => modelContentCb() };
}

function setupComposed(opts: { topForLine?: number; editorTop?: number } = {}) {
  const { editor } = makeComposedEditor(opts);
  const editorRef = { current: editor } as RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  const editorHasFocusRef = { current: false } as RefObject<boolean>;
  const previewRef = {
    current: { clearSelection: vi.fn() } as unknown as MorphIframeHandle,
  } as RefObject<MorphIframeHandle | null>;
  const getPreviewScrollRatio = vi.fn(() => 0.5);

  const { result } = renderHook(() => {
    const scroll = useScrollSync({
      editorRef,
      scrollPreviewToLine: vi.fn(),
      getPreviewScrollRatio,
      enabled: true,
      editorHasFocusRef,
      deferToRender: false,
    });
    const selection = useSelectionSync({
      editorRef,
      previewRef,
      enabled: true,
      revealEditorLine: scroll.revealEditorLine,
    });
    return { ...scroll, ...selection };
  });

  return { result, editor };
}

describe('useSelectionSync wired to a real useScrollSync().revealEditorLine (Phase 2, end-to-end)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('aligns the selected line to hostY through the real arithmetic', () => {
    // topForLine=500, editorTop=100, hostY=150 -> 500 - (150 - 100) = 450.
    // Deliberately not 300 — the value the ratio path below would produce
    // from this harness's fixed 0.5 ratio over the same 1000/400 editor
    // (see useScrollSync.test.ts's A1a note on the poisoned 300 value).
    const { result, editor } = setupComposed({ topForLine: 500, editorTop: 100 });
    act(() => {
      result.current.handlePreviewSelection(loc(20, 1, 20, 1), loc(20, 1, 20, 1), 150);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(450, 1);
  });

  it('clamps to 0 at the lower bound through the selection path', () => {
    const { result, editor } = setupComposed({ topForLine: 100, editorTop: 50 });
    act(() => {
      result.current.handlePreviewSelection(loc(3, 1, 3, 1), loc(3, 1, 3, 1), 500);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(0, 1);
  });

  it('clamps to getScrollHeight() - getLayoutInfo().height at the upper bound through the selection path', () => {
    const { result, editor } = setupComposed({ topForLine: 2000, editorTop: 50 });
    act(() => {
      result.current.handlePreviewSelection(loc(99, 1, 99, 1), loc(99, 1, 99, 1), 50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(600, 1);
  });

  it('A7: a preview scroll within the suppression window does not overwrite a selection-triggered alignment, and resumes after (HTML-path analogue of U2e)', () => {
    const { result, editor } = setupComposed({ topForLine: 500, editorTop: 100 });

    act(() => {
      result.current.handlePreviewSelection(loc(20, 1, 20, 1), loc(20, 1, 20, 1), 150);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(450, 1);

    // A preview scroll arriving within the suppression window (revealEditorLine
    // brackets useScrollSync's OWN isSyncingRef, the flag syncPreviewToEditor
    // reads) must not overwrite the alignment with a ratio-derived position.
    act(() => {
      result.current.handlePreviewScroll();
      vi.advanceTimersByTime(50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledTimes(1);

    // The suppression is time-bounded: past the 300ms window, a fresh
    // preview scroll resumes ratio sync as normal.
    act(() => {
      vi.advanceTimersByTime(300);
    });
    act(() => {
      result.current.handlePreviewScroll();
      vi.advanceTimersByTime(50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledTimes(2);
    expect(editor.setScrollTop).toHaveBeenNthCalledWith(2, 300, 1);
  });
});
