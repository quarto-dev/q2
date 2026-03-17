import { useState, useCallback, useRef } from 'react';
import {
  getFileHandle,
  updateFileContent,
} from '../services/automergeSync';
import {
  createReplaySession,
  type ReplaySession,
} from '@quarto/quarto-sync-client';

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

/** Compute per-chunk actor frequency data for the waveform visualization. */
function computeChunkActors(session: ReplaySession): ChunkActorShare[][] {
  const MAX_CHUNKS = 100;
  const SAMPLES_PER_CHUNK = 5;
  const chunkCount = Math.min(session.length, MAX_CHUNKS);
  const chunkSize = session.length / chunkCount;
  const chunkActors: ChunkActorShare[][] = new Array(chunkCount);

  for (let i = 0; i < chunkCount; i++) {
    const startIdx = Math.round(i * chunkSize);
    const endIdx = Math.min(Math.round((i + 1) * chunkSize), session.length);
    const span = endIdx - startIdx;
    const step = Math.max(1, Math.floor(span / SAMPLES_PER_CHUNK));
    const counts = new Map<string, number>();
    let totalSamples = 0;
    for (let j = startIdx; j < endIdx; j += step) {
      const meta = session.getMetadataAt(j);
      if (meta.actor) {
        counts.set(meta.actor, (counts.get(meta.actor) ?? 0) + 1);
        totalSamples++;
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
  return chunkActors;
}

export function useReplayMode(
  filePath: string | null,
): { state: ReplayState; controls: ReplayControls; isActiveRef: React.RefObject<boolean> } {
  const [state, setState] = useState<ReplayState>(INITIAL_STATE);

  const sessionRef = useRef<ReplaySession | null>(null);
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
    const session = sessionRef.current;
    if (!session) return '';
    return session.getContentAt(index);
  }, []);

  const getMetadataAtIndex = useCallback((index: number): { timestamp: number | null; actor: string | null } => {
    const session = sessionRef.current;
    if (!session) return { timestamp: null, actor: null };
    return session.getMetadataAt(index);
  }, []);

  const getTimestampAtIndex = useCallback((index: number): number | null => {
    return getMetadataAtIndex(index).timestamp;
  }, [getMetadataAtIndex]);

  const enter = useCallback(() => {
    if (!filePath) return;

    // Close any existing session first (fixes clone leak on double-enter)
    if (sessionRef.current) {
      sessionRef.current.close();
      sessionRef.current = null;
    }

    let handle;
    try {
      handle = getFileHandle(filePath);
      if (!handle) return;
    } catch (e) {
      console.error('[useReplayMode] Failed to get file handle:', e);
      return;
    }

    const session = createReplaySession(
      handle,
      (content: string) => updateFileContent(filePath, content),
    );
    if (!session) return;

    sessionRef.current = session;
    isActiveRef.current = true;

    const lastIndex = session.length - 1;
    indexRef.current = lastIndex;

    const lastMeta = getMetadataAtIndex(lastIndex);
    // Render immediately — waveform arrives on the next frame.
    setState({
      isActive: true,
      historyLength: session.length,
      currentIndex: lastIndex,
      isPlaying: false,
      playbackSpeed: 1,
      currentContent: getContentAtIndex(lastIndex),
      timestamp: lastMeta.timestamp,
      actor: lastMeta.actor,
      chunkActors: [],
    });

    // Compute waveform data after paint so replay UI appears instantly.
    requestAnimationFrame(() => {
      // Guard: session may have been closed between setState and this callback.
      if (!isActiveRef.current || sessionRef.current !== session) return;

      const chunkActors = computeChunkActors(session);
      setState(prev => ({ ...prev, chunkActors }));
    });
  }, [filePath, getContentAtIndex, getMetadataAtIndex]);

  const seekTo = useCallback((index: number) => {
    const session = sessionRef.current;
    if (!session) return;

    const clamped = Math.max(0, Math.min(index, session.length - 1));
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
    const session = sessionRef.current;
    if (!session) return;
    const interval = Math.round(PLAY_BASE_INTERVAL_MS / speedRef.current);

    intervalRef.current = setInterval(() => {
      try {
        const nextIndex = indexRef.current + 1;
        if (nextIndex >= session.length) {
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
    const session = sessionRef.current;
    if (!session) return;

    // If at the end, restart from the beginning
    if (indexRef.current >= session.length - 1) {
      seekTo(0);
    }

    setState(prev => ({ ...prev, isPlaying: true }));
    startPlayInterval();
  }, [seekTo, startPlayInterval]);

  const pause = useCallback(() => {
    stopPlaying();
  }, [stopPlaying]);

  const stepForward = useCallback(() => {
    const session = sessionRef.current;
    if (!session) return;
    const next = indexRef.current + 1;
    if (next < session.length) {
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
    if (sessionRef.current) {
      seekTo(0);
    }
  }, [seekTo]);

  const seekToEnd = useCallback(() => {
    const session = sessionRef.current;
    if (session) {
      seekTo(session.length - 1);
    }
  }, [seekTo]);

  const reset = useCallback(() => {
    clearPlayInterval();
    isActiveRef.current = false;
    if (sessionRef.current) {
      sessionRef.current.close();
      sessionRef.current = null;
    }
    indexRef.current = 0;
    speedRef.current = 1;
    setState(INITIAL_STATE);
  }, [clearPlayInterval]);

  const exit = useCallback(() => {
    reset();
  }, [reset]);

  const apply = useCallback(() => {
    const session = sessionRef.current;
    if (session && filePath) {
      session.applyContentAt(indexRef.current);
    }
    reset();
  }, [filePath, reset]);

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
