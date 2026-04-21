/**
 * Run-length-encoded attribution producer (prototype).
 *
 * Alternative to the per-character CharAttribution[] path in attribution.ts.
 * Stores attribution as sorted, non-overlapping, contiguous byte-ranged
 * runs. Plug in via `makeRunListSource(...)` — consumers see a standard
 * `AttributionSource` and never know the storage shape.
 *
 * Hypotheses under test (Phase B):
 *   1. Build stays linear even for prepend/mid-doc workloads (per-char is
 *      quadratic in the splice's O(N) shift).
 *   2. Large bulk inserts become O(1) run operations, obviating the splice
 *      chunking workaround in applyPatch.
 *   3. Memory drops from ~8 bytes/char to ~48 bytes/run (where runs ≪ chars
 *      for realistic workloads).
 */

import { diff } from '@automerge/automerge';
import type { Heads } from '@automerge/automerge';
import { decodeHeads } from '@automerge/automerge-repo';
import type { DocHandle } from '@automerge/automerge-repo';

import {
  CHUNK_SIZE,
  HistoryCompactedError,
  extractChangeHash,
  isTextPatch,
  waitForIdle,
  type AttributionSource,
  type CharAttribution,
  type TextPatch,
  type ViewableHandle,
} from './attribution';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AttributionRun {
  start: number; // inclusive char offset
  end: number;   // exclusive char offset
  actor: string;
  time: number;
}

export interface RunListAttribution {
  runs: AttributionRun[];
  processedHeads: unknown[];
  processedHistoryIndex: number;
}

// ---------------------------------------------------------------------------
// Internal: patch application on run list
// ---------------------------------------------------------------------------

/** Binary search for first run whose `end > p`. Returns runs.length if none. */
function findFirstRunEndingAfter(runs: AttributionRun[], p: number): number {
  let lo = 0;
  let hi = runs.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (runs[mid].end <= p) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Insert a run `[p, p+k)` with the given attribution. Runs at/after `p`
 * shift right by `k`; a run split by `p` is split first.
 */
function runsInsert(
  runs: AttributionRun[],
  p: number,
  k: number,
  attr: CharAttribution,
): void {
  if (k === 0) return;
  let lo = findFirstRunEndingAfter(runs, p);

  // Split if p is strictly inside runs[lo].
  if (lo < runs.length && runs[lo].start < p) {
    const r = runs[lo];
    runs.splice(lo + 1, 0, { start: p, end: r.end, actor: r.actor, time: r.time });
    r.end = p;
    lo++;
  }

  // Shift runs at indices [lo, end) right by k.
  for (let i = lo; i < runs.length; i++) {
    runs[i].start += k;
    runs[i].end += k;
  }

  // Insert new run at position lo.
  runs.splice(lo, 0, { start: p, end: p + k, actor: attr.actor, time: attr.time });

  // Opportunistic merge with left and right neighbours of the new run.
  maybeMergeAt(runs, lo);
}

/** Delete `len` chars starting at position `p`; runs after shift left by `len`. */
function runsDelete(runs: AttributionRun[], p: number, len: number): void {
  if (len === 0) return;
  const endPos = p + len;
  let lo = findFirstRunEndingAfter(runs, p);

  // Process runs overlapping [p, endPos) — possibly trimming both ends.
  let i = lo;
  while (i < runs.length && runs[i].start < endPos) {
    const r = runs[i];
    if (r.start >= p && r.end <= endPos) {
      // Run fully inside delete — remove.
      runs.splice(i, 1);
    } else if (r.start < p && r.end > endPos) {
      // Delete fully inside run — shrink and stop.
      r.end -= len;
      i++;
    } else if (r.start < p) {
      // Run's tail is deleted.
      r.end = p;
      i++;
    } else {
      // Run's head is deleted — shift inline (loop below only covers runs
      // fully past endPos).
      r.start = p;
      r.end -= len;
      i++;
    }
  }

  // Shift runs from i onward left by len.
  for (let j = i; j < runs.length; j++) {
    runs[j].start -= len;
    runs[j].end -= len;
  }

  // Merge any neighbours made adjacent by the delete.
  if (lo > 0 && lo <= runs.length) maybeMergeAt(runs, lo - 1);
  if (lo < runs.length) maybeMergeAt(runs, lo);
}

/** Merge runs[i] with runs[i-1] and runs[i+1] if contiguous with same attribution. */
function maybeMergeAt(runs: AttributionRun[], i: number): void {
  if (i < 0 || i >= runs.length) return;
  // Try right neighbour first so the index of `i` stays valid.
  if (i + 1 < runs.length) {
    const a = runs[i];
    const b = runs[i + 1];
    if (a.end === b.start && a.actor === b.actor && a.time === b.time) {
      a.end = b.end;
      runs.splice(i + 1, 1);
    }
  }
  if (i > 0) {
    const a = runs[i - 1];
    const b = runs[i];
    if (a.end === b.start && a.actor === b.actor && a.time === b.time) {
      a.end = b.end;
      runs.splice(i, 1);
    }
  }
}

function applyPatchToRuns(runs: AttributionRun[], patch: TextPatch, attr: CharAttribution): void {
  if (patch.action === 'put') {
    const text = typeof patch.value === 'string' ? patch.value : '';
    runs.length = 0;
    if (text.length > 0) {
      runs.push({ start: 0, end: text.length, actor: attr.actor, time: attr.time });
    }
    return;
  }
  const idx = patch.path[1] as number;
  if (patch.action === 'splice') {
    runsInsert(runs, idx, patch.value.length, attr);
  } else {
    runsDelete(runs, idx, patch.length ?? 1);
  }
}

// ---------------------------------------------------------------------------
// buildRunListAttribution — full history processing
// ---------------------------------------------------------------------------

export async function buildRunListAttribution(
  handle: DocHandle<unknown>,
  textFieldName: string,
  signal?: AbortSignal,
): Promise<RunListAttribution | null> {
  const viewable = handle as unknown as ViewableHandle;
  const history = viewable.history();
  if (!history) return null;

  if (history.length === 0) {
    return { runs: [], processedHeads: [], processedHistoryIndex: 0 };
  }

  const runs: AttributionRun[] = [];
  let prevHeads: unknown = null;
  let lastHeads: unknown[] = [];

  for (let chunkStart = 0; chunkStart < history.length; chunkStart += CHUNK_SIZE) {
    await waitForIdle();
    if (signal?.aborted) return null;

    const chunkEnd = Math.min(chunkStart + CHUNK_SIZE, history.length);
    for (let i = chunkStart; i < chunkEnd; i++) {
      const currHeads = history[i];
      const changeHash = extractChangeHash(currHeads);
      const meta = changeHash ? viewable.metadata(changeHash) : undefined;
      const attribution: CharAttribution = {
        actor: meta?.actor ?? 'unknown',
        time: meta?.time ?? 0,
      };

      const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);
      let patches: unknown[];
      if (prevHeads === null) {
        patches = diff(
          viewable.doc() as Parameters<typeof diff>[0],
          [] as unknown as Heads,
          decodedCurr as unknown as Heads,
        );
      } else {
        const decodedPrev = decodeHeads(prevHeads as Parameters<typeof decodeHeads>[0]);
        patches = diff(
          viewable.doc() as Parameters<typeof diff>[0],
          decodedPrev as unknown as Heads,
          decodedCurr as unknown as Heads,
        );
      }

      for (const patch of patches) {
        if (isTextPatch(patch, textFieldName)) {
          applyPatchToRuns(runs, patch, attribution);
        }
      }

      prevHeads = currHeads;
      lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
    }
  }

  return {
    runs,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
  };
}

