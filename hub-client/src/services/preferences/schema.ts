import { z } from 'zod';

// Color scheme types
export const ColorSchemeSchema = z.enum(['auto', 'dark', 'light']);
export type ColorScheme = z.infer<typeof ColorSchemeSchema>;

// Schema definition - single source of truth
//
// The Authorship overlay is intentionally NOT persisted. It lives as
// session-only React state in Editor.tsx and is toggled via the pill
// in the replay bar — treating it as an inspection mode rather than a
// setting avoids leaking a previously-curious view across reloads.
export const UserPreferencesSchema = z.object({
  version: z.literal(1),
  scrollSyncEnabled: z.boolean(),
  errorOverlayCollapsed: z.boolean(),
  colorScheme: ColorSchemeSchema,
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
};

// Validation function - returns valid preferences or defaults
export function validatePreferences(data: unknown): UserPreferences {
  const result = UserPreferencesSchema.safeParse(data);
  return result.success ? result.data : DEFAULT_PREFERENCES;
}
