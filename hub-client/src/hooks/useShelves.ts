/**
 * Shelf state for the projects-home UI exploration (explore/projects-shelves-ui).
 *
 * Shelves are a purely local, per-browser grouping of projects: a named list
 * of indexDocIds persisted to localStorage. They deliberately do NOT sync —
 * the short-term design ("Shelves + streamlined entry") is buildable on
 * today's metadata, and shared shelves are a later phase that would make a
 * shelf its own synced document. Keeping the storage local lets us test the
 * UX without touching the project-set schema.
 */

import { useState, useCallback, useEffect } from 'react';

export interface Shelf {
  id: string;
  name: string;
  projectIds: string[];
}

const SHELVES_KEY = 'qh-shelves-v1';
const PENDING_KEY = 'qh-shelf-pending-v1';

interface PendingAssignment {
  title: string;
  shelfId: string;
  ts: number;
}

function loadShelves(): Shelf[] {
  try {
    const raw = localStorage.getItem(SHELVES_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (s): s is Shelf =>
        typeof s?.id === 'string' &&
        typeof s?.name === 'string' &&
        Array.isArray(s?.projectIds),
    );
  } catch {
    return [];
  }
}

/**
 * Record that the next project created with this title should land on a
 * shelf. The indexDocId of a new project isn't known until the parent app
 * finishes creating the Automerge docs, so we reconcile by title when the
 * entry shows up in the project set.
 */
export function setPendingShelfAssignment(title: string, shelfId: string): void {
  const pending: PendingAssignment = { title, shelfId, ts: Date.now() };
  localStorage.setItem(PENDING_KEY, JSON.stringify(pending));
}

export function useShelves() {
  const [shelves, setShelves] = useState<Shelf[]>(loadShelves);

  useEffect(() => {
    localStorage.setItem(SHELVES_KEY, JSON.stringify(shelves));
  }, [shelves]);

  const createShelf = useCallback((name: string): string => {
    const id = `shelf-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    setShelves((prev) => [...prev, { id, name, projectIds: [] }]);
    return id;
  }, []);

  const renameShelf = useCallback((id: string, name: string) => {
    setShelves((prev) => prev.map((s) => (s.id === id ? { ...s, name } : s)));
  }, []);

  const deleteShelf = useCallback((id: string) => {
    setShelves((prev) => prev.filter((s) => s.id !== id));
  }, []);

  /**
   * Put a project on a shelf (or none). A project sits on at most one
   * personal shelf, so it's removed from all others first.
   */
  const moveProject = useCallback((indexDocId: string, shelfId: string | null) => {
    setShelves((prev) =>
      prev.map((s) => {
        const without = s.projectIds.filter((p) => p !== indexDocId);
        if (s.id === shelfId && !without.includes(indexDocId)) {
          return { ...s, projectIds: [...without, indexDocId] };
        }
        return without.length === s.projectIds.length ? s : { ...s, projectIds: without };
      }),
    );
  }, []);

  const shelfFor = useCallback(
    (indexDocId: string): Shelf | undefined =>
      shelves.find((s) => s.projectIds.includes(indexDocId)),
    [shelves],
  );

  /**
   * Reconcile a pending "add to shelf on create" against the current
   * project list. Called whenever the entries change; a no-op when there is
   * nothing pending. Stale pendings (>1 day) are dropped.
   */
  const reconcilePending = useCallback(
    (entries: Array<{ indexDocId: string; description: string; addedAt: string }>) => {
      const raw = localStorage.getItem(PENDING_KEY);
      if (!raw) return;
      let pending: PendingAssignment;
      try {
        pending = JSON.parse(raw);
      } catch {
        localStorage.removeItem(PENDING_KEY);
        return;
      }
      if (Date.now() - pending.ts > 24 * 3600 * 1000) {
        localStorage.removeItem(PENDING_KEY);
        return;
      }
      const match = entries.find(
        (e) =>
          e.description === pending.title &&
          new Date(e.addedAt).getTime() >= pending.ts - 60_000,
      );
      if (match) {
        moveProject(match.indexDocId, pending.shelfId);
        localStorage.removeItem(PENDING_KEY);
      }
    },
    [moveProject],
  );

  return { shelves, createShelf, renameShelf, deleteShelf, moveProject, shelfFor, reconcilePending };
}
