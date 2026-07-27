/**
 * Auth Service
 *
 * Server-side helpers for the cookie-based auth flow. The auth token
 * lives in a server-set HttpOnly cookie — JavaScript never sees or
 * stores it. This module provides helpers to check auth status and
 * clear the session via server endpoints. Session *renewal* is entirely
 * server-side (sliding re-issue); there is no client renewal helper.
 * IdP-side signout is the `AuthProvider`'s concern, not this module's.
 */

import { hubPath } from '../utils/routing';

/** User info returned by GET /auth/me. */
export interface AuthState {
  email: string;
  name: string | null;
  picture: string | null;
  /**
   * Session expiry in ms-epoch (from the server's `exp`). **Sliding**:
   * the hub re-issues the session cookie on authenticated activity, so
   * this moves forward over time (typically days out). Absent on older
   * servers.
   */
  expiresAt?: number;
}

/** Raw JSON shape from GET /auth/me (snake_case). */
interface AuthMeResponse {
  email: string;
  name: string | null;
  picture: string | null;
  /** Token expiry in epoch seconds. */
  exp?: number;
}

/** Fetch user info from the server. Returns null on 401 (not authenticated). */
export async function fetchAuthMe(): Promise<AuthState | null> {
  const res = await fetch(hubPath('/auth/me'), { credentials: 'same-origin' });
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/me failed: ${res.status}`);
  const data = await res.json() as AuthMeResponse;
  return {
    email: data.email,
    name: data.name,
    picture: data.picture,
    expiresAt: data.exp ? data.exp * 1000 : undefined,
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
    hubPath(`/auth/actor?project=${encodeURIComponent(projectId)}`),
    { credentials: 'same-origin' },
  );
  if (res.status === 401 || res.status === 403) return null;
  if (!res.ok) throw new Error(`/auth/actor failed: ${res.status}`);
  const data = await res.json() as AuthActorResponse;
  return data.actor_id;
}

/**
 * Resolve the per-project actor ID for a document open. Three-valued contract
 * the callers depend on:
 *   - string    → actor ID resolved; open with it
 *   - undefined → auth disabled and no fallback; open with a random actor ID
 *   - null      → auth failure (401/403); abandon the open
 *
 * When auth is disabled, `fallbackActorId` (if provided) is returned so the
 * open uses a *stable* local actor — this is how auth-less deployments
 * (local-prod / `--allow-insecure-auth`) still stamp a consistent identity into
 * documents. The network is never touched in the auth-disabled branch.
 *
 * On auth failure we fire `onSessionExpired` — the session has ended, so the
 * SPA shows the login screen — and return `null` so callers' `=== null` guard
 * abandons this attempt. Throws propagate (e.g. 500) so callers' try/catch
 * surfaces a connection error.
 */
export async function resolveActorId(
  indexDocId: string,
  authEnabled: boolean,
  onSessionExpired: () => void,
  fallbackActorId?: string,
): Promise<string | undefined | null> {
  if (!authEnabled) return fallbackActorId;
  const id = await fetchActorId(indexDocId);
  if (id === null) {
    onSessionExpired();
    return null;
  }
  return id;
}

/** Clear the auth cookie server-side. */
export async function logout(): Promise<void> {
  await fetch(hubPath('/auth/logout'), {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'X-Requested-With': 'XMLHttpRequest' },
  });
}
