/**
 * React hook for q2-debug attribution: builds the run list from
 * Automerge history, synthesises the `IdentityMap`, and returns a JSON
 * payload ready to ship to the Rust `PreBuiltAttributionProvider` via
 * `parseQmdToAstWithAttribution`.
 *
 * The hook is the **producer half** of the Phase 5 pipeline. It owns:
 *   - run-list replay (delegated to `attribution-runs.ts`),
 *   - char → byte offset translation,
 *   - identity resolution (profile metadata if present, fallback formula
 *     otherwise — the producer invariant load-bearing for Phase 6).
 *
 * The Rust side then drives `AttributionGenerateTransform` +
 * `AttributionRenderTransform`, and the parsed AST carries the resolved
 * `astContext.attribution` + `astContext.attributionActors` for the
 * q2-debug renderer to consume.
 *
 * **Disabled path:** when `enabled` is false (Attribution toggle off),
 * the hook short-circuits and returns `null`. Callers then route through
 * `parseQmdToAstWithAttribution(content, null)`, which is byte-identical
 * to today's q2-debug output (Phase 0 test #10).
 */

import { useState, useEffect, useRef, useCallback } from 'react';

import {
  buildCharToByteMap,
  buildRunListAttribution,
  createReplayIndex,
  HistoryCompactedError,
  runsCharToByteOffsets,
  TargetNotAheadError,
  updateRunListAttribution,
} from '../services/attribution-runs';
import type {
  AttributionRun,
  ReplayTarget,
  RunListAttribution,
} from '../services/attribution-runs';
import { getFileHandle } from '@quarto/preview-runtime';
import type { ActorIdentity } from '@quarto/preview-runtime';
import { actorColor, fnv1aHex8 } from '../utils/palette';

interface TransportRun {
  start: number;
  end: number;
  actor: string;
  time: number;
}

interface TransportIdentity {
  name: string;
  color: string;
}

interface TransportPayload {
  runs: TransportRun[];
  identities: Record<string, TransportIdentity>;
}

/**
 * Build an `IdentityMap` covering every attribution key referenced by
 * `runs`, satisfying the Phase 6 producer invariant: **every** key in
 * `runs` has an entry in `identities` at the wire.
 *
 * Run `actor` fields are attribution keys (author IDs for post-transition
 * changes, bare actor IDs for pre-transition history — see
 * `attribution-runs.ts`), and `identities` is keyed the same way (D4), so
 * the lookup needs no author/actor distinction here.
 *
 * Resolution order per key:
 *   1. `identities[key]` (Automerge profile metadata when available),
 *   2. fallback `(key.slice(0, 8), actorColor(fnv1aHex8(key)))` —
 *      the same formula `GitBlameProvider` uses for emails, so visual
 *      output stays consistent across native and WASM producers.
 */
function buildIdentityMap(
  runs: AttributionRun[],
  identities: Record<string, ActorIdentity>,
): Record<string, TransportIdentity> {
  const out: Record<string, TransportIdentity> = {};
  for (const r of runs) {
    if (out[r.actor]) continue;
    const fromProfile = identities[r.actor];
    if (fromProfile) {
      out[r.actor] = { name: fromProfile.name, color: fromProfile.color };
    } else {
      out[r.actor] = {
        name: r.actor.slice(0, 8),
        color: actorColor(fnv1aHex8(r.actor)),
      };
    }
  }
  return out;
}

/**
 * Convert a run-list state plus the current document text into a JSON
 * payload ready for `parseQmdToAstWithAttribution`. Bytes-not-chars
 * conversion happens here so the Rust side gets UTF-8 byte offsets that
 * line up with `SourceInfo`.
 */
export function buildAttributionPayload(
  state: RunListAttribution,
  sourceText: string,
  identities: Record<string, ActorIdentity>,
): string {
  const charToByte = buildCharToByteMap(sourceText);
  const byteRuns = runsCharToByteOffsets(state.runs, charToByte);
  const payload: TransportPayload = {
    runs: byteRuns.map(r => ({
      start: r.start,
      end: r.end,
      actor: r.actor,
      time: r.time,
    })),
    identities: buildIdentityMap(state.runs, identities),
  };
  return JSON.stringify(payload);
}

const DEBOUNCE_MS = 500;

/** Replay steps whose runs are kept, so stepping back over them is immediate. */
const STEP_CACHE_SIZE = 128;

export interface UseAttributionOptions {
  /** When false, the hook returns `null` and does no work. */
  enabled: boolean;
  /** The current file's Automerge path (e.g. "index.qmd"). */
  filePath: string | null;
  /** The current full document text — needed for char→byte translation. */
  sourceText: string;
  /** Attribution key → profile identity table (from `App.tsx`'s identities state). */
  identities: Record<string, ActorIdentity>;
  /**
   * Replay: the change hashes of the history step that `sourceText` shows.
   * Attribution is then built for that point in history instead of the live
   * heads, whose run offsets belong to a different text. Null or absent
   * outside replay.
   */
  atHeads?: string[] | null;
}

