/**
 * Tests for useScrollSync — focused on the deferred editor→preview scroll.
 * The editor cursor moves the instant a keystroke lands, but the preview DOM
 * only carries fresh `data-loc` once the async render commits. So (with
 * `deferToRender`, the q2-preview path) a cursor move during an edit defers its
 * scroll until the iframe reports `AST_RENDERED` (`handleAstRendered`) — firing
 * once, against the fresh DOM. Pure navigation flushes on the debounce. The
 * HTML preview leaves `deferToRender` off and scrolls immediately.
 * `scrollToLineDeferred` lets replay drive the same mechanism with an explicit
 * line.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';
import { useScrollSync } from './useScrollSync';

// Matches RENDER_SETTLE_TIMEOUT_MS in useScrollSync.
const RENDER_SETTLE_TIMEOUT_MS = 1000;

/**
 * `topForLine`/`editorTop` back `getTopForLineNumber`/`getDomNode`, which
 * `revealEditorLine`'s alignment computation reads (see the "alignment
 * computation" section of claude-notes/plans/2026-08-22-click-align-editor-y.md).
 * Both default to 0 for tests that don't exercise the alignment arithmetic
 * (the pre-existing suites above, and U2b which only checks setPosition /
 * setSelection / focus).
 */
function makeEditor(
  line: number,
  opts: { topForLine?: number; editorTop?: number } = {},
) {
  let cursorCb: () => void = () => {};
  let contentCb: () => void = () => {};
  const editor = {
    getPosition: () => ({ lineNumber: line, column: 1 }),
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
    revealLineInCenterIfOutsideViewport: vi.fn(),
    setPosition: vi.fn(),
    setSelection: vi.fn(),
    focus: vi.fn(),
    onDidChangeCursorPosition: (cb: () => void) => {
      cursorCb = cb;
      return { dispose: vi.fn() };
    },
    onDidChangeModelContent: (cb: () => void) => {
      contentCb = cb;
      return { dispose: vi.fn() };
    },
  } as unknown as Monaco.editor.IStandaloneCodeEditor;
  return {
    editor,
    fireCursorChange: () => cursorCb(),
    fireContentChange: () => contentCb(),
  };
}

function setup(opts: {
  line: number;
  focus: boolean;
  deferToRender?: boolean;
  topForLine?: number;
  editorTop?: number;
  enabled?: boolean;
}) {
  const { editor, fireCursorChange, fireContentChange } = makeEditor(opts.line, opts);
  const editorRef = { current: editor } as RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  const editorHasFocusRef = { current: opts.focus } as RefObject<boolean>;
  const scrollPreviewToLine = vi.fn();
  const getPreviewScrollRatio = vi.fn(() => 0.5);

  const { result } = renderHook(() =>
    useScrollSync({
      editorRef,
      scrollPreviewToLine,
      getPreviewScrollRatio,
      enabled: opts.enabled ?? true,
      editorHasFocusRef,
      deferToRender: opts.deferToRender ?? true,
    }),
  );

  return { result, scrollPreviewToLine, fireCursorChange, fireContentChange, editorHasFocusRef, editor };
}

