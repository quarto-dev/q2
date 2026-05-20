/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client using HttpOnly cookies.
 *
 * On mount, calls GET /auth/me to check if the user has a valid cookie.
 * If 200, stores the display info in React state. If 401, shows login.
 *
 * Token refresh: ~15 minutes before the cookie expires, the hook asks
 * the active `AuthProvider` to silently obtain a fresh credential. The
 * new credential is sent to POST /auth/refresh which validates it and
 * sets a fresh cookie. If silent refresh fails, auth is cleared at expiry.
 * The 15-minute buffer absorbs Chrome's intensive timer throttling for
 * backgrounded tabs (timers may skew by 1+ minutes after 5 min hidden).
 *
 * Visibility-aware refresh: if a background tab's timers were throttled
 * and the cookie expired, attempts silent renewal before logging out.
 *
 * 401-triggered refresh: callers that observe a mid-session 401 on an
 * authenticated REST request can call `triggerRefresh()` to enable
 * silent renewal without logging the user out. Concurrent calls are
 * coalesced.
 *
 * During refresh, a 401 from /auth/me is handled gracefully: the hook
 * shows a loading state (not the login screen) while the refresh is
 * in progress.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useAuthProvider } from '../auth/AuthProvider';
import type { AuthState } from '../services/authService';
import { fetchAuthMe, logout as serverLogout, refreshToken } from '../services/authService';

/** Buffer before expiry at which we attempt silent refresh (15 minutes). */
export const REFRESH_BUFFER_MS = 15 * 60 * 1000;

/** Cookie max-age matches server (1 hour). */
const COOKIE_MAX_AGE_MS = 3600 * 1000;

export function useAuth() {
  const provider = useAuthProvider();
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

  const cookieExpired = () =>
    cookieSetAt.current > 0 && Date.now() >= cookieSetAt.current + COOKIE_MAX_AGE_MS;

  // Single entry point for activating One Tap. Coalesces concurrent
  // triggers (e.g. N parallel 401s) into one refresh attempt.
  const triggerRefresh = useCallback(() => {
    if (isRefreshing.current) return;
    isRefreshing.current = true;
    setRefreshEnabled(true);
  }, []);

  // Silent renewal via the active AuthProvider. The provider collapses
  // "renewal returned no usable credential" into onError, so the consumer
  // only needs the two-branch (success / failure) shape here.
  provider.useSilentRenewal({
    enabled: refreshEnabled,
    onCredential: (jwt) => {
      refreshToken(jwt)
        .then((me) => {
          if (me) {
            setAuth(me);
            cookieSetAt.current = Date.now();
          } else if (cookieExpired()) {
            setAuth(null);
          }
        })
        .catch(() => {
          if (cookieExpired()) setAuth(null);
        })
        .finally(() => {
          isRefreshing.current = false;
        });
      setRefreshEnabled(false);
    },
    onError: () => {
      isRefreshing.current = false;
      setRefreshEnabled(false);
      if (cookieExpired()) setAuth(null);
    },
  });

  // On visibility change, verify the cookie. If expired, try One Tap
  // refresh before logging out (timers may not have fired in background).
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
            // Cookie expired — try One Tap refresh before logging out.
            triggerRefresh();
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
  }, [auth, triggerRefresh]);

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
      refreshTimer.current = setTimeout(triggerRefresh, msUntilRefresh);
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
  }, [auth, triggerRefresh]);

  const logout = useCallback(() => {
    serverLogout().catch(() => {
      // Best-effort server logout; clear client state regardless.
    });
    provider.signOut();
    setAuth(null);
  }, [provider]);

  return { auth, loading, logout, triggerRefresh };
}
