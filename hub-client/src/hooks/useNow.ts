import { useCallback, useSyncExternalStore } from 'react';

/**
 * useNow(intervalMs) — a shared ticking clock on useSyncExternalStore.
 *
 * One module-level store per distinct interval: the interval runs only
 * while at least one component is subscribed, and every consumer of the
 * same interval reads the same cached timestamp (a `getSnapshot` must be
 * referentially stable between ticks, so `Date.now()` cannot be called
 * in the snapshot itself).
 *
 * The cached timestamp is refreshed on (re)subscription, so a component
 * mounting after an idle spell never renders against a stale clock for
 * up to one interval.
 */

interface ClockStore {
  current: number;
  listeners: Set<() => void>;
  intervalId: ReturnType<typeof setInterval> | null;
}

const stores = new Map<number, ClockStore>();

function getStore(intervalMs: number): ClockStore {
  let store = stores.get(intervalMs);
  if (!store) {
    store = { current: Date.now(), listeners: new Set(), intervalId: null };
    stores.set(intervalMs, store);
  }
  return store;
}

function subscribe(intervalMs: number, onStoreChange: () => void): () => void {
  const store = getStore(intervalMs);
  store.listeners.add(onStoreChange);
  if (store.intervalId === null) {
    // Restarting after an idle spell: refresh so the next read is not stale.
    store.current = Date.now();
    store.intervalId = setInterval(() => {
      store.current = Date.now();
      for (const listener of store.listeners) listener();
    }, intervalMs);
  }
  return () => {
    store.listeners.delete(onStoreChange);
    if (store.listeners.size === 0 && store.intervalId !== null) {
      clearInterval(store.intervalId);
      store.intervalId = null;
    }
  };
}

export function useNow(intervalMs: number): number {
  const subscribeFn = useCallback(
    (onStoreChange: () => void) => subscribe(intervalMs, onStoreChange),
    [intervalMs],
  );
  const getSnapshot = useCallback(() => getStore(intervalMs).current, [intervalMs]);
  return useSyncExternalStore(subscribeFn, getSnapshot);
}
