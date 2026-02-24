/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client. Handles Google
 * credential responses, token expiry monitoring, and logout.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  type AuthState,
  getStoredAuth,
  storeAuth,
  clearAuth,
} from '../services/authService';

export function useAuth() {
  const [auth, setAuth] = useState<AuthState | null>(getStoredAuth);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const expiryTimer = useRef<ReturnType<typeof setInterval>>(null);

  // Start expiry monitor on mount
  useEffect(() => {
    setIsLoading(false);

    expiryTimer.current = setInterval(() => {
      // getStoredAuth() returns null for expired tokens (and clears storage).
      // Sync React state if the stored auth has been cleared.
      if (!getStoredAuth()) setAuth(null);
    }, 60_000);

    return () => {
      if (expiryTimer.current) clearInterval(expiryTimer.current);
    };
  }, []);

  const handleCredentialResponse = useCallback((credential: string) => {
    try {
      setAuth(storeAuth(credential));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Authentication failed');
    }
  }, []);

  const logout = useCallback(() => {
    clearAuth();
    setAuth(null);
  }, []);

  return { auth, isLoading, error, handleCredentialResponse, logout };
}