// ---------------------------------------------------------------------------
// updateRunListAttribution — incremental (synchronous)
// ---------------------------------------------------------------------------

export function updateRunListAttribution(
  state: RunListAttribution,
  handle: DocHandle<unknown>,
  textFieldName: string,
): RunListAttribution {
  const viewable = handle as unknown as ViewableHandle;
  const history = viewable.history();
  if (!history) throw new HistoryCompactedError();
  if (state.processedHistoryIndex > history.length) throw new HistoryCompactedError();

  const runs = state.runs.map(r => ({ ...r }));
  let prevHeads = state.processedHeads;
  let lastHeads = state.processedHeads;

  for (let i = state.processedHistoryIndex; i < history.length; i++) {
    const currHeads = history[i];
    const changeHash = extractChangeHash(currHeads);
    const meta = changeHash ? viewable.metadata(changeHash) : undefined;
    const attribution: CharAttribution = {
      actor: meta?.actor ?? 'unknown',
      time: meta?.time ?? 0,
    };

    const decodedPrev = decodeHeads(prevHeads as Parameters<typeof decodeHeads>[0]);
    const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);
    const patches = diff(
      viewable.doc() as Parameters<typeof diff>[0],
      decodedPrev as unknown as Heads,
      decodedCurr as unknown as Heads,
    );

    for (const patch of patches) {
      if (isTextPatch(patch, textFieldName)) {
        applyPatchToRuns(runs, patch, attribution);
      }
    }

    prevHeads = currHeads as unknown[];
    lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
  }

  return {
    runs,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
  };
}

// ---------------------------------------------------------------------------
// makeRunListSource — AttributionSource backed by sorted runs
// ---------------------------------------------------------------------------

export function makeRunListSource(
  runs: AttributionRun[],
  byteToCharMap: number[],
): AttributionSource {
  return {
    queryByteRange(_fileId, byteStart, byteEnd) {
      const charStart = byteToCharMap[byteStart];
      const charEnd = byteToCharMap[byteEnd];
      if (charStart === undefined || charEnd === undefined) return null;
      if (runs.length === 0) return null;
      if (charStart >= charEnd) return null;

      // Binary search for first run whose end > charStart
      let lo = 0;
      let hi = runs.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (runs[mid].end <= charStart) lo = mid + 1;
        else hi = mid;
      }

      let best: { actor: string; time: number } | null = null;
      for (let i = lo; i < runs.length && runs[i].start < charEnd; i++) {
        const r = runs[i];
        if (!best || r.time > best.time) {
          best = { actor: r.actor, time: r.time };
        }
      }
      return best;
    },
  };
}

// Exposed for tests.
export const __internal = { runsInsert, runsDelete, applyPatchToRuns };
