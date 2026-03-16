/**
 * Tests for useReplayMode hook
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

vi.mock('../services/automergeSync', () => ({
  getFileHandle: vi.fn(),
  updateFileContent: vi.fn(),
  freeDoc: vi.fn(),
  cloneHandleDoc: vi.fn(),
  viewText: vi.fn(),
}));

import { useReplayMode } from './useReplayMode';
import {
  getFileHandle,
  updateFileContent,
  cloneHandleDoc,
  viewText,
} from '../services/automergeSync';

const mockGetFileHandle = vi.mocked(getFileHandle);
const mockUpdateFileContent = vi.mocked(updateFileContent);
const mockCloneHandleDoc = vi.mocked(cloneHandleDoc);
const mockViewText = vi.mocked(viewText);

// Helper to create a mock handle with history support.
// history() returns UrlHeads[] where each UrlHeads is string[].
// metadata() receives a single change hash string (first element of UrlHeads).
// Also configures mockCloneHandleDoc and mockViewText for the given texts.
function createMockHandle(texts: string[], timestamps?: number[], actors?: string[]) {
  const historyHeads = texts.map((_, i) => [`head-${i}`]);

  const handle = {
    history: vi.fn(() => historyHeads),
    metadata: vi.fn((changeHash?: string) => {
      if (!changeHash) return undefined;
      const index = historyHeads.findIndex(h => h[0] === changeHash);
      if (index < 0) return undefined;
      const ts = timestamps?.[index] ?? 1000000 + index * 1000;
      const actor = actors?.[index] ?? `actor${index}abcdef0123456789`;
      return { time: ts, actor };
    }),
    doc: vi.fn(() => ({ text: texts[texts.length - 1] })),
  };

  // cloneHandleDoc returns a sentinel object representing the clone
  const cloneObj = { __clone: true };
  mockCloneHandleDoc.mockReturnValue(cloneObj);

  // viewText extracts text from the clone given heads
  mockViewText.mockImplementation((_clone: unknown, heads: unknown) => {
    const headArr = heads as string[];
    const index = historyHeads.findIndex(h => h[0] === headArr[0]);
    return texts[index] ?? '';
  });

  return handle;
}

describe('useReplayMode', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('starts in inactive state', () => {
    const { result } = renderHook(() => useReplayMode('index.qmd'));
    expect(result.current.state.isActive).toBe(false);
    expect(result.current.state.historyLength).toBe(0);
    expect(result.current.state.currentIndex).toBe(0);
    expect(result.current.state.isPlaying).toBe(false);
    expect(result.current.state.currentContent).toBe('');
  });

  describe('enter()', () => {
    it('loads history and activates replay', () => {
      const handle = createMockHandle(['a', 'ab', 'abc']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(handle.history).toHaveBeenCalled();
      expect(result.current.state.isActive).toBe(true);
      expect(result.current.state.historyLength).toBe(3);
      // Starts at last index (current state)
      expect(result.current.state.currentIndex).toBe(2);
      expect(result.current.state.currentContent).toBe('abc');
      // chunkActors should have entries with fractions summing to 1
      expect(result.current.state.chunkActors.length).toBeGreaterThan(0);
    });

    it('is a no-op when getFileHandle returns null', () => {
      mockGetFileHandle.mockReturnValue(null as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(result.current.state.isActive).toBe(false);
    });

    it('is a no-op when handle.history() returns undefined', () => {
      const handle = {
        history: vi.fn(() => undefined),
        metadata: vi.fn(),
        doc: vi.fn(),
      };
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(result.current.state.isActive).toBe(false);
    });

    it('is a no-op when history is empty', () => {
      const handle = createMockHandle([]);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(result.current.state.isActive).toBe(false);
    });

    it('is a no-op when filePath is null', () => {
      const { result } = renderHook(() => useReplayMode(null));
      act(() => { result.current.controls.enter(); });

      expect(mockGetFileHandle).not.toHaveBeenCalled();
      expect(result.current.state.isActive).toBe(false);
    });
  });

  describe('seekTo()', () => {
    it('updates currentContent with correct text', () => {
      const handle = createMockHandle(['first', 'second', 'third']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });

      expect(result.current.state.currentIndex).toBe(0);
      expect(result.current.state.currentContent).toBe('first');
    });

    it('clamps out-of-bounds index to valid range (too high)', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(100); });

      expect(result.current.state.currentIndex).toBe(2);
    });

    it('clamps out-of-bounds index to valid range (negative)', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(-5); });

      expect(result.current.state.currentIndex).toBe(0);
    });
  });

  describe('play() / pause()', () => {
    it('starts auto-advance interval on play', () => {
      const handle = createMockHandle(['a', 'b', 'c', 'd']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });

      expect(result.current.state.isPlaying).toBe(false);

      act(() => { result.current.controls.play(); });
      expect(result.current.state.isPlaying).toBe(true);

      // Advance one tick (base interval 200ms at 1x speed)
      act(() => { vi.advanceTimersByTime(200); });
      expect(result.current.state.currentIndex).toBe(1);

      // Advance another tick
      act(() => { vi.advanceTimersByTime(200); });
      expect(result.current.state.currentIndex).toBe(2);
    });

    it('stops auto-advance on pause', () => {
      const handle = createMockHandle(['a', 'b', 'c', 'd']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.play(); });
      act(() => { vi.advanceTimersByTime(200); });
      expect(result.current.state.currentIndex).toBe(1);

      act(() => { result.current.controls.pause(); });
      expect(result.current.state.isPlaying).toBe(false);

      act(() => { vi.advanceTimersByTime(200); });
      // Should not advance further
      expect(result.current.state.currentIndex).toBe(1);
    });

    it('stops playing when reaching the end of history', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.play(); });

      // Advance past all items
      act(() => { vi.advanceTimersByTime(200); }); // index 1
      act(() => { vi.advanceTimersByTime(200); }); // index 2
      act(() => { vi.advanceTimersByTime(200); }); // at end, should stop

      expect(result.current.state.currentIndex).toBe(2);
      expect(result.current.state.isPlaying).toBe(false);
    });
  });

  describe('cycleSpeed()', () => {
    it('cycles through 1x, 2x, 4x speeds', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(result.current.state.playbackSpeed).toBe(1);

      act(() => { result.current.controls.cycleSpeed(); });
      expect(result.current.state.playbackSpeed).toBe(2);

      act(() => { result.current.controls.cycleSpeed(); });
      expect(result.current.state.playbackSpeed).toBe(4);

      act(() => { result.current.controls.cycleSpeed(); });
      expect(result.current.state.playbackSpeed).toBe(1);
    });

    it('advances faster at 2x speed', () => {
      const handle = createMockHandle(['a', 'b', 'c', 'd', 'e']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.cycleSpeed(); }); // 2x
      act(() => { result.current.controls.play(); });

      // At 2x, interval is 100ms
      act(() => { vi.advanceTimersByTime(100); });
      expect(result.current.state.currentIndex).toBe(1);

      act(() => { vi.advanceTimersByTime(100); });
      expect(result.current.state.currentIndex).toBe(2);
    });

    it('restarts interval at new speed when changed during playback', () => {
      const handle = createMockHandle(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.play(); });

      // Advance one step at 1x (200ms)
      act(() => { vi.advanceTimersByTime(200); });
      expect(result.current.state.currentIndex).toBe(1);

      // Switch to 4x while playing
      act(() => { result.current.controls.cycleSpeed(); }); // 2x
      act(() => { result.current.controls.cycleSpeed(); }); // 4x

      // At 4x, interval is 50ms
      act(() => { vi.advanceTimersByTime(50); });
      expect(result.current.state.currentIndex).toBe(2);
    });

    it('resets speed on exit', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.cycleSpeed(); });
      expect(result.current.state.playbackSpeed).toBe(2);

      act(() => { result.current.controls.exit(); });
      expect(result.current.state.playbackSpeed).toBe(1);
    });
  });

  describe('stepForward() / stepBackward()', () => {
    it('steps forward by one', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.stepForward(); });

      expect(result.current.state.currentIndex).toBe(1);
    });

    it('does not step forward past the end', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      // Already at index 2 (last)
      act(() => { result.current.controls.stepForward(); });

      expect(result.current.state.currentIndex).toBe(2);
    });

    it('steps backward by one', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.stepBackward(); });

      expect(result.current.state.currentIndex).toBe(1);
    });

    it('does not step backward past the beginning', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.stepBackward(); });

      expect(result.current.state.currentIndex).toBe(0);
    });
  });

  describe('seekToStart() / seekToEnd()', () => {
    it('seekToStart jumps to index 0', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      // Starts at index 2 (last)
      expect(result.current.state.currentIndex).toBe(2);

      act(() => { result.current.controls.seekToStart(); });
      expect(result.current.state.currentIndex).toBe(0);
      expect(result.current.state.currentContent).toBe('a');
    });

    it('seekToEnd jumps to last index', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      expect(result.current.state.currentIndex).toBe(0);

      act(() => { result.current.controls.seekToEnd(); });
      expect(result.current.state.currentIndex).toBe(2);
      expect(result.current.state.currentContent).toBe('c');
    });
  });

  describe('exit()', () => {
    it('resets state', () => {
      const handle = createMockHandle(['a', 'b', 'c']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      expect(result.current.state.isActive).toBe(true);

      act(() => { result.current.controls.exit(); });

      expect(result.current.state.isActive).toBe(false);
      expect(result.current.state.historyLength).toBe(0);
      expect(result.current.state.currentContent).toBe('');
    });

    it('stops playback on exit', () => {
      const handle = createMockHandle(['a', 'b', 'c', 'd']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.play(); });
      expect(result.current.state.isPlaying).toBe(true);

      act(() => { result.current.controls.exit(); });
      expect(result.current.state.isPlaying).toBe(false);

      // Ensure interval is cleared
      act(() => { vi.advanceTimersByTime(1000); });
      expect(result.current.state.isActive).toBe(false);
    });
  });

  describe('apply()', () => {
    it('calls updateFileContent with historical content and resets', () => {
      const handle = createMockHandle(['first', 'second', 'third']);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.currentContent).toBe('second');

      act(() => { result.current.controls.apply(); });

      expect(mockUpdateFileContent).toHaveBeenCalledWith('index.qmd', 'second');
      expect(result.current.state.isActive).toBe(false);
    });
  });

  describe('timestamp and actor', () => {
    it('provides timestamp for current change', () => {
      const timestamps = [1000000, 1001000, 1002000];
      const handle = createMockHandle(['a', 'b', 'c'], timestamps);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.timestamp).toBe(1001000);
    });

    it('provides actor hash for current change', () => {
      const actors = ['aaa111', 'bbb222', 'ccc333'];
      const handle = createMockHandle(['a', 'b', 'c'], undefined, actors);
      mockGetFileHandle.mockReturnValue(handle as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.actor).toBe('bbb222');
    });
  });
});
