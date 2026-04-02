/**
 * Tests for useAutomergeSync hook
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

// Mock automergeSync service
vi.mock('../services/automergeSync', () => ({
  getFileContent: vi.fn(),
  setImmediateFileChangeCallback: vi.fn(),
}));

// Mock diffToMonacoEdits
vi.mock('../utils/diffToMonacoEdits', () => ({
  diffToMonacoEdits: vi.fn(),
}));

import { useAutomergeSync } from './useAutomergeSync';
import { getFileContent, setImmediateFileChangeCallback } from '../services/automergeSync';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import type { FileEntry } from '../types/project';

const mockGetFileContent = vi.mocked(getFileContent);
const mockSetImmediateFileChangeCallback = vi.mocked(setImmediateFileChangeCallback);
const mockDiffToMonacoEdits = vi.mocked(diffToMonacoEdits);

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
    it('should compute diff and apply edits via executeEdits', () => {
      const mockEditor = createMockEditor('# Old Content');
      const fakeEdits = [{ range: {}, text: '# New Content', forceMoveMarkers: true }];
      mockDiffToMonacoEdits.mockReturnValue(fakeEdits as never);

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      act(() => {
        result.current.handleContentRewrite('# New Content');
      });

      expect(mockDiffToMonacoEdits).toHaveBeenCalledWith('# Old Content', '# New Content');
      expect(mockEditor.executeEdits).toHaveBeenCalledWith('ast-rewrite', fakeEdits);
    });

    it('should be a no-op when no editor is mounted', () => {
      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      // Should not throw
      act(() => {
        result.current.handleContentRewrite('# New Content');
      });

      expect(mockDiffToMonacoEdits).not.toHaveBeenCalled();
    });

    it('should skip when diff produces no edits', () => {
      const mockEditor = createMockEditor('# Same');
      mockDiffToMonacoEdits.mockReturnValue([]);

      const { result } = renderHook(() => useAutomergeSync(defaultOptions()));

      act(() => {
        result.current.onEditorMount(mockEditor as never);
      });

      act(() => {
        result.current.handleContentRewrite('# Same');
      });

      expect(mockEditor.executeEdits).not.toHaveBeenCalled();
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
});
