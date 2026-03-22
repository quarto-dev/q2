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

/** Raw JSON shape from GET /auth/me (snake_case). */
interface AuthMeResponse {
  email: string;
  name: string | null;
  picture: string | null;
}

/** Fetch user info from the server. Returns null on 401 (not authenticated). */
export async function fetchAuthMe(): Promise<AuthState | null> {
  const res = await fetch('/auth/me', { credentials: 'same-origin' });
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/me failed: ${res.status}`);
  const data = await res.json() as AuthMeResponse;
  return {
    email: data.email,
    name: data.name,
    picture: data.picture,
  };
}

/** Raw JSON shape from GET /auth/actor (snake_case). */
interface AuthActorResponse {
  actor_id: string;
}

/**
 * Fetch the per-project actor ID for the authenticated user.
 *
 * Returns the actor ID string on success, or null on 401/403 (session expired
 * or forbidden). Throws on unexpected errors (e.g. 500).
 *
 * The server computes `HMAC-SHA256(server_secret, sub || "\0" || projectId)`,
 * so the same user gets a different actor ID in each project.
 */
export async function fetchActorId(projectId: string): Promise<string | null> {
  const res = await fetch(
    `/auth/actor?project=${encodeURIComponent(projectId)}`,
    { credentials: 'same-origin' },
  );
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/actor failed: ${res.status}`);
  const data = await res.json() as AuthActorResponse;
  return data.actor_id;
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
