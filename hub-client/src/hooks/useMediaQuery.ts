import { useCallback, useSyncExternalStore } from 'react';

/**
 * Track a CSS media query reactively (re-renders on breakpoint cross).
 * Where matchMedia is unavailable (jsdom, SSR), reports false — the
 * wide-viewport/default behavior, matching the test-setup mock's
 * convention (test-utils/setup.ts stubs matchMedia with matches: false).
 */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onStoreChange: () => void) => {
      if (typeof window.matchMedia !== 'function') return () => {};
      const mql = window.matchMedia(query);
      mql.addEventListener('change', onStoreChange);
      return () => mql.removeEventListener('change', onStoreChange);
    },
    [query],
  );
  const getSnapshot = useCallback(
    () =>
      typeof window.matchMedia === 'function'
        ? window.matchMedia(query).matches
        : false,
    [query],
  );
  return useSyncExternalStore(subscribe, getSnapshot);
}
