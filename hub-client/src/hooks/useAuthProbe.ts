/**
 * useAuthProbe — out-of-band auth check while sync is disconnected.
 *
 * Browsers hide the HTTP status of a failed WebSocket upgrade, so a sync
 * connection rejected with 401 looks identical to a dropped network. This
 * probe disambiguates by polling GET /auth/me while disconnected
 * (bd-3o8zmz46):
 *
 * - 200 → cookie is fine, the outage is elsewhere; strike counter resets.
 * - network error → offline; never any action (offline editing must survive).
 * - 401/403 → first strike is a **no-op** (just record it); a second
 *   consecutive strike on the next cycle calls `onAuthRejected`
 *   (evidence-based logout).
 *
 * Why the no-op first strike: the strike count is client-side UX, not a
 * security boundary — the server rejects every request the instant a session
 * ends, and this probe only runs while the WS is *already* disconnected (no
 * new data flows, no writes persist during the window). A single transient
 * 401 (multi-instance deploy / key-rotation race) followed by a 200 therefore
 * must not flap the user to the login screen; two consecutive 401s are
 * stronger evidence, and this stays within the evidence-based-logout invariant
 * (still never a network-error logout). Session *renewal* is entirely
 * server-side (sliding re-issue, bd-ey6jg70f); the One-Tap silent-renewal that
 * the first strike once triggered was retired in bd-s042qcxj.
 */

import { useEffect, useRef } from 'react';
import { fetchAuthMe } from '../services/authService';

/** Interval between probes while disconnected. */
export const AUTH_PROBE_INTERVAL_MS = 30_000;

interface AuthProbeOpts {
  /** Probe only while true (auth enabled + signed in + sync disconnected). */
  enabled: boolean;
  /** Second consecutive rejection: the session is over. */
  onAuthRejected: () => void;
}

export function useAuthProbe({ enabled, onAuthRejected }: AuthProbeOpts) {
  // Keep the latest callback in a ref so the probe effect can key on
  // `enabled` alone without re-arming the interval when it changes.
  const onAuthRejectedRef = useRef(onAuthRejected);
  useEffect(() => {
    onAuthRejectedRef.current = onAuthRejected;
  });

  useEffect(() => {
    if (!enabled) return;

    let cancelled = false;
    let strikes = 0;

    const probe = async () => {
      try {
        const me = await fetchAuthMe();
        if (cancelled) return;
        if (me) {
          strikes = 0;
          return;
        }
        strikes += 1;
        if (strikes >= 2) {
          onAuthRejectedRef.current();
        }
        // strike 1 is a no-op: record it and give the next cycle a chance
        // to confirm (or clear) the rejection before logging out.
      } catch {
        // Network error / unreachable hub — no evidence, no action.
      }
    };

    void probe();
    const interval = setInterval(() => void probe(), AUTH_PROBE_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [enabled]);
}
