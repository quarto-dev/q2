import { useSyncExternalStore } from 'react';

/**
 * useOnlineStatus() — the browser's `navigator.onLine` as a proper
 * subscription (online/offline window events), on useSyncExternalStore.
 * Reading `navigator.onLine` during render without this only re-checks
 * when something else happens to re-render the component.
 */

function subscribe(onStoreChange: () => void): () => void {
  window.addEventListener('online', onStoreChange);
  window.addEventListener('offline', onStoreChange);
  return () => {
    window.removeEventListener('online', onStoreChange);
    window.removeEventListener('offline', onStoreChange);
  };
}

function getSnapshot(): boolean {
  return navigator.onLine;
}

export function useOnlineStatus(): boolean {
  return useSyncExternalStore(subscribe, getSnapshot);
}
