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

import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';

import { buildCharToByteMap, runsCharToByteOffsets } from '../services/attribution-runs';
import type { AttributionRun, RunListAttribution } from '../services/attribution-runs';
import {
  ATTRIBUTION_NONE,
  ATTRIBUTION_PENDING,
  AttributionStore,
} from '../services/attribution-store';
import type { AttributionSnapshot } from '../services/attribution-store';
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
   * A replay seek's attribution is being built. The preview holds its render
   * meanwhile, so the step is drawn once, with its own runs.
   */
  pending: boolean;
}

const noStore = () => () => {};
const notGenerating = () => false;

/**
 * Build the attribution payload that the q2-debug WASM entry consumes.
 *
 * The run list lives in an `AttributionStore` and is read during render, so
 * the render that brings a text already has that text's attribution: an
 * edit, a forward replay step, or a replay step shown before. Only a cold
 * build is asynchronous, and `pending` covers the one for a replay seek.
 */
export function useAttribution(opts: UseAttributionOptions): UseAttributionResult {
  const { enabled, filePath, sourceText, identities } = opts;
  // Heads arrive as a fresh array each step; the joined hashes identify it.
  const headsKey = opts.atHeads ? opts.atHeads.join(',') : null;
  const branchId = headsKey !== null ? null : (opts.branchId ?? null);

  const store = useMemo(
    () => (enabled && filePath ? new AttributionStore(filePath, branchId) : null),
    [enabled, filePath, branchId],
  );
  useEffect(() => () => store?.dispose(), [store]);

  const getSnapshot = useCallback(
    (): AttributionSnapshot => (store ? store.snapshot(headsKey) : ATTRIBUTION_NONE),
    [store, headsKey],
  );
  const snapshot = useSyncExternalStore(store ? store.subscribe : noStore, getSnapshot);
  const generating = useSyncExternalStore(
    store ? store.subscribe : noStore,
    store ? store.isGenerating : notGenerating,
  );

  // A read cannot start a build; do it once the render has committed.
  useEffect(() => store?.reconcile());

  const payload = useMemo(() => {
    if (typeof snapshot === 'string') return null;
    // The runs tile the history's text. If that is not the text shown (React
    // state is a change behind the doc), their offsets would be wrong for
    // it: show no overlay until the next text arrives.
    if ((snapshot.runs.at(-1)?.end ?? 0) !== sourceText.length) return null;
    return buildAttributionPayload(snapshot, sourceText, identities);
  }, [snapshot, sourceText, identities]);

  return { payload, generating, pending: snapshot === ATTRIBUTION_PENDING };
}
