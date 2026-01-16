import { z } from 'zod';

// Schema definition - single source of truth
export const UserPreferencesSchema = z.object({
  version: z.literal(1),
  scrollSyncEnabled: z.boolean(),
  errorOverlayCollapsed: z.boolean(),
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
};

// Validation function - returns valid preferences or defaults
export function validatePreferences(data: unknown): UserPreferences {
  const result = UserPreferencesSchema.safeParse(data);
  return result.success ? result.data : DEFAULT_PREFERENCES;
}
