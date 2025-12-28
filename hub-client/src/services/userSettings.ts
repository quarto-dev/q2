/**
 * User settings service for managing user identity.
 *
 * This service provides access to user identity settings stored in IndexedDB.
 * User identity is used for presence features (cursor colors, display names).
 */

import { openDB } from 'idb';
import type { IDBPDatabase } from 'idb';
import type { UserSettings } from './storage/types';
import {
  DB_NAME,
  STORES,
  CURRENT_DB_VERSION,
  generateColorFromId,
  generateAnonymousName,
  isValidHexColor,
  isValidUserName,
} from './storage';

/**
 * Get the database instance.
 * Note: This opens the DB independently to avoid circular dependencies with projectStorage.
 * The DB version and migration system ensures consistency.
 */
async function getDb(): Promise<IDBPDatabase> {
  return openDB(DB_NAME, CURRENT_DB_VERSION);
}

/**
 * Get the current user identity.
 *
 * Returns the stored identity, or creates a default one if none exists.
 * This should always succeed after the migration system has run.
 */
export async function getUserIdentity(): Promise<UserSettings> {
  const db = await getDb();

  // Check if store exists
  if (!db.objectStoreNames.contains(STORES.USER_SETTINGS)) {
    throw new Error('User settings store not found. Database may not be fully initialized.');
  }

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
    throw new Error('Invalid user name: must be 1-50 characters');
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
