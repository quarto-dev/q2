# Hub-Client User Preferences Subsystem

**Issue:** kyoto-3se
**Status:** Completed
**Created:** 2026-01-16

## Problem

Hub-client has settings (scroll sync, error overlay collapsed state) that currently reset on page refresh. We need a subsystem for persisting user preferences that:

1. Survives page refreshes (localStorage)
2. Is hub-client-wide, not per-project
3. Has schema validation for safe parsing
4. Provides a typed API for reading/updating values

## Design Proposal

### Storage Format

Single localStorage key containing a JSON object:

```typescript
// localStorage key
const STORAGE_KEY = 'quarto-hub:preferences';

// Stored as JSON string
{
  "version": 1,
  "scrollSyncEnabled": true,
  "errorOverlayCollapsed": true
}
```

The `version` field enables future migrations if the schema changes.

### Schema & Types (using Zod)

Zod provides runtime validation with TypeScript type inference. The schema is the single source of truth for both types and validation.

```typescript
// src/services/preferences/schema.ts
import { z } from 'zod';

// Schema definition - single source of truth
export const UserPreferencesSchema = z.object({
  version: z.literal(1),
  scrollSyncEnabled: z.boolean(),
  errorOverlayCollapsed: z.boolean(),
});

// Infer TypeScript type from schema
export type UserPreferences = z.infer<typeof UserPreferencesSchema>;

// Default values
export const DEFAULT_PREFERENCES: UserPreferences = {
  version: 1,
  scrollSyncEnabled: true,
  errorOverlayCollapsed: true,  // collapsed by default per user request
};

// Validation function - returns valid preferences or defaults
export function validatePreferences(data: unknown): UserPreferences {
  const result = UserPreferencesSchema.safeParse(data);
  return result.success ? result.data : DEFAULT_PREFERENCES;
}
```

This approach:
- Types are inferred from the schema (no duplication)
- Adding new preferences only requires updating the schema and defaults
- `safeParse` never throws, gracefully falling back to defaults

### Low-Level API

Used internally by the hook. Also available for non-React contexts if needed.

```typescript
// src/services/preferences/index.ts

const STORAGE_KEY = 'quarto-hub:preferences';

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

export function setPreference<K extends keyof Omit<UserPreferences, 'version'>>(
  key: K,
  value: UserPreferences[K]
): void {
  const current = getPreferences();
  const updated = { ...current, [key]: value };
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}

// Convenience for getting a single preference
export function getPreference<K extends keyof UserPreferences>(
  key: K
): UserPreferences[K] {
  return getPreferences()[key];
}
```

### React Hook

The primary interface for React components. Returns a tuple like `useState`:

```typescript
// src/hooks/usePreference.ts

export function usePreference<K extends keyof Omit<UserPreferences, 'version'>>(
  key: K
): [UserPreferences[K], (value: UserPreferences[K]) => void] {
  const [value, setValue] = useState(() => getPreference(key));

  const updateValue = useCallback((newValue: UserPreferences[K]) => {
    setPreference(key, newValue);
    setValue(newValue);
  }, [key]);

  return [value, updateValue];
}
```

## File Structure

```
hub-client/src/services/preferences/
├── index.ts       # Main API (getPreferences, setPreference, getPreference)
├── schema.ts      # Types, defaults, validation
└── constants.ts   # Storage key constant
```

## Migration Path

1. Create the preferences subsystem
2. Update Editor.tsx to use `usePreference` for scrollSyncEnabled
3. Update PreviewErrorOverlay.tsx to use preferences for collapsed state
4. Remove local useState for these settings

## Future Considerations

- **Cross-tab sync**: Listen for `storage` events to sync preferences across tabs. Not needed now, but could be useful if users commonly have multiple tabs open.

## Implementation Tasks

- [x] Add zod dependency to hub-client
- [x] Create preferences service with zod schema, validation, and API
- [x] Create usePreference hook
- [x] Update Editor.tsx to persist scrollSyncEnabled
- [x] Update PreviewErrorOverlay to use persisted collapsed state
- [x] Add "Error overlay" toggle to SettingsTab
- [x] Test persistence across page refreshes
