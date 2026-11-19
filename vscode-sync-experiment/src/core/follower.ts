/**
 * The follower (GE-9, G20, G26): what a client with no repository of its own
 * does when the project owner flips `files[path]` to a different doc. It wraps
 * the shared swap coordinator (`@quarto/quarto-git-epochs`) around a small set
 * of operations `ProjectSync` supplies, folds the `done` events of one batch
 * into one notice, and counts edits that stay with the old doc.
 */
import type { DocHandle } from '@automerge/automerge-repo/slim';
import {
  NOTICE_UNAVAILABLE,
  NoticeAggregator,
  createSwapCoordinator,
  type ReplacedEvent,
  type SwapCoordinator,
} from '@quarto/quarto-git-epochs';
import type { FileDoc } from './create-folder';
import type { Notifier } from './host';

export interface Clock {
  setTimeout: typeof setTimeout;
  clearTimeout: typeof clearTimeout;
}

export interface FollowerOps {
  currentHandle(rel: string): DocHandle<FileDoc> | undefined;
  loadDoc(docId: string, signal: AbortSignal): Promise<DocHandle<FileDoc>>;
  /** Untrack the old handle, track the new one, rebind any open buffer, and (mirror only) write the disk. */
  rebind(rel: string, newHandle: DocHandle<FileDoc>, oldHandle: DocHandle<FileDoc> | undefined): Promise<void>;
  /** True when `rel` is open in an editor buffer (picks the "this file" wording of the notice). */
  isOpen(rel: string): boolean;
  /** A swap for `rel` finished (`done`); the editor-only gate uses it to end its wait for the flip. */
  onSwapped?(rel: string): void;
  notifier: Notifier;
  log(line: string): void;
  clock: Clock;
}

export interface FollowerOptions {
  /** How long a load may take before the path is reported unavailable (G32: 5000). */
  swapTimeoutMs?: number;
}

export class Follower {
  private readonly coordinator: SwapCoordinator<DocHandle<FileDoc>>;
  private readonly aggregator = new NoticeAggregator();
  private readonly batchPaths = new Set<string>();
  /** Swaps announced `pending` and not yet `done` or `unavailable`. */
  private outstanding = 0;
  private readonly pendingPaths = new Set<string>();

  constructor(
    private readonly ops: FollowerOps,
    options: FollowerOptions = {},
  ) {
    this.coordinator = createSwapCoordinator<DocHandle<FileDoc>>(
      {
        currentDocId: (rel) => ops.currentHandle(rel)?.documentId,
        currentHandle: (rel) => ops.currentHandle(rel),
        loadDoc: (docId, signal) => ops.loadDoc(docId, signal),
        rebind: (rel, _newDocId, newHandle, oldHandle) => ops.rebind(rel, newHandle, oldHandle),
        onReplaced: (event) => this.onReplaced(event),
        clock: ops.clock,
      },
      { timeoutMs: options.swapTimeoutMs },
    );
  }

  /** The index lists `rel` at `docId`. Safe to call for every entry on every index change. */
  indexChanged(rel: string, docId: string): void {
    this.coordinator.indexChanged(rel, docId);
  }

  /** True while a swap for `rel` is running. */
  isSwapping(rel: string): boolean {
    return this.pendingPaths.has(rel);
  }

  /**
   * An accepted local edit to the doc `baseDocId` that `rel`'s Binding is bound
   * to. While a swap is pending the edit stays in the old doc; the coordinator
   * counts it as stranded, and the `done` event carries the total. Where the
   * edit goes is not changed.
   */
  noteLocalEdit(rel: string, baseDocId: string): void {
    this.coordinator.resolveWriteTarget(rel, baseDocId);
  }

  dispose(): void {
    this.coordinator.dispose();
  }

  private onReplaced(event: ReplacedEvent): void {
    const { path } = event;
    if (event.phase === 'pending') {
      this.outstanding++;
      this.pendingPaths.add(path);
      return;
    }
    if (this.pendingPaths.delete(path)) this.outstanding--;
    if (event.phase === 'unavailable') {
      this.ops.log(`swap unavailable: ${path} -> ${event.newDocId}`);
      void this.ops.notifier.warn(NOTICE_UNAVAILABLE);
      if (this.outstanding === 0) this.flush();
      return;
    }
    // done
    this.aggregator.add(event);
    if (event.oldDocId !== event.newDocId) this.batchPaths.add(path);
    this.coordinator.takeStranded(path); // the event carried the count
    this.ops.onSwapped?.(path);
    if (this.outstanding === 0) this.flush();
  }

  /** One aggregate message for the batch. */
  private flush(): void {
    const only = this.batchPaths.size === 1 ? [...this.batchPaths][0]! : undefined;
    const openFile = only !== undefined && this.ops.isOpen(only) ? only : '\u0000none';
    const message = this.aggregator.flush(openFile);
    this.batchPaths.clear();
    if (message !== null) void this.ops.notifier.info(message);
  }
}
