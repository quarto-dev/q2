/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client. Handles Google
 * credential responses (from OAuth redirect callback), token expiry
 * monitoring, and logout.
 *
 * Credential ingestion: after Google redirects through the auth callback
 * endpoint, the SPA loads with ?auth_credential=<jwt> in the URL. This
 * hook detects the parameter on mount, stores the credential, and cleans
 * the URL.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type AuthState,
  getStoredAuth,
  storeAuth,
  clearAuth,
} from '../services/authService';

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
  const expiryTimer = useRef<ReturnType<typeof setInterval>>(null);

  // Start expiry monitor on mount
  useEffect(() => {
    expiryTimer.current = setInterval(() => {
      // getStoredAuth() returns null for expired tokens (and clears storage).
      // Sync React state if the stored auth has been cleared.
      if (!getStoredAuth()) setAuth(null);
    }, 60_000);

    return () => {
      if (expiryTimer.current) clearInterval(expiryTimer.current);
    };
  }, []);

  const logout = useCallback(() => {
    clearAuth();
    setAuth(null);
  }, []);

  return { auth, logout };
}
