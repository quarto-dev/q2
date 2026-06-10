/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client using HttpOnly cookies.
 *
 * On mount, calls GET /auth/me to check if the user has a valid cookie.
 * If 200, stores the display info in React state. If 401, shows login.
 *
 * Expiry tracking: /auth/me reports the token's real `exp`
 * (`AuthState.expiresAt`, ms epoch; falls back to +1 h for older servers).
 * Silent refresh is scheduled ~15 minutes before that expiry via the active
 * `AuthProvider`; the fresh credential goes to POST /auth/refresh which sets
 * a new cookie. The 15-minute buffer absorbs Chrome's intensive timer
 * throttling for backgrounded tabs.
 *
 * Evidence-based logout (bd-3o8zmz46): auth is only cleared when a reachable
 * server definitively rejects us (401/403). Network errors — refocus checks,
 * expiry re-checks — never log the user out; offline editing must survive
 * them. When the session ends this way, `sessionExpired` is set so the UI
 * can say "session expired" instead of implying a network problem.
 *
 * 401-triggered refresh: callers that observe a mid-session 401 on an
 * authenticated REST request can call `triggerRefresh()` to enable
 * silent renewal without logging the user out. Concurrent calls are
 * coalesced.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useAuthProvider } from '../auth/AuthProvider';
import type { AuthState } from '../services/authService';
import { fetchAuthMe, logout as serverLogout, refreshToken } from '../services/authService';

/** Buffer before expiry at which we attempt silent refresh (15 minutes). */
export const REFRESH_BUFFER_MS = 15 * 60 * 1000;

/** Assumed session lifetime when the server doesn't report `exp` (1 hour). */
const DEFAULT_SESSION_MS = 3600 * 1000;

/** Re-check interval when an expiry-time verdict couldn't be reached. */
const EXPIRY_RECHECK_MS = 60 * 1000;

export function useAuth() {
  const provider = useAuthProvider();
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshEnabled, setRefreshEnabled] = useState(false);
  const [sessionExpired, setSessionExpired] = useState(false);
  const isRefreshing = useRef(false);
  const refreshTimer = useRef<ReturnType<typeof setTimeout>>(null);
  const expiryTimer = useRef<ReturnType<typeof setTimeout>>(null);

  // Effective expiry of the current session (0 = no session).
  const expiresAtRef = useRef<number>(0);

  /** Install a (re)confirmed session; clears any expired flag. */
  const applyAuth = useCallback((me: AuthState) => {
    setSessionExpired(false);
    setAuth(me);
  }, []);

  /** Evidence-based logout: the server rejected our credential. */
  const expireSession = useCallback(() => {
    setSessionExpired(true);
    setAuth(null);
  }, []);

  const sessionLapsed = () =>
    expiresAtRef.current > 0 && Date.now() >= expiresAtRef.current;

  // Check auth status on mount.
  useEffect(() => {
    let cancelled = false;
    fetchAuthMe()
      .then((me) => {
        if (cancelled) return;
        setAuth(me);
        setLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setAuth(null);
        setLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

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
            applyAuth(me);
          } else if (sessionLapsed()) {
            expireSession();
          }
        })
        .catch(() => {
          if (sessionLapsed()) expireSession();
        })
        .finally(() => {
          isRefreshing.current = false;
        });
      setRefreshEnabled(false);
    },
    onError: () => {
      isRefreshing.current = false;
      setRefreshEnabled(false);
      if (sessionLapsed()) expireSession();
    },
  });

  // On visibility change, verify the cookie. If rejected, try One Tap
  // refresh; a network error means offline and never clears the session.
  useEffect(() => {
    if (!auth) return;

    const handleVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;
      if (isRefreshing.current) return;

      fetchAuthMe()
        .then((me) => {
          if (me) {
            // Still valid. The reported expiry is the same token's exp,
            // so this does NOT extend the schedule (the old cookieSetAt
            // reset here drifted assumed expiry past the real one).
            applyAuth(me);
          } else {
            // Definitive rejection — try One Tap refresh before logging out.
            triggerRefresh();
          }
        })
        .catch(() => {
          // Offline — keep the session; sync layer probes will escalate
          // if the server actually rejects us once reachable.
        });
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [auth, applyAuth, triggerRefresh]);

  // Schedule silent refresh and an expiry-time server re-check from the
  // session's real expiry.
  useEffect(() => {
    if (refreshTimer.current) clearTimeout(refreshTimer.current);
    if (expiryTimer.current) clearTimeout(expiryTimer.current);

    if (!auth) {
      expiresAtRef.current = 0;
      return;
    }

    const expiresAt = auth.expiresAt ?? Date.now() + DEFAULT_SESSION_MS;
    expiresAtRef.current = expiresAt;

    // Silent refresh before expiry; immediately if already inside the buffer.
    refreshTimer.current = setTimeout(
      triggerRefresh,
      Math.max(expiresAt - REFRESH_BUFFER_MS - Date.now(), 0),
    );

    // Expiry-time re-check. Only a definitive 401/403 clears the session;
    // network errors reschedule (logout on evidence, not on schedule).
    const scheduleExpiryCheck = (delay: number) => {
      expiryTimer.current = setTimeout(() => {
        fetchAuthMe()
          .then((me) => {
            if (me) {
              applyAuth(me); // reschedules from the (possibly refreshed) exp
            } else if (!isRefreshing.current) {
              expireSession();
            } else {
              // Renewal in flight — re-check for a definitive verdict even
              // if the IdP never calls back (e.g. GIS blocked).
              scheduleExpiryCheck(EXPIRY_RECHECK_MS);
            }
          })
          .catch(() => {
            scheduleExpiryCheck(EXPIRY_RECHECK_MS);
          });
      }, delay);
    };
    const msUntilExpiry = expiresAt - Date.now();
    scheduleExpiryCheck(msUntilExpiry > 0 ? msUntilExpiry + 1000 : EXPIRY_RECHECK_MS);

    return () => {
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      if (expiryTimer.current) clearTimeout(expiryTimer.current);
    };
  }, [auth, applyAuth, expireSession, triggerRefresh]);

  const logout = useCallback(() => {
    serverLogout().catch(() => {
      // Best-effort server logout; clear client state regardless.
    });
    provider.signOut();
    setAuth(null);
  }, [provider]);

  return { auth, loading, logout, triggerRefresh, sessionExpired, expireSession };
}
