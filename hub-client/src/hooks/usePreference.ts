import { useState, useCallback, useEffect } from 'react';
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
 * The value is initialized from localStorage and persisted on update.
 * Every `usePreference(key)` instance in the window stays in sync —
 * `setPreference` dispatches a same-window `PREFERENCE_CHANGE_EVENT`
 * and each instance re-reads when it matches `key`. This lets a
 * toggle in one component (e.g. SettingsTab) take effect immediately
 * in a sibling component (e.g. ReactPreview) without a page refresh.
 *
 * @param key - The preference key to read/write
 * @returns [currentValue, updateFunction]
 */
export function usePreference<K extends PreferenceKey>(
  key: K
): [UserPreferences[K], (value: UserPreferences[K]) => void] {
  const [value, setValue] = useState(() => getPreference(key));

  const updateValue = useCallback(
    (newValue: UserPreferences[K]) => {
      setPreference(key, newValue);
      setValue(newValue);
    },
    [key]
  );

  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<PreferenceChangeDetail>).detail;
      if (detail?.key === key) {
        setValue(getPreference(key));
      }
    };
    window.addEventListener(PREFERENCE_CHANGE_EVENT, onChange);
    return () => window.removeEventListener(PREFERENCE_CHANGE_EVENT, onChange);
  }, [key]);

  return [value, updateValue];
}
