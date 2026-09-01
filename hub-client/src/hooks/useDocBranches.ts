/**
 * useDocBranches — minimal React binding for branchService.
 * Re-renders on service notifications; actions are called on the service
 * directly by the consumer (Editor.tsx). Prototype-simple by design.
 */

import { useEffect, useState } from 'react';
import { getBranches, getActiveBranchId, subscribe, type BranchMeta } from '../services/branchService';

export function useDocBranches(path: string | null): {
  branches: BranchMeta[];
  activeBranchId: string | null;
} {
  const [, setVersion] = useState(0);
  useEffect(() => subscribe(() => setVersion((v) => v + 1)), []);
  return {
    branches: path ? getBranches(path) : [],
    activeBranchId: path ? getActiveBranchId(path) : null,
  };
}
