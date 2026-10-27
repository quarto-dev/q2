/**
 * useDocBranches — minimal React binding for branchService.
 * Subscribes via useSyncExternalStore over the service's memoized
 * per-path snapshots; actions are called on the service directly by the
 * consumer (Editor.tsx). Prototype-simple by design.
 */

import { useCallback, useSyncExternalStore } from 'react';
import {
  getBranchSnapshot,
  subscribe,
  type BranchSnapshot,
} from '../services/branchService';

/** Shared empty result; one constant so the snapshot stays referentially stable. */
const EMPTY_SNAPSHOT: BranchSnapshot = { branches: [], activeBranchId: null };

export function useDocBranches(path: string | null): BranchSnapshot {
  const subscribeFn = useCallback(
    (onStoreChange: () => void) => subscribe(onStoreChange),
    [],
  );
  const getSnapshot = useCallback(
    () => (path ? getBranchSnapshot(path) : EMPTY_SNAPSHOT),
    [path],
  );
  return useSyncExternalStore(subscribeFn, getSnapshot);
}