export interface UseAttributionResult {
  /**
   * JSON payload for `parseQmdToAstWithAttribution`, or `null` while
   * disabled, the file is unknown, or no build has completed yet.
   * Once a build completes the string remains referentially stable
   * across re-renders until the source text changes enough to
   * invalidate it. In replay it is non-null only when it was built for
   * the current `atHeads`.
   */
  payload: string | null;
  /**
   * True while the hook is doing work the user is waiting on: the
   * cold-start build, the debounced incremental update window, and
   * the synchronous update step itself. Drives the Attribution pill's
   * "work in progress" border animation upstream.
   *
   * Distinct from `payload === null` because incremental updates
   * keep the previous payload until the new one is ready — so a
   * naïve `payload === null && enabled` check would miss them.
   */
  generating: boolean;
  /**
   * Replay only: the attribution of the current step is not settled yet
   * (neither built nor failed). The preview holds its render meanwhile, so
   * a step is never drawn with another step's runs.
   */
  pending: boolean;
}

/** A finished attribution and what it was built for (`key` null = live heads). */
interface AttributionResult {
  payload: string | null;
  key: string | null;
}

/**
 * Build the attribution payload that the q2-debug WASM entry consumes.
 *
 * Returns the payload plus a `generating` flag that is true whenever
 * the hook is mid-build (cold start, debounce window, or synchronous
 * incremental update).
 */
