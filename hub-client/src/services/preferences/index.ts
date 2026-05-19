import type { UserPreferences, PreferenceKey, ColorScheme } from './schema';
import { DEFAULT_PREFERENCES, validatePreferences } from './schema';

export type { UserPreferences, PreferenceKey, ColorScheme };
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
 *
 * Dispatches `PREFERENCE_CHANGE_EVENT` so every `usePreference(key)`
 * instance in this window can re-read the new value. The browser's
 * native `storage` event only fires in *other* windows, so a custom
 * same-window event is required for in-page reactivity (toggling in
 * SettingsTab and having ReactPreview re-render without a manual
 * page refresh).
 */
export function setPreference<K extends PreferenceKey>(
  key: K,
  value: UserPreferences[K]
): void {
  const current = getPreferences();
  const updated = { ...current, [key]: value };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
  if (typeof window !== 'undefined') {
    window.dispatchEvent(
      new CustomEvent(PREFERENCE_CHANGE_EVENT, { detail: { key } }),
    );
  }
}

/**
 * Name of the same-window CustomEvent dispatched by [`setPreference`].
 * `detail.key` is the [`PreferenceKey`] that changed; consumers
 * filter on it.
 */
export const PREFERENCE_CHANGE_EVENT = 'quarto-hub:preference-change';

export interface PreferenceChangeDetail {
  key: PreferenceKey;
}
