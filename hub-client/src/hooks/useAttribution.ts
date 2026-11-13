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
import { getBranchDoc } from '../services/branchService';
import { getFileHandle } from '@quarto/preview-runtime';
import type { ActorIdentity } from '@quarto/preview-runtime';
import type { DocHandle } from '@automerge/automerge-repo';
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
  /**
   * The active local branch, when `sourceText` is that branch's text. The
   * history is then read from the branch's doc, not the synced one. Replay
   * walks the synced doc's history, so this is ignored while `atHeads` is set.
   */
  branchId?: string | null;
}

export interface UseAttributionResult {
  /**
   * JSON payload for `parseQmdToAstWithAttribution`, built for exactly the
   * current `sourceText` (and `atHeads`), or `null`: while disabled, the
   * file is unknown, no build has completed yet, or the history and the
   * text disagree. It is never the payload of another text, whose run
   * offsets would land on the wrong words.
   */
  payload: string | null;
  /**
   * True while a cold build runs (the overlay was just turned on, the file
   * or branch changed, or a replay seek needs a rebuild). Drives the
   * Attribution pill's "work in progress" border animation upstream.
   */
  generating: boolean;
  /**
   * The attribution for the current text is about to be settled (built or
   * given up on): by this render's effects for an edit or a forward replay
   * step, or by the running build for a replay seek. The preview holds its
   * render meanwhile, so no text is drawn with another text's runs.
   */
  pending: boolean;
}

/** A settled attribution and what it was settled for (`key` null = live heads). */
interface AttributionResult {
  payload: string | null;
  key: string | null;
  text: string;
}

/** The doc whose history gives the attribution: a local branch's, or the synced one. */
function resolveHandle(path: string, branchId: string | null): DocHandle<unknown> | null {
  if (branchId === null) return getFileHandle(path) ?? null;
  if (!getBranchDoc(path, branchId)) return null;
  // Branch edits replace the doc object; read it afresh on each access.
  return { doc: () => getBranchDoc(path, branchId) } as unknown as DocHandle<unknown>;
}

/**
 * Build the attribution payload that the q2-debug WASM entry consumes.
 *
 * The run list follows the document synchronously: an edit, or a forward
 * replay step, is folded in by the effects of the render that brought the
 * new text, and `pending` covers the gap. Only cold builds are asynchronous.
 */
