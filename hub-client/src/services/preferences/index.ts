import type { UserPreferences, PreferenceKey } from './schema';
import { DEFAULT_PREFERENCES, validatePreferences } from './schema';

export type { UserPreferences, PreferenceKey };
export { DEFAULT_PREFERENCES };

const STORAGE_KEY = 'quarto-hub:preferences';

/**
 * Get all user preferences from localStorage.
 * Returns defaults if storage is empty or invalid.
 */
export function getPreferences(): UserPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_PREFERENCES;
    const parsed = JSON.parse(raw);
    return validatePreferences(parsed);
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

/**
 * Get a single preference value.
 */
export function getPreference<K extends keyof UserPreferences>(
  key: K
): UserPreferences[K] {
  return getPreferences()[key];
}

/**
 * Update a single preference value.
 * Cannot update the version field.
 */
export function setPreference<K extends PreferenceKey>(
  key: K,
  value: UserPreferences[K]
): void {
  const current = getPreferences();
  const updated = { ...current, [key]: value };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}
