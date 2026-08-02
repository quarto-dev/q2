/**
 * Tests for useAutomergeSync hook
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

// Mock automergeSync service (now @quarto/preview-runtime; also home of
// diffToEditorChanges since bd-ov4gqk3m)
vi.mock('@quarto/preview-runtime', () => ({
  getFileContent: vi.fn(),
  setImmediateFileChangeCallback: vi.fn(),
  diffToEditorChanges: vi.fn(),
}));

// Mock diffToMonacoEdits
vi.mock('../utils/diffToMonacoEdits', () => ({
  diffToMonacoEdits: vi.fn(),
}));

import { useAutomergeSync } from './useAutomergeSync';
import {
  getFileContent,
  setImmediateFileChangeCallback,
  diffToEditorChanges,
} from '@quarto/preview-runtime';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { getEditorTextProvider } from '../services/editorDebugRegistry';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { setVisibility, resetVisibility, fireWindowFocus } from '../test-utils/visibility';

const mockGetFileContent = vi.mocked(getFileContent);
const mockSetImmediateFileChangeCallback = vi.mocked(setImmediateFileChangeCallback);
const mockDiffToMonacoEdits = vi.mocked(diffToMonacoEdits);
const mockDiffToEditorChanges = vi.mocked(diffToEditorChanges);

// Mock Monaco editor
function createMockEditor(initialContent = '') {
  let content = initialContent;
  const model = {
    getValue: () => content,
  };
  return {
    getModel: () => model,
    executeEdits: vi.fn((_source: string, edits: Array<{ text: string }>) => {
      // Simulate edit: for simplicity, just update content for single full-replace edits
      if (edits.length > 0 && edits[0].text !== undefined) {
        content = edits[0].text;
      }
    }),
    _setContent: (c: string) => { content = c; },
  };
}

const testFile: FileEntry = { path: 'test.qmd' };

function defaultOptions(overrides: Partial<Parameters<typeof useAutomergeSync>[0]> = {}) {
  return {
    currentFile: testFile,
    fileContents: new Map([['test.qmd', '# Hello']]),
    onContentOperations: vi.fn(),
    replayActiveRef: { current: false },
    replayIsActive: false,
    ...overrides,
  };
}

describe('useAutomergeSync', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetFileContent.mockReturnValue('# Hello');
    mockDiffToMonacoEdits.mockReturnValue([]);
    mockDiffToEditorChanges.mockReturnValue([]);
  });

  afterEach(() => {
    resetVisibility();
    vi.restoreAllMocks();
  });

  describe('initialization', () => {
    it('should register immediate callback on mount', () => {
      renderHook(() => useAutomergeSync(defaultOptions()));
      expect(mockSetImmediateFileChangeCallback).toHaveBeenCalledWith(expect.any(Function));
    });

    it('should unregister immediate callback on unmount', () => {
      const { unmount } = renderHook(() => useAutomergeSync(defaultOptions()));
      unmount();
      expect(mockSetImmediateFileChangeCallback).toHaveBeenLastCalledWith(null);
    });

    it('should not register callback when currentFile is null', () => {
      renderHook(() => useAutomergeSync(defaultOptions({ currentFile: null })));
      // Called with cleanup only, no function
      expect(mockSetImmediateFileChangeCallback).not.toHaveBeenCalledWith(expect.any(Function));
    });
  });

  describe('reconciliation on mount/file-switch', () => {
    it('should set content from Automerge when Monaco is not ready', () => {
      mockGetFileContent.mockReturnValue('# Updated');

      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({
          fileContents: new Map([['test.qmd', '# Updated']]),
        }))
      );

      // Without an editor mounted, content should be synced via React state
      expect(result.current.content).toBe('# Updated');
    });

    it('should apply diffs when Monaco content differs from Automerge', () => {
      const mockEditor = createMockEditor('# Old');
      const fakeEdits = [{ range: {}, text: '# New', forceMoveMarkers: true }];
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      // Start with matching content so the initial effect doesn't diff
      mockGetFileContent.mockReturnValue('# Old');

      const { result, rerender } = renderHook(
        (props) => useAutomergeSync(props),
        { initialProps: defaultOptions({ fileContents: new Map([['test.qmd', '# Old']]) }) }
      );

      // Mount the editor
      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      // Now simulate a remote change
      mockGetFileContent.mockReturnValue('# New');
      mockDiffToMonacoEdits.mockClear();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      rerender(defaultOptions({ fileContents: new Map([['test.qmd', '# New']]) }));

      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('# Old', '# New');
      expect(mockEditor.executeEdits).toHaveBeenCalledWith('remote-sync', fakeEdits);
    });

    it('should skip sync during replay mode', () => {
      mockGetFileContent.mockReturnValue('# Remote');

      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({
          replayIsActive: true,
          replayActiveRef: { current: true },
        }))
      );

      // Content should remain empty (initial state), not synced from Automerge
      expect(result.current.content).toBe('');
    });

    it('reconciles into Monaco when the editor mounts after content arrived (blank-editor regression)', () => {
      // Regression (bd-eakukmlr): content can arrive before onMount stores the
      // editor ref, and the editor is created from a stale empty defaultValue.
      // Mounting must itself reconcile, or the editor stays blank until reload.
      mockGetFileContent.mockReturnValue('# Hello');
      const fakeEdits = [{ range: {}, text: '# Hello', forceMoveMarkers: true }];

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      // Content reconciled into React state while Monaco is absent
      expect(result.current.content).toBe('# Hello');

      // Monaco mounts late, created from the stale (empty) defaultValue
      const mockEditor = createMockEditor('');
      mockDiffToMonacoEdits.mockClear();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('', '# Hello');
      expect(mockEditor.executeEdits).toHaveBeenCalledWith('remote-sync', fakeEdits);
    });

    it('reconciles a fresh editor instance on key remount (stale ref to disposed editor)', () => {
      // Every mount must reconcile, not just the first: after a file-switch
      // key remount, a new empty editor replaces the disposed one.
      mockGetFileContent.mockReturnValue('# Hello');
      const fakeEdits = [{ range: {}, text: '# Hello', forceMoveMarkers: true }];

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      const editor1 = createMockEditor('# Hello');
      act(() => {
        result.current.onEditorMount(editor1 as never);
      });

      // Remount: a fresh, empty editor instance replaces the disposed one
      const editor2 = createMockEditor('');
      mockDiffToMonacoEdits.mockClear();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      act(() => {
        result.current.onEditorMount(editor2 as never);
      });

      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('', '# Hello');
      expect(editor2.executeEdits).toHaveBeenCalledWith('remote-sync', fakeEdits);
    });
  });

  describe('real-time remote edits', () => {
    it('should apply edits when callback fires for current file', () => {
      const mockEditor = createMockEditor('# Hello');
      const fakeEdits = [{ range: {}, text: 'X', forceMoveMarkers: true }];
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      // Get the registered callback
      const registeredCallback = mockSetImmediateFileChangeCallback.mock.calls.find(
        call => typeof call[0] === 'function'
      )?.[0];
      expect(registeredCallback).toBeDefined();

      // Simulate a remote change
      act(() => {
        registeredCallback!('test.qmd', '# Hello World');
      });

      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('# Hello', '# Hello World');
      expect(mockEditor.executeEdits).toHaveBeenCalledWith('remote-sync', fakeEdits);
    });

    it('should ignore callback for different file', () => {
      const mockEditor = createMockEditor('# Hello');

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      const registeredCallback = mockSetImmediateFileChangeCallback.mock.calls.find(
        call => typeof call[0] === 'function'
      )?.[0];

      act(() => {
        registeredCallback!('other.qmd', '# Other');
      });

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('should skip callback during replay', () => {
      const mockEditor = createMockEditor('# Hello');
      const replayActiveRef = { current: false };

      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ replayActiveRef }))
      );

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      const registeredCallback = mockSetImmediateFileChangeCallback.mock.calls.find(
        call => typeof call[0] === 'function'
      )?.[0];

      // Activate replay after registration
      replayActiveRef.current = true;

      act(() => {
        registeredCallback!('test.qmd', '# Changed');
      });

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('should skip callback when content matches (local change)', () => {
      const mockEditor = createMockEditor('# Hello');

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      const registeredCallback = mockSetImmediateFileChangeCallback.mock.calls.find(
        call => typeof call[0] === 'function'
      )?.[0];

      // Content matches Monaco — this is a local change echo
      act(() => {
        registeredCallback!('test.qmd', '# Hello');
      });

      expect(mockDiffToMonacoEdits).not.toHaveBeenCalled();
    });
  });

  describe('handleEditorChange (Monaco → Automerge)', () => {
    it('should propagate changes to onContentOperations', () => {
      const onContentOperations = vi.fn();
      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ onContentOperations }))
      );

      const mockEvent = {
        changes: [{ rangeOffset: 0, rangeLength: 0, text: 'a' }],
      };

      act(() => {
        result.current.handleEditorChange('# Hello a', mockEvent as never);
      });

      expect(onContentOperations).toHaveBeenCalledWith('test.qmd', mockEvent.changes);
    });

    it('should not propagate when applyingRemoteRef is true (echo prevention)', () => {
      const onContentOperations = vi.fn();
      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ onContentOperations }))
      );

      // Simulate applying remote edits
      result.current.applyingRemoteRef.current = true;

      const mockEvent = { changes: [{ rangeOffset: 0, rangeLength: 0, text: 'x' }] };
      act(() => {
        result.current.handleEditorChange('# Hello x', mockEvent as never);
      });

      expect(onContentOperations).not.toHaveBeenCalled();
    });

    it('should not propagate during replay mode', () => {
      const onContentOperations = vi.fn();
      const replayActiveRef = { current: true };

      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ onContentOperations, replayActiveRef, replayIsActive: true }))
      );

      const mockEvent = { changes: [{ rangeOffset: 0, rangeLength: 0, text: 'x' }] };
      act(() => {
        result.current.handleEditorChange('# Hello x', mockEvent as never);
      });

      expect(onContentOperations).not.toHaveBeenCalled();
    });

    it('should ignore undefined value', () => {
      const onContentOperations = vi.fn();
      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ onContentOperations }))
      );

      const mockEvent = { changes: [] };
      act(() => {
        result.current.handleEditorChange(undefined, mockEvent as never);
      });

      expect(onContentOperations).not.toHaveBeenCalled();
    });

    it('should ignore when no current file', () => {
      const onContentOperations = vi.fn();
      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ currentFile: null, onContentOperations }))
      );

      const mockEvent = { changes: [{ rangeOffset: 0, rangeLength: 0, text: 'x' }] };
      act(() => {
        result.current.handleEditorChange('text', mockEvent as never);
      });

      expect(onContentOperations).not.toHaveBeenCalled();
    });

    it('should maintain stable identity across re-renders (prevents listener churn)', () => {
      // Regression: unstable handleEditorChange caused @monaco-editor/react to
      // dispose and re-subscribe its onDidChangeModelContent listener on every
      // render, which could race with keystrokes and drop the first character
      // typed after a selection.
      const onContentOperations = vi.fn();
      const { result, rerender } = renderHook(
        (props) => useAutomergeSync(props),
        { initialProps: defaultOptions({ onContentOperations }) }
      );

      const first = result.current.handleEditorChange;

      // Re-render with a different fileContents map (simulates remote edit to
      // another file, presence state update, or any other unrelated state change).
      rerender(defaultOptions({
        onContentOperations,
        fileContents: new Map([['test.qmd', '# Hello'], ['other.qmd', '# Other']]),
      }));

      expect(result.current.handleEditorChange).toBe(first);

      // Re-render again with yet another map.
      rerender(defaultOptions({
        onContentOperations,
        fileContents: new Map([['test.qmd', '# Hello changed']]),
      }));

      expect(result.current.handleEditorChange).toBe(first);
    });

    it('should read latest currentFile via ref after re-render', () => {
      // Ensures the stable callback picks up file switches through the ref,
      // not through a stale closure.
      const onContentOperations = vi.fn();
      const file1: FileEntry = { path: 'file1.qmd' };
      const file2: FileEntry = { path: 'file2.qmd' };

      const { result, rerender } = renderHook(
        (props) => useAutomergeSync(props),
        { initialProps: defaultOptions({ currentFile: file1, onContentOperations }) }
      );

      const stableCallback = result.current.handleEditorChange;

      // Switch to a different file
      rerender(defaultOptions({ currentFile: file2, onContentOperations }));

      // Identity must not change
      expect(result.current.handleEditorChange).toBe(stableCallback);

      // But the callback must use the NEW file path
      const mockEvent = { changes: [{ rangeOffset: 0, rangeLength: 0, text: 'x' }] };
      act(() => {
        result.current.handleEditorChange('x', mockEvent as never);
      });

      expect(onContentOperations).toHaveBeenCalledWith('file2.qmd', mockEvent.changes);
    });
  });

  describe('handleContentRewrite', () => {
    it('should compute diff and apply operations via onContentOperations', () => {
      const options = defaultOptions();
      const fakeChanges = [{ offset: 0, length: 7, text: '# New Content' }];
      mockDiffToEditorChanges.mockReturnValue(fakeChanges as never);

      const { result } = renderHook(() => useAutomergeSync(options));

      act(() => {
        result.current.handleContentRewrite('# New Content');
      });

      expect(mockDiffToEditorChanges).toHaveBeenCalledWith('# Hello', '# New Content');
      expect(options.onContentOperations).toHaveBeenCalledWith('test.qmd', fakeChanges);
    });

    it('should be a no-op when diff produces no changes', () => {
      const options = defaultOptions();
      mockDiffToEditorChanges.mockReturnValue([]);

      const { result } = renderHook(() => useAutomergeSync(options));

      act(() => {
        result.current.handleContentRewrite('# Hello');
      });

      expect(options.onContentOperations).not.toHaveBeenCalled();
    });

    it('should skip when diff produces no edits', () => {
      const options = defaultOptions();
      mockDiffToEditorChanges.mockReturnValue([]);

      const { result } = renderHook(() => useAutomergeSync(options));

      act(() => {
        result.current.handleContentRewrite('# Same');
      });

      expect(options.onContentOperations).not.toHaveBeenCalled();
    });
  });

  describe('setContent', () => {
    it('should allow external content updates (file switching)', () => {
      // Use a null file so the async fallback effect doesn't overwrite
      const { result } = renderHook(() =>
        useAutomergeSync(defaultOptions({ currentFile: null }))
      );

      act(() => {
        result.current.setContent('# Different File');
      });

      expect(result.current.content).toBe('# Different File');
    });
  });

  // ── Visibility gating ───────────────────────────────────────────────────
  //
  // Backgrounded tabs queue remote Automerge change events; on refocus they
  // drain and each one applies a separate executeEdits, producing a visible
  // "replay animation". The gate stashes the latest content while hidden and
  // flushes once on visibilitychange→visible (or window focus, for browsers
  // that skip visibilitychange). See
  // claude-notes/plans/2026-04-24-hub-client-visibility-gating.md.
  describe('visibility gating', () => {
    function fakeEdits(text: string) {
      return [{ range: {}, text, forceMoveMarkers: true }] as never;
    }

    function getRegisteredCallback(): (path: string, newContent: string) => void {
      // The latest registered handler (effects re-run on currentFile change).
      const calls = mockSetImmediateFileChangeCallback.mock.calls;
      for (let i = calls.length - 1; i >= 0; i--) {
        if (typeof calls[i][0] === 'function') {
          return calls[i][0] as (path: string, newContent: string) => void;
        }
      }
      throw new Error('no immediate callback registered');
    }

    // Stable opts avoid the reconciliation effect re-firing on every render
    // from a fresh fileContents Map reference, which would inflate
    // executeEdits counts. Each test builds its own baseline then reuses it.
    function setup(initialContent: string) {
      mockGetFileContent.mockReturnValue(initialContent);
      const opts = defaultOptions({
        fileContents: new Map([['test.qmd', initialContent]]),
      });
      const mockEditor = createMockEditor(initialContent);
      const { result, unmount } = renderHook(() => useAutomergeSync(opts));
      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });
      return { result, unmount, mockEditor };
    }

    it('baseline: when visible, each remote change applies one executeEdits', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      mockDiffToMonacoEdits.mockImplementation((from: string, to: string) => fakeEdits(to));

      act(() => cb('test.qmd', '# B'));
      act(() => cb('test.qmd', '# C'));
      act(() => cb('test.qmd', '# D'));

      expect(mockEditor.executeEdits).toHaveBeenCalledTimes(3);
    });

    it('hidden: successive remote changes produce zero executeEdits calls', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('should not apply'));

      act(() => setVisibility('hidden'));
      act(() => cb('test.qmd', '# B'));
      act(() => cb('test.qmd', '# C'));
      act(() => cb('test.qmd', '# D'));

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('flush on visible: exactly one executeEdits, diffed against latest stashed content', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      act(() => setVisibility('hidden'));
      act(() => cb('test.qmd', '# B'));
      act(() => cb('test.qmd', '# C'));
      act(() => cb('test.qmd', '# D'));

      mockDiffToMonacoEdits.mockClear();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# D'));

      act(() => setVisibility('visible'));

      expect(mockEditor.executeEdits).toHaveBeenCalledTimes(1);
      expect(mockEditor.executeEdits).toHaveBeenCalledWith('remote-sync', fakeEdits('# D'));
      // diff is between current Monaco value ('# A') and the latest stashed content ('# D')
      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('# A', '# D');
    });

    it('flush on visible: second hide→visible with no stash produces zero calls', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# D'));
      act(() => setVisibility('hidden'));
      act(() => cb('test.qmd', '# D'));
      act(() => setVisibility('visible'));

      expect(mockEditor.executeEdits).toHaveBeenCalledTimes(1);

      // Second hide→visible cycle with no stashed content.
      mockEditor.executeEdits.mockClear();
      act(() => setVisibility('hidden'));
      act(() => setVisibility('visible'));

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('file switch while hidden: no stale edit for old file is applied', () => {
      const mockEditor = createMockEditor('# A');
      const fileA: FileEntry = { path: 'a.qmd' };
      const fileB: FileEntry = { path: 'b.qmd' };
      mockGetFileContent.mockImplementation((p: string) =>
        p === 'a.qmd' ? '# A' : '# B'
      );

      const optsA = defaultOptions({
        currentFile: fileA,
        fileContents: new Map([['a.qmd', '# A']]),
      });
      const optsB = defaultOptions({
        currentFile: fileB,
        fileContents: new Map([['b.qmd', '# B']]),
      });

      const { result, rerender } = renderHook(
        (props: typeof optsA) => useAutomergeSync(props),
        { initialProps: optsA }
      );
      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      act(() => setVisibility('hidden'));
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# A (stale remote)'));
      act(() => getRegisteredCallback()('a.qmd', '# A (stale remote)'));

      // Simulate the file-switch code separately updating Monaco to fileB's
      // content (that happens via a different code path in the app). Keep
      // the diff mock returning [] so reconciliation doesn't fire an edit.
      mockEditor._setContent('# B');
      mockDiffToMonacoEdits.mockReturnValue([]);

      rerender(optsB);
      act(() => setVisibility('visible'));

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('cleanup on unmount removes visibilitychange and focus listeners', () => {
      const docRemove = vi.spyOn(document, 'removeEventListener');
      const winRemove = vi.spyOn(window, 'removeEventListener');

      const { unmount } = setup('# A');
      unmount();

      const visCall = docRemove.mock.calls.find((c) => c[0] === 'visibilitychange');
      const focusCall = winRemove.mock.calls.find((c) => c[0] === 'focus');
      expect(visCall).toBeDefined();
      expect(focusCall).toBeDefined();
    });

    it('React content state still updates while hidden (for Preview etc.)', () => {
      const { result, mockEditor } = setup('# A');
      const cb = getRegisteredCallback();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# Hidden new'));

      act(() => setVisibility('hidden'));
      act(() => cb('test.qmd', '# Hidden new'));

      expect(result.current.content).toBe('# Hidden new');
      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('focus-only flush: window focus without visibilitychange triggers exactly one executeEdits', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      act(() => setVisibility('hidden'));
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# D'));
      act(() => cb('test.qmd', '# B'));
      act(() => cb('test.qmd', '# D'));

      mockDiffToMonacoEdits.mockClear();
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# D'));

      // Simulate the documented cases where visibilitychange never fires:
      // fire focus while visibilityState still reports 'hidden'.
      act(() => fireWindowFocus());

      expect(mockEditor.executeEdits).toHaveBeenCalledTimes(1);
      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('# A', '# D');

      // Ref was cleared by the flush: a subsequent focus is a no-op.
      mockEditor.executeEdits.mockClear();
      act(() => fireWindowFocus());
      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
    });

    it('idempotent double-fire: visibilitychange + focus produces exactly one executeEdits', () => {
      const { mockEditor } = setup('# A');
      const cb = getRegisteredCallback();

      act(() => setVisibility('hidden'));
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits('# D'));
      act(() => cb('test.qmd', '# D'));

      act(() => setVisibility('visible'));
      act(() => fireWindowFocus());

      expect(mockEditor.executeEdits).toHaveBeenCalledTimes(1);
    });
  });

  // quartoDebug.am.doctor() reads the live Monaco text through this
  // provider (bd-6ogrov5r).
  describe('editor debug registry', () => {
    it('exposes the current path and model text while mounted, and unregisters on unmount', () => {
      const { result, unmount } = renderHook(() =>
        useAutomergeSync(defaultOptions()),
      );

      const provider = getEditorTextProvider();
      expect(provider).not.toBeNull();
      // Before an editor mounts there is no model to read.
      expect(provider!.getPath()).toBe('test.qmd');
      expect(provider!.getText()).toBeNull();

      const editor = createMockEditor('live text');
      act(() => {
        result.current.onEditorMount(editor as never);
      });
      expect(provider!.getText()).toBe('live text');

      unmount();
      expect(getEditorTextProvider()).toBeNull();
    });
  });
});
