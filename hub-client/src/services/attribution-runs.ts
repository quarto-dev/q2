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
  diff,
  getChanges,
  getChangesMetaSince,
  getChangesSince,
  getHeads,
  hasHeads,
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
  /**
   * Internal: the heads the runs describe, when the work doc is behind them.
   * A live update whose changes share one attribution reads their patches
   * from the live doc and does not apply them to the work doc (which costs
   * time proportional to the document). Absent when the two are level.
   */
  _heads?: string[];
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

/**
 * The replay target is not ahead of the work doc: the work doc holds a change
 * the target lacks (a backward seek, or a step onto a concurrent branch). The
 * work doc only moves forward, so the caller must rebuild from an empty doc.
 */
export class TargetNotAheadError extends Error {
  constructor() {
    super('Replay target is not ahead of the work doc — full rebuild required');
    this.name = 'TargetNotAheadError';
  }
}

interface IndexedChange {
  change: Change;
  /** Attribution inputs; `author` is null on an actor's seq>1 changes. */
  actor: string;
  author: string | null;
  time: number;
  deps: string[];
  /** Position in the live doc's topological order. */
  order: number;
}

/** Every change of the live doc by hash, read once per replay. */
export type ReplayIndex = Map<string, IndexedChange>;

export function createReplayIndex(): ReplayIndex {
  return new Map();
}

/**
 * A point in history to build attribution for: the document made of `heads`
 * and their ancestors (a replay step is one change hash). `index` is filled
 * on first use and refilled when a head is missing from it; share one across
 * the steps of a replay and drop it when the replay ends.
 */
