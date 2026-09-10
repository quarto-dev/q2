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
      richText: false,
    });
    expect(result.scrollSyncEnabled).toBe(true);
    expect(result.errorOverlayCollapsed).toBe(false);
    expect(result.colorScheme).toBe('dark');
    expect(result.unlockNestingCursor).toBe(true);
    expect(result.richText).toBe(false);
  });

  it('defaults richText to ON (rich-text editor enabled by default)', () => {
    expect(DEFAULT_PREFERENCES.richText).toBe(true);
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
    expect(result.unlockNestingCursor).toBe(true);
  });

  // Same migration-safety concern for richText (bd-j1nto6eq): adding it as a
  // required field would wipe older stored prefs that predate it. z.boolean()
  // .default(true) keeps them parsing and fills the default.
  it('preserves other settings when richText is absent (prefs from before it existed)', () => {
    const oldPrefs = {
      version: 1,
      scrollSyncEnabled: false,
      errorOverlayCollapsed: false,
      colorScheme: 'dark',
      unlockNestingCursor: false, // non-default, must be preserved
    };
    const result = validatePreferences(oldPrefs);
    expect(result.scrollSyncEnabled).toBe(false);
    expect(result.unlockNestingCursor).toBe(false);
    expect(result.colorScheme).toBe('dark');
    // Missing richText fills in its ON default:
    expect(result.richText).toBe(true);
  });

  // bd-ew0vak6b: the q2-preview Edit pill in the bottom bar persists its
  // state as `previewEditing`. Same migration-safety shape as richText —
  // a required key would wipe stored prefs that predate it.
  it('defaults previewEditing to ON (block editing enabled by default)', () => {
    expect(DEFAULT_PREFERENCES.previewEditing).toBe(true);
  });

  it('round-trips an explicit previewEditing: false', () => {
    const result = validatePreferences({
      ...DEFAULT_PREFERENCES,
      previewEditing: false,
    });
    expect(result.previewEditing).toBe(false);
  });

  it('preserves other settings when previewEditing is absent (prefs from before it existed)', () => {
    const oldPrefs = {
      version: 1,
      scrollSyncEnabled: false,
      errorOverlayCollapsed: false,
      colorScheme: 'dark',
      unlockNestingCursor: false,
      richText: false,
    };
    const result = validatePreferences(oldPrefs);
    expect(result.scrollSyncEnabled).toBe(false);
    expect(result.richText).toBe(false);
    expect(result.colorScheme).toBe('dark');
    // Missing previewEditing fills in its ON default:
    expect(result.previewEditing).toBe(true);
  });
});