export function useAttribution(opts: UseAttributionOptions): UseAttributionResult {
  const { enabled, filePath, sourceText, identities } = opts;
  const atHeads = opts.atHeads ?? null;
  // Heads arrive as a fresh array each step; the joined hashes identify it.
  const headsKey = atHeads ? atHeads.join(',') : null;
  const replaying = headsKey !== null;
  const branchId = replaying ? null : (opts.branchId ?? null);

  const [result, setResult] = useState<AttributionResult | null>(null);
  const [generating, setGenerating] = useState(false);

  // Latest run-list state — used by the incremental update path.
  const stateRef = useRef<RunListAttribution | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  // The running cold build's target (`key` null = live heads), or null.
  const buildingRef = useRef<{ key: string | null } | null>(null);
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

  /** Settle the current text for `key` (null = live heads) with the runs of `state`. */
  const publish = useCallback((state: RunListAttribution, key: string | null) => {
    if (key !== null) {
      const cache = stepCacheRef.current;
      cache.delete(key);
      cache.set(key, state);
      if (cache.size > STEP_CACHE_SIZE) cache.delete(cache.keys().next().value!);
    }
    const text = sourceTextRef.current;
    // The runs tile the history's text. If that is not the text shown (React
    // state is a change behind the doc), their offsets would be wrong for
    // it: show no overlay until the next text arrives.
    const covers = (state.runs.at(-1)?.end ?? 0) === text.length;
    setResult({
      payload: covers ? buildAttributionPayload(state, text, identitiesRef.current) : null,
      key,
      text,
    });
  }, []);

  /** Settle the current text for `key` without attribution. */
  const giveUp = useCallback((key: string | null) => {
    setResult({ payload: null, key, text: sourceTextRef.current });
  }, []);

  const abortBuild = useCallback(() => {
    if (abortRef.current) abortRef.current.abort();
    abortRef.current = null;
    buildingRef.current = null;
  }, []);

  /** Cold build, to the live heads of `branch` or (replay) to `target`, identified by `key`. */
  const startBuild = useCallback(
    (path: string, branch: string | null, target?: ReplayTarget, key: string | null = null) => {
      abortBuild();
      const controller = new AbortController();
      abortRef.current = controller;

      // A live rebuild drops the payload; a replay one is hidden by its key.
      if (key === null) setResult(null);
      setGenerating(true);
      stateRef.current = null;

      const fail = () => {
        buildingRef.current = null;
        // In replay, settle the step so the preview stops waiting for it.
        if (key !== null) giveUp(key);
        setGenerating(false);
      };

      let handle;
      try {
        handle = resolveHandle(path, branch);
      } catch {
        fail();
        return;
      }
      if (!handle) {
        fail();
        return;
      }

      buildingRef.current = { key };
      buildRunListAttribution(handle, 'text', controller.signal, target)
        .then(state => {
          // If aborted, a newer build owns `generating` — leave it alone.
          if (controller.signal.aborted || !state) return;
          buildingRef.current = null;
          stateRef.current = state;
          publish(state, key);
          setGenerating(false);
        })
        .catch(err => {
          if (controller.signal.aborted) return;
          console.warn('[useAttribution] build failed:', err);
          fail();
        });
    },
    [abortBuild, publish, giveUp],
  );

  // (Re)start the build when enabled / filePath / branch changes.
  useEffect(() => {
    replayIndexRef.current.clear();
    stepCacheRef.current.clear();
    if (!enabled || !filePath) {
      setResult(null);
      setGenerating(false);
      stateRef.current = null;
      abortBuild();
      return;
    }

    // In replay the effect below builds for the step's heads instead.
    if (replayingRef.current) {
      stateRef.current = null;
    } else {
      startBuild(filePath, branchId);
    }

    return abortBuild;
  }, [enabled, filePath, branchId, startBuild, abortBuild]);

  // Replay: attribution for the step's heads. A forward step extends the
  // current state synchronously; anything else is a cold build to the step.
  useEffect(() => {
    if (!enabled || !filePath || headsKey === null) return;
    const heads = atHeadsRef.current;
    if (!heads) return;

    // Whatever is in flight targets another step (or the live heads).
    abortBuild();

    const cached = stepCacheRef.current.get(headsKey);
    if (cached) {
      publish(cached, headsKey);
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
          publish(next, headsKey);
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
    startBuild(filePath, null, target, headsKey);
  }, [headsKey, identities, enabled, filePath, abortBuild, publish, startBuild]);

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
    if (buildingRef.current && buildingRef.current.key !== null) abortBuild();
    if (!stateRef.current && !buildingRef.current) startBuild(filePath, branchId);
  }, [replaying, enabled, filePath, branchId, abortBuild, startBuild]);

  // Live heads: fold each edit into the runs as its text arrives. With no
  // state yet, a cold build is running; it ends at the then-current heads
  // and publishes for the then-current text.
  useEffect(() => {
    if (!enabled || !filePath || replaying || !stateRef.current) return;

    let handle;
    try {
      handle = resolveHandle(filePath, branchId);
    } catch {
      handle = null;
    }
    if (!handle) {
      giveUp(null);
      return;
    }

    try {
      const next = updateRunListAttribution(stateRef.current, handle, 'text');
      stateRef.current = next;
      publish(next, null);
    } catch (err) {
      if (err instanceof HistoryCompactedError) {
        startBuild(filePath, branchId);
      } else {
        console.warn('[useAttribution] update failed:', err);
        giveUp(null);
      }
    }
  }, [sourceText, identities, enabled, filePath, replaying, branchId, publish, giveUp, startBuild]);

  // A result counts only for what it was settled for: this text, at the
  // current replay step or the live heads.
  const current =
    result !== null && result.key === headsKey && result.text === sourceText ? result : null;
  // Live, a result for an earlier text means a state exists, and this
  // render's effects settle the new text. With no result, only a cold build
  // is under way: the text is drawn without the overlay until it finishes.
  const pending = enabled && !!filePath && current === null && (replaying || result !== null);
  return { payload: current?.payload ?? null, generating, pending };
}
