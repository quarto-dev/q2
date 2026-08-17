/**
 * useAuth Hook
 *
 * Manages authentication state for the hub client using HttpOnly cookies.
 *
 * On mount, calls GET /auth/me to check if the user has a valid cookie.
 * If 200, stores the display info in React state. If 401, shows login.
 *
 * Sliding sessions (bd-ey6jg70f): the cookie is a hub-minted session
 * token whose expiry **slides** — the server re-issues it on
 * authenticated HTTP activity, and `useSessionKeepAlive` probes
 * /auth/me periodically while a WS is open (WS traffic never slides
 * the window). Session renewal is therefore **entirely server-side**;
 * no IdP round-trip is involved.
 *
 * Definitive session end (bd-s042qcxj): the GIS One-Tap silent-renewal
 * fallback was retired. A session that hits a hard boundary
 * (logout-everywhere revocation, absolute cap, or idle timeout) ends
 * definitively — the SPA shows the login screen and the user re-logs-in
 * through the GIS button. Day-to-day renewal stays invisible (the
 * server-side slide above); re-authentication is required only at the
 * rarely-reached hard boundaries.
 *
 * Expiry tracking: /auth/me reports the session's current `exp`
 * (`AuthState.expiresAt`, ms epoch — sliding, typically days out). An
 * expiry-time re-check runs against the reported `exp`. When the server
 * reports no `exp` (only conceivable on a pre-sliding hub), no expiry
 * re-check is scheduled — the mount check, visibility-change re-check,
 * hourly keep-alive, and the disconnected-state auth probe still cover
 * session-end detection. (The old 1 h fallback here would have probed
 * ~168× too often against a sliding session; bd-aw8f3sp8.)
 *
 * Evidence-based logout (bd-3o8zmz46): auth is only cleared when a reachable
 * server definitively rejects us (401/403). Network errors — refocus checks,
 * expiry re-checks — never log the user out; offline editing must survive
 * them. When the session ends this way, `sessionExpired` is set so the UI
 * can say "session expired" instead of implying a network problem.
 */

import { useCallback, useEffect, useState } from 'react';
import { useAuthProvider } from '../auth/AuthProvider';
import type { AuthState } from '../services/authService';
import { fetchAuthMe, logout as serverLogout } from '../services/authService';

/** Re-check interval when an expiry-time verdict couldn't be reached. */
const EXPIRY_RECHECK_MS = 60 * 1000;

export function useAuth() {
  const provider = useAuthProvider();
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [loading, setLoading] = useState(true);
  const [sessionExpired, setSessionExpired] = useState(false);

  /**
   * Install a (re)confirmed session; clears any expired flag. Keeps
   * the previous state object when nothing changed, so the keep-alive
   * probe doesn't re-render the app (or reset the expiry schedules)
   * unless the session actually slid.
   */
  const applyAuth = useCallback((me: AuthState) => {
    setSessionExpired(false);
    setAuth((prev) =>
      prev
      && prev.email === me.email
      && prev.name === me.name
      && prev.picture === me.picture
      && prev.expiresAt === me.expiresAt
        ? prev
        : me,
    );
  }, []);

  /** Evidence-based logout: the server rejected our credential. */
  const expireSession = useCallback(() => {
    setSessionExpired(true);
    setAuth(null);
  }, []);

  // Check auth status on mount. A 401 here means "not signed in", not
  // "session expired" — a fresh visitor should see the sign-in prompt,
  // not an expiry message — so this never sets `sessionExpired`.
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

  // On visibility change, verify the cookie. A definitive rejection ends
  // the session (server-side renewal already slid it if it was alive); a
  // network error means offline and never clears the session.
  useEffect(() => {
    if (!auth) return;

    const handleVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;

      fetchAuthMe()
        .then((me) => {
          if (me) {
            // Still valid. The reported expiry is the same token's exp,
            // so this does NOT extend the schedule (the old cookieSetAt
            // reset here drifted assumed expiry past the real one).
            applyAuth(me);
          } else {
            // Definitive rejection — the session is over.
            expireSession();
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
  }, [auth, applyAuth, expireSession]);

  // Schedule an expiry-time server re-check from the session's real
  // expiry. No reported exp → nothing to schedule from (the other
  // checks — mount, visibility, keep-alive, disconnected probe — still
  // run); guessing a lifetime here mis-scheduled badly (bd-aw8f3sp8).
  useEffect(() => {
    if (!auth || auth.expiresAt === undefined) return;

    const expiresAt = auth.expiresAt;

    // Expiry-time re-check. Only a definitive 401/403 clears the session;
    // network errors reschedule (logout on evidence, not on schedule).
    let expiryTimer: ReturnType<typeof setTimeout>;
    const scheduleExpiryCheck = (delay: number) => {
      expiryTimer = setTimeout(() => {
        fetchAuthMe()
          .then((me) => {
            if (me) {
              applyAuth(me); // reschedules from the (possibly slid) exp
            } else {
              expireSession();
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
      clearTimeout(expiryTimer);
    };
  }, [auth, applyAuth, expireSession]);

  const logout = useCallback(() => {
    serverLogout().catch(() => {
      // Best-effort server logout; clear client state regardless.
    });
    provider.signOut();
    setAuth(null);
  }, [provider]);

  return { auth, loading, logout, sessionExpired, expireSession, applyAuth };
}
