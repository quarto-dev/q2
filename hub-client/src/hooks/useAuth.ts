/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client using HttpOnly cookies.
 *
 * On mount, calls GET /auth/me to check if the user has a valid cookie.
 * If 200, stores the display info in React state. If 401, shows login.
 *
 * Token refresh: ~5 minutes before the token expires, the hook enables
 * Google One Tap with `auto_select` to silently obtain a fresh credential.
 * The new credential is sent to POST /auth/refresh which validates it and
 * sets a fresh cookie. If silent refresh fails, auth is cleared at expiry.
 *
 * During refresh, a 401 from /auth/me is handled gracefully: the hook
 * shows a loading state (not the login screen) while the refresh is
 * in progress.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useGoogleOneTapLogin } from '@react-oauth/google';
import type { AuthState } from '../services/authService';
import { fetchAuthMe, logout as serverLogout, refreshToken } from '../services/authService';

/** Buffer before expiry at which we attempt silent refresh (5 minutes). */
const REFRESH_BUFFER_MS = 5 * 60 * 1000;

/** Cookie max-age matches server (1 hour). */
const COOKIE_MAX_AGE_MS = 3600 * 1000;

export function useAuth() {
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshEnabled, setRefreshEnabled] = useState(false);
  const isRefreshing = useRef(false);
  const refreshTimer = useRef<ReturnType<typeof setTimeout>>(null);
  const expiryTimer = useRef<ReturnType<typeof setTimeout>>(null);

  // Track when the current cookie was set (for scheduling refresh/expiry).
  const cookieSetAt = useRef<number>(0);

  // Check auth status on mount.
  useEffect(() => {
    let cancelled = false;
    fetchAuthMe()
      .then((me) => {
        if (cancelled) return;
        setAuth(me);
        if (me) cookieSetAt.current = Date.now();
        setLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setAuth(null);
        setLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  // One Tap: disabled until refreshEnabled is set. When enabled with
  // auto_select, it silently returns a credential if the user has an
  // active Google session — no UI shown.
  useGoogleOneTapLogin({
    onSuccess: (response) => {
      if (response.credential) {
        refreshToken(response.credential)
          .then((me) => {
            if (me) {
              setAuth(me);
              cookieSetAt.current = Date.now();
            }
          })
          .catch(() => {
            // Refresh failed — let hard expiry handle it.
          })
          .finally(() => {
            isRefreshing.current = false;
          });
      } else {
        isRefreshing.current = false;
      }
      setRefreshEnabled(false);
    },
    onError: () => {
      isRefreshing.current = false;
      setRefreshEnabled(false);
    },
    auto_select: true,
    disabled: !refreshEnabled,
  });

  // When the tab becomes visible again, check if the cookie is still valid.
  // Browsers throttle/suspend setTimeout in background tabs, so the refresh
  // and expiry timers may not fire before the cookie actually expires.
  useEffect(() => {
    if (!auth) return;

    const handleVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;
      if (isRefreshing.current) return;

      fetchAuthMe()
        .then((me) => {
          if (me) {
            // Cookie still valid — update timestamp so timers reschedule.
            cookieSetAt.current = Date.now();
            setAuth(me);
          } else {
            setAuth(null);
          }
        })
        .catch(() => {
          setAuth(null);
        });
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [auth]);

  // Schedule silent refresh and hard expiry based on cookie lifetime.
  useEffect(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    if (expiryTimer.current) clearTimeout(expiryTimer.current);

    if (!auth || !cookieSetAt.current) return;

    const expiresAt = cookieSetAt.current + COOKIE_MAX_AGE_MS;
    const msUntilExpiry = expiresAt - Date.now();
    if (msUntilExpiry <= 0) {
      setAuth(null);
      return;
    }

    // Schedule silent refresh attempt before expiry.
    const msUntilRefresh = msUntilExpiry - REFRESH_BUFFER_MS;
    if (msUntilRefresh > 0) {
      refreshTimer.current = setTimeout(() => {
        isRefreshing.current = true;
        setRefreshEnabled(true);
      }, msUntilRefresh);
    }

    // Hard expiry: re-check auth when the cookie should have expired.
    // If a refresh succeeded in the meantime, /auth/me will return 200.
    expiryTimer.current = setTimeout(() => {
      fetchAuthMe().then((me) => {
        if (me) {
          setAuth(me);
          cookieSetAt.current = Date.now();
        } else if (!isRefreshing.current) {
          setAuth(null);
        }
        // If isRefreshing, the refresh handler will update state.
      }).catch(() => {
        if (!isRefreshing.current) setAuth(null);
      });
    }, msUntilExpiry);

    return () => {
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      if (expiryTimer.current) clearTimeout(expiryTimer.current);
    };
  }, [auth]);

  const logout = useCallback(() => {
    serverLogout().catch(() => {
      // Best-effort server logout; clear client state regardless.
    });
    setAuth(null);
  }, []);

  return { auth, loading, logout };
}
