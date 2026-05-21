/**
 * Run-length-encoded attribution **producer** for Automerge documents.
 *
 * Replays the Automerge history of a text field and emits a sorted,
 * non-overlapping, contiguous run list `[{start, end, actor, time}, ...]`
 * keyed in **JS character offsets** (Automerge splice positions = UTF-16
 * code units). Consumers must convert to UTF-8 byte offsets before
 * shipping to the Rust pipeline; see `useAttribution` for that step.
 *
 * Only the producer side lives here. Per the Phase 5 plan, the
 * consumer-side query / reconstruction / cache code from the prototype's
 * `attribution.ts` / `attribution-runs.ts` is deliberately *not* ported
 * — the Rust `AttributionMap::query_byte_range` replaces it.
 *
 * Algorithm reference (and known-good baseline): the prototype branch
 * `feat/node-attribution` carries this file along with the consumer-side
 * surface and the `attribution-runs.test.ts` invariant suite.
 *
 * See `claude-notes/designs/attribution-encoding-contract.md` for the
 * full statement of the UTF-16 / UTF-8 boundary and why both sides are
 * correct in their own coordinate space.
 */

import {
  applyChanges,
  decodeChange,
  getAllChanges,
  getChanges,
  init,
} from '@automerge/automerge';
import type { Change, Doc, Patch } from '@automerge/automerge';
import { decodeHeads } from '@automerge/automerge-repo';
import type { DocHandle } from '@automerge/automerge-repo';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CharAttribution {
  actor: string;
  time: number;
}

export interface AttributionRun {
  /** inclusive char offset */
  start: number;
  /** exclusive char offset */
  end: number;
  actor: string;
  time: number;
}

export interface RunListAttribution {
  runs: AttributionRun[];
  processedHeads: unknown[];
  processedHistoryIndex: number;
  /**
   * Internal: forward-replay doc held at `processedHeads`, fed to
   * `A.applyChanges` so each incremental update only pays for new
   * changes (not a full doc-state load per step like `A.diff` did).
   * Absent when state is hand-constructed (tests), in which case
   * `updateRunListAttribution` throws `HistoryCompactedError` and
   * the caller (`useAttribution`) falls back to a full rebuild.
   */
  _workDoc?: Doc<unknown>;
}

interface SplicePatch {
  action: 'splice';
  path: [string, number];
  value: string;
}

interface DelPatch {
  action: 'del';
  path: [string, number];
  length?: number;
}

interface PutPatch {
  action: 'put';
  path: [string];
  value: string;
}

export type TextPatch = SplicePatch | DelPatch | PutPatch;

export interface ViewableHandle {
  history(): unknown[] | undefined;
  metadata(change?: string): { time?: number; actor?: string } | undefined;
  doc(): unknown;
}

export class HistoryCompactedError extends Error {
  constructor() {
    super('History has been compacted — full rebuild required');
    this.name = 'HistoryCompactedError';
  }
}

export function isTextPatch(patch: unknown, textFieldName: string): patch is TextPatch {
  const p = patch as { action?: string; path?: unknown[] };
  if (!p || !Array.isArray(p.path) || p.path[0] !== textFieldName) return false;
  return p.action === 'splice' || p.action === 'del' || p.action === 'put';
}

export function extractChangeHash(heads: unknown): string | null {
  const h = Array.isArray(heads) ? heads[0] : heads;
  return typeof h === 'string' ? h : null;
}

/**
 * History entries processed between idle-callback yields. Larger
 * values cut yield overhead at the cost of bigger per-slice CPU.
 *
 * Per-entry CPU is ~15 µs (applyChanges forward-replay, roughly
 * constant in N), so a 500-entry slice is ≈7-8 ms — comfortably
 * under one frame (16.67 ms). Slices above ~1000 risk overrunning a
 * frame in busier browsers.
 */
export const CHUNK_SIZE = 500;

export function waitForIdle(timeout = 100): Promise<void> {
  return new Promise<void>(resolve => {
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(() => resolve(), { timeout });
    } else {
      setTimeout(resolve, 0);
    }
  });
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

