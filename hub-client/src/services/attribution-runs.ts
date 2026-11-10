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
  getChanges,
  init,
} from '@automerge/automerge';
import type { Change, Doc, Patch } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface CharAttribution {
  /**
   * Attribution key: the change's author ID when one is recorded
   * (post-transition changes), else the bare actor ID (pre-transition
   * history, which has no author anywhere). Never a raw actor when an
   * author is known. The field keeps the `actor` name per D6.
   */
  actor: string;
  time: number;
}

export interface AttributionRun {
  /** inclusive char offset */
  start: number;
  /** exclusive char offset */
  end: number;
  /** Attribution key (author ID, or actor ID for pre-transition history). */
  actor: string;
  time: number;
}

export interface RunListAttribution {
  runs: AttributionRun[];
  /**
   * Internal: forward-replay doc held at the last replayed heads, so each
   * incremental update replays only the changes the live doc has gained.
   * Absent when state is hand-constructed (tests), in which case
   * `updateRunListAttribution` throws `HistoryCompactedError` and
   * the caller (`useAttribution`) falls back to a full rebuild.
   */
  _workDoc?: Doc<unknown>;
  /**
   * Internal: actor → author index accumulated from seq-1 author footers
   * during replay (Phase 0 finding 1: seq>1 changes carry no footer).
   * Consulted for every change whose own footer is absent, so the
   * incremental path attributes later same-actor changes to the author
   * without rescanning history. Absent on hand-constructed state (tests)
   * — resolution then covers footers seen in the current batch and falls
   * back to bare actors for the rest.
   */
  _authorByActor?: Map<string, string>;
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

interface ViewableHandle {
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

/**
 * How long a cold build replays before it yields to the event loop. A yield
 * can only happen between batches, so one slow batch may overrun this.
 */
export const SLICE_BUDGET_MS = 8;

/**
 * Most changes applied in one batch. Bounds a slice, and keeps an abort
 * checkpoint, when a flood of changes shares one author and second.
 */
export const MAX_BATCH_SIZE = 500;

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

/** Replay state with every field present; the loop below mutates it. */
type ReplayState = Required<RunListAttribution>;

/**
 * The attribution key and time of one change.
 *
 * Author-first resolution (author-ID transition): the author footer rides
 * on the actor's seq-1 change only (Phase 0 finding 1), so `authorByActor`
 * is accumulated as replay proceeds; seq>1 changes resolve through it, and
 * pre-transition changes (no author anywhere) fall through to the bare
 * actor — which by D5 equals the author's value, keeping one user's key
 * continuous across the transition boundary.
 */
function resolveAttribution(change: Change, authorByActor: Map<string, string>): CharAttribution {
  const decoded = decodeChange(change);
  if (decoded.author) authorByActor.set(decoded.actor, decoded.author);
  return {
    actor: decoded.author ?? authorByActor.get(decoded.actor) ?? decoded.actor,
    time: decoded.time,
  };
}

/** Apply one batch to the work doc and fold its text patches into the runs. */
function applyBatch(
  state: ReplayState,
  batch: Change[],
  attribution: CharAttribution,
  textFieldName: string,
): void {
  let collected: Patch[] = [];
  const [next] = applyChanges(state._workDoc, batch, {
    patchCallback: (patches: Patch[]) => { collected = patches; },
  });
  state._workDoc = next;
  for (const patch of collected) {
    if (isTextPatch(patch, textFieldName)) applyPatchToRuns(state.runs, patch, attribution);
  }
}

/**
 * Replay every change the live doc has that `state._workDoc` lacks, and
 * re-check until none is left: when the generator finishes, the state is at
 * the handle's current heads, including edits that arrived while the caller
 * was paused. It yields between batches (never after the last), which is
 * where the cold build may pause; the incremental update just drains it.
 *
 * One `applyChanges` call costs time proportional to the text length however
 * many changes it carries. Consecutive changes with the same attribution
 * (Automerge stamps changes in whole seconds, so a typing burst shares one)
 * give every inserted character the same run either way, so they go in one
 * call.
 */
function* replayPending(
  state: ReplayState,
  handle: ViewableHandle,
  textFieldName: string,
): Generator<void, void, void> {
  for (;;) {
    let pending: Change[];
    try {
      pending = getChanges(state._workDoc, handle.doc() as Doc<unknown>);
    } catch {
      // `getChanges` throws if the work doc has changes the live doc lacks.
      // The work doc only ever advances by applying the live doc's changes,
      // so this needs hand-constructed state; report it as a rebuild signal.
      throw new HistoryCompactedError();
    }
    if (pending.length === 0) return;

    let batch: Change[] = [];
    let batchAttribution: CharAttribution | null = null;
    for (const change of pending) {
      const attribution = resolveAttribution(change, state._authorByActor);
      if (
        batchAttribution !== null &&
        (batch.length >= MAX_BATCH_SIZE ||
          attribution.actor !== batchAttribution.actor ||
          attribution.time !== batchAttribution.time)
      ) {
        applyBatch(state, batch, batchAttribution, textFieldName);
        batch = [];
        yield;
      }
      batch.push(change);
      batchAttribution = attribution;
    }
    if (batchAttribution !== null) applyBatch(state, batch, batchAttribution, textFieldName);
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
  if (!viewable.doc() || signal?.aborted) return null;

  // A cold build is the incremental replay started from an empty doc.
  const state: ReplayState = { runs: [], _workDoc: init(), _authorByActor: new Map() };
  const replay = replayPending(state, viewable, textFieldName);
  let sliceStart = performance.now();
  while (!replay.next().done) {
    if (performance.now() - sliceStart < SLICE_BUDGET_MS) continue;
    await waitForIdle();
    if (signal?.aborted) return null;
    sliceStart = performance.now();
  }
  return state;
}

// ---------------------------------------------------------------------------
// updateRunListAttribution — incremental (synchronous)
// ---------------------------------------------------------------------------

export function updateRunListAttribution(
  state: RunListAttribution,
  handle: DocHandle<unknown>,
  textFieldName: string,
): RunListAttribution {
  if (!state._workDoc) throw new HistoryCompactedError();

  const next: ReplayState = {
    runs: state.runs.map(r => ({ ...r })),
    _workDoc: state._workDoc,
    _authorByActor: state._authorByActor ?? new Map<string, string>(),
  };
  const replay = replayPending(next, handle as unknown as ViewableHandle, textFieldName);
  while (!replay.next().done);
  // Nothing was applied: keep the caller's state object.
  return next._workDoc === state._workDoc ? state : next;
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
