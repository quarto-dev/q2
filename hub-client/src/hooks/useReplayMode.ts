import { useState, useCallback, useRef } from 'react';
import {
  getFileHandle,
  updateFileContent,
  freeDoc,
  cloneHandleDoc,
  viewText,
} from '../services/automergeSync';

export const PLAYBACK_SPEEDS = [1, 2, 4] as const;
export type PlaybackSpeed = (typeof PLAYBACK_SPEEDS)[number];

export interface ChunkActorShare {
  actor: string;
  fraction: number;
}

export interface ReplayState {
  isActive: boolean;
  historyLength: number;
  currentIndex: number;
  isPlaying: boolean;
  playbackSpeed: PlaybackSpeed;
  currentContent: string;
  timestamp: number | null;
  actor: string | null; // short hash of the actor who made the change
  chunkActors: ChunkActorShare[][]; // per-chunk actor fractions for the waveform
}

/** Deterministic color from an actor hash string. */
export function actorColor(actor: string): string {
  const hue = parseInt(actor.slice(0, 6), 16) % 360;
  return `hsl(${hue}, 60%, 55%)`;
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
  actor: null,
  chunkActors: [],
};

// Base interval at 1x speed; divided by playback speed multiplier
const PLAY_BASE_INTERVAL_MS = 200;

// Type helpers for DocHandle methods we use (avoids importing Automerge types)
interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string } | undefined;
}

function asViewable(handle: unknown): ViewableHandle {
  return handle as ViewableHandle;
}

