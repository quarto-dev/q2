/**
 * The swap coordinator (G26): what a follower does when the project owner flips `files[path]` to a different doc.
 * It is a pure state machine with injected I/O. It imports nothing environment-specific and uses no global timer;
 * the only clock is the injected one.
 */
import {
  formatNotice,
  REPLACED_MANY_FILES,
  REPLACED_ONE_FILE,
  REPLACED_THIS_FILE,
  STRANDED_EDITS,
} from './notices.js';

export type SwapPhase = 'pending' | 'done' | 'unavailable';

export interface ReplacedEvent {
  path: string;
  phase: SwapPhase;
  oldDocId: string;
  newDocId: string;
  /** On `done`: writes that stayed with the old doc so far (cumulative, not reset by the event). */
  stranded?: number;
}

/** Everything environment-specific is injected. `H` is the host's handle type. */
export interface SwapHost<H> {
  currentDocId(path: string): string | undefined;
  /** The handle currently bound to the path, if any. It becomes the retired handle when a swap completes. */
  currentHandle(path: string): H | undefined;
  /** Rejects if the doc is unavailable. Must stop when `signal` aborts. */
  loadDoc(docId: string, signal: AbortSignal): Promise<H>;
  rebind(path: string, newDocId: string, newHandle: H, oldHandle?: H): void | Promise<void>;
  /**
   * Optional. Edits this peer made to the old doc that the server has not confirmed yet (typed offline, for
   * example); counted as stranded when a swap starts.
   */
  unconfirmedLocalEdits?(path: string, oldHandle: H): number;
  onReplaced(event: ReplacedEvent): void;
  /** The only source of timers; pass `{ setTimeout, clearTimeout }` from the host environment. */
  clock: { setTimeout: typeof setTimeout; clearTimeout: typeof clearTimeout };
}

export interface SwapOptions {
  /** How long a load may take before the path is reported `unavailable`. Default 5000. */
  timeoutMs?: number;
  /** Retired handles kept per path. Default 8. */
  retiredCap?: number;
}

export type WriteTarget<H> =
  | { kind: 'live'; docId: string; handle: H }
  /** The write stays with a previous version of the doc; counted as stranded. */
  | { kind: 'retired'; docId: string; handle: H }
  | { kind: 'drop'; reason: string };

export interface SwapCoordinator<H> {
  /** The index now says `path` is `docId`: start a swap, queue behind a running one, or coalesce. */
  indexChanged(path: string, docId: string): void;
  resolveWriteTarget(path: string, baseDocId?: string): WriteTarget<H>;
  retired(path: string): readonly { docId: string; handle: H }[];
  /** Returns the stranded count since the last call and resets it. */
  takeStranded(path: string): number;
  dispose(): void;
}

interface Retired<H> {
  docId: string;
  handle: H;
}

interface PathState<H> {
  retired: Retired<H>[];
  stranded: number;
  /** Incremented for every load attempt; a stale attempt's result is ignored. */
  gen: number;
  inFlight?: {
    targetDocId: string;
    oldDocId: string;
    oldHandle: H | undefined;
    controller: AbortController;
    timer: unknown;
  };
  rebinding: boolean;
  queued?: string;
}

const DEFAULT_TIMEOUT_MS = 5000;
const DEFAULT_RETIRED_CAP = 8;

