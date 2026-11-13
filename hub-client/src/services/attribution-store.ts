/**
 * The run-list attribution of one document, kept outside React so a component
 * can read it during render (`useSyncExternalStore`) instead of from an
 * effect that sets state.
 *
 * `snapshot` answers at once whenever it can: a live edit, a forward replay
 * step and a replay step shown before are all settled by the read itself.
 * Only a cold build is asynchronous. A read never starts or aborts one (reads
 * happen during render); it records what is needed, and `reconcile`, called
 * after the render commits, acts on it.
 */

import type { DocHandle } from '@automerge/automerge-repo';
import { getFileHandle } from '@quarto/preview-runtime';

import {
  buildRunListAttribution,
  createReplayIndex,
  HistoryCompactedError,
  TargetNotAheadError,
  updateRunListAttribution,
} from './attribution-runs';
import type { ReplayTarget, RunListAttribution } from './attribution-runs';
import { getBranchDoc } from './branchService';

/** The attribution for the document asked for is being built. */
export const ATTRIBUTION_PENDING = 'pending';
/** There is no attribution to show: none built yet (live), or it failed. */
export const ATTRIBUTION_NONE = 'none';

export type AttributionSnapshot =
  | RunListAttribution
  | typeof ATTRIBUTION_PENDING
  | typeof ATTRIBUTION_NONE;

/** Replay steps whose runs are kept, so stepping back over them is immediate. */
const STEP_CACHE_SIZE = 128;

/** A build target: a replay step's joined change hashes, or null for the live heads. */
type TargetKey = string | null;

/** The doc whose history gives the attribution: a local branch's, or the synced one. */
function resolveHandle(path: string, branchId: string | null): DocHandle<unknown> | null {
  if (branchId === null) return getFileHandle(path) ?? null;
  if (!getBranchDoc(path, branchId)) return null;
  // Branch edits replace the doc object; read it afresh on each access.
  return { doc: () => getBranchDoc(path, branchId) } as unknown as DocHandle<unknown>;
}

export class AttributionStore {
  /** The latest runs, at the live heads or at the last replay step built. */
  private state: RunListAttribution | null = null;
  private building: { key: TargetKey; controller: AbortController } | null = null;
  /** What the last read needs: a build to this target, or (null) none running. */
  private wanted: { key: TargetKey } | null = null;
  /** Targets whose build failed; they are not retried. */
  private failed = new Set<TargetKey>();
  /** Replay: the live doc's changes by hash, and the runs of steps shown. */
  private index = createReplayIndex();
  private steps = new Map<string, RunListAttribution>();
  private generating = false;
  private listeners = new Set<() => void>();

  private readonly path: string;
  /** The local branch whose doc is read, or null for the synced doc. */
  private readonly branchId: string | null;

  constructor(path: string, branchId: string | null) {
    this.path = path;
    this.branchId = branchId;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** True while a cold build runs. */
  isGenerating = (): boolean => this.generating;

  /**
   * The attribution at a replay step (`headsKey`: its change hashes, joined
   * by commas) or, for null, at the document's live heads. The same object
   * is returned for as long as it stays valid.
   */
  snapshot(headsKey: string | null): AttributionSnapshot {
    const result = headsKey === null ? this.readLive() : this.readStep(headsKey);
    // Once answered, a build still running is for something no longer shown.
    this.wanted = result === ATTRIBUTION_PENDING ? { key: headsKey } : null;
    // Live, a pending build means "no overlay yet", not a held render.
    return headsKey === null && result === ATTRIBUTION_PENDING ? ATTRIBUTION_NONE : result;
  }

  /** Make the running cold build the one the last read needs: start, replace or drop it. */
  reconcile(): void {
    const wanted = this.wanted;
    if (wanted === null) {
      if (!this.building) return;
      this.abort();
      this.setGenerating(false);
    } else if (this.building?.key !== wanted.key && !this.failed.has(wanted.key)) {
      this.startBuild(wanted.key);
    }
  }

  /** Stop any build. The store can be read and reconciled again afterwards. */
  dispose(): void {
    this.abort();
    this.generating = false;
  }

  private readLive(): AttributionSnapshot {
    // Replay is over: free its history index and step cache.
    if (this.index.size > 0) this.index.clear();
    if (this.steps.size > 0) this.steps.clear();
    for (const key of this.failed) if (key !== null) this.failed.delete(key);

    if (!this.state) {
      if (this.failed.has(null)) return ATTRIBUTION_NONE;
      return ATTRIBUTION_PENDING;
    }
    let handle: DocHandle<unknown> | null;
    try {
      handle = resolveHandle(this.path, this.branchId);
    } catch {
      handle = null;
    }
    if (!handle) return ATTRIBUTION_NONE;
    try {
      // Fold in what the document has gained; a no-op keeps the same object.
      this.state = updateRunListAttribution(this.state, handle, 'text');
      return this.state;
    } catch (err) {
      if (err instanceof HistoryCompactedError) {
        this.state = null;
        return ATTRIBUTION_PENDING;
      }
      console.warn('[attribution] update failed:', err);
      return ATTRIBUTION_NONE;
    }
  }

  private readStep(key: string): AttributionSnapshot {
    const cached = this.steps.get(key);
    if (cached) {
      this.remember(key, cached);
      return cached;
    }
    if (this.failed.has(key)) return ATTRIBUTION_NONE;

    // A forward step extends the current state; anything else is a cold build.
    if (this.state) {
      try {
        const handle = getFileHandle(this.path);
        if (handle) {
          this.state = updateRunListAttribution(this.state, handle, 'text', this.target(key));
          this.remember(key, this.state);
          return this.state;
        }
      } catch (err) {
        if (!(err instanceof TargetNotAheadError || err instanceof HistoryCompactedError)) {
          console.warn('[attribution] replay update failed:', err);
        }
      }
    }
    return ATTRIBUTION_PENDING;
  }

  private target(key: string): ReplayTarget {
    return { heads: key.split(','), index: this.index };
  }

  private remember(key: string, state: RunListAttribution): void {
    // Re-inserting moves the step to the young end of the eviction order.
    this.steps.delete(key);
    this.steps.set(key, state);
    if (this.steps.size > STEP_CACHE_SIZE) this.steps.delete(this.steps.keys().next().value!);
  }

  private startBuild(key: TargetKey): void {
    this.abort();
    const controller = new AbortController();
    this.state = null;

    const fail = () => {
      this.building = null;
      this.failed.add(key);
      this.generating = false;
      this.notify();
    };

    let handle: DocHandle<unknown> | null;
    try {
      handle = resolveHandle(this.path, this.branchId);
    } catch {
      handle = null;
    }
    if (!handle) {
      fail();
      return;
    }

    this.building = { key, controller };
    this.setGenerating(true);
    buildRunListAttribution(
      handle,
      'text',
      controller.signal,
      key === null ? undefined : this.target(key),
    )
      .then(state => {
        // If aborted, a newer build (or none) owns the store.
        if (controller.signal.aborted || !state) return;
        this.building = null;
        this.state = state;
        if (key !== null) this.remember(key, state);
        this.generating = false;
        this.notify();
      })
      .catch(err => {
        if (controller.signal.aborted) return;
        console.warn('[attribution] build failed:', err);
        fail();
      });
  }

  private abort(): void {
    this.building?.controller.abort();
    this.building = null;
  }

  private setGenerating(value: boolean): void {
    if (this.generating === value) return;
    this.generating = value;
    this.notify();
  }

  private notify(): void {
    for (const listener of [...this.listeners]) listener();
  }
}