export function useReplayMode(
  filePath: string | null,
): { state: ReplayState; controls: ReplayControls; isActiveRef: React.RefObject<boolean> } {
  const [state, setState] = useState<ReplayState>(INITIAL_STATE);

  // Store history array and handle in refs (stable across renders, not reactive)
  const historyRef = useRef<unknown[]>([]);
  const handleRef = useRef<unknown>(null);
  // Independent clone of the doc used for all view() operations during replay.
  // Views of this clone borrow from the clone's WASM state — not the original
  // handle's — so handle.history() on re-entry is never blocked.
  const cloneRef = useRef<unknown>(null);
  // Cache of extracted text content keyed by history index.
  // Avoids repeated WASM view() calls for the same index.
  const textCacheRef = useRef<Map<number, string>>(new Map());
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  // Keep current index and speed in refs for the interval callback
  const indexRef = useRef(0);
  const speedRef = useRef<PlaybackSpeed>(1);
  // Synchronous replay-active flag: updated immediately in enter()/reset(),
  // before React re-renders.  Consumers can read this ref to guard against
  // stale closures that still see isActive === false.
  const isActiveRef = useRef(false);

  const clearPlayInterval = useCallback(() => {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  const getContentAtIndex = useCallback((index: number): string => {
    const clone = cloneRef.current;
    const history = historyRef.current;
    if (!clone || index < 0 || index >= history.length) return '';

    const cached = textCacheRef.current.get(index);
    if (cached !== undefined) return cached;

    const text = viewText(clone, history[index]);
    textCacheRef.current.set(index, text);
    return text;
  }, []);

  const getMetadataAtIndex = useCallback((index: number): { timestamp: number | null; actor: string | null } => {
    const handle = handleRef.current;
    const history = historyRef.current;
    if (!handle || index < 0 || index >= history.length) return { timestamp: null, actor: null };
    try {
      // metadata() expects a single change hash string.
      // history entries are UrlHeads (string[]), so extract the first element.
      const heads = history[index];
      const changeHash = Array.isArray(heads) ? heads[0] : heads;
      if (typeof changeHash !== 'string') return { timestamp: null, actor: null };
      const meta = asViewable(handle).metadata(changeHash);
      return { timestamp: meta?.time ?? null, actor: meta?.actor ?? null };
    } catch {
      return { timestamp: null, actor: null };
    }
  }, []);

  const getTimestampAtIndex = useCallback((index: number): number | null => {
    return getMetadataAtIndex(index).timestamp;
  }, [getMetadataAtIndex]);

  const enter = useCallback(() => {
    if (!filePath) return;

    let handle, history: unknown[], clone;
    try {
      handle = getFileHandle(filePath);
      if (!handle) return;

      history = asViewable(handle).history() ?? [];
      if (history.length === 0) return;

      clone = cloneHandleDoc(handle);
    } catch (e) {
      console.error('[useReplayMode] Failed to enter replay mode:', e);
      return;
    }

    handleRef.current = handle;
    historyRef.current = history;
    cloneRef.current = clone;
    textCacheRef.current = new Map();
    isActiveRef.current = true;

    const lastIndex = history.length - 1;
    indexRef.current = lastIndex;

    // Split history into ≤100 equal chunks for the actor-colored waveform.
    const MAX_CHUNKS = 100;
    const chunkCount = Math.min(history.length, MAX_CHUNKS);
    const chunkSize = history.length / chunkCount;

    // Collect actor frequencies per chunk — ≤500 metadata() calls (100 chunks × 5 samples).
    const SAMPLES_PER_CHUNK = 5;
    const viewable = asViewable(handle);
    const chunkActors: ChunkActorShare[][] = new Array(chunkCount);

    for (let i = 0; i < chunkCount; i++) {
      const startIdx = Math.round(i * chunkSize);
      const endIdx = Math.min(Math.round((i + 1) * chunkSize), history.length);
      const span = endIdx - startIdx;
      const step = Math.max(1, Math.floor(span / SAMPLES_PER_CHUNK));
      const counts = new Map<string, number>();
      let totalSamples = 0;
      for (let j = startIdx; j < endIdx; j += step) {
        const heads = history[j];
        const changeHash = Array.isArray(heads) ? heads[0] : heads;
        if (typeof changeHash === 'string') {
          const meta = viewable.metadata(changeHash);
          if (meta?.actor) {
            counts.set(meta.actor, (counts.get(meta.actor) ?? 0) + 1);
            totalSamples++;
          }
        }
      }
      if (totalSamples === 0) {
        chunkActors[i] = [];
      } else {
        chunkActors[i] = Array.from(counts.entries()).map(([actor, count]) => ({
          actor,
          fraction: count / totalSamples,
        }));
      }
    }

    const lastMeta = getMetadataAtIndex(lastIndex);
    setState({
      isActive: true,
      historyLength: history.length,
      currentIndex: lastIndex,
      isPlaying: false,
      playbackSpeed: 1,
      currentContent: getContentAtIndex(lastIndex),
      timestamp: lastMeta.timestamp,
      actor: lastMeta.actor,
      chunkActors,
    });
  }, [filePath, getContentAtIndex, getMetadataAtIndex]);

  const seekTo = useCallback((index: number) => {
    const history = historyRef.current;
    if (history.length === 0) return;

    const clamped = Math.max(0, Math.min(index, history.length - 1));
    indexRef.current = clamped;
    const content = getContentAtIndex(clamped);
    const meta = getMetadataAtIndex(clamped);

    setState(prev => ({
      ...prev,
      currentIndex: clamped,
      currentContent: content,
      timestamp: meta.timestamp,
      actor: meta.actor,
    }));
  }, [getContentAtIndex, getMetadataAtIndex]);

  const stopPlaying = useCallback(() => {
    clearPlayInterval();
    setState(prev => ({ ...prev, isPlaying: false }));
  }, [clearPlayInterval]);

  const startPlayInterval = useCallback(() => {
    clearPlayInterval();
    const history = historyRef.current;
    const interval = Math.round(PLAY_BASE_INTERVAL_MS / speedRef.current);

    intervalRef.current = setInterval(() => {
      try {
        const nextIndex = indexRef.current + 1;
        if (nextIndex >= history.length) {
          clearPlayInterval();
          setState(prev => ({ ...prev, isPlaying: false }));
          return;
        }
        indexRef.current = nextIndex;
        const content = getContentAtIndex(nextIndex);
        const meta = getMetadataAtIndex(nextIndex);
        setState(prev => ({
          ...prev,
          currentIndex: nextIndex,
          currentContent: content,
          timestamp: meta.timestamp,
          actor: meta.actor,
        }));
      } catch (e) {
        console.error('[useReplayMode] Playback error, stopping:', e);
        clearPlayInterval();
        setState(prev => ({ ...prev, isPlaying: false }));
      }
    }, interval);
  }, [clearPlayInterval, getContentAtIndex, getMetadataAtIndex]);

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
    isActiveRef.current = false;
    // Free the clone's WASM state immediately so borrows don't linger.
    if (cloneRef.current) {
      freeDoc(cloneRef.current);
      cloneRef.current = null;
    }
    handleRef.current = null;
    historyRef.current = [];
    textCacheRef.current = new Map();
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
    isActiveRef,
  };
}
