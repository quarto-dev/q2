import { describe, it, expect } from 'vitest';
import { validatePreferences, DEFAULT_PREFERENCES } from './schema';

describe('validatePreferences', () => {
  it('accepts a fully-populated v1 prefs object', () => {
    const result = validatePreferences({
      version: 1,
      scrollSyncEnabled: true,
      errorOverlayCollapsed: false,
      colorScheme: 'dark',
      unlockNestingCursor: true,
    });
    expect(result.scrollSyncEnabled).toBe(true);
    expect(result.errorOverlayCollapsed).toBe(false);
    expect(result.colorScheme).toBe('dark');
    expect(result.unlockNestingCursor).toBe(true);
  });

  it('falls back to defaults for invalid data', () => {
    const result = validatePreferences({ version: 2, garbage: true });
    expect(result).toEqual(DEFAULT_PREFERENCES);
  });

  // Regression test for P3.2 follow-up bug:
  // Adding unlockNestingCursor as a required field (z.boolean()) caused
  // safeParse to fail on old stored prefs missing that key, silently
  // wiping scrollSyncEnabled, colorScheme, and errorOverlayCollapsed.
  // The fix: z.boolean().default(false) — old objects still parse OK.
  it('preserves other settings when unlockNestingCursor is absent (old stored prefs)', () => {
    // Simulate a prefs object written before P3.2 — no unlockNestingCursor key.
    const oldPrefs = {
      version: 1,
      scrollSyncEnabled: false,        // non-default (default is true)
      errorOverlayCollapsed: false,    // non-default (default is true)
      colorScheme: 'dark',             // non-default (default is 'auto')
    };
    const result = validatePreferences(oldPrefs);
    // Must NOT fall back to DEFAULT_PREFERENCES:
    expect(result.scrollSyncEnabled).toBe(false);
    expect(result.errorOverlayCollapsed).toBe(false);
    expect(result.colorScheme).toBe('dark');
    // Must fill in the missing field with its default:
    expect(result.unlockNestingCursor).toBe(false);
  });
});