export interface ReplayTarget {
  heads: string[];
  index: ReplayIndex;
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
type ReplayState = Required<Omit<RunListAttribution, '_heads'>> & Pick<RunListAttribution, '_heads'>;

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
 * Apply `changes` in order and yield between batches (never after the last).
 *
 * One `applyChanges` call costs time proportional to the text length however
 * many changes it carries. Consecutive changes with the same attribution
 * (Automerge stamps changes in whole seconds, so a typing burst shares one)
 * give every inserted character the same run either way, so they go in one
 * call.
 */
function* applyInBatches(
  state: ReplayState,
  changes: Iterable<[Change, CharAttribution]>,
  textFieldName: string,
): Generator<void, void, void> {
  let batch: Change[] = [];
  let batchAttribution: CharAttribution | null = null;
  for (const [change, attribution] of changes) {
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

/** Resolve each change's attribution as it is reached, in replay order. */
function* withAttribution(
  changes: Change[],
  authorByActor: Map<string, string>,
): Generator<[Change, CharAttribution]> {
  for (const change of changes) yield [change, resolveAttribution(change, authorByActor)];
}

/**
 * Replay every change the live doc has that `state._workDoc` lacks, and
 * re-check until none is left: when the generator finishes, the state is at
 * the handle's current heads, including edits that arrived while the caller
 * was paused. It yields between batches (never after the last), which is
 * where the cold build may pause; the incremental update just drains it.
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
    yield* applyInBatches(state, withAttribution(pending, state._authorByActor), textFieldName);
  }
}

/** Fill `index` from the live doc unless it already has every target head. */
function ensureIndexed(target: ReplayTarget, live: Doc<unknown>): void {
  const { heads, index } = target;
  if (heads.every(h => index.has(h))) return;
  const metas = getChangesMetaSince(live, []);
  const changes = getChangesSince(live, []);
  // Both lists come from the same traversal; position pairs bytes with hash.
  if (metas.length !== changes.length) throw new Error('change metadata and changes disagree');
  index.clear();
  metas.forEach((meta, order) => {
    index.set(meta.hash, {
      change: changes[order],
      actor: meta.actor,
      author: meta.author,
      time: meta.time,
      deps: meta.deps,
      order,
    });
  });
}

/**
 * The changes that take the work doc to `target`, in topological order: the
 * target heads' ancestors the work doc lacks. Throws `TargetNotAheadError`
 * when the work doc has a change that the target does not.
 */
function changesUpTo(state: ReplayState, live: Doc<unknown>, target: ReplayTarget): IndexedChange[] {
  ensureIndexed(target, live);
  const needed: IndexedChange[] = [];
  const reachedApplied = new Set<string>();
  const seen = new Set<string>();
  const stack = [...target.heads];
  while (stack.length > 0) {
    const hash = stack.pop()!;
    if (seen.has(hash)) continue;
    seen.add(hash);
    // The work doc is closed under ancestors, so the walk can stop here.
    if (hasHeads(state._workDoc, [hash])) {
      reachedApplied.add(hash);
      continue;
    }
    const entry = target.index.get(hash);
    if (!entry) throw new Error(`replay target change ${hash} is not in the document`);
    needed.push(entry);
    stack.push(...entry.deps);
  }
  // A work-doc head that the walk did not reach is not an ancestor of the
  // target (a path down to a head crosses no other applied change).
  if (getHeads(state._workDoc).some(h => !reachedApplied.has(h))) throw new TargetNotAheadError();
  return needed.sort((a, b) => a.order - b.order);
}

/** Replay the work doc forward to `target`; the yields are as in `replayPending`. */
function* replayUpTo(
  state: ReplayState,
  handle: ViewableHandle,
  textFieldName: string,
  target: ReplayTarget,
): Generator<void, void, void> {
  const needed = changesUpTo(state, handle.doc() as Doc<unknown>, target);
  const authorByActor = state._authorByActor;
  function* attributed(): Generator<[Change, CharAttribution]> {
    for (const c of needed) {
      // Same author-first rule as `resolveAttribution`; an actor's seq-1
      // change is an ancestor of its later ones, so it is seen first.
      if (c.author) authorByActor.set(c.actor, c.author);
      yield [c.change, { actor: c.author ?? authorByActor.get(c.actor) ?? c.actor, time: c.time }];
    }
  }
  yield* applyInBatches(state, attributed(), textFieldName);
}

/**
 * Live update without the work doc: when every change the live doc has gained
 * carries one attribution, fold the doc's own patches for them into a copy of
 * the runs. Automerge keeps the patches of a doc's latest change, so for an
 * edit this costs nothing that grows with the document. Returns null when the
 * changes differ in attribution; they must then be replayed one batch each.
 */
function updateFromDocPatches(
  state: RunListAttribution & { _workDoc: Doc<unknown> },
  live: Doc<unknown>,
  textFieldName: string,
): RunListAttribution | null {
  const at = state._heads ?? getHeads(state._workDoc);
  // The runs describe changes this doc lacks (it is another history).
  if (!hasHeads(live, at)) throw new HistoryCompactedError();
  const pending = getChangesMetaSince(live, at);
  if (pending.length === 0) return state;

  const authorByActor = state._authorByActor ?? new Map<string, string>();
  let attribution: CharAttribution | null = null;
  for (const c of pending) {
    // Same author-first rule as `resolveAttribution`.
    if (c.author) authorByActor.set(c.actor, c.author);
    const actor = c.author ?? authorByActor.get(c.actor) ?? c.actor;
    if (attribution === null) attribution = { actor, time: c.time };
    else if (actor !== attribution.actor || c.time !== attribution.time) return null;
  }

  const heads = getHeads(live);
  const runs = state.runs.map(r => ({ ...r }));
  for (const patch of diff(live, at, heads)) {
    if (isTextPatch(patch, textFieldName)) applyPatchToRuns(runs, patch, attribution!);
  }
  return { runs, _workDoc: state._workDoc, _authorByActor: authorByActor, _heads: heads };
}

/** Bring a work doc left behind by `updateFromDocPatches` level with the runs. */
function catchUpWorkDoc(state: ReplayState, live: Doc<unknown>): void {
  const at = state._heads;
  if (!at) return;
  if (!hasHeads(live, at)) throw new HistoryCompactedError();
  const workHeads = getHeads(state._workDoc);
  const metas = getChangesMetaSince(live, workHeads);
  const changes = getChangesSince(live, workHeads);
  // Both lists come from the same traversal; position pairs bytes with hash.
  if (metas.length !== changes.length) throw new Error('change metadata and changes disagree');
  const beyond = new Set(getChangesMetaSince(live, at).map(m => m.hash));
  const behind = changes.filter((_, i) => !beyond.has(metas[i].hash));
  [state._workDoc] = applyChanges(state._workDoc, behind);
  state._heads = undefined;
}

// ---------------------------------------------------------------------------
// buildRunListAttribution — full history processing
// ---------------------------------------------------------------------------

export async function buildRunListAttribution(
  handle: DocHandle<unknown>,
  textFieldName: string,
  signal?: AbortSignal,
  target?: ReplayTarget,
): Promise<RunListAttribution | null> {
  const viewable = handle as unknown as ViewableHandle;
  if (!viewable.doc() || signal?.aborted) return null;

  // A cold build is the incremental replay started from an empty doc.
  const state: ReplayState = {
    runs: [],
    _workDoc: init(),
    _authorByActor: new Map(),
    _heads: undefined,
  };
  // With a `target` the build stops at that point in history instead of
  // the live heads.
  const replay = target
    ? replayUpTo(state, viewable, textFieldName, target)
    : replayPending(state, viewable, textFieldName);
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
  target?: ReplayTarget,
): RunListAttribution {
  const workDoc = state._workDoc;
  if (!workDoc) throw new HistoryCompactedError();
  const viewable = handle as unknown as ViewableHandle;
  const live = viewable.doc() as Doc<unknown>;

  if (!target) {
    const folded = updateFromDocPatches({ ...state, _workDoc: workDoc }, live, textFieldName);
    if (folded) return folded.runs === state.runs ? state : folded;
  }

  const next: ReplayState = {
    runs: state.runs.map(r => ({ ...r })),
    _workDoc: workDoc,
    _authorByActor: state._authorByActor ?? new Map<string, string>(),
    _heads: state._heads,
  };
  catchUpWorkDoc(next, live);
  const replay = target
    ? replayUpTo(next, viewable, textFieldName, target)
    : replayPending(next, viewable, textFieldName);
  while (!replay.next().done);
  // Nothing was applied: keep the caller's state object.
  return next._workDoc === workDoc ? state : next;
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
