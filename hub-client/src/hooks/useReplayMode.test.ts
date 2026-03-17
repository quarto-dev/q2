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
}));

vi.mock('@quarto/quarto-sync-client', () => ({
  createReplaySession: vi.fn(),
}));

import { useReplayMode } from './useReplayMode';
import {
  getFileHandle,
  updateFileContent,
} from '../services/automergeSync';
import { createReplaySession } from '@quarto/quarto-sync-client';

const mockGetFileHandle = vi.mocked(getFileHandle);
const mockUpdateFileContent = vi.mocked(updateFileContent);
const mockCreateReplaySession = vi.mocked(createReplaySession);

/**
 * Helper to create a mock ReplaySession and configure mocks.
 * texts[i] is the content at history index i.
 */
function createMockSession(texts: string[], timestamps?: number[], actors?: string[]) {
  const handle = { __mockHandle: true };
  mockGetFileHandle.mockReturnValue(handle as never);

  const session = {
    get length() { return texts.length; },
    getContentAt: vi.fn((index: number) => {
      if (index < 0 || index >= texts.length) return '';
      return texts[index];
    }),
    getMetadataAt: vi.fn((index: number) => {
      if (index < 0 || index >= texts.length) return { timestamp: null, actor: null };
      const ts = timestamps?.[index] ?? 1000000 + index * 1000;
      const actor = actors?.[index] ?? `actor${index}abcdef0123456789`;
      return { timestamp: ts, actor };
    }),
    applyContentAt: vi.fn(),
    close: vi.fn(),
  };

  mockCreateReplaySession.mockReturnValue(session);
  return { handle, session };
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
      const { session } = createMockSession(['a', 'ab', 'abc']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(mockCreateReplaySession).toHaveBeenCalled();
      expect(result.current.state.isActive).toBe(true);
      expect(result.current.state.historyLength).toBe(3);
      // Starts at last index (current state)
      expect(result.current.state.currentIndex).toBe(2);
      expect(result.current.state.currentContent).toBe('abc');
      // chunkActors is deferred — initially empty, populated after rAF
      expect(result.current.state.chunkActors).toEqual([]);
      act(() => { vi.advanceTimersByTime(16); }); // flush requestAnimationFrame
      expect(result.current.state.chunkActors.length).toBeGreaterThan(0);
      // Session should not be closed
      expect(session.close).not.toHaveBeenCalled();
    });

    it('is a no-op when getFileHandle returns null', () => {
      mockGetFileHandle.mockReturnValue(null as never);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });

      expect(result.current.state.isActive).toBe(false);
    });

    it('is a no-op when createReplaySession returns null', () => {
      mockGetFileHandle.mockReturnValue({ __mockHandle: true } as never);
      mockCreateReplaySession.mockReturnValue(null);

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

    it('closes previous session on re-enter', () => {
      const { session: session1 } = createMockSession(['a', 'b']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      expect(session1.close).not.toHaveBeenCalled();

      // Create a new session for re-enter
      const session2 = {
        get length() { return 3; },
        getContentAt: vi.fn((i: number) => ['x', 'y', 'z'][i] ?? ''),
        getMetadataAt: vi.fn(() => ({ timestamp: 5000, actor: 'bob' })),
        applyContentAt: vi.fn(),
        close: vi.fn(),
      };
      mockCreateReplaySession.mockReturnValue(session2);

      act(() => { result.current.controls.enter(); });
      expect(session1.close).toHaveBeenCalled();
    });
  });

  describe('seekTo()', () => {
    it('updates currentContent with correct text', () => {
      createMockSession(['first', 'second', 'third']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });

      expect(result.current.state.currentIndex).toBe(0);
      expect(result.current.state.currentContent).toBe('first');
    });

    it('clamps out-of-bounds index to valid range (too high)', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(100); });

      expect(result.current.state.currentIndex).toBe(2);
    });

    it('clamps out-of-bounds index to valid range (negative)', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(-5); });

      expect(result.current.state.currentIndex).toBe(0);
    });
  });

  describe('play() / pause()', () => {
    it('starts auto-advance interval on play', () => {
      createMockSession(['a', 'b', 'c', 'd']);

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
      createMockSession(['a', 'b', 'c', 'd']);

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
      createMockSession(['a', 'b', 'c']);

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
      createMockSession(['a', 'b', 'c']);

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
      createMockSession(['a', 'b', 'c', 'd', 'e']);

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
      createMockSession(['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h']);

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
      createMockSession(['a', 'b', 'c']);

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
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.stepForward(); });

      expect(result.current.state.currentIndex).toBe(1);
    });

    it('does not step forward past the end', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      // Already at index 2 (last)
      act(() => { result.current.controls.stepForward(); });

      expect(result.current.state.currentIndex).toBe(2);
    });

    it('steps backward by one', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.stepBackward(); });

      expect(result.current.state.currentIndex).toBe(1);
    });

    it('does not step backward past the beginning', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(0); });
      act(() => { result.current.controls.stepBackward(); });

      expect(result.current.state.currentIndex).toBe(0);
    });
  });

  describe('seekToStart() / seekToEnd()', () => {
    it('seekToStart jumps to index 0', () => {
      createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      // Starts at index 2 (last)
      expect(result.current.state.currentIndex).toBe(2);

      act(() => { result.current.controls.seekToStart(); });
      expect(result.current.state.currentIndex).toBe(0);
      expect(result.current.state.currentContent).toBe('a');
    });

    it('seekToEnd jumps to last index', () => {
      createMockSession(['a', 'b', 'c']);

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
    it('resets state and closes session', () => {
      const { session } = createMockSession(['a', 'b', 'c']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      expect(result.current.state.isActive).toBe(true);

      act(() => { result.current.controls.exit(); });

      expect(result.current.state.isActive).toBe(false);
      expect(result.current.state.historyLength).toBe(0);
      expect(result.current.state.currentContent).toBe('');
      expect(session.close).toHaveBeenCalled();
    });

    it('stops playback on exit', () => {
      createMockSession(['a', 'b', 'c', 'd']);

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
    it('calls session.applyContentAt and resets', () => {
      const { session } = createMockSession(['first', 'second', 'third']);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.currentContent).toBe('second');

      act(() => { result.current.controls.apply(); });

      expect(session.applyContentAt).toHaveBeenCalledWith(1);
      expect(result.current.state.isActive).toBe(false);
    });
  });

  describe('timestamp and actor', () => {
    it('provides timestamp for current change', () => {
      const timestamps = [1000000, 1001000, 1002000];
      createMockSession(['a', 'b', 'c'], timestamps);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.timestamp).toBe(1001000);
    });

    it('provides actor hash for current change', () => {
      const actors = ['aaa111', 'bbb222', 'ccc333'];
      createMockSession(['a', 'b', 'c'], undefined, actors);

      const { result } = renderHook(() => useReplayMode('index.qmd'));
      act(() => { result.current.controls.enter(); });
      act(() => { result.current.controls.seekTo(1); });

      expect(result.current.state.actor).toBe('bbb222');
    });
  });
});
