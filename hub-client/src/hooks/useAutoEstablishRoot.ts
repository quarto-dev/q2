/**
 * Silently establish the personal root collection when none exists.
 *
 * The collections hook reports `needs-setup` (fresh browser) or
 * `needs-migration` (pre-project-set IDB projects, no pointer) when it
 * finds nothing to connect to. Rather than stopping the user at a setup
 * screen, this hook creates the root against `DEFAULT_SYNC_SERVER` (or
 * migrates the legacy projects into a new root — non-destructive, the
 * legacy store is retained) and the app proceeds straight to the home
 * (bd-4h1hv60p). Which sync server a deployment uses is the deployment's
 * concern (`VITE_DEFAULT_SYNC_SERVER` at build time); users never choose.
 *
 * This generalizes the effects that the collection-invite landing
 * (bd-fxdcxbpq) and the ephemeral `q2 preview` boot (bd-zf4ryvuq) used to
 * carry as two gated copies in App.tsx.
 *
 * Fires at most once per initialization cycle: `migrateProjects` resets
 * status to `needs-migration` on failure (and `createProjectSet` to
 * `error`), so an unguarded effect would retry-loop against an unreachable
 * server. The guard re-arms when status passes through `loading`, which is
 * what `useCollectionSets.retry()` does, so the retry card's button gets a
 * second attempt.
 *
 * `enabled: false` hands setup to someone else — the `#/link-project-set`
 * boot route, whose handler links the other browser's set as the root.
 *
 * `onFreshRoot` (bd-3fwtdhil) runs once, when a `needs-setup` boot has
 * created its root and reached `connected`. That is the one moment a
 * brand-new browser exists with an empty root — where the "Examples /
 * Templates" collection gets seeded. It never runs for a migration (the
 * user already has projects), a returning browser, or a setup that ended
 * in `error`; like the setup itself it re-arms through `loading`.
 */

import { useEffect, useRef } from 'react';
import type { CollectionsStatus } from './useCollectionSets';
import { DEFAULT_SYNC_SERVER } from '../utils/routing';

export interface AutoEstablishRootOptions {
  status: CollectionsStatus;
  /** False when a boot-time route owns setup (link-project-set). */
  enabled: boolean;
  createProjectSet: (syncServer: string) => Promise<void>;
  migrateProjects: (syncServer: string) => Promise<void>;
  /** Fired once after a fresh root (from `needs-setup`) becomes connected. */
  onFreshRoot?: () => Promise<void> | void;
}

export function useAutoEstablishRoot({
  status,
  enabled,
  createProjectSet,
  migrateProjects,
  onFreshRoot,
}: AutoEstablishRootOptions): void {
  const firedRef = useRef(false);
  // Set when this cycle's root was created from `needs-setup`; cleared once
  // `onFreshRoot` has run (or the cycle re-arms through `loading`).
  const freshRootPendingRef = useRef(false);

  useEffect(() => {
    if (status === 'loading') {
      firedRef.current = false;
      freshRootPendingRef.current = false;
      return;
    }
    if (!enabled) return;
    if (status === 'connected' && freshRootPendingRef.current) {
      freshRootPendingRef.current = false;
      void onFreshRoot?.();
      return;
    }
    if (status === 'error') {
      // A failed setup never seeds; a retry re-arms through `loading`.
      freshRootPendingRef.current = false;
      return;
    }
    if (firedRef.current) return;
    if (status === 'needs-setup') {
      firedRef.current = true;
      freshRootPendingRef.current = true;
      void createProjectSet(DEFAULT_SYNC_SERVER);
    } else if (status === 'needs-migration') {
      firedRef.current = true;
      void migrateProjects(DEFAULT_SYNC_SERVER);
    }
    // The actions are stable useCallbacks; status/enabled are the triggers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, enabled]);
}
