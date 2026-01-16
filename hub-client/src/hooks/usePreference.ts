import { useState, useCallback } from 'react';
import type { PreferenceKey, UserPreferences } from '../services/preferences';
import { getPreference, setPreference } from '../services/preferences';

/**
 * React hook for reading and updating a user preference.
 * Returns a tuple like useState: [value, setValue]
 *
 * The value is initialized from localStorage and persisted on update.
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

  return [value, updateValue];
}
