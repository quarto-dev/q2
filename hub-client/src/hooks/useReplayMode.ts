import { useState, useCallback, useRef } from 'react';
import {
  getFileHandle,
  updateFileContent,
} from '../services/automergeSync';

export const PLAYBACK_SPEEDS = [1, 2, 4] as const;
export type PlaybackSpeed = (typeof PLAYBACK_SPEEDS)[number];

export interface ReplayState {
  isActive: boolean;
  historyLength: number;
  currentIndex: number;
  isPlaying: boolean;
  playbackSpeed: PlaybackSpeed;
  currentContent: string;
  timestamp: number | null;
}

export interface ReplayControls {
  enter: () => void;
  exit: () => void;
  apply: () => void;
  seekTo: (index: number) => void;
  seekToStart: () => void;
  seekToEnd: () => void;
  play: () => void;
  pause: () => void;
  stepForward: () => void;
  stepBackward: () => void;
  cycleSpeed: () => void;
  getTimestampAtIndex: (index: number) => number | null;
}

const INITIAL_STATE: ReplayState = {
  isActive: false,
  historyLength: 0,
  currentIndex: 0,
  isPlaying: false,
  playbackSpeed: 1,
  currentContent: '',
  timestamp: null,
};

// Base interval at 1x speed; divided by playback speed multiplier
const PLAY_BASE_INTERVAL_MS = 200;

// Type helpers for DocHandle methods we use (avoids importing Automerge types)
interface ViewableHandle {
  history(): unknown[] | undefined;
  view(heads: unknown): { doc(): { text?: string } | undefined | null };
  metadata(change?: string): { time?: number } | undefined;
}

function asViewable(handle: unknown): ViewableHandle {
  return handle as ViewableHandle;
}