describe('useScrollSync deferred editor→preview scroll', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('navigation: flushes on the debounce when no edit is in flight', () => {
    const { scrollPreviewToLine, fireCursorChange } = setup({ line: 12, focus: true });
    act(() => { fireCursorChange(); vi.advanceTimersByTime(50); });
    expect(scrollPreviewToLine).toHaveBeenCalledExactlyOnceWith(12);
  });

  it('edit: defers past the debounce, then scrolls once when the render reports back', () => {
    const { scrollPreviewToLine, fireCursorChange, fireContentChange, result } =
      setup({ line: 42, focus: true });
    // A keystroke: content changes and the cursor moves.
    act(() => { fireContentChange(); fireCursorChange(); vi.advanceTimersByTime(50); });
    // The debounce elapsed but a render is pending, so nothing scrolled yet.
    expect(scrollPreviewToLine).not.toHaveBeenCalled();
    // Render committed → scroll exactly once, against the fresh DOM.
    act(() => { result.current.handleAstRendered(); });
    expect(scrollPreviewToLine).toHaveBeenCalledExactlyOnceWith(42);
    // A subsequent render with no new cursor move must not scroll again.
    act(() => { result.current.handleAstRendered(); });
    expect(scrollPreviewToLine).toHaveBeenCalledTimes(1);
  });

  it('does nothing on AST_RENDERED when no cursor move is pending', () => {
    const { scrollPreviewToLine, result } = setup({ line: 5, focus: true });
    act(() => { result.current.handleAstRendered(); });
    expect(scrollPreviewToLine).not.toHaveBeenCalled();
  });

  it('does not scroll on a cursor move when the editor is not focused', () => {
    const { scrollPreviewToLine, fireCursorChange } = setup({ line: 9, focus: false });
    act(() => { fireCursorChange(); vi.advanceTimersByTime(50); });
    expect(scrollPreviewToLine).not.toHaveBeenCalled();
  });

  it('safety: flushes a deferred scroll if the render never reports back', () => {
    const { scrollPreviewToLine, fireCursorChange, fireContentChange } =
      setup({ line: 7, focus: true });
    act(() => { fireContentChange(); fireCursorChange(); vi.advanceTimersByTime(50); });
    expect(scrollPreviewToLine).not.toHaveBeenCalled();
    act(() => { vi.advanceTimersByTime(RENDER_SETTLE_TIMEOUT_MS); });
    expect(scrollPreviewToLine).toHaveBeenCalledExactlyOnceWith(7);
  });

  it('scrollToLineDeferred (replay): waits for the render, scrolls to the explicit line, not focus-gated', () => {
    // Replay: editor is not focused, content changed, then replay asks to
    // scroll to the changed line. It must defer to the render and target the
    // explicit line (not the cursor), despite the editor lacking focus.
    const { scrollPreviewToLine, fireContentChange, result } =
      setup({ line: 1, focus: false });
    act(() => { fireContentChange(); result.current.scrollToLineDeferred(73); });
    expect(scrollPreviewToLine).not.toHaveBeenCalled();
    act(() => { result.current.handleAstRendered(); });
    expect(scrollPreviewToLine).toHaveBeenCalledExactlyOnceWith(73);
  });

  it('HTML path (deferToRender: false): scrolls immediately on cursor move, no render wait', () => {
    const { scrollPreviewToLine, fireCursorChange } =
      setup({ line: 20, focus: true, deferToRender: false });
    act(() => { fireCursorChange(); vi.advanceTimersByTime(50); });
    expect(scrollPreviewToLine).toHaveBeenCalledExactlyOnceWith(20);
  });
});

/**
 * `revealEditorLine` — Phase 1 of
 * claude-notes/plans/2026-08-22-click-align-editor-y.md: top/aligns the
 * clicked block's source line to the same on-screen y as the clicked block
 * (`hostY`), rather than centring it. See that plan's "alignment
 * computation" section for the arithmetic these rows pin:
 *
 *   scrollTop = getTopForLineNumber(line) - (hostY - editorTop)
 *               where editorTop = getDomNode().getBoundingClientRect().top
 *
 * clamped to [0, getScrollHeight() - getLayoutInfo().height]. `hostY`
 * omitted ⇒ top-align (decision A3). Still deliberately narrow, per the
 * original click-to-editor-scroll plan this builds on: no `setPosition`
 * (would fire `onDidChangeCursorPosition` and bounce back into
 * editor→preview sync), no `setSelection`, no `focus()`, no debounce, no
 * focus gate — pulling focus to Monaco would break the inline-edit gesture
 * the same click just started in q2-preview.
 *
 * A1a-e supersede the old U2a ("calls revealLineInCenterIfOutsideViewport
 * with the given line") — that method is no longer called at all (A1e).
 * U2b is unchanged (its assertions never touched that method). U2c/U2d/U2e
 * originally also asserted `revealLineInCenterIfOutsideViewport` was called
 * — the same now-retired premise as U2a, just not flagged when U2a was
 * named the sole exception. Rewritten here to assert `setScrollTop` instead,
 * preserving each row's actual behavioral intent (reveals despite editor
 * focus / immediately, without a debounce / suppresses the ratio-sync echo)
 * — see the 2026-08-22 plan handoff notes for why.
 */
