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
 * Deletions are the flip side of that comparison. An entry absent from the
 * set is ambiguous — "never synced" and "deleted" look identical — so
 * deletions write a tombstone (key -> deletion timestamp) into the set
 * document. A missing IDB row is re-added only when its lastAccessed is
 * NEWER than the tombstone (latest one wins); otherwise the stale row is
 * purged from IDB so it can never resurrect the project. Because the
 * tombstones sync with the document, a deletion made on one browser also
 * wins on every other browser's reconcile.
 *
 * This module is split into pure `computeReconcileAdds` /
 * `computeReconcilePurges` (unit-tested) and an imperative
 * `reconcileIntoConnectedProjectSet` that wires them up to the real
 * services. Keep the pure functions pure.
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
 * Dedupe IDB rows by canonical key, keeping the most recently accessed.
 * (IDB can hold two rows for the same key because code paths historically
 * stored the id with and without the `automerge:` prefix.)
 */
function dedupeByKey(
  idbProjects: ReadonlyArray<ReconcilableEntry>,
): Map<string, ReconcilableEntry> {
  const byKey = new Map<string, ReconcilableEntry>();
  for (const entry of idbProjects) {
    const key = projectSetKey(entry.indexDocId);
    const existing = byKey.get(key);
    if (!existing || entry.lastAccessed > existing.lastAccessed) {
      byKey.set(key, entry);
    }
  }
  return byKey;
}

/**
 * Latest-wins verdict for one key: the deletion tombstone wins when it is
 * at or newer than the row's last access (ties go to the deletion — a
 * simultaneous delete/re-add pair resolved this way can't loop, because
 * the re-add clears the tombstone).
 */
function tombstoneWins(
  tombstones: Record<string, string>,
  key: string,
  lastAccessed: string,
): boolean {
  const deletedAt = tombstones[key];
  return deletedAt !== undefined && deletedAt >= lastAccessed;
}

/**
 * Return the IDB entries that are missing from the synced project set and
 * not suppressed by a deletion tombstone.
 *
 * Comparison is by the `automerge:`-stripped indexDocId (same key as the
 * project set's Record). If IDB contains multiple rows that resolve to the
 * same key, the most-recently-accessed one is the candidate.
 */
export function computeReconcileAdds(
  idbProjects: ReadonlyArray<ReconcilableEntry>,
  setEntries: ReadonlyArray<Pick<ProjectSetEntry, 'indexDocId'>>,
  tombstones: Record<string, string> = {},
): ReconcilableEntry[] {
  const setKeys = new Set(setEntries.map((e) => projectSetKey(e.indexDocId)));

  const adds: ReconcilableEntry[] = [];
  for (const [key, entry] of dedupeByKey(idbProjects)) {
    if (setKeys.has(key)) continue;
    // A tombstone at or newer than the row's last access means this row is
    // a stale pre-delete copy, not a project waiting to be restored.
    if (tombstoneWins(tombstones, key, entry.lastAccessed)) continue;
    adds.push(entry);
  }
  return adds;
}

/**
 * Return the IDB rows that lost to a deletion tombstone — stale local
 * copies of projects deleted from the set. Callers purge them so they can
 * never resurrect the project on a later reconcile.
 */
export function computeReconcilePurges(
  idbProjects: ReadonlyArray<ReconcilableEntry>,
  setEntries: ReadonlyArray<Pick<ProjectSetEntry, 'indexDocId'>>,
  tombstones: Record<string, string>,
): ReconcilableEntry[] {
  const setKeys = new Set(setEntries.map((e) => projectSetKey(e.indexDocId)));

  const purges: ReconcilableEntry[] = [];
  for (const [key, entry] of dedupeByKey(idbProjects)) {
    if (setKeys.has(key)) continue;
    if (tombstoneWins(tombstones, key, entry.lastAccessed)) purges.push(entry);
  }
  return purges;
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
  const tombstones = projectSetService.getRootTombstones();

  // Purge stale local copies of deleted projects first, so a losing row
  // can never resurrect its project on a later load.
  for (const entry of computeReconcilePurges(idbProjects, setEntries, tombstones)) {
    await projectStorage.deleteProjectByIndexDocId(entry.indexDocId);
  }

  const adds = computeReconcileAdds(idbProjects, setEntries, tombstones);
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
