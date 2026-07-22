/**
 * Shared ordering for project lists on the projects home: the global
 * "Everything else" list and each collection's card row both offer the same
 * three orderings.
 */

export type SortOrder = 'newest' | 'oldest' | 'name';

export interface SortableProject {
  description: string;
  /** ISO timestamp of the last time the project was opened. */
  lastAccessed: string;
}

/** Returns a newly sorted copy; the input array is not mutated. */
export function sortProjectItems<T extends SortableProject>(items: T[], order: SortOrder): T[] {
  const sorted = [...items];
  if (order === 'newest') {
    sorted.sort((a, b) => (a.lastAccessed < b.lastAccessed ? 1 : a.lastAccessed > b.lastAccessed ? -1 : 0));
  } else if (order === 'oldest') {
    sorted.sort((a, b) => (a.lastAccessed > b.lastAccessed ? 1 : a.lastAccessed < b.lastAccessed ? -1 : 0));
  } else {
    sorted.sort((a, b) => a.description.localeCompare(b.description));
  }
  return sorted;
}

export const sortOrderLabel = (order: SortOrder): string =>
  order === 'newest' ? 'newest first' : order === 'oldest' ? 'oldest first' : 'A to Z';
