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
 * **Disabled path:** when `enabled` is false (Authorship toggle off),
 * the hook short-circuits and returns `null`. Callers then route through
 * `parseQmdToAst(content)` (or `parseQmdToAstWithAttribution(content, null)`),
 * which is byte-identical to today's q2-debug output (Phase 0 test #10).
 */

import { useState, useEffect, useRef, useCallback } from 'react';

import {
  buildCharToByteMap,
  buildRunListAttribution,
  HistoryCompactedError,
  runsCharToByteOffsets,
  updateRunListAttribution,
} from '../services/attribution-runs';
import type {
  AttributionRun,
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
 * Build an `IdentityMap` covering every actor referenced by `runs`,
 * satisfying the Phase 6 producer invariant: **every** actor in `runs`
 * has an entry in `identities` at the wire.
 *
 * Resolution order per actor:
 *   1. `identities[actor]` (Automerge profile metadata when available),
 *   2. fallback `(actor.slice(0, 8), actorColor(fnv1aHex8(actor)))` —
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

export interface UseAttributionOptions {
  /** When false, the hook returns `null` and does no work. */
  enabled: boolean;
  /** The current file's Automerge path (e.g. "index.qmd"). */
  filePath: string | null;
  /** The current full document text — needed for char→byte translation. */
  sourceText: string;
  /** Actor → profile identity table (from `App.tsx`'s identities state). */
  identities: Record<string, ActorIdentity>;
}

export interface UseAttributionResult {
  /**
   * JSON payload for `parseQmdToAstWithAttribution`, or `null` while
   * disabled, the file is unknown, or no build has completed yet.
   * Once a build completes the string remains referentially stable
   * across re-renders until the source text changes enough to
   * invalidate it.
   */
  payload: string | null;
  /**
   * True while the hook is doing work the user is waiting on: the
   * cold-start build, the debounced incremental update window, and
   * the synchronous update step itself. Drives the Authorship pill's
   * "work in progress" border animation upstream.
   *
   * Distinct from `payload === null` because incremental updates
   * keep the previous payload until the new one is ready — so a
   * naïve `payload === null && enabled` check would miss them.
   */
  generating: boolean;
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

  const [payload, setPayload] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);

  // Latest run-list state — used by the incremental update path.
  const stateRef = useRef<RunListAttribution | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Capture latest inputs in refs so async callbacks see the freshest
  // values without re-binding.
  const sourceTextRef = useRef(sourceText);
  sourceTextRef.current = sourceText;
  const identitiesRef = useRef(identities);
  identitiesRef.current = identities;

  const finalisePayload = useCallback((state: RunListAttribution) => {
    setPayload(
      buildAttributionPayload(state, sourceTextRef.current, identitiesRef.current),
    );
  }, []);

  const startBuild = useCallback(
    (path: string) => {
      if (abortRef.current) abortRef.current.abort();
      const controller = new AbortController();
      abortRef.current = controller;

      setPayload(null);
      setGenerating(true);
      stateRef.current = null;

      let handle;
      try {
        handle = getFileHandle(path);
      } catch {
        setGenerating(false);
        return;
      }
      if (!handle) {
        setGenerating(false);
        return;
      }

      buildRunListAttribution(handle, 'text', controller.signal)
        .then(state => {
          // If aborted, a newer build owns `generating` — leave it alone.
          if (controller.signal.aborted || !state) return;
          stateRef.current = state;
          finalisePayload(state);
          setGenerating(false);
        })
        .catch(err => {
          if (controller.signal.aborted) return;
          console.warn('[useAttribution] build failed:', err);
          setGenerating(false);
        });
    },
    [finalisePayload],
  );

  // (Re)start the build when enabled / filePath changes.
  useEffect(() => {
    if (!enabled || !filePath) {
      setPayload(null);
      setGenerating(false);
      stateRef.current = null;
      if (abortRef.current) abortRef.current.abort();
      if (debounceRef.current) clearTimeout(debounceRef.current);
      return;
    }

    startBuild(filePath);

    return () => {
      if (abortRef.current) abortRef.current.abort();
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [enabled, filePath, startBuild]);

  // Debounced incremental update on text edits.
  useEffect(() => {
    if (!enabled || !filePath || !stateRef.current) return;

    if (debounceRef.current) clearTimeout(debounceRef.current);
    // Pulse the indicator from the moment we know an update is coming,
    // not just during the synchronous slice — the debounce window is
    // part of the "still being generated" period from the user's view.
    setGenerating(true);
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
        finalisePayload(next);
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
    }, DEBOUNCE_MS);

    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [sourceText, identities, enabled, filePath, finalisePayload, startBuild]);

  return { payload, generating };
}
