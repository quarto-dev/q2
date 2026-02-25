/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client. Handles Google
 * credential responses (from OAuth redirect callback), token expiry
 * monitoring, silent token refresh, and logout.
 *
 * Credential ingestion: after Google redirects through the auth callback
 * endpoint, the SPA loads with ?auth_credential=<jwt> in the URL. This
 * hook detects the parameter on mount, stores the credential, and cleans
 * the URL.
 *
 * Token refresh: ~5 minutes before the token expires, the hook enables
 * Google One Tap with `auto_select` to silently obtain a fresh credential.
 * If the user has an active Google session, the token is renewed without
 * any UI. If silent refresh fails, auth is cleared at expiry.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useGoogleOneTapLogin } from '@react-oauth/google';
import {
  type AuthState,
  getStoredAuth,
  storeAuth,
  clearAuth,
} from '../services/authService';

/** Buffer before expiry at which we attempt silent refresh (5 minutes). */
const REFRESH_BUFFER_MS = 5 * 60 * 1000;

export function useAuth() {
  const [auth, setAuth] = useState<AuthState | null>(() => {
    // Check URL search params first (OAuth redirect callback), then localStorage.
    const params = new URLSearchParams(window.location.search);
    const credential = params.get('auth_credential');
    if (credential) {
      try {
        const authState = storeAuth(credential);
        // Clean the URL — remove the credential parameter without triggering navigation.
        const url = new URL(window.location.href);
        url.searchParams.delete('auth_credential');
        window.history.replaceState(null, '', url.pathname + url.search + url.hash);
        return authState;
      } catch {
        // Fall through to localStorage check
      }
    }
    return getStoredAuth();
  });

  // Enable One Tap silent refresh when approaching token expiry.
  const [refreshEnabled, setRefreshEnabled] = useState(false);
  const refreshTimer = useRef<ReturnType<typeof setTimeout>>(null);
  const expiryTimer = useRef<ReturnType<typeof setTimeout>>(null);

  // One Tap: disabled until refreshEnabled is set. When enabled with
  // auto_select, it silently returns a credential if the user has an
  // active Google session — no UI shown.
  useGoogleOneTapLogin({
    onSuccess: (response) => {
      if (response.credential) {
        try {
          setAuth(storeAuth(response.credential));
        } catch {
          // Invalid credential — let hard expiry handle it.
        }
      }
      setRefreshEnabled(false);
    },
    onError: () => setRefreshEnabled(false),
    auto_select: true,
    disabled: !refreshEnabled,
  });

  // Schedule silent refresh and hard expiry based on the token's exp claim.
  useEffect(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    if (expiryTimer.current) clearTimeout(expiryTimer.current);

    if (!auth) return;

    const msUntilExpiry = auth.expiresAt - Date.now();
    if (msUntilExpiry <= 0) {
      clearAuth();
      setAuth(null);
      return;
    }

    // Schedule silent refresh attempt before expiry.
    const msUntilRefresh = msUntilExpiry - REFRESH_BUFFER_MS;
    if (msUntilRefresh > 0) {
      refreshTimer.current = setTimeout(() => {
        setRefreshEnabled(true);
      }, msUntilRefresh);
    }

    // Hard expiry: clear auth when the token actually expires.
    expiryTimer.current = setTimeout(() => {
      clearAuth();
      setAuth(null);
    }, msUntilExpiry);

    return () => {
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      if (expiryTimer.current) clearTimeout(expiryTimer.current);
    };
  }, [auth]);

  const logout = useCallback(() => {
    clearAuth();
    setAuth(null);
  }, []);

  return { auth, logout };
}
