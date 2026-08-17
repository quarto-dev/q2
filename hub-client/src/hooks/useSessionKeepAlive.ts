/**
 * useSessionKeepAlive — keeps a hub sliding session alive while a
 * WebSocket is open (bd-exk3hfxk, sliding sessions C6).
 *
 * The hub re-issues the session cookie on authenticated **HTTP**
 * activity (tokens ≥ 1 h old), but WS traffic never qualifies — the
 * upgrade is validated once and `Set-Cookie` on a 101 is unreliable. A
 * client that only syncs over the WS would idle out of a session it is
 * actively using. This hook owns the keep-alive: a periodic
 * GET /auth/me while signed in and online. The response reports the
 * (sliding) expiry, which `onAuthState` feeds back into `useAuth` so
 * its schedules follow the session; the request itself is what
 * triggers the server-side re-issue. No IdP round-trip is involved —
 * renewal is entirely server-side.
 *
 * Failure semantics mirror the evidence-based rules from bd-3o8zmz46:
 * a definitive 401/403 ends the session (`onAuthRejected`) — e.g. the
 * session was revoked via logout-everywhere elsewhere, or hit the
 * absolute cap — and the user re-logs-in through the GIS button (the
 * One-Tap silent-renewal fallback was retired in bd-s042qcxj); network
 * errors take no action (offline editing must survive them). While sync
 * is *disconnected*, `useAuthProbe`'s faster two-strike probe governs
 * instead.
 */

import { useEffect, useRef } from 'react';
import type { AuthState } from '../services/authService';
import { fetchAuthMe } from '../services/authService';

/**
 * Cadence of the keep-alive probe. Matches the server's re-issue age
 * gate (≥ 1 h) — probing faster would never slide the window sooner —
 * and sits comfortably inside the 7-day idle timeout even under heavy
 * background-tab timer throttling.
 */
export const SESSION_KEEP_ALIVE_INTERVAL_MS = 60 * 60 * 1000;

interface SessionKeepAliveOpts {
  /** Probe only while true (auth enabled + signed in + sync online). */
  enabled: boolean;
  /** Fresh session info (sliding expiry) from a successful probe. */
  onAuthState: (me: AuthState) => void;
  /** Definitive rejection: the session is over. */
  onAuthRejected: () => void;
}

export function useSessionKeepAlive({ enabled, onAuthState, onAuthRejected }: SessionKeepAliveOpts) {
  // Latest callbacks in refs so the interval keys on `enabled` alone.
  const onAuthStateRef = useRef(onAuthState);
  const onAuthRejectedRef = useRef(onAuthRejected);
  useEffect(() => {
    onAuthStateRef.current = onAuthState;
    onAuthRejectedRef.current = onAuthRejected;
  });

  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;

    const probe = async () => {
      try {
        const me = await fetchAuthMe();
        if (cancelled) return;
        if (me) {
          onAuthStateRef.current(me);
        } else {
          onAuthRejectedRef.current();
        }
      } catch {
        // Network error / unreachable hub — no evidence, no action.
      }
    };

    void probe();
    const interval = setInterval(() => void probe(), SESSION_KEEP_ALIVE_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [enabled]);
}
