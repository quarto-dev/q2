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
}

export function useAutoEstablishRoot({
  status,
  enabled,
  createProjectSet,
  migrateProjects,
}: AutoEstablishRootOptions): void {
  const firedRef = useRef(false);

  useEffect(() => {
    if (status === 'loading') {
      firedRef.current = false;
      return;
    }
    if (!enabled || firedRef.current) return;
    if (status === 'needs-setup') {
      firedRef.current = true;
      void createProjectSet(DEFAULT_SYNC_SERVER);
    } else if (status === 'needs-migration') {
      firedRef.current = true;
      void migrateProjects(DEFAULT_SYNC_SERVER);
    }
    // The actions are stable useCallbacks; status/enabled are the triggers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, enabled]);
}
