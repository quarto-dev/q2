/**
 * Reconcile local IndexedDB project entries into the synced project set.
 *
 * Motivation: when a user visits a `#/share/...` link, the share handler
 * writes the project to IDB immediately but can only push it to the synced
 * project set if the project-set websocket happens to be `connected` at that
 * instant. On a fresh page load it almost never is, so the project lives in
 * IDB only and is invisible on the landing page (which renders from the
 * synced set). The reconciler closes that gap: once the project set becomes
 * connected, we compare IDB against it and upsert anything missing.
 *
 * This module is split into a pure `computeReconcileAdds` (unit-tested) and
 * an imperative `reconcileIntoConnectedProjectSet` that wires it up to the
 * real services. Keep the pure function pure.
 *
 * See claude-notes/plans/2026-04-16-share-link-project-not-added.md.
 */

import { projectSetKey } from '@quarto/quarto-automerge-schema';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import * as projectStorage from './projectStorage';
import * as projectSetService from './projectSetService';

/**
 * The subset of an IDB `ProjectEntry` that the reconciler needs. Accepting
 * this narrowed shape keeps the pure function decoupled from the full
 * `ProjectEntry` type and makes tests easy to write.
 */
export interface ReconcilableEntry {
  indexDocId: string;
  syncServer: string;
  description: string;
  lastAccessed: string;
}

/**
 * Return the IDB entries that are missing from the synced project set.
 *
 * Comparison is by the `automerge:`-stripped indexDocId (same key as the
 * project set's Record). If IDB contains multiple rows that resolve to the
 * same key (which can happen historically because two code paths stored
 * the id with and without the prefix), the most-recently-accessed one wins.
 */
export function computeReconcileAdds(
  idbProjects: ReadonlyArray<ReconcilableEntry>,
  setEntries: ReadonlyArray<Pick<ProjectSetEntry, 'indexDocId'>>,
): ReconcilableEntry[] {
  const setKeys = new Set(setEntries.map((e) => projectSetKey(e.indexDocId)));

  // Dedupe IDB rows by canonical key, keeping the most recently accessed.
  const byKey = new Map<string, ReconcilableEntry>();
  for (const entry of idbProjects) {
    const key = projectSetKey(entry.indexDocId);
    const existing = byKey.get(key);
    if (!existing || entry.lastAccessed > existing.lastAccessed) {
      byKey.set(key, entry);
    }
  }

  const adds: ReconcilableEntry[] = [];
  for (const [key, entry] of byKey) {
    if (!setKeys.has(key)) adds.push(entry);
  }
  return adds;
}

/**
 * Run the reconciler against the live services.
 *
 * Safe to call repeatedly (idempotent): each invocation reads the current
 * state of both sides and only pushes the missing adds. Call this after the
 * project set transitions to `connected`.
 *
 * @returns number of entries added to the synced set.
 */
export async function reconcileIntoConnectedProjectSet(): Promise<number> {
  if (!projectSetService.isConnected()) return 0;

  const idbProjects = await projectStorage.listProjects();
  const setEntries = projectSetService.listProjects();
  const adds = computeReconcileAdds(idbProjects, setEntries);
  if (adds.length === 0) return 0;

  return projectSetService.addProjectsBulk(adds);
}
