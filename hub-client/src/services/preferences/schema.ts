import { z } from 'zod';

// Color scheme types
export const ColorSchemeSchema = z.enum(['auto', 'dark', 'light']);
export type ColorScheme = z.infer<typeof ColorSchemeSchema>;

// Schema definition - single source of truth
//
// The Attribution overlay is intentionally NOT persisted. It lives as
// session-only React state in Editor.tsx and is toggled via the pill
// in the replay bar — treating it as an inspection mode rather than a
// setting avoids leaking a previously-curious view across reloads.
export const UserPreferencesSchema = z.object({
  version: z.literal(1),
  scrollSyncEnabled: z.boolean(),
  errorOverlayCollapsed: z.boolean(),
  colorScheme: ColorSchemeSchema,
  /**
   * P3.2: nesting-cursor mode for nested blocks (default-off).
   * When true, the preview iframe descends into nested list/quote blocks
   * and enables per-level editing. The flag gates the expensive
   * `regenerateNestedBuffers` WASM pass — off means that call is
   * unreachable.
   */
  unlockNestingCursor: z.boolean().default(false),
});

// Infer TypeScript type from schema
export type UserPreferences = z.infer<typeof UserPreferencesSchema>;

// Keys that can be updated (excludes version)
export type PreferenceKey = keyof Omit<UserPreferences, 'version'>;

// Default values
export const DEFAULT_PREFERENCES: UserPreferences = {
  version: 1,
  scrollSyncEnabled: true,
  errorOverlayCollapsed: true, // collapsed by default
  colorScheme: 'auto',
  unlockNestingCursor: false,   // nesting-cursor off by default (P3.2)
};

// Validation function - returns valid preferences or defaults
export function validatePreferences(data: unknown): UserPreferences {
  const result = UserPreferencesSchema.safeParse(data);
  return result.success ? result.data : DEFAULT_PREFERENCES;
}
