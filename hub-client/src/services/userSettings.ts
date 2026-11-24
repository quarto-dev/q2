/**
 * User settings service for managing user identity.
 *
 * This service provides access to user identity settings stored in IndexedDB.
 * User identity is used for presence features (cursor colors, display names).
 */

import type { UserSettings } from './storage/types';
import {
  STORES,
  getDb,
  generateColorFromId,
  generateAnonymousName,
  isValidHexColor,
  isValidUserName,
  MAX_USER_NAME_LENGTH,
} from './storage';
import { isEphemeralStorage } from './ephemeralStorage';

/**
 * Derive a stable Automerge author id from the local user id.
 *
 * Author ids are opaque even-length hex strings. A `userId` produced by
 * `crypto.randomUUID()` is 32 hex digits plus dashes, so stripping the dashes
 * yields a valid author id. This lets auth-less deployments (local-prod /
 * `--allow-insecure-auth`, where the server exposes no `/auth/author`) stamp a
 * *stable* attribution identity into documents, while the actor id stays
 * automerge's random per-document-instance value (author-ID transition,
 * D2). The derivation is unchanged from the old stable-actor construction
 * (D5), so a user's attribution key is continuous across the transition.
 *
 * Defensive fallback: any userId that isn't already clean hex is hex-encoded
 * from its UTF-8 bytes, so the result is always a valid author id. In practice
 * the app only ever passes `randomUUID()` ids.
 */
export function authorIdFromUserId(userId: string): string {
  const stripped = userId.replace(/-/g, '').toLowerCase();
  if (/^[0-9a-f]+$/.test(stripped) && stripped.length % 2 === 0) {
    return stripped;
  }
  return Array.from(new TextEncoder().encode(userId))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

/**
 * Get the current user identity.
 *
 * Returns the stored identity, or creates a default one if none exists.
 * This should always succeed after the migration system has run.
 */
export async function getUserIdentity(): Promise<UserSettings> {
  const db = await getDb();
  const settings = await db.get(STORES.USER_SETTINGS, 'identity');

  if (settings) {
    return settings as UserSettings;
  }

  // Create default identity if none exists
  // This normally happens in migration, but handle it here as fallback
  const userId = crypto.randomUUID();
  const now = new Date().toISOString();
  const defaultSettings: UserSettings = {
    key: 'identity',
    userId,
    userName: generateAnonymousName(),
    userColor: generateColorFromId(userId),
    createdAt: now,
    updatedAt: now,
  };

  await db.put(STORES.USER_SETTINGS, defaultSettings);
  return defaultSettings;
}

/**
 * Update the user's display name.
 *
 * @param name - The new display name (will be trimmed)
 * @throws Error if name is invalid (empty or too long)
 */
export async function updateUserName(name: string): Promise<UserSettings> {
  const trimmedName = name.trim();

  if (!isValidUserName(trimmedName)) {
    throw new Error(`Invalid user name: must be 1-${MAX_USER_NAME_LENGTH} characters`);
  }

  const db = await getDb();
  const settings = await getUserIdentity();

  const updated: UserSettings = {
    ...settings,
    userName: trimmedName,
    updatedAt: new Date().toISOString(),
  };

  await db.put(STORES.USER_SETTINGS, updated);
  return updated;
}

/**
 * Update the user's cursor/presence color.
 *
 * @param color - Hex color string (e.g., "#FF5722")
 * @throws Error if color is not a valid hex color
 */
export async function updateUserColor(color: string): Promise<UserSettings> {
  if (!isValidHexColor(color)) {
    throw new Error('Invalid color: must be a hex color (e.g., #FF5722)');
  }

  const db = await getDb();
  const settings = await getUserIdentity();

  const updated: UserSettings = {
    ...settings,
    userColor: color,
    updatedAt: new Date().toISOString(),
  };

  await db.put(STORES.USER_SETTINGS, updated);
  return updated;
}

/**
 * The identity a page URL asks for: `?as=<name>` and an optional `?color=#RRGGBB`.
 *
 * Without a valid `color` the color is derived from the name, so the same name always gets the same cursor color.
 * Returns null when there is no usable name.
 */
export function identityFromSearch(search: string): { name: string; color: string } | null {
  const params = new URLSearchParams(search);
  const name = (params.get('as') ?? '').trim();
  if (!isValidUserName(name)) return null;
  const color = params.get('color') ?? '';
  return { name, color: isValidHexColor(color) ? color : generateColorFromId(name) };
}

/**
 * Take the identity from the page URL (`?as=Maurice`), but only in the ephemeral `q2 preview` build.
 *
 * Every preview session starts with a random anonymous name. A link, or a scripted demo, can name the person instead.
 * A persistent hub-client profile is never renamed by a link: outside the ephemeral build this does nothing.
 * Call it before presence starts, since presence reads the name and color back from storage.
 */
export async function applyIdentityParam(search: string): Promise<UserSettings | null> {
  if (!isEphemeralStorage()) return null;
  const wanted = identityFromSearch(search);
  if (!wanted) return null;
  await updateUserName(wanted.name);
  return updateUserColor(wanted.color);
}

/**
 * Reset the user identity to a new random identity.
 *
 * This generates a new userId, userName, and userColor.
 * Use this if the user wants a completely fresh identity.
 */
export async function resetUserIdentity(): Promise<UserSettings> {
  const db = await getDb();
  const userId = crypto.randomUUID();
  const now = new Date().toISOString();

  const newSettings: UserSettings = {
    key: 'identity',
    userId,
    userName: generateAnonymousName(),
    userColor: generateColorFromId(userId),
    createdAt: now,
    updatedAt: now,
  };

  await db.put(STORES.USER_SETTINGS, newSettings);
  return newSettings;
}

/**
 * Get just the userId without loading the full settings.
 * Useful for quick identity checks.
 */
export async function getUserId(): Promise<string> {
  const settings = await getUserIdentity();
  return settings.userId;
}
