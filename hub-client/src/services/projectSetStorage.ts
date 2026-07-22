/**
 * IndexedDB storage for the project set pointer.
 *
 * After migration, IndexedDB stores only a singleton pointer to the
 * Automerge-backed ProjectSetDocument. The actual project list lives
 * in the Automerge document, synced across browsers.
 */
import type { ProjectSetPointer, CollectionPointerEntry, CollectionsPointer } from './storage/types';
import { STORES, getDb } from './storage';
import { migratePointerToCollections } from './storage/migrations';

/**
 * Get the stored project set pointer, or null if not yet configured.
 */
export async function getProjectSetPointer(): Promise<ProjectSetPointer | null> {
  const db = await getDb();
  if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) {
    return null;
  }
  const pointer = await db.get(STORES.PROJECT_SET, 'projectSet');
  return pointer ?? null;
}

/**
 * Store the project set pointer.
 * This is the commit point for migration — only call this after the
 * Automerge ProjectSetDocument has been successfully created and synced.
 */
export async function setProjectSetPointer(
  projectSetDocId: string,
  syncServer: string,
): Promise<void> {
  const db = await getDb();
  const pointer: ProjectSetPointer = {
    key: 'projectSet',
    projectSetDocId,
    syncServer,
  };
  await db.put(STORES.PROJECT_SET, pointer);
}

/**
 * Clear the project set pointer.
 * Used when unlinking from a project set (e.g., to switch to a different one).
 */
export async function clearProjectSetPointer(): Promise<void> {
  const db = await getDb();
  if (db.objectStoreNames.contains(STORES.PROJECT_SET)) {
    await db.delete(STORES.PROJECT_SET, 'projectSet');
  }
}

// ============================================================================
// Collections pointer (array of collection ProjectSetDocuments)
// ============================================================================

/**
 * Get the collections this browser is subscribed to.
 *
 * Self-healing: when the collections record is missing but a legacy
 * singleton pointer exists (e.g. written by an older code path after the
 * v5 migration ran), it is converted on the spot. Returns [] for a fresh
 * browser.
 */
export async function getCollectionPointers(): Promise<CollectionPointerEntry[]> {
  const db = await getDb();
  if (!db.objectStoreNames.contains(STORES.PROJECT_SET)) {
    return [];
  }
  let collections: CollectionPointerEntry[];
  const record: CollectionsPointer | undefined = await db.get(STORES.PROJECT_SET, 'collections');
  if (record) {
    collections = record.collections;
  } else {
    await migratePointerToCollections(db);
    const migrated: CollectionsPointer | undefined = await db.get(STORES.PROJECT_SET, 'collections');
    collections = migrated?.collections ?? [];
  }
  const singleton: ProjectSetPointer | undefined = await db.get(STORES.PROJECT_SET, 'projectSet');
  return pinRootFirst(collections, singleton);
}

/**
 * Ensure the root — the doc named by the legacy singleton pointer — is first.
 *
 * Root identity is positional elsewhere (`collections[0]` is treated as the
 * personal superset). But the array can be built out of order when the
 * collections record is first created by a path *other* than the
 * singleton→collections migration (notably the `qh-collections-v1`
 * localStorage-collections migration, which creates collection pointers before
 * the singleton self-heal can prepend the root). Reorder on read so the root
 * is always index 0, regardless of how the array was assembled. No-op when
 * there is no singleton, the array is trivial, or the root is already first.
 */
function pinRootFirst(
  collections: CollectionPointerEntry[],
  singleton: ProjectSetPointer | undefined,
): CollectionPointerEntry[] {
  if (!singleton || collections.length < 2) return collections;
  const norm = (id: string) => id.replace(/^automerge:/, '');
  const rootId = norm(singleton.projectSetDocId);
  const idx = collections.findIndex((c) => norm(c.projectSetDocId) === rootId);
  if (idx <= 0) return collections;
  const reordered = [...collections];
  const [root] = reordered.splice(idx, 1);
  reordered.unshift(root);
  return reordered;
}

/** Replace the full collections array. */
export async function setCollectionPointers(
  collections: CollectionPointerEntry[],
): Promise<void> {
  const db = await getDb();
  const record: CollectionsPointer = { key: 'collections', collections };
  await db.put(STORES.PROJECT_SET, record);
}

/** Subscribe to a collection (no-op if the doc id is already present). */
export async function addCollectionPointer(
  entry: CollectionPointerEntry,
): Promise<void> {
  const existing = await getCollectionPointers();
  if (existing.some((c) => c.projectSetDocId === entry.projectSetDocId)) {
    return;
  }
  await setCollectionPointers([...existing, entry]);
}

/** Unsubscribe from a collection. The document itself is untouched. */
export async function removeCollectionPointer(
  projectSetDocId: string,
): Promise<void> {
  const existing = await getCollectionPointers();
  const filtered = existing.filter((c) => c.projectSetDocId !== projectSetDocId);
  if (filtered.length !== existing.length) {
    await setCollectionPointers(filtered);
  }
}
