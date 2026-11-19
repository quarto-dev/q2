/**
 * A small bus for the sync client's swap events (GE-7), so each part of the app that holds a doc handle can react to
 * `done` on its own terms (presence rebinds, a replay session closes, branches reset) without `App` knowing them all.
 *
 * `App` forwards the runtime's `onFileReplaced` here; consumers subscribe.
 */
import type { ReplacedEvent } from '@quarto/quarto-git-epochs';

type Listener = (event: ReplacedEvent) => void;

const listeners = new Set<Listener>();

/** Deliver one swap event to every subscriber. A throwing subscriber does not stop the others. */
export function emitFileReplaced(event: ReplacedEvent): void {
  for (const listener of [...listeners]) {
    try {
      listener(event);
    } catch (err) {
      console.error('[swapEvents] subscriber threw:', err);
    }
  }
}

/** Subscribe to swap events. Returns an unsubscribe function. */
export function subscribeFileReplaced(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * True for a completed swap that really changed the doc. The sync client also sends `done` with equal ids when a flip
 * back to the still-bound doc abandons a pending swap; nothing was replaced then.
 */
export function isCompletedSwap(event: ReplacedEvent): boolean {
  return event.phase === 'done' && event.oldDocId !== event.newDocId;
}
