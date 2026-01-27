/**
 * Mock User Settings Module
 *
 * Provides a mock implementation of the userSettings service for testing.
 * Used by presenceService and other modules that need user identity.
 */

import { vi } from 'vitest';
import type { UserSettings } from '../services/storage/types';

/**
 * Default mock user identity used when no custom identity is set.
 */
const DEFAULT_MOCK_IDENTITY: UserSettings = {
  key: 'identity',
  userId: 'test-user-id-12345',
  userName: 'Test User',
  userColor: '#3498db',
  createdAt: '2026-01-01T00:00:00.000Z',
  updatedAt: '2026-01-01T00:00:00.000Z',
};

/**
 * Current mock identity (can be customized per test).
 */
let mockIdentity: UserSettings = { ...DEFAULT_MOCK_IDENTITY };

/**
 * Get the current user identity.
 *
 * This is a Vitest mock function that can be configured per-test.
 */
export const getUserIdentity = vi.fn(async (): Promise<UserSettings> => {
  return { ...mockIdentity };
});

/**
 * Update the user's display name.
 */
export const updateUserName = vi.fn(async (name: string): Promise<UserSettings> => {
  mockIdentity = {
    ...mockIdentity,
    userName: name.trim(),
    updatedAt: new Date().toISOString(),
  };
  return { ...mockIdentity };
});

/**
 * Update the user's cursor/presence color.
 */
export const updateUserColor = vi.fn(async (color: string): Promise<UserSettings> => {
  mockIdentity = {
    ...mockIdentity,
    userColor: color,
    updatedAt: new Date().toISOString(),
  };
  return { ...mockIdentity };
});

/**
 * Reset the user identity to a new random identity.
 */
export const resetUserIdentity = vi.fn(async (): Promise<UserSettings> => {
  const userId = `test-user-${Math.random().toString(36).substring(2, 11)}`;
  mockIdentity = {
    key: 'identity',
    userId,
    userName: 'Anonymous Test User',
    userColor: '#' + Math.floor(Math.random() * 16777215).toString(16).padStart(6, '0'),
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  return { ...mockIdentity };
});

/**
 * Get just the userId without loading the full settings.
 */
export const getUserId = vi.fn(async (): Promise<string> => {
  return mockIdentity.userId;
});

// ============================================================================
// Test Helpers (not part of the real module)
// ============================================================================

/**
 * Set a custom mock identity for the current test.
 *
 * @param identity - Custom identity or partial identity to merge
 *
 * @example
 * ```typescript
 * import { _setMockIdentity } from '../__mocks__/userSettings';
 *
 * beforeEach(() => {
 *   _setMockIdentity({
 *     userId: 'custom-user',
 *     userName: 'Custom User',
 *     userColor: '#ff0000',
 *   });
 * });
 * ```
 */
export function _setMockIdentity(identity: Partial<UserSettings>): void {
  mockIdentity = {
    ...DEFAULT_MOCK_IDENTITY,
    ...identity,
  };
}

/**
 * Reset the mock to its default state.
 *
 * Call this in `beforeEach()` to ensure clean test isolation.
 *
 * @example
 * ```typescript
 * import { _resetMock } from '../__mocks__/userSettings';
 *
 * beforeEach(() => {
 *   _resetMock();
 * });
 * ```
 */
export function _resetMock(): void {
  mockIdentity = { ...DEFAULT_MOCK_IDENTITY };
  getUserIdentity.mockClear();
  updateUserName.mockClear();
  updateUserColor.mockClear();
  resetUserIdentity.mockClear();
  getUserId.mockClear();
}

/**
 * Get the current mock identity (for assertions).
 */
export function _getMockIdentity(): UserSettings {
  return { ...mockIdentity };
}