export function useReplayMode(
  filePath: string | null,
): { state: ReplayState; controls: ReplayControls } {
  const [state, setState] = useState<ReplayState>(INITIAL_STATE);

  // Store history array and handle in refs (stable across renders, not reactive)
  const historyRef = useRef<unknown[]>([]);
  const handleRef = useRef<unknown>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  // Keep current index and speed in refs for the interval callback
  const indexRef = useRef(0);
  const speedRef = useRef<PlaybackSpeed>(1);

  const clearPlayInterval = useCallback(() => {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  const getContentAtIndex = useCallback((index: number): string => {
    const handle = handleRef.current;
    const history = historyRef.current;
    if (!handle || index < 0 || index >= history.length) return '';
    try {
      const viewedHandle = asViewable(handle).view(history[index]);
      const doc = viewedHandle.doc();
      return doc?.text ?? '';
    } catch (e) {
      console.warn('[useReplayMode] Failed to get content at index', index, e);
      return '';
    }
  }, []);

  const getTimestampAtIndex = useCallback((index: number): number | null => {
    const handle = handleRef.current;
    const history = historyRef.current;
    if (!handle || index < 0 || index >= history.length) return null;
    try {
      // metadata() expects a single change hash string.
      // history entries are UrlHeads (string[]), so extract the first element.
      const heads = history[index];
      const changeHash = Array.isArray(heads) ? heads[0] : heads;
      if (typeof changeHash !== 'string') return null;
      const meta = asViewable(handle).metadata(changeHash);
      return meta?.time ?? null;
    } catch {
      return null;
    }
  }, []);

  const enter = useCallback(() => {
    if (!filePath) return;

    try {
      const handle = getFileHandle(filePath);
      if (!handle) return;

      const history = asViewable(handle).history();
      if (!history || history.length === 0) return;

      handleRef.current = handle;
      historyRef.current = history;

      const lastIndex = history.length - 1;
      indexRef.current = lastIndex;
      const content = getContentAtIndex(lastIndex);
      const timestamp = getTimestampAtIndex(lastIndex);

      setState({
        isActive: true,
        historyLength: history.length,
        currentIndex: lastIndex,
        isPlaying: false,
        playbackSpeed: 1,
        currentContent: content,
        timestamp,
      });
    } catch (e) {
      console.error('[useReplayMode] Failed to enter replay mode:', e);
    }
  }, [filePath, getContentAtIndex, getTimestampAtIndex]);

  const seekTo = useCallback((index: number) => {
    const history = historyRef.current;
    if (history.length === 0) return;

    const clamped = Math.max(0, Math.min(index, history.length - 1));
    indexRef.current = clamped;
    const content = getContentAtIndex(clamped);
    const timestamp = getTimestampAtIndex(clamped);

    setState(prev => ({
      ...prev,
      currentIndex: clamped,
      currentContent: content,
      timestamp,
    }));
  }, [getContentAtIndex, getTimestampAtIndex]);

  const stopPlaying = useCallback(() => {
    clearPlayInterval();
    setState(prev => ({ ...prev, isPlaying: false }));
  }, [clearPlayInterval]);

  const startPlayInterval = useCallback(() => {
    clearPlayInterval();
    const history = historyRef.current;
    const interval = Math.round(PLAY_BASE_INTERVAL_MS / speedRef.current);

    intervalRef.current = setInterval(() => {
      const nextIndex = indexRef.current + 1;
      if (nextIndex >= history.length) {
        clearPlayInterval();
        setState(prev => ({ ...prev, isPlaying: false }));
        return;
      }
      indexRef.current = nextIndex;
      const content = getContentAtIndex(nextIndex);
      const timestamp = getTimestampAtIndex(nextIndex);
      setState(prev => ({
        ...prev,
        currentIndex: nextIndex,
        currentContent: content,
        timestamp,
      }));
    }, interval);
  }, [clearPlayInterval, getContentAtIndex, getTimestampAtIndex]);

  const play = useCallback(() => {
    const history = historyRef.current;
    if (history.length === 0) return;

    // If at the end, restart from the beginning
    if (indexRef.current >= history.length - 1) {
      seekTo(0);
    }

    setState(prev => ({ ...prev, isPlaying: true }));
    startPlayInterval();
  }, [seekTo, startPlayInterval]);

  const pause = useCallback(() => {
    stopPlaying();
  }, [stopPlaying]);

  const stepForward = useCallback(() => {
    const history = historyRef.current;
    const next = indexRef.current + 1;
    if (next < history.length) {
      seekTo(next);
    }
  }, [seekTo]);

  const stepBackward = useCallback(() => {
    const prev = indexRef.current - 1;
    if (prev >= 0) {
      seekTo(prev);
    }
  }, [seekTo]);

  const cycleSpeed = useCallback(() => {
    const currentIdx = PLAYBACK_SPEEDS.indexOf(speedRef.current);
    const nextSpeed = PLAYBACK_SPEEDS[(currentIdx + 1) % PLAYBACK_SPEEDS.length];
    speedRef.current = nextSpeed;
    setState(prev => ({ ...prev, playbackSpeed: nextSpeed }));
    // If currently playing, restart the interval at the new speed
    if (intervalRef.current !== null) {
      startPlayInterval();
    }
  }, [startPlayInterval]);

  const seekToStart = useCallback(() => {
    if (historyRef.current.length > 0) {
      seekTo(0);
    }
  }, [seekTo]);

  const seekToEnd = useCallback(() => {
    const history = historyRef.current;
    if (history.length > 0) {
      seekTo(history.length - 1);
    }
  }, [seekTo]);

  const reset = useCallback(() => {
    clearPlayInterval();
    handleRef.current = null;
    historyRef.current = [];
    indexRef.current = 0;
    speedRef.current = 1;
    setState(INITIAL_STATE);
  }, [clearPlayInterval]);

  const exit = useCallback(() => {
    reset();
  }, [reset]);

  const apply = useCallback(() => {
    const content = getContentAtIndex(indexRef.current);
    if (filePath) {
      updateFileContent(filePath, content);
    }
    reset();
  }, [filePath, getContentAtIndex, reset]);

  return {
    state,
    controls: {
      enter,
      exit,
      apply,
      seekTo,
      seekToStart,
      seekToEnd,
      play,
      pause,
      stepForward,
      stepBackward,
      cycleSpeed,
      getTimestampAtIndex,
    },
  };
}
