/**
 * Collection state for the projects-home UI exploration (explore/projects-collections-ui).
 *
 * Collections are a purely local, per-browser grouping of projects: a named list
 * of indexDocIds persisted to localStorage. They deliberately do NOT sync —
 * the short-term design ("Collections + streamlined entry") is buildable on
 * today's metadata, and shared collections are a later phase that would make a
 * collection its own synced document. Keeping the storage local lets us test the
 * UX without touching the project-set schema.
 */

import { useState, useCallback, useEffect } from 'react';

/** A member of a shared collection. Mock data until collections become synced docs. */
export interface CollectionMember {
  name: string;
  initials: string;
  color: string;
  joinedAt: string;
  isOwner?: boolean;
  isYou?: boolean;
}

export interface Collection {
  id: string;
  name: string;
  projectIds: string[];
  /** Present once the collection has been explicitly shared. */
  shared?: {
    sharedAt: string;
    members: CollectionMember[];
  };
}

const COLLECTIONS_KEY = 'qh-collections-v1';
// Pre-rename storage key ("shelf" era); read once as a migration fallback so
// existing exploration data survives the shelf → collection rename.
const LEGACY_SHELVES_KEY = 'qh-shelves-v1';
const PENDING_KEY = 'qh-collection-pending-v1';

interface PendingAssignment {
  title: string;
  collectionId: string;
  ts: number;
}

function loadCollections(): Collection[] {
  try {
    const raw = localStorage.getItem(COLLECTIONS_KEY) ?? localStorage.getItem(LEGACY_SHELVES_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (s): s is Collection =>
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
 * collection. The indexDocId of a new project isn't known until the parent app
 * finishes creating the Automerge docs, so we reconcile by title when the
 * entry shows up in the project set.
 */
export function setPendingCollectionAssignment(title: string, collectionId: string): void {
  const pending: PendingAssignment = { title, collectionId, ts: Date.now() };
  localStorage.setItem(PENDING_KEY, JSON.stringify(pending));
}

export function useCollections() {
  const [collections, setCollections] = useState<Collection[]>(loadCollections);

  useEffect(() => {
    localStorage.setItem(COLLECTIONS_KEY, JSON.stringify(collections));
  }, [collections]);

  const createCollection = useCallback((name: string): string => {
    const id = `collection-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    setCollections((prev) => [...prev, { id, name, projectIds: [] }]);
    return id;
  }, []);

  const renameCollection = useCallback((id: string, name: string) => {
    setCollections((prev) => prev.map((s) => (s.id === id ? { ...s, name } : s)));
  }, []);

  const deleteCollection = useCallback((id: string) => {
    setCollections((prev) => prev.filter((s) => s.id !== id));
  }, []);

  /**
   * Put a project on a collection (or none). A project sits on at most one
   * personal collection, so it's removed from all others first.
   */
  const moveProject = useCallback((indexDocId: string, collectionId: string | null) => {
    setCollections((prev) =>
      prev.map((s) => {
        const without = s.projectIds.filter((p) => p !== indexDocId);
        if (s.id === collectionId && !without.includes(indexDocId)) {
          return { ...s, projectIds: [...without, indexDocId] };
        }
        return without.length === s.projectIds.length ? s : { ...s, projectIds: without };
      }),
    );
  }, []);

  const collectionFor = useCallback(
    (indexDocId: string): Collection | undefined =>
      collections.find((s) => s.projectIds.includes(indexDocId)),
    [collections],
  );

  /** Convert a personal collection to shared, seeding the member list. */
  const shareCollection = useCallback((id: string, members: CollectionMember[]) => {
    setCollections((prev) =>
      prev.map((s) =>
        s.id === id && !s.shared
          ? { ...s, shared: { sharedAt: new Date().toISOString(), members } }
          : s,
      ),
    );
  }, []);

  const removeMember = useCallback((collectionId: string, initials: string) => {
    setCollections((prev) =>
      prev.map((s) =>
        s.id === collectionId && s.shared
          ? { ...s, shared: { ...s.shared, members: s.shared.members.filter((m) => m.initials !== initials) } }
          : s,
      ),
    );
  }, []);

  /**
   * Reconcile a pending "add to collection on create" against the current
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
        moveProject(match.indexDocId, pending.collectionId);
        localStorage.removeItem(PENDING_KEY);
      }
    },
    [moveProject],
  );

  return {
    collections,
    createCollection,
    renameCollection,
    deleteCollection,
    moveProject,
    collectionFor,
    reconcilePending,
    shareCollection,
    removeMember,
  };
}

/**
 * Create a shared collection from an invite, writing localStorage directly.
 * Used by the join-collection landing screen, which renders before ProjectsHome
 * mounts (so there is no live hook instance to go through). Returns false
 * if the collection already exists in this browser.
 */
export function createSharedCollectionFromInvite(args: {
  collectionId: string;
  name: string;
  projectIds: string[];
  members: CollectionMember[];
}): boolean {
  const collections = loadCollections();
  if (collections.some((s) => s.id === args.collectionId)) return false;
  collections.push({
    id: args.collectionId,
    name: args.name,
    projectIds: args.projectIds,
    shared: { sharedAt: new Date().toISOString(), members: args.members },
  });
  localStorage.setItem(COLLECTIONS_KEY, JSON.stringify(collections));
  return true;
}