export function useAttribution(opts: UseAttributionOptions): UseAttributionResult {
  const { enabled, filePath, sourceText, identities } = opts;
  const atHeads = opts.atHeads ?? null;
  // Heads arrive as a fresh array each step; the joined hashes identify it.
  const headsKey = atHeads ? atHeads.join(',') : null;
  const replaying = headsKey !== null;

  const [result, setResult] = useState<AttributionResult | null>(null);
  const [generating, setGenerating] = useState(false);

  // Latest run-list state — used by the incremental update path.
  const stateRef = useRef<RunListAttribution | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // True while a cold build is running (cleared when it settles or is aborted).
  const buildingRef = useRef(false);
  // What the last finished result was built for; `undefined` = no result.
  const resultKeyRef = useRef<string | null | undefined>(undefined);
  // Replay: the live doc's changes by hash, and the runs of steps shown.
  const replayIndexRef = useRef(createReplayIndex());
  const stepCacheRef = useRef(new Map<string, RunListAttribution>());

  // Capture latest inputs in refs so async callbacks see the freshest
  // values without re-binding.
  const sourceTextRef = useRef(sourceText);
  sourceTextRef.current = sourceText;
  const identitiesRef = useRef(identities);
  identitiesRef.current = identities;
  const atHeadsRef = useRef(atHeads);
  atHeadsRef.current = atHeads;
  const replayingRef = useRef(replaying);
  replayingRef.current = replaying;

  const settle = useCallback((next: AttributionResult | null) => {
    resultKeyRef.current = next ? next.key : undefined;
    setResult(next);
  }, []);

  /** Publish `state` as the attribution for `key` (null = live heads). */
  const finalisePayload = useCallback((state: RunListAttribution, key: string | null) => {
    if (key !== null) {
      const cache = stepCacheRef.current;
      cache.delete(key);
      cache.set(key, state);
      if (cache.size > STEP_CACHE_SIZE) cache.delete(cache.keys().next().value!);
    }
    settle({
      payload: buildAttributionPayload(state, sourceTextRef.current, identitiesRef.current),
      key,
    });
  }, [settle]);

  const abortBuild = useCallback(() => {
    if (abortRef.current) abortRef.current.abort();
    abortRef.current = null;
    buildingRef.current = false;
  }, []);

  /** Cold build, to the live heads or (replay) to `target`, identified by `key`. */
  const startBuild = useCallback(
    (path: string, target?: ReplayTarget, key: string | null = null) => {
      abortBuild();
      const controller = new AbortController();
      abortRef.current = controller;

      // A live rebuild drops the payload; a replay one is hidden by its key.
      if (key === null) settle(null);
      setGenerating(true);
      stateRef.current = null;

      const fail = () => {
        buildingRef.current = false;
        // In replay, settle the step without attribution so the preview
        // stops waiting for it.
        if (key !== null) settle({ payload: null, key });
        setGenerating(false);
      };

      let handle;
      try {
        handle = getFileHandle(path);
      } catch {
        fail();
        return;
      }
      if (!handle) {
        fail();
        return;
      }

      buildingRef.current = true;
      buildRunListAttribution(handle, 'text', controller.signal, target)
        .then(state => {
          // If aborted, a newer build owns `generating` — leave it alone.
          if (controller.signal.aborted || !state) return;
          buildingRef.current = false;
          stateRef.current = state;
          finalisePayload(state, key);
          setGenerating(false);
        })
        .catch(err => {
          if (controller.signal.aborted) return;
          console.warn('[useAttribution] build failed:', err);
          fail();
        });
    },
    [abortBuild, finalisePayload, settle],
  );

  // (Re)start the build when enabled / filePath changes.
  useEffect(() => {
    replayIndexRef.current.clear();
    stepCacheRef.current.clear();
    if (!enabled || !filePath) {
      settle(null);
      setGenerating(false);
      stateRef.current = null;
      abortBuild();
      if (debounceRef.current) clearTimeout(debounceRef.current);
      return;
    }

    // In replay the effect below builds for the step's heads instead.
    if (!replayingRef.current) startBuild(filePath);

    return () => {
      abortBuild();
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [enabled, filePath, startBuild, abortBuild, settle]);

  // Replay: attribution for the step's heads. A forward step extends the
  // current state synchronously; anything else is a cold build to the step.
  useEffect(() => {
    if (!enabled || !filePath || headsKey === null) return;
    const heads = atHeadsRef.current;
    if (!heads) return;

    // Whatever is in flight targets another step (or the live heads).
    abortBuild();
    if (debounceRef.current) clearTimeout(debounceRef.current);

    const cached = stepCacheRef.current.get(headsKey);
    if (cached) {
      finalisePayload(cached, headsKey);
      setGenerating(false);
      return;
    }

    const target: ReplayTarget = { heads, index: replayIndexRef.current };
    if (stateRef.current) {
      try {
        const handle = getFileHandle(filePath);
        if (handle) {
          const next = updateRunListAttribution(stateRef.current, handle, 'text', target);
          stateRef.current = next;
          finalisePayload(next, headsKey);
          setGenerating(false);
          return;
        }
      } catch (err) {
        if (!(err instanceof TargetNotAheadError || err instanceof HistoryCompactedError)) {
          console.warn('[useAttribution] replay update failed:', err);
        }
        // Fall through to the cold build.
      }
    }
    startBuild(filePath, target, headsKey);
  }, [headsKey, identities, enabled, filePath, abortBuild, finalisePayload, startBuild]);

  // Leaving replay: free the per-replay history index and step cache, and
  // drop a build still aimed at a replay step. With a state in hand the
  // effect below catches it up to the live heads; without one, build afresh.
  const wasReplayingRef = useRef(replaying);
  useEffect(() => {
    const wasReplaying = wasReplayingRef.current;
    wasReplayingRef.current = replaying;
    if (replaying || !wasReplaying) return;
    replayIndexRef.current.clear();
    stepCacheRef.current.clear();
    if (!enabled || !filePath) return;
    abortBuild();
    if (!stateRef.current) startBuild(filePath);
  }, [replaying, enabled, filePath, abortBuild, startBuild]);

  // Debounced incremental update on text edits (live heads only).
  useEffect(() => {
    if (!enabled || !filePath || replaying || !stateRef.current) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    // Pulse the indicator from the moment we know an update is coming,
    // not just during the synchronous slice — the debounce window is
    // part of the "still being generated" period from the user's view.
    setGenerating(true);
    // Coming back from replay there is no live payload to keep showing,
    // so catch up to the live heads at once.
    const delay = resultKeyRef.current === null ? DEBOUNCE_MS : 0;
    debounceRef.current = setTimeout(() => {
      if (!stateRef.current) return;
      let handle;
      try {
        handle = getFileHandle(filePath);
      } catch {
        setGenerating(false);
        return;
      }
      if (!handle) {
        setGenerating(false);
        return;
      }

      try {
        const next = updateRunListAttribution(stateRef.current, handle, 'text');
        stateRef.current = next;
        finalisePayload(next, null);
        setGenerating(false);
      } catch (err) {
        if (err instanceof HistoryCompactedError) {
          // startBuild flips generating back to true for the cold-start.
          startBuild(filePath);
        } else {
          console.warn('[useAttribution] update failed:', err);
          setGenerating(false);
        }
      }
    }, delay);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [sourceText, identities, enabled, filePath, replaying, finalisePayload, startBuild]);

  // A result counts only for what it was built for: the current replay
  // step, or the live heads outside replay.
  const current = result !== null && result.key === headsKey ? result : null;
  return {
    payload: current?.payload ?? null,
    generating,
    pending: enabled && !!filePath && replaying && current === null,
  };
}
