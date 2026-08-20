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

/** Result of a user-initiated project-list import in project-set mode. */
export interface ImportReconcileResult {
  /** New rows written to the legacy IDB `projects` store (dedup by indexDocId). */
  imported: number;
  /** Entries pushed into the synced root set — what actually becomes visible. */
  reconciled: number;
  /** False when the set was offline: entries are saved in IDB but stay
   * invisible until the on-load reconciler sweeps them in. Callers use this
   * to word the result message honestly. */
  connected: boolean;
}

/**
 * Import a project-list JSON export and immediately reconcile it into the
 * connected project set.
 *
 * `importData` alone writes only the legacy IDB `projects` store, which the
 * set-mode UI never renders — the on-load reconciler would sweep the entries
 * in on the NEXT page load, so an import appeared to do nothing until a
 * manual reload ("Imported 30 project(s)" showing zero). Running the
 * reconcile inline right after the import closes that gap.
 */
export async function importProjectsAndReconcile(json: string): Promise<ImportReconcileResult> {
  const imported = await projectStorage.importData(json);
  const connected = projectSetService.isConnected();
  const reconciled = await reconcileIntoConnectedProjectSet();
  return { imported, reconciled, connected };
}
