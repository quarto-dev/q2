/**
 * Auth Service
 *
 * Manages Google OAuth2 authentication state via HttpOnly cookies.
 * The auth token lives in a server-set HttpOnly cookie — JavaScript
 * never sees or stores it. This module provides helpers to check auth
 * status and refresh tokens via server endpoints.
 */

import { googleLogout } from '@react-oauth/google';

/** User info returned by GET /auth/me. */
export interface AuthState {
  email: string;
  name: string | null;
  picture: string | null;
}

/** Fetch user info from the server. Returns null on 401 (not authenticated). */
export async function fetchAuthMe(): Promise<AuthState | null> {
  const res = await fetch('/auth/me', { credentials: 'same-origin' });
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/me failed: ${res.status}`);
  return res.json() as Promise<AuthState>;
}

/** Clear the auth cookie server-side and revoke Google session. */
export async function logout(): Promise<void> {
  await fetch('/auth/logout', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'X-Requested-With': 'XMLHttpRequest' },
  });
  googleLogout();
}

/**
 * Send a fresh Google JWT to the server for validation and cookie refresh.
 * Returns the updated user info on success, null on auth failure.
 */
export async function refreshToken(credential: string): Promise<AuthState | null> {
  const res = await fetch('/auth/refresh', {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'Content-Type': 'application/json',
      'X-Requested-With': 'XMLHttpRequest',
    },
    body: JSON.stringify({ credential }),
  });
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/refresh failed: ${res.status}`);

  // After refresh, fetch fresh user info from the new cookie.
  return fetchAuthMe();
}