describe('useScrollSync revealEditorLine (align, not centre — Phase 1)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('A1a: revealEditorLine(line, hostY) computes exact scrollTop = topForLine - (hostY - editorTop)', () => {
    // topForLine=400, editorTop=80, hostY=200 → 400 - (200 - 80) = 280,
    // comfortably inside [0, 600] (getScrollHeight 1000 - getLayoutInfo 400)
    // so the clamp cannot be masking a wrong unclamped value. 280 is also
    // deliberately NOT 300 — the value `syncPreviewToEditor`'s ratio path
    // would produce from this harness's fixed 0.5 ratio over the same
    // 1000/400 editor (0.5 * (1000 - 400) = 300, see U4). An implementation
    // that wrongly routed revealEditorLine through the ratio path would
    // otherwise pass this row by coincidence.
    //
    // Because `getPreviewScrollRatio` is fixed at 0.5 and the fake editor's
    // geometry is fixed at 1000/400 everywhere in this file, 300 is the
    // ONLY value any row in this describe block must avoid landing on by
    // coincidence — this collision risk is silent until someone changes the
    // fake's geometry or ratio, so re-check it then.
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 400, editorTop: 80 });
    act(() => {
      result.current.revealEditorLine(73, 200);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(280, 1);
  });

  it('A1b: clamps to 0 when the computation goes negative', () => {
    // Block near the top of the document (topForLine=100), pane low on
    // screen (hostY=500) → 100 - (500 - 50) = -350, clamped to 0.
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 100, editorTop: 50 });
    act(() => {
      result.current.revealEditorLine(3, 500);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(0, 1);
  });

  it('A1c: clamps to getScrollHeight() - getLayoutInfo().height at the upper bound', () => {
    // topForLine=2000, hostY=editorTop=50 → desired 2000, but max scroll is
    // 1000 - 400 = 600.
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 2000, editorTop: 50 });
    act(() => {
      result.current.revealEditorLine(99, 50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(600, 1);
  });

  it('A1d: hostY omitted falls back to top-aligning the line (setScrollTop(topForLine, 1))', () => {
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 222 });
    act(() => {
      result.current.revealEditorLine(50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(222, 1);
  });

  it('A1e: never calls revealLineInCenterIfOutsideViewport (replaces U2a — centring is gone, not just no-longer-asserted)', () => {
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 100, editorTop: 0 });
    act(() => {
      result.current.revealEditorLine(73, 50);
    });
    expect(editor.revealLineInCenterIfOutsideViewport).not.toHaveBeenCalled();
  });

  it('U2b: never moves the cursor, changes the selection, or steals focus', () => {
    const { result, editor } = setup({ line: 1, focus: false });
    act(() => {
      result.current.revealEditorLine(73);
    });
    expect(editor.setPosition).not.toHaveBeenCalled();
    expect(editor.setSelection).not.toHaveBeenCalled();
    expect(editor.focus).not.toHaveBeenCalled();
  });

  it('U2c: still reveals when the editor has focus (not routed through the focus-gated syncPreviewToEditor)', () => {
    // focus: true is the point of this row. syncPreviewToEditor's first
    // statement is `if (... || editorHasFocusRef.current) return` — an
    // implementation that (wrongly) delegates revealEditorLine to that
    // function would silently no-op here. With the convenient focus: false
    // default this row would pass even for that broken implementation.
    const { result, editor } = setup({ line: 1, focus: true, topForLine: 100, editorTop: 0 });
    act(() => {
      result.current.revealEditorLine(73, 50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(50, 1);
  });

  it('U2d: reveals immediately, with no debounce', () => {
    // Deliberately no vi.advanceTimersByTime anywhere in this test. The
    // suite runs under vi.useFakeTimers(), so an implementation that wraps
    // the reveal in the existing 50ms editorDebounceRef timer would leave
    // this assertion unmet until a timer advance — asserting before any
    // advance is what discriminates a debounced call from an immediate one.
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 100, editorTop: 0 });
    act(() => {
      result.current.revealEditorLine(73, 50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(50, 1);
  });

  it('A1h: still aligns when scroll-sync is disabled (enabled: false) — decision A6', () => {
    // revealEditorLine has no enabledRef check at all, unlike
    // syncPreviewToEditor / flushPendingScroll / scrollToLineDeferred, which
    // all gate on `enabled`. That started as an accident but is now a
    // deliberate decision (A6, 2026-08-22 plan): with the scroll-sync toggle
    // off, click-align is the ONLY scroll coupling left in either direction —
    // which is how the feature is best evaluated, and how a user actually
    // relies on it. This row is load-bearing, not a formality: adding an
    // enabledRef gate here looks like a plausible "consistency" fix (every
    // other scroll path in this file has one), and this is the only row that
    // would catch it — see the fail-on-revert probe in the phase report.
    const { result, editor } = setup({
      line: 1,
      focus: false,
      enabled: false,
      topForLine: 100,
      editorTop: 0,
    });
    act(() => {
      result.current.revealEditorLine(73, 50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(50, 1);
  });
});

/**
 * U2e — Task 9: `revealEditorLine` must suppress the pre-existing
 * preview→editor ratio sync for a short window after a reveal, the same way
 * `flushPendingScroll` / `syncPreviewToEditor` already suppress each other's
 * echo. Without this, a real (even tiny) `scroll` event on the preview within
 * ~50ms of the click reaches `syncPreviewToEditor` unguarded and overwrites
 * the just-completed, correct reveal with a position derived from the
 * preview's raw scroll ratio — see
 * `.superpowers/sdd/2026-08-21-preview-click-to-editor-scroll/task-8-report.md`.
 *
 * Rewritten for Phase 1 (2026-08-22 plan): the reveal itself now calls
 * `setScrollTop`, not `revealLineInCenterIfOutsideViewport`, so "the
 * suppressed scroll didn't overwrite the reveal" is asserted as "still only
 * one `setScrollTop` call", not "no `setScrollTop` call at all" (that would
 * be wrong now — the reveal's own call already happened).
 */
describe('useScrollSync revealEditorLine suppresses the ratio-sync echo (Task 9)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('U2e: a preview scroll within the suppression window does not overwrite the reveal, and the window is time-bounded', () => {
    const { result, editor } = setup({ line: 1, focus: false, topForLine: 100, editorTop: 0 });

    act(() => {
      result.current.revealEditorLine(73, 50);
    });
    // The reveal itself still happens synchronously — a fix that made
    // revealEditorLine a no-op would satisfy the assertions below vacuously.
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(50, 1);

    // A scroll event arriving within the suppression window (today: 50ms
    // debounce, then unguarded) must not echo through and add a second,
    // overwriting setScrollTop call.
    act(() => {
      result.current.handlePreviewScroll();
      vi.advanceTimersByTime(50);
    });
    expect(editor.setScrollTop).toHaveBeenCalledTimes(1);

    // The suppression must be time-bounded, not permanent — otherwise a fix
    // that disabled ratio sync forever would satisfy the row above while
    // breaking the HTML preview path (U4) in production. Advance past the
    // 300ms window, then a fresh preview scroll must resume ratio sync as
    // normal (a second, distinct setScrollTop call).
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

/**
 * U4 — regression cover for the pre-existing preview→editor ratio path
 * (`syncPreviewToEditor`, reached via `handlePreviewScroll`). Nothing in the
 * suite exercised this before; the upcoming revealEditorLine refactor touches
 * this function's neighbourhood and could silently delete its body.
 */
describe('useScrollSync handlePreviewScroll (pre-existing preview→editor ratio path)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('U4: applies the preview scroll ratio to the editor scrollTop, smoothly, when unfocused', () => {
    const { result, editor } = setup({ line: 1, focus: false });
    act(() => {
      result.current.handlePreviewScroll();
      vi.advanceTimersByTime(50);
    });
    // getPreviewScrollRatio() fakes 0.5; getScrollHeight() is 1000 and
    // getLayoutInfo().height is 400, so 0.5 * (1000 - 400) = 300. The `1` is
    // Monaco's ScrollType.Smooth — a bare toHaveBeenCalledWith(300) would
    // pass on a call with no second argument at all, so assert both.
    expect(editor.setScrollTop).toHaveBeenCalledExactlyOnceWith(300, 1);
  });

  it('U4: does not scroll the editor when it has focus (feedback-loop guard)', () => {
    const { result, editor } = setup({ line: 1, focus: true });
    act(() => {
      result.current.handlePreviewScroll();
      vi.advanceTimersByTime(50);
    });
    expect(editor.setScrollTop).not.toHaveBeenCalled();
  });
});
