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
 * - 401/403 → first strike triggers silent renewal; a second consecutive
 *   strike on the next cycle calls `onAuthRejected` (evidence-based logout).
 *   The two-strike shape gives renewal a full cycle to land and avoids
 *   depending on IdP callbacks that may never fire (e.g. GIS blocked).
 */

import { useEffect, useRef } from 'react';
import { fetchAuthMe } from '../services/authService';

/** Interval between probes while disconnected. */
export const AUTH_PROBE_INTERVAL_MS = 30_000;

interface AuthProbeOpts {
  /** Probe only while true (auth enabled + signed in + sync disconnected). */
  enabled: boolean;
  /** First definitive rejection: ask for silent renewal. */
  triggerRefresh: () => void;
  /** Second consecutive rejection: the session is over. */
  onAuthRejected: () => void;
}

export function useAuthProbe({ enabled, triggerRefresh, onAuthRejected }: AuthProbeOpts) {
  // Keep the latest callbacks in refs so the probe effect can key on
  // `enabled` alone without re-arming the interval when they change.
  const triggerRefreshRef = useRef(triggerRefresh);
  const onAuthRejectedRef = useRef(onAuthRejected);
  useEffect(() => {
    triggerRefreshRef.current = triggerRefresh;
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
        if (strikes === 1) {
          triggerRefreshRef.current();
        } else {
          onAuthRejectedRef.current();
        }
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
