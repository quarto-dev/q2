/**
 * Auth Service
 *
 * Manages Google OAuth2 authentication state. Stores ID tokens in
 * localStorage, decodes JWT payloads client-side (server validates),
 * and handles token expiry detection.
 */

import { googleLogout } from '@react-oauth/google';

export interface AuthState {
  idToken: string;
  email: string;
  name: string | null;
  picture: string | null;
  expiresAt: number;
}

const AUTH_STORAGE_KEY = 'quarto-hub-auth';

/** Decode JWT payload without verification (server validates). */
function decodeJwtPayload(jwt: string): Record<string, unknown> {
  const base64 = jwt.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
  return JSON.parse(atob(base64));
}

export function getStoredAuth(): AuthState | null {
  const stored = localStorage.getItem(AUTH_STORAGE_KEY);
  if (!stored) return null;

  try {
    const state: AuthState = JSON.parse(stored);
    if (Date.now() > state.expiresAt) {
      clearAuth();
      return null;
    }
    return state;
  } catch {
    return null;
  }
}

/** Store an ID token received from Google Sign-In. */
export function storeAuth(idToken: string): AuthState {
  const payload = decodeJwtPayload(idToken);

  const state: AuthState = {
    idToken,
    email: payload.email as string,
    name: (payload.name as string) ?? null,
    picture: (payload.picture as string) ?? null,
    expiresAt: (payload.exp as number) * 1000, // JWT exp is seconds
  };

  localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(state));
  return state;
}

export function clearAuth(): void {
  localStorage.removeItem(AUTH_STORAGE_KEY);
  googleLogout();
}

export function getIdToken(): string | null {
  return getStoredAuth()?.idToken ?? null;
}
