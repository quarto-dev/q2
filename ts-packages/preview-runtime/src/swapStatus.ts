/**
 * Swap status (GE-7, G26): what a UI needs to know while the project owner replaces files.
 *
 * The sync client reports `pending`, `done`, and `unavailable` per path. This store folds those into two things:
 *
 * - `pending`: the paths whose editor must be frozen right now;
 * - `notice`: one aggregate sentence per batch of `done` events (the wording is GE-6's `NoticeAggregator`, the only
 *   source of it), published after a quiet window so a burst of swaps reads as one notice.
 *
 * It has no framework dependency. A React component reads it with `useSyncExternalStore(store.subscribe,
 * store.getSnapshot)`; `getSnapshot` returns the same object until something changes.
 */
import { NoticeAggregator, type ReplacedEvent } from '@quarto/quarto-git-epochs';

export interface SwapStatusSnapshot {
  /** Paths with a swap in flight, in the order they started. */
  readonly pending: readonly string[];
  /** The latest aggregate notice, or null when there is nothing to say (or it was dismissed). */
  readonly notice: string | null;
  /** Counts the notices published so far; a UI keys on it so an identical notice is announced again. */
  readonly noticeSeq: number;
}

export interface SwapStatusStore {
  /** Feed one event from the sync client's `onFileReplaced`. */
  handle(event: ReplacedEvent): void;
  /** The file the user has open; a batch about only that file says "This file". */
  setOpenFile(path: string | undefined): void;
  /** Add a line to the notice of the batch in progress (for example the branches that stay). */
  appendToNextNotice(line: string): void;
  dismissNotice(): void;
  /** Forget everything: on disconnect, so one project's state never shows in the next. */
  reset(): void;
  getSnapshot(): SwapStatusSnapshot;
  subscribe(listener: () => void): () => void;
}

export interface SwapStatusOptions {
  /** How long after the last `done` event the batch's notice is published. Default 150 ms. */
  quietMs?: number;
  clock?: { setTimeout: typeof setTimeout; clearTimeout: typeof clearTimeout };
}

const DEFAULT_QUIET_MS = 150;

export function createSwapStatusStore(opts: SwapStatusOptions = {}): SwapStatusStore {
  const quietMs = opts.quietMs ?? DEFAULT_QUIET_MS;
  const clock = opts.clock ?? { setTimeout, clearTimeout };

  let aggregator = new NoticeAggregator();
  let extraLines: string[] = [];
  let openFile: string | undefined;
  let pending: string[] = [];
  let notice: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let noticeSeq = 0;
  let snapshot: SwapStatusSnapshot = { pending, notice, noticeSeq };
  const listeners = new Set<() => void>();

  function publish(): void {
    snapshot = { pending: [...pending], notice, noticeSeq };
    for (const l of [...listeners]) l();
  }

  function cancelTimer(): void {
    if (timer !== undefined) {
      clock.clearTimeout(timer);
      timer = undefined;
    }
  }

  function flush(): void {
    timer = undefined;
    const text = aggregator.flush(openFile);
    const extras = extraLines;
    extraLines = [];
    if (text === null) return;
    notice = [text, ...extras].join(' ');
    noticeSeq++;
    publish();
  }

  return {
    handle(event) {
      if (event.phase === 'pending') {
        if (!pending.includes(event.path)) pending = [...pending, event.path];
        publish();
        return;
      }
      pending = pending.filter((p) => p !== event.path);
      if (event.phase === 'done') {
        aggregator.add(event);
        cancelTimer();
        timer = clock.setTimeout(flush, quietMs);
      }
      publish();
    },

    setOpenFile(path) {
      openFile = path;
    },

    appendToNextNotice(line) {
      extraLines.push(line);
    },

    dismissNotice() {
      if (notice === null) return;
      notice = null;
      publish();
    },

    reset() {
      cancelTimer();
      aggregator = new NoticeAggregator();
      extraLines = [];
      pending = [];
      notice = null;
      publish();
    },

    getSnapshot() {
      return snapshot;
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The runtime's one store. `automergeSync` feeds it; UIs read it. */
export const swapStatus: SwapStatusStore = createSwapStatusStore();