function runsInsert(
  runs: AttributionRun[],
  p: number,
  k: number,
  attr: CharAttribution,
): void {
  if (k === 0) return;
  let lo = findFirstRunEndingAfter(runs, p);

  if (lo < runs.length && runs[lo].start < p) {
    const r = runs[lo];
    runs.splice(lo + 1, 0, { start: p, end: r.end, actor: r.actor, time: r.time });
    r.end = p;
    lo++;
  }

  for (let i = lo; i < runs.length; i++) {
    runs[i].start += k;
    runs[i].end += k;
  }

  runs.splice(lo, 0, { start: p, end: p + k, actor: attr.actor, time: attr.time });
  maybeMergeAt(runs, lo);
}

function runsDelete(runs: AttributionRun[], p: number, len: number): void {
  if (len === 0) return;
  const endPos = p + len;
  const lo = findFirstRunEndingAfter(runs, p);

  let i = lo;
  while (i < runs.length && runs[i].start < endPos) {
    const r = runs[i];
    if (r.start >= p && r.end <= endPos) {
      runs.splice(i, 1);
    } else if (r.start < p && r.end > endPos) {
      r.end -= len;
      i++;
    } else if (r.start < p) {
      r.end = p;
      i++;
    } else {
      r.start = p;
      r.end -= len;
      i++;
    }
  }

  for (let j = i; j < runs.length; j++) {
    runs[j].start -= len;
    runs[j].end -= len;
  }

  if (lo > 0 && lo <= runs.length) maybeMergeAt(runs, lo - 1);
  if (lo < runs.length) maybeMergeAt(runs, lo);
}

function maybeMergeAt(runs: AttributionRun[], i: number): void {
  if (i < 0 || i >= runs.length) return;
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

function applyPatchToRuns(
  runs: AttributionRun[],
  patch: TextPatch,
  attr: CharAttribution,
): void {
  if (patch.action === 'put') {
    const text = typeof patch.value === 'string' ? patch.value : '';
    runs.length = 0;
    if (text.length > 0) {
      runs.push({ start: 0, end: text.length, actor: attr.actor, time: attr.time });
    }
    return;
  }
  const idx = patch.path[1];
  if (patch.action === 'splice') {
    runsInsert(runs, idx, patch.value.length, attr);
  } else {
    runsDelete(runs, idx, patch.length ?? 1);
  }
}

// ---------------------------------------------------------------------------
// Internal: shared replay loop
// ---------------------------------------------------------------------------

/**
 * The new change introduced at this history step is whichever hash is in
 * `currHeads` but not in `prevHeads`. For the first step, take the first
 * head. Returns null if no new change can be identified (defensive).
 */
function newChangeHashAt(prevHeads: string[] | null, currHeads: string[]): string | null {
  if (currHeads.length === 0) return null;
  if (prevHeads === null) return currHeads[0];
  const prevSet = new Set(prevHeads);
  return currHeads.find(h => !prevSet.has(h)) ?? null;
}

/**
 * Apply one change to `workDoc`, collect any patches via patchCallback,
 * and fold them into the running runs list using the change's own
 * actor/time. Returns the advanced workDoc.
 */
function replayChange(
  workDoc: Doc<unknown>,
  change: Change,
  textFieldName: string,
  runs: AttributionRun[],
): Doc<unknown> {
  const decoded = decodeChange(change);
  const attribution: CharAttribution = { actor: decoded.actor, time: decoded.time };
  let collected: Patch[] = [];
  const [next] = applyChanges(workDoc, [change], {
    patchCallback: (patches: Patch[]) => { collected = patches; },
  });
  for (const patch of collected) {
    if (isTextPatch(patch, textFieldName)) applyPatchToRuns(runs, patch, attribution);
  }
  return next;
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
    return { runs: [], processedHeads: [], processedHistoryIndex: 0, _workDoc: init() };
  }

  // Pre-index every change in the doc by hash so each history step can
  // look up its corresponding change in O(1).
  const doc = viewable.doc() as Doc<unknown>;
  const changeByHash = new Map<string, Change>();
  for (const c of getAllChanges(doc)) {
    changeByHash.set(decodeChange(c).hash, c);
  }

  const runs: AttributionRun[] = [];
  let prevHeads: string[] | null = null;
  let lastHeads: unknown[] = [];
  let workDoc: Doc<unknown> = init();

  for (let chunkStart = 0; chunkStart < history.length; chunkStart += CHUNK_SIZE) {
    if (signal?.aborted) return null;

    const chunkEnd = Math.min(chunkStart + CHUNK_SIZE, history.length);
    for (let i = chunkStart; i < chunkEnd; i++) {
      const currHeads = history[i];
      const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);
      const newHash = newChangeHashAt(prevHeads, decodedCurr);
      const change = newHash ? changeByHash.get(newHash) : undefined;
      if (change) {
        workDoc = replayChange(workDoc, change, textFieldName, runs);
      }
      prevHeads = decodedCurr;
      lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
    }

    if (chunkEnd < history.length) await waitForIdle();
  }

  return {
    runs,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
    _workDoc: workDoc,
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
  if (!state._workDoc) throw new HistoryCompactedError();

  if (state.processedHistoryIndex === history.length) {
    return state;
  }

  // Pull just the new changes (since workDoc's heads), index by hash.
  const doc = viewable.doc() as Doc<unknown>;
  const newChanges = getChanges(state._workDoc, doc);
  const changeByHash = new Map<string, Change>();
  for (const c of newChanges) {
    changeByHash.set(decodeChange(c).hash, c);
  }

  const runs = state.runs.map(r => ({ ...r }));
  let prevHeads = decodeHeads(state.processedHeads as Parameters<typeof decodeHeads>[0]);
  let lastHeads: unknown[] = state.processedHeads;
  let workDoc: Doc<unknown> = state._workDoc;

  for (let i = state.processedHistoryIndex; i < history.length; i++) {
    const currHeads = history[i];
    const decodedCurr = decodeHeads(currHeads as Parameters<typeof decodeHeads>[0]);
    const newHash = newChangeHashAt(prevHeads, decodedCurr);
    const change = newHash ? changeByHash.get(newHash) : undefined;
    if (change) {
      workDoc = replayChange(workDoc, change, textFieldName, runs);
    }
    prevHeads = decodedCurr;
    lastHeads = Array.isArray(currHeads) ? currHeads : [currHeads];
  }

  return {
    runs,
    processedHeads: lastHeads as unknown[],
    processedHistoryIndex: history.length,
    _workDoc: workDoc,
  };
}

