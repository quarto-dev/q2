/**
 * pwaPrompt — the bridge between the DOM-free service-worker update
 * flow in `pwa.ts` and the `UpdateAvailableToast` React component
 * (GH #447, bd-axqunnx9).
 *
 * `setupSwUpdates` runs at module evaluation, before React mounts, so
 * the store keeps a pending flag: a `show()` fired before the toast
 * subscribes is not lost — the component reads `isPending()` as its
 * `useSyncExternalStore` snapshot and appears on mount.
 *
 * Dismissal is deliberately *not* store state: dismissing only hides
 * the toast (component-local), while the pending flag keeps its
 * meaning — an update is still waiting, and the hide-reload listener
 * in `pwa.ts` still reloads the tab when it is next backgrounded.
 */

export interface PwaPromptStore {
  /** Mark an update prompt pending and notify subscribers. Idempotent. */
  show(): void;
  /** Subscribe to show() notifications; returns an unsubscribe function. */
  subscribe(listener: () => void): () => void;
  /** Whether an update prompt is pending (useSyncExternalStore snapshot). */
  isPending(): boolean;
}

export function createPwaPromptStore(): PwaPromptStore {
  let pending = false;
  const listeners = new Set<() => void>();
  return {
    show() {
      if (pending) return;
      pending = true;
      for (const listener of listeners) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    isPending() {
      return pending;
    },
  };
}

/** The app-wide instance, wired to `setupSwUpdates` in `main.tsx`. */
export const pwaPrompt = createPwaPromptStore();
