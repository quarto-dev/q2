import { useCallback, useSyncExternalStore } from 'react';
import type {
  PreferenceChangeDetail,
  PreferenceKey,
  UserPreferences,
} from '../services/preferences';
import {
  PREFERENCE_CHANGE_EVENT,
  getPreference,
  setPreference,
} from '../services/preferences';

/**
 * React hook for reading and updating a user preference.
 * Returns a tuple like useState: [value, setValue]
 *
 * The value is read from localStorage and persisted on update.
 * Every `usePreference(key)` instance in the window stays in sync —
 * `setPreference` dispatches a same-window `PREFERENCE_CHANGE_EVENT`
 * and each instance re-reads when it matches `key`. This lets a
 * toggle in one component (e.g. SettingsTab) take effect immediately
 * in a sibling component (e.g. ReactPreview) without a page refresh.
 *
 * Backed by useSyncExternalStore: preference values are primitives
 * (booleans / string enums), so `getPreference(key)` is a stable
 * snapshot by value. The writer needs no local echo — `setPreference`
 * dispatches the change event synchronously, which notifies the
 * writer's own subscription too.
 *
 * @param key - The preference key to read/write
 * @returns [currentValue, updateFunction]
 */
export function usePreference<K extends PreferenceKey>(
  key: K
): [UserPreferences[K], (value: UserPreferences[K]) => void] {
  const subscribe = useCallback(
    (onStoreChange: () => void) => {
      const onChange = (event: Event) => {
        const detail = (event as CustomEvent<PreferenceChangeDetail>).detail;
        if (detail?.key === key) onStoreChange();
      };
      window.addEventListener(PREFERENCE_CHANGE_EVENT, onChange);
      return () => window.removeEventListener(PREFERENCE_CHANGE_EVENT, onChange);
    },
    [key]
  );
  const getSnapshot = useCallback(() => getPreference(key), [key]);
  const value = useSyncExternalStore(subscribe, getSnapshot);

  const updateValue = useCallback(
    (newValue: UserPreferences[K]) => {
      setPreference(key, newValue);
    },
    [key]
  );

  return [value, updateValue];
}