export function createSwapCoordinator<H>(
  host: SwapHost<H>,
  opts: SwapOptions = {},
): SwapCoordinator<H> {
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const retiredCap = opts.retiredCap ?? DEFAULT_RETIRED_CAP;
  const clock = host.clock;
  const states = new Map<string, PathState<H>>();
  let disposed = false;

  function state(path: string): PathState<H> {
    let s = states.get(path);
    if (!s) {
      s = { retired: [], stranded: 0, gen: 0, rebinding: false };
      states.set(path, s);
    }
    return s;
  }

  function cancelInFlight(s: PathState<H>): void {
    const f = s.inFlight;
    if (!f) return;
    clock.clearTimeout(f.timer as ReturnType<typeof setTimeout>);
    f.controller.abort();
    s.inFlight = undefined;
  }

  function retire(s: PathState<H>, entry: Retired<H>): void {
    s.retired = s.retired.filter((r) => r.docId !== entry.docId);
    s.retired.push(entry);
    // The oldest handle is forgotten; nothing is deleted from any repo.
    while (s.retired.length > retiredCap) s.retired.shift();
  }

  function startLoad(path: string, s: PathState<H>, targetDocId: string): void {
    const f = s.inFlight!;
    const gen = ++s.gen;
    f.targetDocId = targetDocId;
    f.controller = new AbortController();
    const { signal } = f.controller;
    f.timer = clock.setTimeout(() => {
      if (disposed || s.gen !== gen || !s.inFlight) return;
      cancelInFlight(s);
      host.onReplaced({ path, phase: 'unavailable', oldDocId: f.oldDocId, newDocId: targetDocId });
    }, timeoutMs);

    const reused = s.retired.find((r) => r.docId === targetDocId);
    const loaded: Promise<H> = reused ? Promise.resolve(reused.handle) : host.loadDoc(targetDocId, signal);
    loaded.then(
      (handle) => {
        if (disposed || s.gen !== gen || !s.inFlight) return;
        void complete(path, s, targetDocId, handle);
      },
      () => {
        if (disposed || s.gen !== gen || !s.inFlight) return;
        cancelInFlight(s);
        host.onReplaced({ path, phase: 'unavailable', oldDocId: f.oldDocId, newDocId: targetDocId });
      },
    );
  }

  async function complete(path: string, s: PathState<H>, newDocId: string, handle: H): Promise<void> {
    const f = s.inFlight!;
    clock.clearTimeout(f.timer as ReturnType<typeof setTimeout>);
    s.inFlight = undefined;
    // Bookkeeping first, so writes that arrive while an async rebind runs already resolve against the new state.
    s.retired = s.retired.filter((r) => r.docId !== newDocId);
    if (f.oldHandle !== undefined) retire(s, { docId: f.oldDocId, handle: f.oldHandle });
    s.rebinding = true;
    try {
      await host.rebind(path, newDocId, handle, f.oldHandle);
    } finally {
      s.rebinding = false;
    }
    if (disposed) return;
    host.onReplaced({
      path,
      phase: 'done',
      oldDocId: f.oldDocId,
      newDocId,
      stranded: s.stranded,
    });
    const queued = s.queued;
    s.queued = undefined;
    if (queued !== undefined) indexChanged(path, queued);
  }

  function indexChanged(path: string, docId: string): void {
    if (disposed) return;
    const s = state(path);
    if (s.rebinding) {
      s.queued = docId; // latest state wins
      return;
    }
    const current = host.currentDocId(path);
    const f = s.inFlight;
    if (f) {
      if (f.targetDocId === docId) return;
      if (docId === f.oldDocId) {
        // Flipped back to what is still bound: abandon the swap and tell the host to unfreeze.
        cancelInFlight(s);
        s.gen++;
        host.onReplaced({
          path,
          phase: 'done',
          oldDocId: f.oldDocId,
          newDocId: f.oldDocId,
          stranded: s.stranded,
        });
        return;
      }
      // Coalesce: abort the load in progress and go straight to the final docId. No second `pending` event.
      clock.clearTimeout(f.timer as ReturnType<typeof setTimeout>);
      f.controller.abort();
      startLoad(path, s, docId);
      return;
    }
    if (current === undefined || current === docId) return;
    const oldHandle = host.currentHandle(path);
    if (oldHandle !== undefined && host.unconfirmedLocalEdits) {
      s.stranded += host.unconfirmedLocalEdits(path, oldHandle);
    }
    s.inFlight = {
      targetDocId: docId,
      oldDocId: current,
      oldHandle,
      controller: new AbortController(),
      timer: undefined,
    };
    host.onReplaced({ path, phase: 'pending', oldDocId: current, newDocId: docId });
    startLoad(path, s, docId);
  }

  return {
    indexChanged,

    resolveWriteTarget(path, baseDocId) {
      const s = state(path);
      const current = host.currentDocId(path);
      const handle = host.currentHandle(path);
      if (baseDocId === undefined) {
        if (current === undefined || handle === undefined) {
          return { kind: 'drop', reason: `no doc is bound to ${path}` };
        }
        return { kind: 'live', docId: current, handle };
      }
      if (s.inFlight && baseDocId === s.inFlight.oldDocId) {
        // Still bound, but about to be retired: the write stays with the old doc.
        if (handle === undefined) return { kind: 'drop', reason: `no handle for ${baseDocId}` };
        s.stranded++;
        return { kind: 'retired', docId: baseDocId, handle };
      }
      if (baseDocId === current) {
        if (handle === undefined) return { kind: 'drop', reason: `no handle for ${baseDocId}` };
        return { kind: 'live', docId: baseDocId, handle };
      }
      const r = s.retired.find((x) => x.docId === baseDocId);
      if (r) {
        s.stranded++;
        return { kind: 'retired', docId: r.docId, handle: r.handle };
      }
      return { kind: 'drop', reason: `unknown doc ${baseDocId} for ${path}` };
    },

    retired(path) {
      return [...state(path).retired];
    },

    takeStranded(path) {
      const s = state(path);
      const n = s.stranded;
      s.stranded = 0;
      return n;
    },

    dispose() {
      disposed = true;
      for (const s of states.values()) {
        cancelInFlight(s);
        s.queued = undefined;
      }
    },
  };
}

/** Folds the `done` events of one batch into a single notice. `unavailable` and `pending` events are ignored. */
export class NoticeAggregator {
  private paths = new Set<string>();
  private strandedByPath = new Map<string, number>();
  private extraStranded = 0;

  add(event: ReplacedEvent): void {
    if (event.phase !== 'done' || event.oldDocId === event.newDocId) return;
    this.paths.add(event.path);
    const prev = this.strandedByPath.get(event.path) ?? 0;
    this.strandedByPath.set(event.path, Math.max(prev, event.stranded ?? 0));
  }

  /** Stranded edits not carried by a `done` event (writes counted after it). */
  addStranded(n: number): void {
    this.extraStranded += n;
  }

  /** One aggregate per batch; null when there is nothing to say. Resets the batch. */
  flush(openFile?: string): string | null {
    const stranded =
      [...this.strandedByPath.values()].reduce((a, b) => a + b, 0) + this.extraStranded;
    const paths = [...this.paths];
    this.paths.clear();
    this.strandedByPath.clear();
    this.extraStranded = 0;

    const parts: string[] = [];
    if (paths.length === 1) {
      const only = paths[0];
      parts.push(
        openFile === undefined || openFile === only
          ? REPLACED_THIS_FILE
          : formatNotice(REPLACED_ONE_FILE, { path: only! }),
      );
    } else if (paths.length > 1) {
      parts.push(formatNotice(REPLACED_MANY_FILES, { n: paths.length }));
    }
    if (stranded > 0) parts.push(formatNotice(STRANDED_EDITS, { n: stranded }));
    return parts.length === 0 ? null : parts.join(' ');
  }
}