// ---------------------------------------------------------------------------
// Char → byte offset translation (for the WASM wire)
// ---------------------------------------------------------------------------

/**
 * Build a JS-char-offset → UTF-8-byte-offset map for `text`.
 *
 * Indexed by **UTF-16 code unit** (Automerge's splice positions), so
 * surrogate-pair halves each get an entry. The map's length is
 * `text.length + 1`; `map[text.length]` is the total byte count.
 *
 * This is the inverse direction of the prototype's `buildByteToCharMap`.
 * The Rust pipeline's `SourceInfo` carries byte offsets, so producer runs
 * must be byte-translated before serializing for `PreBuiltAttributionProvider`.
 *
 * ASCII-only docs: map is the identity. Non-ASCII docs require this
 * translation for correctness — a missing translation would silently
 * misattribute any range past the first multi-byte character.
 *
 * Returned as `Uint32Array` so the per-codeunit storage is a single
 * contiguous buffer of 32-bit ints rather than boxed `number` slots —
 * matters because this is rebuilt on every debounced payload update.
 */
export function buildCharToByteMap(text: string): Uint32Array {
  const map = new Uint32Array(text.length + 1);
  let byteOff = 0;
  for (let i = 0; i < text.length; i++) {
    map[i] = byteOff;
    const ch = text.charCodeAt(i);
    // Surrogate pair (4-byte UTF-8) — first half here, low half on next iter.
    if (ch >= 0xd800 && ch <= 0xdbff && i + 1 < text.length) {
      const low = text.charCodeAt(i + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        byteOff += 4;
        i++; // skip low surrogate index in the outer loop
        map[i] = byteOff; // boundary entry: low-surrogate index points past the 4 bytes
        continue;
      }
    }
    if (ch < 0x80) byteOff += 1;
    else if (ch < 0x800) byteOff += 2;
    else byteOff += 3;
  }
  map[text.length] = byteOff;
  return map;
}

/**
 * Translate a `runs[]` slice from char offsets to byte offsets using the
 * map produced by `buildCharToByteMap`. Returns a fresh array — the
 * input is not mutated.
 */
export function runsCharToByteOffsets(
  runs: AttributionRun[],
  charToByte: Uint32Array,
): AttributionRun[] {
  return runs.map(r => ({
    start: charToByte[r.start] ?? r.start,
    end: charToByte[r.end] ?? r.end,
    actor: r.actor,
    time: r.time,
  }));
}

// Exposed for tests.
export const __internal = { runsInsert, runsDelete, applyPatchToRuns, findFirstRunEndingAfter };
